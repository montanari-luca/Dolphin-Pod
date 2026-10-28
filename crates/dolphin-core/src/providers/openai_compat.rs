//! OpenAI-compatible Chat Completions adapter: OpenRouter, LM Studio, custom endpoints, xAI, Mistral,
//! DeepSeek, Groq, Together, Fireworks and Azure OpenAI (v1 API). Reasoning arrives in different
//! places depending on the server (`reasoning`, `reasoning_content`, Mistral thinking chunks, or
//! inline `<think>` tags); all of them are normalized to `ReasoningDelta`.

use std::collections::BTreeMap;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};

use super::sse::sse_stream;
use super::{
    ChatRequest, EventStream, Part, Provider, ProviderEvent, Role, ThinkTagSplitter, ToolCall, Usage, error_message,
    http_client, send_with_retry,
};
use crate::catalog;
use crate::error::{CoreError, Result};
use crate::model::{EffortControl, EffortValue, ModelInfo, ProviderKind};

pub struct OpenAiCompat {
    kind: ProviderKind,
    base: String,
    key: Option<String>,
    http: reqwest::Client,
}

impl OpenAiCompat {
    pub fn new(kind: ProviderKind, base: String, key: Option<String>, _extra: Value) -> Self {
        Self { kind, base, key, http: http_client() }
    }

    fn authed(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        let rb = match (&self.key, self.kind) {
            (Some(k), ProviderKind::AzureOpenai) => rb.header("api-key", k),
            (Some(k), _) if !k.is_empty() => rb.bearer_auth(k),
            _ => rb,
        };
        if self.kind == ProviderKind::Openrouter {
            rb.header("HTTP-Referer", "https://mintflow.dev").header("X-Title", "Dolphin Pod")
        } else {
            rb
        }
    }
}

pub(crate) fn content_value(parts: &[Part]) -> Value {
    if parts.iter().all(|p| matches!(p, Part::Text(_))) {
        let text: Vec<&str> = parts.iter().filter_map(|p| if let Part::Text(t) = p { Some(t.as_str()) } else { None }).collect();
        return json!(text.join("\n"));
    }
    json!(parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => json!({ "type": "text", "text": t }),
            Part::Image { mime, data_b64 } => json!({ "type": "image_url", "image_url": { "url": format!("data:{mime};base64,{data_b64}") } }),
        })
        .collect::<Vec<_>>())
}

pub(crate) fn build_body(kind: ProviderKind, req: &ChatRequest) -> Value {
    let mut messages = Vec::new();
    if !req.system.is_empty() {
        messages.push(json!({ "role": "system", "content": req.system }));
    }
    let mut stable_at: Option<usize> = None;
    for (i, m) in req.messages.iter().enumerate() {
        if req.stable_messages == Some(i) {
            stable_at = messages.len().checked_sub(1);
        }
        match m.role {
            Role::User => messages.push(json!({ "role": "user", "content": content_value(&m.parts) })),
            Role::Assistant => {
                let mut msg = json!({ "role": "assistant", "content": m.text_content() });
                if !m.tool_calls.is_empty() {
                    msg["tool_calls"] = json!(m
                        .tool_calls
                        .iter()
                        .map(|c| json!({ "id": c.id, "type": "function", "function": { "name": c.name, "arguments": c.arguments.to_string() } }))
                        .collect::<Vec<_>>());
                }
                messages.push(msg);
            }
            Role::Tool => messages.push(json!({
                "role": "tool",
                "tool_call_id": m.tool_call_id,
                "content": m.text_content(),
            })),
        }
    }
    if kind == ProviderKind::Openrouter && req.model.starts_with("anthropic/") {
        // Anthropic models behind OpenRouter only cache at explicit breakpoints: system, end of the
        // stable history and end of the request.
        let last = messages.len().checked_sub(1);
        for i in [(!req.system.is_empty()).then_some(0), stable_at, last].into_iter().flatten() {
            mark_cacheable(&mut messages[i]);
        }
    }
    let mut body = json!({ "model": req.model, "messages": messages, "stream": true });
    if kind != ProviderKind::Mistral {
        body["stream_options"] = json!({ "include_usage": true });
    }
    let mut tools: Vec<Value> = req
        .tools
        .iter()
        .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } }))
        .collect();
    if req.web_search && kind == ProviderKind::Openrouter {
        tools.push(json!({ "type": "openrouter:web_search" }));
    }
    if !tools.is_empty() {
        body["tools"] = json!(tools);
    }
    if req.json_mode {
        body["response_format"] = json!({ "type": "json_object" });
    }
    if let Some(n) = req.max_tokens {
        body["max_tokens"] = json!(n);
    }
    match kind {
        ProviderKind::Openrouter => {
            let r = match &req.effort {
                Some(EffortValue::Level(l)) => Some(json!({ "effort": l })),
                Some(EffortValue::Budget(n)) => Some(json!({ "max_tokens": n })),
                Some(EffortValue::Toggle(b)) => Some(json!({ "enabled": b })),
                None if req.want_reasoning => Some(json!({ "enabled": true })),
                None => None,
            };
            if let Some(r) = r {
                body["reasoning"] = r;
            }
        }
        ProviderKind::Deepseek | ProviderKind::Mistral => {}
        _ => {
            if let Some(EffortValue::Level(l)) = &req.effort {
                body["reasoning_effort"] = json!(l);
            }
        }
    }
    body
}

/// Turn a message's text content into a content-part array whose last text part carries a cache breakpoint.
fn mark_cacheable(msg: &mut Value) {
    if msg["role"] == "tool" {
        return;
    }
    let parts = match msg["content"].take() {
        Value::String(t) if !t.trim().is_empty() => vec![json!({ "type": "text", "text": t })],
        Value::Array(a) => a,
        other => {
            msg["content"] = other;
            return;
        }
    };
    msg["content"] = json!(parts);
    if let Some(last) = msg["content"].as_array_mut().and_then(|a| a.iter_mut().rev().find(|p| p["type"] == "text")) {
        last["cache_control"] = json!({ "type": "ephemeral" });
    }
}

/// Stateful mapper for one streamed completion.
#[derive(Default)]
pub(crate) struct ChunkMapper {
    think: ThinkTagSplitter,
    calls: BTreeMap<u64, (String, String, String)>,
    finish: Option<String>,
    usage: Option<Usage>,
    cache: Option<(i64, i64)>,
}

impl ChunkMapper {
    pub fn map(&mut self, v: &Value) -> Vec<ProviderEvent> {
        let mut out = Vec::new();
        if let Some(err) = v.get("error") {
            let msg = err["message"].as_str().map(String::from).unwrap_or_else(|| err.to_string());
            out.push(ProviderEvent::Error(msg));
            return out;
        }
        if let Some(u) = v.get("usage").filter(|u| u.is_object()) {
            self.usage = Some(Usage {
                input_tokens: u["prompt_tokens"].as_i64().unwrap_or(0),
                output_tokens: u["completion_tokens"].as_i64().unwrap_or(0),
            });
            let d = &u["prompt_tokens_details"];
            let read = d["cached_tokens"].as_i64().or(u["prompt_cache_hit_tokens"].as_i64()).unwrap_or(0);
            let written = d["cache_write_tokens"].as_i64().unwrap_or(0);
            self.cache = (read + written > 0).then_some((read, written));
        }
        let Some(choice) = v["choices"].get(0) else { return out };
        let delta = &choice["delta"];
        for key in ["reasoning_content", "reasoning"] {
            if let Some(r) = delta[key].as_str().filter(|s| !s.is_empty()) {
                out.push(ProviderEvent::ReasoningDelta(r.to_string()));
                break;
            }
        }
        match &delta["content"] {
            Value::String(s) if !s.is_empty() => self.push_text(s, &mut out),
            // Mistral Magistral streams typed chunks: [{type: thinking, thinking: [{type: text, text}]}, {type: text, text}]
            Value::Array(items) => {
                for it in items {
                    match it["type"].as_str() {
                        Some("thinking") => {
                            let t: String = it["thinking"]
                                .as_array()
                                .map(|a| a.iter().filter_map(|x| x["text"].as_str()).collect())
                                .unwrap_or_default();
                            if !t.is_empty() {
                                out.push(ProviderEvent::ReasoningDelta(t));
                            }
                        }
                        Some("text") => {
                            if let Some(t) = it["text"].as_str() {
                                self.push_text(t, &mut out);
                            }
                        }
                        _ => {}
                    }
                }
            }
            _ => {}
        }
        if let Some(calls) = delta["tool_calls"].as_array() {
            for c in calls {
                let idx = c["index"].as_u64().unwrap_or(self.calls.len() as u64);
                let e = self.calls.entry(idx).or_default();
                if let Some(id) = c["id"].as_str() {
                    e.0 = id.to_string();
                }
                if let Some(n) = c["function"]["name"].as_str() {
                    e.1.push_str(n);
                }
                if let Some(a) = c["function"]["arguments"].as_str() {
                    e.2.push_str(a);
                }
            }
        }
        if let Some(f) = choice["finish_reason"].as_str() {
            self.finish = Some(f.to_string());
        }
        out
    }

    fn push_text(&mut self, s: &str, out: &mut Vec<ProviderEvent>) {
        let (r, t) = self.think.push(s);
        if !r.is_empty() {
            out.push(ProviderEvent::ReasoningDelta(r));
        }
        if !t.is_empty() {
            out.push(ProviderEvent::TextDelta(t));
        }
    }

    pub fn finish(&mut self) -> Vec<ProviderEvent> {
        let mut out = Vec::new();
        let (r, t) = self.think.finish();
        if !r.is_empty() {
            out.push(ProviderEvent::ReasoningDelta(r));
        }
        if !t.is_empty() {
            out.push(ProviderEvent::TextDelta(t));
        }
        for (i, (id, name, args)) in std::mem::take(&mut self.calls) {
            let arguments = if args.trim().is_empty() {
                json!({})
            } else {
                serde_json::from_str(&args).unwrap_or_else(|_| json!({ "_invalid_json": args }))
            };
            out.push(ProviderEvent::ToolCall(ToolCall {
                id: if id.is_empty() { format!("call_{i}") } else { id },
                name,
                arguments,
            }));
        }
        if let Some(u) = self.usage.take() {
            out.push(ProviderEvent::Usage(u));
        }
        if let Some((read, written)) = self.cache.take() {
            out.push(ProviderEvent::CacheUsage { read, written });
        }
        out.push(ProviderEvent::Done { stop_reason: self.finish.take(), raw_assistant: None });
        out
    }
}

#[async_trait]
impl Provider for OpenAiCompat {
    fn kind(&self) -> ProviderKind {
        self.kind
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let resp = send_with_retry(self.authed(self.http.get(format!("{}/models", self.base)))).await?;
        let v: Value = resp.json().await?;
        let items = v["data"].as_array().cloned().or_else(|| v.as_array().cloned()).unwrap_or_default();

        // LM Studio's native REST API says which models take images.
        let mut lm_vision = std::collections::HashSet::new();
        if self.kind == ProviderKind::LmStudio {
            let native = format!("{}/api/v0/models", self.base.trim_end_matches("/v1"));
            if let Ok(r) = self.http.get(native).send().await
                && let Ok(nv) = r.json::<Value>().await
            {
                for m in nv["data"].as_array().cloned().unwrap_or_default() {
                    if m["type"] == "vlm"
                        && let Some(id) = m["id"].as_str()
                    {
                        lm_vision.insert(id.to_string());
                    }
                }
            }
        }

        let mut out = Vec::new();
        for m in items {
            let Some(id) = m["id"].as_str() else { continue };
            if self.kind == ProviderKind::LmStudio && id.contains("embed") {
                continue;
            }
            let mut info = catalog::info_from_catalog(id, m["name"].as_str());
            if self.kind == ProviderKind::Openrouter {
                let params: Vec<&str> = m["supported_parameters"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
                let input: Vec<&str> = m["architecture"]["input_modalities"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
                let output: Vec<&str> = m["architecture"]["output_modalities"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
                info.reasoning = params.contains(&"reasoning") || params.contains(&"include_reasoning");
                info.reason = (!info.reasoning).then(|| catalog::NOT_REASONING.to_string());
                if info.reasoning && !matches!(info.effort, EffortControl::Levels { .. } | EffortControl::Budget { .. }) {
                    info.effort = EffortControl::Levels { levels: vec!["low".into(), "medium".into(), "high".into()], default: "medium".into() };
                }
                info.vision = input.contains(&"image");
                info.image_output = output.contains(&"image");
                info.tools = params.contains(&"tools");
                info.context_window = m["context_length"].as_u64().map(|n| n as u32);
                let zero = |p: &Value| p.as_str().is_some_and(|s| s.parse::<f64>().map(|f| f == 0.0).unwrap_or(false));
                info.free = id.ends_with(":free") || (zero(&m["pricing"]["prompt"]) && zero(&m["pricing"]["completion"]));
            }
            if self.kind == ProviderKind::Mistral
                && let Some(v) = m["capabilities"]["vision"].as_bool()
            {
                info.vision = v;
            }
            if lm_vision.contains(id) {
                info.vision = true;
            }
            out.push(info);
        }
        catalog::sort_models(&mut out);
        Ok(out)
    }

    async fn embed(&self, model: &str, inputs: &[String]) -> Result<Vec<Vec<f32>>> {
        let url = format!("{}/embeddings", self.base);
        let resp = send_with_retry(self.authed(self.http.post(url).json(&json!({ "model": model, "input": inputs })))).await.map_err(|e| match e {
            CoreError::Provider(m) if m.contains("could not connect") && self.kind.is_local() => {
                CoreError::provider(format!("{} is not reachable at {} ({m})", self.kind.label(), self.base))
            }
            other => other,
        })?;
        let v: Value = resp.json().await.map_err(|e| CoreError::provider(format!("unreadable embeddings answer: {e}")))?;
        super::parse_openai_embeddings(&v, inputs.len())
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<EventStream> {
        let body = build_body(self.kind, &req);
        let url = format!("{}/chat/completions", self.base);
        let resp = send_with_retry(self.authed(self.http.post(url).json(&body))).await.map_err(|e| match e {
            CoreError::Provider(m) if m.contains("could not connect") && self.kind.is_local() => {
                CoreError::provider(format!("{} is not reachable at {} ({m})", self.kind.label(), self.base))
            }
            other => other,
        })?;
        let mut events = Box::pin(sse_stream(resp.bytes_stream()));
        Ok(Box::pin(async_stream::stream! {
            let mut mapper = ChunkMapper::default();
            while let Some(item) = events.next().await {
                match item {
                    Ok(ev) => {
                        if ev.data.trim() == "[DONE]" { break; }
                        match serde_json::from_str::<Value>(&ev.data) {
                            Ok(v) => for e in mapper.map(&v) {
                                let stop = matches!(e, ProviderEvent::Error(_));
                                yield e;
                                if stop { return; }
                            },
                            Err(_) => { yield ProviderEvent::Error(error_message(&ev.data)); return; }
                        }
                    }
                    Err(e) => { yield ProviderEvent::Error(e); return; }
                }
            }
            for e in mapper.finish() { yield e; }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::sse::SseDecoder;

    fn run(fixture: &str) -> Vec<ProviderEvent> {
        let mut dec = SseDecoder::default();
        let mut m = ChunkMapper::default();
        let mut out = Vec::new();
        for ev in dec.push(fixture.as_bytes()) {
            if ev.data == "[DONE]" {
                break;
            }
            out.extend(m.map(&serde_json::from_str(&ev.data).unwrap()));
        }
        out.extend(m.finish());
        out
    }

    fn texts(evs: &[ProviderEvent]) -> (String, String) {
        let mut r = String::new();
        let mut t = String::new();
        for e in evs {
            match e {
                ProviderEvent::ReasoningDelta(x) => r.push_str(x),
                ProviderEvent::TextDelta(x) => t.push_str(x),
                _ => {}
            }
        }
        (r, t)
    }

    #[test]
    fn openrouter_reasoning_and_tool_call() {
        let evs = run(include_str!("../../tests/fixtures/openrouter_stream.sse"));
        let (r, t) = texts(&evs);
        assert_eq!(r, "Need to read the file first.");
        assert_eq!(t, "Let me check.");
        assert!(evs.iter().any(|e| matches!(e, ProviderEvent::ToolCall(c) if c.name == "read_file" && c.arguments == json!({"path": "src/main.rs"}))));
        assert!(evs.contains(&ProviderEvent::Usage(Usage { input_tokens: 50, output_tokens: 31 })));
    }

    #[test]
    fn lm_studio_inline_think_tags() {
        let evs = run(include_str!("../../tests/fixtures/lmstudio_think_tags.sse"));
        let (r, t) = texts(&evs);
        assert_eq!(r, "simple greeting");
        assert_eq!(t.trim(), "Hi there!");
    }

    #[test]
    fn deepseek_reasoning_content_and_mistral_chunks() {
        let mut m = ChunkMapper::default();
        let a = m.map(&json!({"choices": [{"delta": {"reasoning_content": "hmm"}}]}));
        assert_eq!(a, vec![ProviderEvent::ReasoningDelta("hmm".into())]);
        let b = m.map(&json!({"choices": [{"delta": {"content": [{"type": "thinking", "thinking": [{"type": "text", "text": "ponder"}]}, {"type": "text", "text": "Done"}]}}]}));
        assert_eq!(b, vec![ProviderEvent::ReasoningDelta("ponder".into()), ProviderEvent::TextDelta("Done".into())]);
    }

    #[test]
    fn openrouter_gets_its_server_side_web_search_tool() {
        let req = ChatRequest { model: "m".into(), web_search: true, ..Default::default() };
        assert_eq!(build_body(ProviderKind::Openrouter, &req)["tools"], json!([{ "type": "openrouter:web_search" }]));
        assert!(build_body(ProviderKind::Groq, &req).get("tools").is_none());
    }

    #[test]
    fn effort_mapping_per_provider() {
        let req = ChatRequest { model: "m".into(), effort: Some(EffortValue::Level("high".into())), ..Default::default() };
        assert_eq!(build_body(ProviderKind::Openrouter, &req)["reasoning"]["effort"], "high");
        assert_eq!(build_body(ProviderKind::Groq, &req)["reasoning_effort"], "high");
        assert!(build_body(ProviderKind::Deepseek, &req).get("reasoning_effort").is_none());
        let budget = ChatRequest { model: "m".into(), effort: Some(EffortValue::Budget(4000)), ..Default::default() };
        assert_eq!(build_body(ProviderKind::Openrouter, &budget)["reasoning"]["max_tokens"], 4000);
    }

    #[test]
    fn openrouter_anthropic_models_get_cache_breakpoints() {
        use crate::providers::ChatMessage;
        let req = ChatRequest {
            model: "anthropic/claude-sonnet-5-5".into(),
            system: "sys".into(),
            messages: vec![ChatMessage::user("one"), ChatMessage::assistant("two"), ChatMessage::user("task")],
            stable_messages: Some(2),
            ..Default::default()
        };
        let b = build_body(ProviderKind::Openrouter, &req);
        let m = b["messages"].as_array().unwrap();
        assert_eq!(m[0]["content"][0]["cache_control"]["type"], "ephemeral", "system");
        assert_eq!(m[2]["content"][0]["cache_control"]["type"], "ephemeral", "end of the stable history");
        assert_eq!(m[3]["content"][0]["cache_control"]["type"], "ephemeral", "end of the request");
        assert!(m[1]["content"].is_string(), "other turns stay plain strings");
        // Other providers and models are untouched.
        let plain = build_body(ProviderKind::Openrouter, &ChatRequest { model: "openai/gpt-5".into(), ..req.clone() });
        assert!(!plain.to_string().contains("cache_control"));
        assert!(!build_body(ProviderKind::Groq, &req).to_string().contains("cache_control"));
    }

    #[test]
    fn cached_prompt_tokens_are_reported() {
        let mut m = ChunkMapper::default();
        m.map(&json!({ "choices": [], "usage": { "prompt_tokens": 3000, "completion_tokens": 10, "prompt_tokens_details": { "cached_tokens": 2048 } } }));
        let evs = m.finish();
        assert!(evs.contains(&ProviderEvent::CacheUsage { read: 2048, written: 0 }));
    }
}
