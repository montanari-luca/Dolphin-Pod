//! Ollama native API: `/api/chat` (NDJSON stream, `think` parameter) and `/api/show` capabilities.

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};

use super::sse::ndjson_stream;
use super::{
    ChatRequest, EventStream, Part, Provider, ProviderEvent, Role, ToolCall, Usage, http_client, send_with_retry,
};
use crate::catalog;
use crate::error::{CoreError, Result};
use crate::model::{EffortControl, EffortValue, ModelInfo, ProviderKind};

pub struct Ollama {
    base: String,
    http: reqwest::Client,
}

impl Ollama {
    pub fn new(base: String) -> Self {
        Self { base, http: http_client() }
    }

    async fn show(&self, model: &str) -> Option<Value> {
        let r = self.http.post(format!("{}/api/show", self.base)).json(&json!({ "model": model })).send().await.ok()?;
        r.json::<Value>().await.ok()
    }
}

pub(crate) fn build_body(req: &ChatRequest) -> Value {
    let mut messages = Vec::new();
    if !req.system.is_empty() {
        messages.push(json!({ "role": "system", "content": req.system }));
    }
    for m in &req.messages {
        let role = match m.role {
            Role::User => "user",
            Role::Assistant => "assistant",
            Role::Tool => "tool",
        };
        let images: Vec<&str> = m
            .parts
            .iter()
            .filter_map(|p| if let Part::Image { data_b64, .. } = p { Some(data_b64.as_str()) } else { None })
            .collect();
        let mut msg = json!({ "role": role, "content": m.text_content() });
        if !images.is_empty() {
            msg["images"] = json!(images);
        }
        if !m.tool_calls.is_empty() {
            msg["tool_calls"] = json!(m
                .tool_calls
                .iter()
                .map(|c| json!({ "function": { "name": c.name, "arguments": c.arguments } }))
                .collect::<Vec<_>>());
        }
        if let Some(name) = &m.tool_name {
            msg["tool_name"] = json!(name);
        }
        messages.push(msg);
    }
    let think = match &req.effort {
        Some(EffortValue::Level(l)) => json!(l),
        Some(EffortValue::Toggle(b)) => json!(b),
        _ => json!(true),
    };
    let mut body = json!({ "model": req.model, "messages": messages, "stream": true, "think": think });
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({ "type": "function", "function": { "name": t.name, "description": t.description, "parameters": t.parameters } }))
            .collect::<Vec<_>>());
    }
    if req.json_mode {
        body["format"] = json!("json");
    }
    if let Some(n) = req.max_tokens {
        body["options"] = json!({ "num_predict": n });
    }
    body
}

/// Map one NDJSON chunk to events. `n_calls` numbers tool calls since Ollama gives no ids.
pub(crate) fn map_chunk(v: &Value, n_calls: &mut usize) -> Vec<ProviderEvent> {
    let mut out = Vec::new();
    if let Some(err) = v.get("error").and_then(|e| e.as_str()) {
        out.push(ProviderEvent::Error(err.to_string()));
        return out;
    }
    let msg = &v["message"];
    if let Some(t) = msg["thinking"].as_str().filter(|s| !s.is_empty()) {
        out.push(ProviderEvent::ReasoningDelta(t.to_string()));
    }
    if let Some(t) = msg["content"].as_str().filter(|s| !s.is_empty()) {
        out.push(ProviderEvent::TextDelta(t.to_string()));
    }
    if let Some(calls) = msg["tool_calls"].as_array() {
        for c in calls {
            *n_calls += 1;
            let args = match &c["function"]["arguments"] {
                Value::String(s) => serde_json::from_str(s).unwrap_or(json!({})),
                other => other.clone(),
            };
            out.push(ProviderEvent::ToolCall(ToolCall {
                id: c["id"].as_str().map(String::from).unwrap_or_else(|| format!("call_{n_calls}")),
                name: c["function"]["name"].as_str().unwrap_or_default().to_string(),
                arguments: args,
            }));
        }
    }
    if v["done"].as_bool() == Some(true) {
        out.push(ProviderEvent::Usage(Usage {
            input_tokens: v["prompt_eval_count"].as_i64().unwrap_or(0),
            output_tokens: v["eval_count"].as_i64().unwrap_or(0),
        }));
        out.push(ProviderEvent::Done { stop_reason: v["done_reason"].as_str().map(String::from), raw_assistant: None });
    }
    out
}

#[async_trait]
impl Provider for Ollama {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let resp = send_with_retry(self.http.get(format!("{}/api/tags", self.base))).await?;
        let tags: Value = resp.json().await?;
        let names: Vec<String> = tags["models"]
            .as_array()
            .map(|a| a.iter().filter_map(|m| m["name"].as_str().map(String::from)).collect())
            .unwrap_or_default();
        let shows = futures::stream::iter(names.into_iter().map(|n| async move {
            let show = self.show(&n).await;
            (n, show)
        }))
        .buffer_unordered(6)
        .collect::<Vec<_>>()
        .await;

        let mut out = Vec::new();
        for (name, show) in shows {
            let mut info = catalog::info_from_catalog(&name, None);
            if let Some(show) = show {
                let caps: Vec<&str> = show["capabilities"].as_array().map(|a| a.iter().filter_map(|c| c.as_str()).collect()).unwrap_or_default();
                if !caps.is_empty() {
                    info.reasoning = caps.contains(&"thinking");
                    info.reason = (!info.reasoning).then(|| catalog::NOT_REASONING.to_string());
                    info.vision = caps.contains(&"vision");
                    info.tools = caps.contains(&"tools");
                    info.image_output = false;
                    if info.reasoning && !matches!(info.effort, EffortControl::Levels { .. }) {
                        info.effort = EffortControl::Toggle;
                    }
                }
                if let Some(mi) = show["model_info"].as_object() {
                    info.context_window = mi.iter().find(|(k, _)| k.ends_with(".context_length")).and_then(|(_, v)| v.as_u64()).map(|n| n as u32);
                }
            }
            out.push(info);
        }
        catalog::sort_models(&mut out);
        Ok(out)
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<EventStream> {
        let body = build_body(&req);
        let resp = send_with_retry(self.http.post(format!("{}/api/chat", self.base)).json(&body))
            .await
            .map_err(|e| match e {
                CoreError::Provider(m) if m.contains("could not connect") => {
                    CoreError::provider(format!("Ollama is not running at {} ({m})", self.base))
                }
                other => other,
            })?;
        let mut lines = Box::pin(ndjson_stream(resp.bytes_stream()));
        Ok(Box::pin(async_stream::stream! {
            let mut n = 0usize;
            let mut done = false;
            while let Some(item) = lines.next().await {
                match item {
                    Ok(v) => for ev in map_chunk(&v, &mut n) {
                        if matches!(ev, ProviderEvent::Done { .. }) { done = true; }
                        yield ev;
                    },
                    Err(e) => { yield ProviderEvent::Error(e); return; }
                }
            }
            if !done { yield ProviderEvent::Done { stop_reason: None, raw_assistant: None }; }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{ChatMessage, ToolSpec};

    #[test]
    fn maps_recorded_stream() {
        let fixture = include_str!("../../tests/fixtures/ollama_stream.ndjson");
        let mut n = 0;
        let evs: Vec<_> = fixture.lines().flat_map(|l| map_chunk(&serde_json::from_str(l).unwrap(), &mut n)).collect();
        let reasoning: String = evs.iter().filter_map(|e| if let ProviderEvent::ReasoningDelta(t) = e { Some(t.as_str()) } else { None }).collect();
        let text: String = evs.iter().filter_map(|e| if let ProviderEvent::TextDelta(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(reasoning, "The user greets me. Reply briefly.");
        assert_eq!(text, "Hello! How can I help?");
        assert!(evs.contains(&ProviderEvent::Usage(Usage { input_tokens: 12, output_tokens: 20 })));
        assert!(matches!(evs.last(), Some(ProviderEvent::Done { .. })));
    }

    #[test]
    fn tool_call_chunk() {
        let v = json!({"message": {"role": "assistant", "content": "", "tool_calls": [{"function": {"name": "read_file", "arguments": {"path": "a.rs"}}}]}, "done": false});
        let mut n = 0;
        let evs = map_chunk(&v, &mut n);
        assert_eq!(evs, vec![ProviderEvent::ToolCall(ToolCall { id: "call_1".into(), name: "read_file".into(), arguments: json!({"path": "a.rs"}) })]);
    }

    #[test]
    fn body_has_think_and_tools() {
        let req = ChatRequest {
            model: "gpt-oss:20b".into(),
            system: "sys".into(),
            messages: vec![ChatMessage::user("hi")],
            tools: vec![ToolSpec { name: "t".into(), description: "d".into(), parameters: json!({"type": "object"}) }],
            effort: Some(EffortValue::Level("high".into())),
            ..Default::default()
        };
        let b = build_body(&req);
        assert_eq!(b["think"], "high");
        assert_eq!(b["messages"][0]["role"], "system");
        assert_eq!(b["tools"][0]["function"]["name"], "t");
    }
}
