//! OpenAI Responses API. Streams reasoning summaries and replays encrypted reasoning items
//! through tool loops (`store: false` + `include: ["reasoning.encrypted_content"]`).

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};

use super::sse::sse_stream;
use super::{
    ChatRequest, EventStream, Part, Provider, ProviderEvent, Role, ToolCall, Usage, http_client, send_with_retry,
};
use crate::catalog;
use crate::error::{CoreError, Result};
use crate::model::{EffortValue, ModelInfo, ProviderKind};

pub struct OpenAiResponses {
    base: String,
    key: Option<String>,
    http: reqwest::Client,
}

impl OpenAiResponses {
    pub fn new(base: String, key: Option<String>) -> Self {
        Self { base, key, http: http_client() }
    }

    fn authed(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.bearer_auth(self.key.clone().unwrap_or_default())
    }
}

pub(crate) fn build_body(req: &ChatRequest, summaries: bool) -> Value {
    let mut input: Vec<Value> = Vec::new();
    for m in &req.messages {
        match m.role {
            Role::User => input.push(json!({
                "role": "user",
                "content": m.parts.iter().map(|p| match p {
                    Part::Text(t) => json!({ "type": "input_text", "text": t }),
                    Part::Image { mime, data_b64 } => json!({ "type": "input_image", "image_url": format!("data:{mime};base64,{data_b64}") }),
                }).collect::<Vec<_>>(),
            })),
            Role::Assistant => {
                if let Some(Value::Array(raw)) = &m.raw {
                    input.extend(raw.iter().cloned());
                    continue;
                }
                let text = m.text_content();
                if !text.trim().is_empty() {
                    input.push(json!({ "role": "assistant", "content": text }));
                }
                for c in &m.tool_calls {
                    input.push(json!({ "type": "function_call", "call_id": c.id, "name": c.name, "arguments": c.arguments.to_string() }));
                }
            }
            Role::Tool => input.push(json!({ "type": "function_call_output", "call_id": m.tool_call_id, "output": m.text_content() })),
        }
    }
    let mut body = json!({ "model": req.model, "input": input, "stream": true, "store": false });
    if !req.system.is_empty() {
        body["instructions"] = json!(req.system);
    }
    let effort = match &req.effort {
        Some(EffortValue::Level(l)) => Some(l.clone()),
        _ => None,
    };
    if effort.is_some() || req.want_reasoning {
        let mut r = json!({});
        if let Some(e) = effort {
            r["effort"] = json!(e);
        }
        if summaries {
            r["summary"] = json!("auto");
        }
        body["reasoning"] = r;
        body["include"] = json!(["reasoning.encrypted_content"]);
    }
    if !req.tools.is_empty() {
        body["tools"] = json!(req
            .tools
            .iter()
            .map(|t| json!({ "type": "function", "name": t.name, "description": t.description, "parameters": t.parameters }))
            .collect::<Vec<_>>());
    }
    if let Some(n) = req.max_tokens {
        body["max_output_tokens"] = json!(n);
    }
    if req.json_mode {
        body["text"] = json!({ "format": { "type": "json_object" } });
    }
    body
}

#[derive(Default)]
pub(crate) struct StreamState {
    summary_parts: usize,
}

impl StreamState {
    pub fn map(&mut self, v: &Value) -> Vec<ProviderEvent> {
        let mut out = Vec::new();
        match v["type"].as_str().unwrap_or_default() {
            "response.output_text.delta" => {
                if let Some(d) = v["delta"].as_str() {
                    out.push(ProviderEvent::TextDelta(d.to_string()));
                }
            }
            "response.reasoning_summary_part.added" => {
                if self.summary_parts > 0 {
                    out.push(ProviderEvent::ReasoningDelta("\n\n".into()));
                }
                self.summary_parts += 1;
            }
            "response.reasoning_summary_text.delta" => {
                if let Some(d) = v["delta"].as_str() {
                    out.push(ProviderEvent::ReasoningDelta(d.to_string()));
                }
            }
            "response.output_item.done" => {
                let item = &v["item"];
                if item["type"] == "function_call" {
                    let args = item["arguments"].as_str().unwrap_or("{}");
                    out.push(ProviderEvent::ToolCall(ToolCall {
                        id: item["call_id"].as_str().unwrap_or_default().to_string(),
                        name: item["name"].as_str().unwrap_or_default().to_string(),
                        arguments: serde_json::from_str(args).unwrap_or_else(|_| json!({ "_invalid_json": args })),
                    }));
                }
            }
            "response.completed" | "response.incomplete" => {
                let r = &v["response"];
                out.push(ProviderEvent::Usage(Usage {
                    input_tokens: r["usage"]["input_tokens"].as_i64().unwrap_or(0),
                    output_tokens: r["usage"]["output_tokens"].as_i64().unwrap_or(0),
                }));
                let stop = r["incomplete_details"]["reason"].as_str().or(r["status"].as_str()).map(String::from);
                out.push(ProviderEvent::Done { stop_reason: stop, raw_assistant: Some(r["output"].clone()) });
            }
            "response.failed" => out.push(ProviderEvent::Error(
                v["response"]["error"]["message"].as_str().unwrap_or("OpenAI response failed").to_string(),
            )),
            "error" => out.push(ProviderEvent::Error(
                v["message"].as_str().or(v["error"]["message"].as_str()).unwrap_or("OpenAI stream error").to_string(),
            )),
            _ => {}
        }
        out
    }
}

const NON_CHAT: &[&str] =
    &["embedding", "tts", "whisper", "moderation", "transcribe", "realtime", "audio", "search", "davinci", "babbage"];

#[async_trait]
impl Provider for OpenAiResponses {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Openai
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let resp = send_with_retry(self.authed(self.http.get(format!("{}/models", self.base)))).await?;
        let v: Value = resp.json().await?;
        let mut out: Vec<ModelInfo> = v["data"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .filter_map(|m| m["id"].as_str())
            .filter(|id| !NON_CHAT.iter().any(|n| id.contains(n)))
            .map(|id| catalog::info_from_catalog(id, None))
            .collect();
        catalog::sort_models(&mut out);
        Ok(out)
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<EventStream> {
        let url = format!("{}/responses", self.base);
        let resp = match send_with_retry(self.authed(self.http.post(&url).json(&build_body(&req, true)))).await {
            // Reasoning summaries need a verified organization; fall back to no summaries.
            Err(CoreError::Provider(m)) if m.contains("summar") => {
                send_with_retry(self.authed(self.http.post(&url).json(&build_body(&req, false)))).await?
            }
            other => other?,
        };
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
            if !done { yield ProviderEvent::Done { stop_reason: None, raw_assistant: None }; }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::sse::SseDecoder;
    use crate::providers::ChatMessage;

    #[test]
    fn maps_summary_text_and_function_call() {
        let mut dec = SseDecoder::default();
        let mut st = StreamState::default();
        let evs: Vec<_> = dec
            .push(include_bytes!("../../tests/fixtures/openai_responses.sse"))
            .into_iter()
            .flat_map(|e| st.map(&serde_json::from_str(&e.data).unwrap()))
            .collect();
        let reasoning: String = evs.iter().filter_map(|e| if let ProviderEvent::ReasoningDelta(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(reasoning, "Considering options.\n\nPicking one.");
        assert!(evs.iter().any(|e| matches!(e, ProviderEvent::ToolCall(c) if c.id == "call_9" && c.arguments == json!({"q": "rust"}))));
        let Some(ProviderEvent::Done { raw_assistant: Some(raw), .. }) = evs.last() else { panic!() };
        assert_eq!(raw[0]["type"], "reasoning");
    }

    #[test]
    fn replays_raw_output_items() {
        let mut a = ChatMessage::assistant("");
        a.raw = Some(json!([{ "type": "reasoning", "encrypted_content": "enc" }, { "type": "function_call", "call_id": "c", "name": "n", "arguments": "{}" }]));
        let req = ChatRequest { model: "gpt-5".into(), messages: vec![ChatMessage::user("hi"), a], effort: Some(EffortValue::Level("low".into())), ..Default::default() };
        let b = build_body(&req, true);
        assert_eq!(b["input"][1]["encrypted_content"], "enc");
        assert_eq!(b["reasoning"]["effort"], "low");
        assert_eq!(b["reasoning"]["summary"], "auto");
        assert!(build_body(&req, false)["reasoning"].get("summary").is_none());
    }
}
