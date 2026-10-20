//! Google Gemini API (`streamGenerateContent?alt=sse`) with thought summaries.
//! Gemini 2.5 takes `thinkingBudget`; Gemini 3+ takes `thinkingLevel`. Thought signatures on
//! returned parts are replayed unchanged through tool loops via `raw_assistant`.

use async_trait::async_trait;
use futures::StreamExt;
use serde_json::{Value, json};

use super::sse::sse_stream;
use super::{ChatRequest, EventStream, Part, Provider, ProviderEvent, Role, ToolCall, Usage, http_client, send_with_retry};
use crate::catalog;
use crate::error::Result;
use crate::model::{EffortControl, EffortValue, ModelInfo, ProviderKind};

pub struct Gemini {
    base: String,
    key: Option<String>,
    http: reqwest::Client,
}

impl Gemini {
    pub fn new(base: String, key: Option<String>) -> Self {
        Self { base, key, http: http_client() }
    }

    fn authed(&self, rb: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        rb.header("x-goog-api-key", self.key.clone().unwrap_or_default())
    }
}

fn model_path(model: &str) -> String {
    if model.starts_with("models/") { model.to_string() } else { format!("models/{model}") }
}

pub(crate) fn build_body(req: &ChatRequest) -> Value {
    let mut contents: Vec<Value> = Vec::new();
    let mut push = |role: &str, mut parts: Vec<Value>| {
        if parts.is_empty() {
            return;
        }
        if let Some(last) = contents.last_mut()
            && last["role"] == role
        {
            last["parts"].as_array_mut().unwrap().append(&mut parts);
            return;
        }
        contents.push(json!({ "role": role, "parts": parts }));
    };
    for m in &req.messages {
        match m.role {
            Role::User => push(
                "user",
                m.parts
                    .iter()
                    .filter_map(|p| match p {
                        Part::Text(t) if t.trim().is_empty() => None,
                        Part::Text(t) => Some(json!({ "text": t })),
                        Part::Image { mime, data_b64 } => Some(json!({ "inline_data": { "mime_type": mime, "data": data_b64 } })),
                    })
                    .collect(),
            ),
            Role::Assistant => {
                let parts = if let Some(Value::Array(raw)) = &m.raw {
                    raw.clone()
                } else {
                    let mut p = Vec::new();
                    let t = m.text_content();
                    if !t.trim().is_empty() {
                        p.push(json!({ "text": t }));
                    }
                    p.extend(m.tool_calls.iter().map(|c| json!({ "functionCall": { "name": c.name, "args": c.arguments } })));
                    p
                };
                push("model", parts);
            }
            Role::Tool => push(
                "user",
                vec![json!({ "functionResponse": { "name": m.tool_name, "response": { "result": m.text_content() } } })],
            ),
        }
    }
    let mut gen_cfg = json!({});
    match &req.effort {
        Some(EffortValue::Budget(n)) => gen_cfg["thinkingConfig"] = json!({ "includeThoughts": true, "thinkingBudget": n }),
        Some(EffortValue::Level(l)) => gen_cfg["thinkingConfig"] = json!({ "includeThoughts": true, "thinkingLevel": l }),
        Some(EffortValue::Toggle(false)) => {}
        _ if req.want_reasoning || req.effort.is_some() => gen_cfg["thinkingConfig"] = json!({ "includeThoughts": true }),
        _ => {}
    }
    if let Some(n) = req.max_tokens {
        gen_cfg["maxOutputTokens"] = json!(n);
    }
    if req.json_mode {
        gen_cfg["responseMimeType"] = json!("application/json");
    }
    let mut body = json!({ "contents": contents, "generationConfig": gen_cfg });
    if !req.system.is_empty() {
        body["systemInstruction"] = json!({ "parts": [{ "text": req.system }] });
    }
    if !req.tools.is_empty() {
        body["tools"] = json!([{ "functionDeclarations": req.tools.iter().map(|t| json!({
            "name": t.name, "description": t.description, "parameters": t.parameters
        })).collect::<Vec<_>>() }]);
    }
    body
}

#[derive(Default)]
pub(crate) struct StreamState {
    raw: Vec<Value>,
    usage: Usage,
    cached: i64,
    finish: Option<String>,
    n_calls: usize,
}

impl StreamState {
    pub fn map(&mut self, v: &Value) -> Vec<ProviderEvent> {
        let mut out = Vec::new();
        if let Some(e) = v.get("error") {
            out.push(ProviderEvent::Error(e["message"].as_str().unwrap_or("Gemini error").to_string()));
            return out;
        }
        if let Some(u) = v.get("usageMetadata") {
            self.usage.input_tokens = u["promptTokenCount"].as_i64().unwrap_or(self.usage.input_tokens);
            self.cached = u["cachedContentTokenCount"].as_i64().unwrap_or(self.cached);
            let out_tokens = u["candidatesTokenCount"].as_i64().unwrap_or(0) + u["thoughtsTokenCount"].as_i64().unwrap_or(0);
            if out_tokens > 0 {
                self.usage.output_tokens = out_tokens;
            }
        }
        let Some(cand) = v["candidates"].get(0) else { return out };
        for p in cand["content"]["parts"].as_array().cloned().unwrap_or_default() {
            if p["thought"].as_bool() == Some(true) {
                if let Some(t) = p["text"].as_str().filter(|s| !s.is_empty()) {
                    out.push(ProviderEvent::ReasoningDelta(t.to_string()));
                }
                continue;
            }
            if let Some(t) = p["text"].as_str() {
                if !t.is_empty() {
                    out.push(ProviderEvent::TextDelta(t.to_string()));
                }
                // Merge plain text parts; parts carrying a signature are kept as-is.
                if p.get("thoughtSignature").is_none()
                    && let Some(last) = self.raw.last_mut()
                    && last.get("text").is_some()
                    && last.get("thoughtSignature").is_none()
                {
                    let merged = format!("{}{}", last["text"].as_str().unwrap_or_default(), t);
                    last["text"] = json!(merged);
                    continue;
                }
                self.raw.push(p.clone());
                continue;
            }
            if let Some(fc) = p.get("functionCall") {
                self.n_calls += 1;
                out.push(ProviderEvent::ToolCall(ToolCall {
                    id: fc["id"].as_str().map(String::from).unwrap_or_else(|| format!("call_{}", self.n_calls)),
                    name: fc["name"].as_str().unwrap_or_default().to_string(),
                    arguments: fc["args"].clone(),
                }));
                self.raw.push(p.clone());
            }
        }
        if let Some(f) = cand["finishReason"].as_str() {
            self.finish = Some(f.to_string());
        }
        out
    }

    pub fn finish(&mut self) -> Vec<ProviderEvent> {
        let mut out = vec![ProviderEvent::Usage(std::mem::take(&mut self.usage))];
        if self.cached > 0 {
            out.push(ProviderEvent::CacheUsage { read: self.cached, written: 0 });
        }
        out.push(ProviderEvent::Done { stop_reason: self.finish.take(), raw_assistant: Some(Value::Array(std::mem::take(&mut self.raw))) });
        out
    }
}

#[async_trait]
impl Provider for Gemini {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Gemini
    }

    async fn list_models(&self) -> Result<Vec<ModelInfo>> {
        let resp = send_with_retry(self.authed(self.http.get(format!("{}/v1beta/models?pageSize=1000", self.base)))).await?;
        let v: Value = resp.json().await?;
        let mut out = Vec::new();
        for m in v["models"].as_array().cloned().unwrap_or_default() {
            let methods: Vec<&str> = m["supportedGenerationMethods"].as_array().map(|a| a.iter().filter_map(|x| x.as_str()).collect()).unwrap_or_default();
            if !methods.contains(&"generateContent") {
                continue;
            }
            let id = m["name"].as_str().unwrap_or_default().trim_start_matches("models/").to_string();
            let mut info = catalog::info_from_catalog(&id, m["displayName"].as_str());
            if m["thinking"].as_bool() == Some(true) {
                info.reasoning = true;
                info.reason = None;
                if matches!(info.effort, EffortControl::None) {
                    info.effort = EffortControl::Budget { min: 128, max: 32_768, default: 8_192 };
                }
            }
            info.image_output = id.contains("image");
            if info.image_output {
                info.reasoning = false;
                info.reason = Some(catalog::NOT_REASONING.into());
            }
            info.vision = true;
            info.context_window = m["inputTokenLimit"].as_u64().map(|n| n as u32);
            out.push(info);
        }
        catalog::sort_models(&mut out);
        Ok(out)
    }

    async fn stream_chat(&self, req: ChatRequest) -> Result<EventStream> {
        let body = build_body(&req);
        let url = format!("{}/v1beta/{}:streamGenerateContent?alt=sse", self.base, model_path(&req.model));
        let resp = send_with_retry(self.authed(self.http.post(url).json(&body))).await?;
        let mut events = Box::pin(sse_stream(resp.bytes_stream()));
        Ok(Box::pin(async_stream::stream! {
            let mut st = StreamState::default();
            while let Some(item) = events.next().await {
                match item {
                    Ok(ev) => {
                        let Ok(v) = serde_json::from_str::<Value>(&ev.data) else { continue };
                        for e in st.map(&v) {
                            let stop = matches!(e, ProviderEvent::Error(_));
                            yield e;
                            if stop { return; }
                        }
                    }
                    Err(e) => { yield ProviderEvent::Error(e); return; }
                }
            }
            for e in st.finish() { yield e; }
        }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn maps_thoughts_text_and_calls() {
        let mut st = StreamState::default();
        let mut evs = Vec::new();
        for line in include_str!("../../tests/fixtures/gemini_stream.jsonl").lines() {
            evs.extend(st.map(&serde_json::from_str(line).unwrap()));
        }
        evs.extend(st.finish());
        let reasoning: String = evs.iter().filter_map(|e| if let ProviderEvent::ReasoningDelta(t) = e { Some(t.as_str()) } else { None }).collect();
        let text: String = evs.iter().filter_map(|e| if let ProviderEvent::TextDelta(t) = e { Some(t.as_str()) } else { None }).collect();
        assert_eq!(reasoning, "**Planning** I need the weather.");
        assert_eq!(text, "Checking the weather.");
        assert!(evs.iter().any(|e| matches!(e, ProviderEvent::ToolCall(c) if c.name == "get_weather")));
        assert!(evs.contains(&ProviderEvent::Usage(Usage { input_tokens: 30, output_tokens: 25 })));
        let Some(ProviderEvent::Done { raw_assistant: Some(Value::Array(raw)), .. }) = evs.last() else { panic!() };
        assert_eq!(raw.len(), 2);
        assert_eq!(raw[1]["thoughtSignature"], "abc");
    }

    #[test]
    fn thinking_config_by_effort() {
        let r = ChatRequest { model: "gemini-2.5-pro".into(), effort: Some(EffortValue::Budget(2048)), ..Default::default() };
        assert_eq!(build_body(&r)["generationConfig"]["thinkingConfig"]["thinkingBudget"], 2048);
        let r = ChatRequest { model: "gemini-3-pro".into(), effort: Some(EffortValue::Level("low".into())), ..Default::default() };
        assert_eq!(build_body(&r)["generationConfig"]["thinkingConfig"]["thinkingLevel"], "low");
    }
}
