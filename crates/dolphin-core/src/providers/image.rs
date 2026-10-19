//! Image generation for the image specialist. Her reasoning model writes the prompt; the separately
//! configured image model renders it here.

use base64::Engine;
use serde_json::{Value, json};

use super::{http_client, send_with_retry, trim_base};
use crate::error::{CoreError, Result};
use crate::model::{ProviderConfig, ProviderKind};

pub struct GeneratedImage {
    pub bytes: Vec<u8>,
    pub mime: String,
    /// Text the model returned alongside the image, if any.
    pub caption: Option<String>,
}

fn decode_data_url(url: &str) -> Option<(Vec<u8>, String)> {
    let rest = url.strip_prefix("data:")?;
    let (meta, data) = rest.split_once(',')?;
    let mime = meta.trim_end_matches(";base64").to_string();
    let bytes = base64::engine::general_purpose::STANDARD.decode(data).ok()?;
    Some((bytes, mime))
}

fn b64(data: &str) -> Result<Vec<u8>> {
    base64::engine::general_purpose::STANDARD.decode(data).map_err(|e| CoreError::provider(format!("bad image data: {e}")))
}

pub async fn generate(cfg: &ProviderConfig, key: Option<String>, model: &str, prompt: &str) -> Result<GeneratedImage> {
    let http = http_client();
    let base = trim_base(&cfg.base_url);
    let key = key.unwrap_or_default();
    match cfg.kind {
        ProviderKind::Gemini => {
            if model.contains("imagen") {
                let url = format!("{base}/v1beta/models/{model}:predict");
                let body = json!({ "instances": [{ "prompt": prompt }], "parameters": { "sampleCount": 1 } });
                let v: Value = send_with_retry(http.post(url).header("x-goog-api-key", &key).json(&body)).await?.json().await?;
                let data = v["predictions"][0]["bytesBase64Encoded"].as_str().ok_or_else(|| CoreError::provider("no image returned"))?;
                let mime = v["predictions"][0]["mimeType"].as_str().unwrap_or("image/png").to_string();
                return Ok(GeneratedImage { bytes: b64(data)?, mime, caption: None });
            }
            let url = format!("{base}/v1beta/models/{model}:generateContent");
            let body = json!({
                "contents": [{ "role": "user", "parts": [{ "text": prompt }] }],
                "generationConfig": { "responseModalities": ["TEXT", "IMAGE"] }
            });
            let v: Value = send_with_retry(http.post(url).header("x-goog-api-key", &key).json(&body)).await?.json().await?;
            let parts = v["candidates"][0]["content"]["parts"].as_array().cloned().unwrap_or_default();
            let caption: String = parts.iter().filter_map(|p| p["text"].as_str()).collect::<Vec<_>>().join("\n");
            let img = parts
                .iter()
                .find_map(|p| p.get("inlineData").or_else(|| p.get("inline_data")))
                .ok_or_else(|| CoreError::provider("the model returned no image"))?;
            let mime = img["mimeType"].as_str().or(img["mime_type"].as_str()).unwrap_or("image/png").to_string();
            Ok(GeneratedImage { bytes: b64(img["data"].as_str().unwrap_or_default())?, mime, caption: (!caption.is_empty()).then_some(caption) })
        }
        ProviderKind::Openrouter => {
            let body = json!({
                "model": model,
                "messages": [{ "role": "user", "content": prompt }],
                "modalities": ["image", "text"],
            });
            let rb = http
                .post(format!("{base}/chat/completions"))
                .bearer_auth(&key)
                .header("HTTP-Referer", "https://mintflow.dev")
                .header("X-Title", "Dolphin Pod")
                .json(&body);
            let v: Value = send_with_retry(rb).await?.json().await?;
            let msg = &v["choices"][0]["message"];
            let url = msg["images"][0]["image_url"]["url"].as_str().ok_or_else(|| CoreError::provider("the model returned no image"))?;
            let (bytes, mime) = decode_data_url(url).ok_or_else(|| CoreError::provider("unexpected image format"))?;
            let caption = msg["content"].as_str().filter(|s| !s.trim().is_empty()).map(String::from);
            Ok(GeneratedImage { bytes, mime, caption })
        }
        ProviderKind::Ollama | ProviderKind::Anthropic | ProviderKind::Deepseek | ProviderKind::Mistral | ProviderKind::Groq => {
            Err(CoreError::provider(format!("{} does not offer image generation; pick another provider for the image model", cfg.kind.label())))
        }
        _ => {
            // OpenAI Images API shape (OpenAI, Azure, xAI, Together, Fireworks, LM Studio/custom servers that implement it).
            let mut body = json!({ "model": model, "prompt": prompt, "n": 1 });
            if !model.starts_with("gpt-image") {
                body["response_format"] = json!("b64_json");
            }
            let rb = http.post(format!("{base}/images/generations")).json(&body);
            let rb = if cfg.kind == ProviderKind::AzureOpenai { rb.header("api-key", &key) } else { rb.bearer_auth(&key) };
            let v: Value = send_with_retry(rb).await?.json().await?;
            let d = &v["data"][0];
            if let Some(data) = d["b64_json"].as_str() {
                return Ok(GeneratedImage { bytes: b64(data)?, mime: "image/png".into(), caption: d["revised_prompt"].as_str().map(String::from) });
            }
            if let Some(url) = d["url"].as_str() {
                let r = send_with_retry(http.get(url)).await?;
                let mime = r.headers().get("content-type").and_then(|v| v.to_str().ok()).unwrap_or("image/png").to_string();
                return Ok(GeneratedImage { bytes: r.bytes().await?.to_vec(), mime, caption: None });
            }
            Err(CoreError::provider("the model returned no image"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_data_urls() {
        let (b, m) = decode_data_url("data:image/png;base64,aGk=").unwrap();
        assert_eq!((b.as_slice(), m.as_str()), (b"hi".as_slice(), "image/png"));
    }
}
