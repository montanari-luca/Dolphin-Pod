//! Provider abstraction: one trait, normalized streaming events, adapters per protocol.

mod anthropic;
mod gemini;
pub mod image;
mod ollama;
mod openai_compat;
mod openai_responses;
pub mod sse;
mod think_tags;

use std::collections::HashMap;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use futures::{Stream, StreamExt};
use serde_json::Value;
use tokio::sync::Semaphore;

use crate::error::{CoreError, Result};
use crate::model::{EffortValue, ModelInfo, ProviderConfig, ProviderKind};

pub use think_tags::ThinkTagSplitter;

// ------------------------------------------------------------------ request / event types

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Part {
    Text(String),
    Image { mime: String, data_b64: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolCall {
    pub id: String,
    pub name: String,
    pub arguments: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ChatMessage {
    pub role: Role,
    pub parts: Vec<Part>,
    pub tool_calls: Vec<ToolCall>,
    /// For `Role::Tool`: which call this answers.
    pub tool_call_id: Option<String>,
    pub tool_name: Option<String>,
    /// Provider-native assistant turn (thinking signatures etc.), replayed unchanged by the same adapter.
    pub raw: Option<Value>,
}

impl ChatMessage {
    pub fn user(text: impl Into<String>) -> Self {
        Self::text(Role::User, text)
    }
    pub fn assistant(text: impl Into<String>) -> Self {
        Self::text(Role::Assistant, text)
    }
    pub fn text(role: Role, text: impl Into<String>) -> Self {
        Self {
            role,
            parts: vec![Part::Text(text.into())],
            tool_calls: vec![],
            tool_call_id: None,
            tool_name: None,
            raw: None,
        }
    }
    pub fn tool_result(call: &ToolCall, output: impl Into<String>) -> Self {
        Self {
            role: Role::Tool,
            parts: vec![Part::Text(output.into())],
            tool_calls: vec![],
            tool_call_id: Some(call.id.clone()),
            tool_name: Some(call.name.clone()),
            raw: None,
        }
    }
    pub fn text_content(&self) -> String {
        self.parts
            .iter()
            .filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None })
            .collect::<Vec<_>>()
            .join("\n")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    pub parameters: Value,
}

#[derive(Debug, Clone, Default)]
pub struct ChatRequest {
    pub model: String,
    pub system: String,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolSpec>,
    pub effort: Option<EffortValue>,
    pub max_tokens: Option<u32>,
    /// Ask for a JSON object reply where the provider supports it.
    pub json_mode: bool,
    /// The model is a reasoning model: ask the provider to return its reasoning stream.
    pub want_reasoning: bool,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Usage {
    pub input_tokens: i64,
    pub output_tokens: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ProviderEvent {
    TextDelta(String),
    ReasoningDelta(String),
    ToolCall(ToolCall),
    Usage(Usage),
    /// End of one model turn. `raw_assistant` is the native assistant turn for tool-loop replay.
    Done { stop_reason: Option<String>, raw_assistant: Option<Value> },
    Error(String),
}

pub type EventStream = Pin<Box<dyn Stream<Item = ProviderEvent> + Send>>;

#[async_trait]
pub trait Provider: Send + Sync {
    fn kind(&self) -> ProviderKind;
    /// Models with reasoning/effort/vision capabilities resolved as far as the provider exposes them.
    async fn list_models(&self) -> Result<Vec<ModelInfo>>;
    async fn stream_chat(&self, req: ChatRequest) -> Result<EventStream>;
}

// ------------------------------------------------------------------ shared HTTP plumbing

pub(crate) fn http_client() -> reqwest::Client {
    reqwest::Client::builder()
        .connect_timeout(Duration::from_secs(10))
        .pool_idle_timeout(Duration::from_secs(90))
        .user_agent(concat!("DolphinPod/", env!("CARGO_PKG_VERSION")))
        .build()
        .expect("http client")
}

/// Send with retry + exponential backoff on 429/5xx and connection errors (before any byte is streamed).
pub(crate) async fn send_with_retry(rb: reqwest::RequestBuilder) -> Result<reqwest::Response> {
    let mut delay = Duration::from_millis(600);
    for attempt in 0..4 {
        let req = rb.try_clone().ok_or_else(|| CoreError::provider("request not cloneable"))?;
        match req.send().await {
            Ok(resp) if resp.status().is_success() => return Ok(resp),
            Ok(resp) => {
                let status = resp.status();
                let retryable = status.as_u16() == 429 || status.is_server_error();
                if retryable && attempt < 3 {
                    let wait = resp
                        .headers()
                        .get("retry-after")
                        .and_then(|v| v.to_str().ok())
                        .and_then(|s| s.parse::<u64>().ok())
                        .map(|s| Duration::from_secs(s.min(30)))
                        .unwrap_or(delay);
                    tokio::time::sleep(wait).await;
                    delay *= 2;
                    continue;
                }
                let body = resp.text().await.unwrap_or_default();
                return Err(CoreError::provider(format!("HTTP {status}: {}", error_message(&body))));
            }
            Err(e) if attempt < 3 && (e.is_connect() || e.is_timeout()) => {
                tokio::time::sleep(delay).await;
                delay *= 2;
            }
            Err(e) if e.is_connect() => {
                return Err(CoreError::provider(format!("could not connect: {e}")));
            }
            Err(e) => return Err(e.into()),
        }
    }
    Err(CoreError::provider("gave up after retries"))
}

/// Pull a readable message out of a provider error body.
pub(crate) fn error_message(body: &str) -> String {
    if let Ok(v) = serde_json::from_str::<Value>(body) {
        for path in ["/error/message", "/message", "/error", "/detail", "/0/error/message"] {
            if let Some(s) = v.pointer(path).and_then(|x| x.as_str()) {
                return s.to_string();
            }
        }
    }
    crate::util::truncate(body.trim(), 400)
}

pub(crate) fn trim_base(url: &str) -> String {
    url.trim().trim_end_matches('/').to_string()
}

// ------------------------------------------------------------------ registry

/// Builds adapters from configs and enforces a per-provider concurrency limit.
type Versioned = (i64, Arc<dyn Provider>);

pub struct ProviderRegistry {
    cache: Mutex<HashMap<String, Versioned>>,
    limits: Mutex<HashMap<String, Arc<Semaphore>>>,
    overrides: Mutex<HashMap<String, Arc<dyn Provider>>>,
}

impl Default for ProviderRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl ProviderRegistry {
    pub fn new() -> Self {
        Self { cache: Mutex::new(HashMap::new()), limits: Mutex::new(HashMap::new()), overrides: Mutex::new(HashMap::new()) }
    }

    pub fn build(cfg: &ProviderConfig, api_key: Option<String>) -> Arc<dyn Provider> {
        let base = trim_base(&cfg.base_url);
        match cfg.kind {
            ProviderKind::Ollama => Arc::new(ollama::Ollama::new(base)),
            ProviderKind::Anthropic => Arc::new(anthropic::Anthropic::new(base, api_key)),
            ProviderKind::Gemini => Arc::new(gemini::Gemini::new(base, api_key)),
            ProviderKind::Openai => Arc::new(openai_responses::OpenAiResponses::new(base, api_key)),
            kind => Arc::new(openai_compat::OpenAiCompat::new(kind, base, api_key, cfg.extra.clone())),
        }
    }

    pub fn get(&self, cfg: &ProviderConfig, api_key: Option<String>) -> Arc<dyn Provider> {
        if let Some(p) = self.overrides.lock().unwrap().get(&cfg.id) {
            return p.clone();
        }
        let mut cache = self.cache.lock().unwrap();
        if let Some((ver, p)) = cache.get(&cfg.id)
            && *ver == cfg.updated_at
        {
            return p.clone();
        }
        let p = Self::build(cfg, api_key);
        cache.insert(cfg.id.clone(), (cfg.updated_at, p.clone()));
        p
    }

    /// Substitute an adapter for a provider id (tests, demos).
    #[doc(hidden)]
    pub fn set_override(&self, provider_id: &str, provider: Arc<dyn Provider>) {
        self.overrides.lock().unwrap().insert(provider_id.to_string(), provider);
    }

    pub fn invalidate(&self, provider_id: &str) {
        self.cache.lock().unwrap().remove(provider_id);
    }

    /// Local servers run one model at a time well; cloud APIs tolerate more parallel requests.
    pub fn semaphore(&self, cfg: &ProviderConfig) -> Arc<Semaphore> {
        let permits = if cfg.kind.is_local() { 2 } else { 6 };
        self.limits.lock().unwrap().entry(cfg.id.clone()).or_insert_with(|| Arc::new(Semaphore::new(permits))).clone()
    }
}

/// Collect a whole streamed reply (used for internal calls: moderator, orchestrator, utility tasks).
pub async fn complete(provider: &dyn Provider, req: ChatRequest) -> Result<(String, Usage)> {
    let mut stream = provider.stream_chat(req).await?;
    let mut text = String::new();
    let mut usage = Usage::default();
    while let Some(ev) = stream.next().await {
        match ev {
            ProviderEvent::TextDelta(t) => text.push_str(&t),
            ProviderEvent::Usage(u) => {
                usage.input_tokens = usage.input_tokens.max(u.input_tokens);
                usage.output_tokens = usage.output_tokens.max(u.output_tokens);
            }
            ProviderEvent::Error(e) => return Err(CoreError::Provider(e)),
            _ => {}
        }
    }
    Ok((text, usage))
}
