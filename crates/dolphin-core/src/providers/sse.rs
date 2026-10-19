//! Incremental Server-Sent Events and NDJSON decoding over a reqwest byte stream.

use bytes::Bytes;
use futures::{Stream, StreamExt};

#[derive(Debug, Clone, PartialEq)]
pub struct SseEvent {
    pub event: Option<String>,
    pub data: String,
}

/// Feeds raw bytes and yields complete SSE events. Handles CRLF and events split across chunks.
#[derive(Default)]
pub struct SseDecoder {
    buf: String,
    event: Option<String>,
    data: Vec<String>,
}

impl SseDecoder {
    pub fn push(&mut self, chunk: &[u8]) -> Vec<SseEvent> {
        self.buf.push_str(&String::from_utf8_lossy(chunk));
        let mut out = Vec::new();
        while let Some(pos) = self.buf.find('\n') {
            let line: String = self.buf.drain(..=pos).collect();
            let line = line.trim_end_matches(['\n', '\r']);
            if line.is_empty() {
                if !self.data.is_empty() {
                    out.push(SseEvent { event: self.event.take(), data: self.data.join("\n") });
                    self.data.clear();
                }
                self.event = None;
            } else if let Some(v) = line.strip_prefix("data:") {
                self.data.push(v.strip_prefix(' ').unwrap_or(v).to_string());
            } else if let Some(v) = line.strip_prefix("event:") {
                self.event = Some(v.trim().to_string());
            }
            // Comments (":") and other fields (id, retry) are ignored.
        }
        out
    }

    pub fn finish(&mut self) -> Option<SseEvent> {
        if !self.buf.trim().is_empty() {
            let rest = std::mem::take(&mut self.buf);
            let _ = self.push(format!("{rest}\n").as_bytes());
        }
        if self.data.is_empty() {
            return None;
        }
        Some(SseEvent { event: self.event.take(), data: std::mem::take(&mut self.data).join("\n") })
    }
}

pub fn sse_stream(
    bytes: impl Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
) -> impl Stream<Item = Result<SseEvent, String>> + Send {
    async_stream::stream! {
        let mut dec = SseDecoder::default();
        let mut bytes = Box::pin(bytes);
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(c) => for ev in dec.push(&c) { yield Ok(ev); },
                Err(e) => { yield Err(format!("stream interrupted: {e}")); return; }
            }
        }
        if let Some(ev) = dec.finish() { yield Ok(ev); }
    }
}

/// Newline-delimited JSON (Ollama).
pub fn ndjson_stream(
    bytes: impl Stream<Item = reqwest::Result<Bytes>> + Send + 'static,
) -> impl Stream<Item = Result<serde_json::Value, String>> + Send {
    async_stream::stream! {
        let mut buf = String::new();
        let mut bytes = Box::pin(bytes);
        while let Some(chunk) = bytes.next().await {
            match chunk {
                Ok(c) => {
                    buf.push_str(&String::from_utf8_lossy(&c));
                    while let Some(pos) = buf.find('\n') {
                        let line: String = buf.drain(..=pos).collect();
                        let line = line.trim();
                        if line.is_empty() { continue; }
                        match serde_json::from_str(line) {
                            Ok(v) => yield Ok(v),
                            Err(e) => yield Err(format!("bad stream line: {e}")),
                        }
                    }
                }
                Err(e) => { yield Err(format!("stream interrupted: {e}")); return; }
            }
        }
        let rest = buf.trim();
        if !rest.is_empty() {
            if let Ok(v) = serde_json::from_str(rest) { yield Ok(v); }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_split_events() {
        let mut d = SseDecoder::default();
        assert!(d.push(b"event: ping\r\ndata: {\"a\"").is_empty());
        let evs = d.push(b":1}\r\n\r\ndata: [DONE]\n\n");
        assert_eq!(evs.len(), 2);
        assert_eq!(evs[0], SseEvent { event: Some("ping".into()), data: "{\"a\":1}".into() });
        assert_eq!(evs[1].data, "[DONE]");
    }

    #[test]
    fn multi_line_data_joined() {
        let mut d = SseDecoder::default();
        let evs = d.push(b"data: a\ndata: b\n\n");
        assert_eq!(evs[0].data, "a\nb");
    }
}
