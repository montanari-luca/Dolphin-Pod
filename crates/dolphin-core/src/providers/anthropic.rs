//! Anthropic Messages API (raw HTTP; there is no official Rust SDK).
//! - Current models: `thinking: {type: "adaptive", display: "summarized"}` + `output_config.effort`.
//! - Older budget models (e.g. Haiku 4.5): `thinking: {type: "enabled", budget_tokens}`.
//! - Thinking blocks (with signatures) are replayed unchanged through tool loops via `raw_assistant`.

use std::collections::HashMap;

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};

use super::sse::sse_stream;
use super::{
    ChatMessage, ChatRequest, EventStream, Part, Provider, ProviderEvent, Role, ToolCall, Usage, http_client,
    send_with_retry,
};
use crate::catalog;
use crate::error::Result;
use crate::model::{EffortControl, EffortValue, ModelInfo, ProviderKind};

const VERSION: &str = "2023-06-01";
const DEFAULT_MAX_TOKENS: u32 = 32_000;

pub struct Anthropic {
    base: String,
    key: Option<String>,
    http: reqwest::Client,
}

impl Anthropic {
    pub fn new(base: String, key: Option<String>) -> Self {
        Self { base, key, http: http_client() }
    }

    fn authed(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("x-api-key", self.key.clone().unwrap_or_default()).header("anthropic-version", VERSION)
    }
}

fn user_parts(parts: &[Part]) -> Vec<Value> {
    parts
        .iter()
        .filter_map(|p| match p {
            Part::Text(t) if t.trim().is_empty() => None,
            Part::Text(t) => Some(json!({ "type": "text", "text": t })),
            Part::Image { mime, data_b64 } => {
                Some(json!({ "type": "image", "source": { "type": "base64", "media_type": mime, "data": data_b64 } }))
            }
        })
        .collect()
}

fn assistant_blocks(m: &ChatMessage) -> Vec<Value> {
    if let Some(Value::Array(raw)) = &m.raw {
        return raw.clone();
    }
    let mut blocks = Vec::new();
    let text = m.text_content();
    if !text.trim().is_empty() {
        blocks.push(json!({ "type": "text", "text": text }));
    }
    for c in &m.tool_calls {
        blocks.push(json!({ "type": "tool_use", "id": c.id, "name": c.name, "input": c.arguments }));
    }
    blocks
}

pub(crate) fn build_body(req: &ChatRequest) -> Value {
    let mut messages: Vec<Value> = Vec::new();
    let push = |messages: &mut Vec<Value>, role: &str, mut content: Vec<Value>| {
        if content.is_empty() {
            return;
        }
        if let Some(last) = messages.last_mut()
            && last["role"] == role
        {
            last["content"].as_array_mut().unwrap().append(&mut content);
            return;
        }
        messages.push(json!({ "role": role, "content": content }));
    };
    for m in &req.messages {
        match m.role {
            Role::User => push(&mut messages, "user", user_parts(&m.parts)),
            Role::Assistant => push(&mut messages, "assistant", assistant_blocks(m)),
            // All results of one parallel tool turn go into a single user message.
            Role::Tool => push(
                &mut messages,
                "user",
                vec![json!({ "type": "tool_result", "tool_use_id": m.tool_call_id, "content": m.text_content() })],
            ),
        }
    }
    let mut max_tokens = req.max_tokens.unwrap_or(DEFAULT_MAX_TOKENS);
    let mut body = json!({ "model": req.model, "messages": messages, "stream": true });
    if !req.system.is_empty() {
        body["system"] = json!([{ "type": "text", "text": req.system, "cache_control": { "type": "ephemeral" } }]);
    }
    match &req.effort {
        Some(EffortValue::Level(l)) => {
            body["thinking"] = json!({ "type": "adaptive", "display": "summarized" });
            body["output_config"] = json!({ "effort": l });
        }
        Some(EffortValue::Budget(n)) => {
            body["thinking"] = json!({ "type": "enabled", "budget_tokens": n });
            max_tokens = max_tokens.max(n + 4096);
        }
        Some(EffortValue::Toggle(false)) => {}
        Some(EffortValue::Toggle(true)) => body["thinking"] = json!({ "type": "adaptive", "display": "summarized" }),
        None if req.want_reasoning => body["thinking"] = json!({ "type": "adaptive", "display": "summarized" }),
        None => {}
    }
    body["max_tokens"] = json!(max_tokens);
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({ "name": t.name, "description": t.description, "input_schema": t.parameters, "eager_input_streaming": true }))
            .collect::<Vec<_>>());
    }
    body
}

#[derive(Default)]
pub(crate) struct StreamState {
    blocks: HashMap<u64, Value>,
    json_buf: HashMap<u64, String>,
    usage: Usage,
    stop_reason: Option<String>,
}

impl StreamState {
    pub fn map(&mut self, v: &Value) -> Vec<ProviderEvent> {
        let mut out = Vec::new();
        let idx = v["index"].as_u64().unwrap_or(0);
        match v["type"].as_str().unwrap_or_default() {
            "message_start" => {
                let u = &v["message"]["usage"];
                self.usage.input_tokens = u["input_tokens"].as_i64().unwrap_or(0)
                    + u["cache_read_input_tokens"].as_i64().unwrap_or(0)
                    + u["cache_creation_input_tokens"].as_i64().unwrap_or(0);
            }
            "content_block_start" => {
                let mut b = v["content_block"].clone();
                if b["type"] == "tool_use" {
                    b["input"] = json!({});
                    self.json_buf.insert(idx, String::new());
                }
                self.blocks.insert(idx, b);
            }
            "content_block_delta" => {
                let d = &v["delta"];
                let block = self.blocks.entry(idx).or_insert_with(|| json!({ "type": "text", "text": "" }));
                match d["type"].as_str().unwrap_or_default() {
                    "thinking_delta" => {
                        let t = d["thinking"].as_str().unwrap_or_default();
                        append(block, "thinking", t);
                        if !t.is_empty() {
                            out.push(ProviderEvent::ReasoningDelta(t.to_string()));
                        }
                    }
                    "signature_delta" => block["signature"] = d["signature"].clone(),
                    "text_delta" => {
                        let t = d["text"].as_str().unwrap_or_default();
                        append(block, "text", t);
                        if !t.is_empty() {
                            out.push(ProviderEvent::TextDelta(t.to_string()));
                        }
                    }
                    "input_json_delta" => {
                        self.json_buf.entry(idx).or_default().push_str(d["partial_json"].as_str().unwrap_or_default());
                    }
                    _ => {}
                }
            }
            "content_block_stop" => {
                if let Some(buf) = self.json_buf.remove(&idx)
                    && let Some(block) = self.blocks.get_mut(&idx)
                {
                    // Eager input streaming means the input is not validated server-side: guard the parse.
                    let input: Value = if buf.trim().is_empty() {
                        json!({})
                    } else {
                        serde_json::from_str(&buf).unwrap_or_else(|_| json!({ "_invalid_json": buf }))
                    };
                    block["input"] = input.clone();
                    out.push(ProviderEvent::ToolCall(ToolCall {
                        id: block["id"].as_str().unwrap_or_default().to_string(),
                        name: block["name"].as_str().unwrap_or_default().to_string(),
                        arguments: input,
                    }));
                }
            }
            "message_delta" => {
                if let Some(n) = v["usage"]["output_tokens"].as_i64() {
                    self.usage.output_tokens = n;
                }
                if let Some(s) = v["delta"]["stop_reason"].as_str() {
                    self.stop_reason = Some(s.to_string());
                }
            }
            "message_stop" => out.extend(self.finish()),
            "error" => {
                out.push(ProviderEvent::Error(v["error"]["message"].as_str().unwrap_or("Anthropic stream error").to_string()))
            }
            _ => {}
        }
        out
    }

    pub fn finish(&mut self) -> Vec<ProviderEvent> {
        let mut out = Vec::new();
        if self.stop_reason.as_deref() == Some("refusal") {
            out.push(ProviderEvent::TextDelta("\n\n_The model declined to continue with this request._".into()));
        }
        let mut idx: Vec<_> = self.blocks.keys().copied().collect();
        idx.sort();
        let raw: Vec<Value> = idx
            .into_iter()
            .filter_map(|i| self.blocks.remove(&i))
            .filter(|b| !(b["type"] == "text" && b["text"].as_str().is_some_and(|t| t.is_empty())))
            .collect();
        out.push(ProviderEvent::Usage(std::mem::take(&mut self.usage)));
        out.push(ProviderEvent::Done { stop_reason: self.stop_reason.take(), raw_assistant: Some(Value::Array(raw)) });
        out
    }
}

fn append(block: &mut Value, key: &str, t: &str) {
    let cur = block[key].as_str().unwrap_or_default().to_string();
    block[key] = json!(cur + t);
}

#[async_trait]
impl Provider for Anthropic {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Anthropic
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let resp = send_with_retry(self.authed(self.http.get(format!("{}/v1/models?limit=1000", self.base)))).await?;
        let v: Value = resp.json().await?;
        let mut out = Vec::new();
        for m in v["data"].as_array().cloned().unwrap_or_default() {
            let Some(id) = m["id"].as_str() else { continue };
            let mut info = catalog::info_from_catalog(id, m["display_name"].as_str());
            let caps = &m["capabilities"];
            if caps.is_object() {
                info.reasoning = caps["thinking"]["supported"].as_bool().unwrap_or(info.reasoning);
                info.reason = (!info.reasoning).then(|| catalog::NOT_REASONING.to_string());
                info.vision = caps["image_input"]["supported"].as_bool().unwrap_or(info.vision);
                if caps["effort"]["supported"].as_bool() == Some(true) {
                    let levels: Vec<String> = ["low", "medium", "high", "xhigh", "max"]
                        .into_iter()
                        .filter(|l| caps["effort"][*l]["supported"].as_bool() == Some(true))
                        .map(String::from)
                        .collect();
                    if !levels.is_empty() {
                        let default = if levels.iter().any(|l| l == "medium") { "medium" } else { &levels[0] }.to_string();
                        info.effort = EffortControl::Levels { levels, default };
                    }
                } else if caps["thinking"]["types"]["enabled"]["supported"].as_bool() == Some(true) {
                    info.effort = EffortControl::Budget { min: 1024, max: 32_000, default: 8_000 };
                }
            }
            info.context_window = m["max_input_tokens"].as_u64().map(|n| n as u32);
            out.push(info);
        }
        catalog::sort_models(&mut out);
        Ok(out)
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<EventStream> {
        let body = build_body(&req);
        let resp = send_with_retry(self.authed(self.http.post(format!("{}/v1/messages", self.base)).json(&body))).await?;
        let mut events = Box::pin(sse_stream(resp.bytes_stream()));
        Ok(Box::pin(async_stream::stream! {
            let mut st = StreamState::default();
            let mut done = false;
            while let Some(item) = events.next().await {
                match item {
                    Ok(ev) => {
                        let Ok(v) = serde_json::from_str::<Value>(&ev.data) else { continue };
                        for e in st.map(&v) {
                            match &e {
                                ProviderEvent::Done { .. } => done = true,
                                ProviderEvent::Error(_) => { yield e; return; }
                                _ => {}
                            }
                            yield e;
                        }
                    }
                    Err(e) => { yield ProviderEvent::Error(e); return; }
                }
            }
            if !done { for e in st.finish() { yield e; } }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::sse::SseDecoder;
    use crate::providers::ToolSpec;

    #[test]
    fn maps_thinking_text_and_tool_use_with_raw_replay() {
        let mut dec = SseDecoder::default();
        let mut st = StreamState::default();
        let evs: Vec<_> = dec
            .push(include_bytes!("../../tests/fixtures/anthropic_stream.sse"))
            .into_iter()
            .flat_map(|e| st.map(&serde_json::from_str(&e.data).unwrap()))
            .collect();
        let reasoning: String = evs.iter().filter_map(|e| if let ProviderEvent::ReasoningDelta(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(reasoning, "I should list the files.");
        assert!(evs.iter().any(|e| matches!(e, ProviderEvent::ToolCall(c) if c.name == "list_dir" && c.arguments == json!({"path": "."}))));
        assert!(evs.contains(&ProviderEvent::Usage(Usage { input_tokens: 110, output_tokens: 42 })));
        let Some(ProviderEvent::Done { raw_assistant: Some(Value::Array(raw)), stop_reason }) = evs.last() else { panic!("no done") };
        assert_eq!(stop_reason.as_deref(), Some("tool_use"));
        assert_eq!(raw[0]["type"], "thinking");
        assert_eq!(raw[0]["signature"], "sig123");
        assert_eq!(raw[2]["input"], json!({"path": "."}));
    }

    #[test]
    fn body_effort_and_tool_results() {
        let call = ToolCall { id: "tu_1".into(), name: "list_dir".into(), arguments: json!({}) };
        let mut assistant = ChatMessage::assistant("");
        assistant.raw = Some(json!([{ "type": "thinking", "thinking": "x", "signature": "s" }, { "type": "tool_use", "id": "tu_1", "name": "list_dir", "input": {} }]));
        let req = ChatRequest {
            model: "claude-opus-5-5".into(),
            system: "sys".into(),
            messages: vec![ChatMessage::user("hi"), assistant, ChatMessage::tool_result(&call, "a.rs"), ChatMessage::tool_result(&call, "b.rs")],
            tools: vec![ToolSpec { name: "list_dir".into(), description: "d".into(), parameters: json!({"type": "object"}) }],
            effort: Some(EffortValue::Level("high".into())),
            ..Default::default()
        };
        let b = build_body(&req);
        assert_eq!(b["thinking"]["type"], "adaptive");
        assert_eq!(b["output_config"]["effort"], "high");
        assert_eq!(b["messages"][1]["content"][0]["signature"], "s");
        assert_eq!(b["messages"][2]["content"].as_array().unwrap().len(), 2, "parallel tool results share one user turn");
        let budget = ChatRequest { model: "claude-haiku-4-5".into(), effort: Some(EffortValue::Budget(30_000)), ..Default::default() };
        let bb = build_body(&budget);
        assert_eq!(bb["thinking"]["budget_tokens"], 30_000);
        assert!(bb["max_tokens"].as_u64().unwrap() > 30_000);
    }
}
