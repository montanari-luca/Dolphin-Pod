//! Context-window resolution and the token budget derived from it.

use crate::model::{ModelRef, ProviderKind};

/// Windows for model families whose provider does not report one. First match wins, so the more
/// specific ids come first. Only used when the model picker did not record a window.
const HINTS: &[(&str, u32)] = &[
    ("gpt-4.1", 1_000_000),
    ("gpt-5", 400_000),
    ("gpt-oss", 131_072),
    ("gpt-4o", 128_000),
    ("o3", 200_000),
    ("o4", 200_000),
    ("claude", 200_000),
    ("gemini", 1_000_000),
    ("grok-4-fast", 2_000_000),
    ("grok", 256_000),
    ("deepseek", 128_000),
    ("magistral", 128_000),
    ("mistral", 128_000),
    ("qwq", 131_072),
    ("qwen3", 131_072),
    ("qwen", 32_768),
    ("llama", 128_000),
    ("gemma", 128_000),
    ("phi4", 16_384),
    ("phi", 16_384),
];

const UNKNOWN_LOCAL: u32 = 16_384;
const UNKNOWN_CLOUD: u32 = 128_000;
pub const MIN_WINDOW: u32 = 2_048;

pub fn hint(model_id: &str) -> Option<u32> {
    let id = model_id.to_lowercase();
    HINTS.iter().find(|(needle, _)| id.contains(needle)).map(|(_, w)| *w)
}

/// The context length a request is budgeted for. Ollama is capped at `local_cap` because the app
/// sets `num_ctx` itself (bigger windows cost memory and speed).
pub fn resolve(model: &ModelRef, kind: ProviderKind, local_cap: u32) -> u32 {
    let known = model.context_window.filter(|w| *w >= MIN_WINDOW).or_else(|| hint(&model.model_id));
    let window = match kind {
        ProviderKind::Ollama => known.unwrap_or(local_cap).min(local_cap),
        k if k.is_local() => known.unwrap_or(UNKNOWN_LOCAL),
        _ => known.unwrap_or(UNKNOWN_CLOUD),
    };
    window.max(MIN_WINDOW)
}

/// Token budget for one request.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Budget {
    pub window: u32,
    /// Room kept free for the reply (and thinking).
    pub reserve: usize,
    /// Most input tokens a request may carry.
    pub limit: usize,
    /// Above this, the engine starts reducing context.
    pub trigger: usize,
    /// After a compaction the conversation should be at most this large.
    pub target: usize,
}

impl Budget {
    pub fn new(window: u32, trigger_pct: u32) -> Self {
        let w = window as usize;
        let reserve = (w / 5).clamp(1024.min(w / 2), 40_000);
        let limit = w.saturating_sub(reserve).max(512);
        let trigger = limit * trigger_pct.clamp(30, 98) as usize / 100;
        let target = (limit * 4 / 10).min(trigger);
        Self { window, reserve, limit, trigger, target }
    }
}

/// Does a provider error mean "the prompt does not fit"?
pub fn is_overflow_error(msg: &str) -> bool {
    let m = msg.to_lowercase();
    [
        "context length",
        "context_length",
        "context window",
        "maximum context",
        "prompt is too long",
        "prompt too long",
        "too many tokens",
        "exceeds the maximum",
        "exceed the maximum",
        "input is too long",
        "request too large",
        "reduce the length",
        "token limit",
    ]
    .iter()
    .any(|needle| m.contains(needle))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn model(id: &str, window: Option<u32>) -> ModelRef {
        ModelRef { provider_id: "p".into(), model_id: id.into(), effort: None, vision: false, tools: true, context_window: window }
    }

    #[test]
    fn resolves_windows() {
        assert_eq!(resolve(&model("claude-opus-5-5", None), ProviderKind::Anthropic, 32_768), 200_000);
        assert_eq!(resolve(&model("x", Some(64_000)), ProviderKind::Openrouter, 32_768), 64_000);
        assert_eq!(resolve(&model("gpt-oss:20b", Some(131_072)), ProviderKind::Ollama, 32_768), 32_768, "Ollama is capped");
        assert_eq!(resolve(&model("tiny", Some(4096)), ProviderKind::Ollama, 32_768), 4096);
        assert_eq!(resolve(&model("my-finetune", None), ProviderKind::LmStudio, 32_768), 16_384);
        assert_eq!(resolve(&model("my-finetune", None), ProviderKind::Openai, 32_768), 128_000);
    }

    #[test]
    fn budget_leaves_room_for_the_reply() {
        let b = Budget::new(200_000, 85);
        assert_eq!(b.reserve, 40_000);
        assert_eq!(b.limit, 160_000);
        assert_eq!(b.trigger, 136_000);
        assert!(b.target < b.trigger);
        let small = Budget::new(4096, 85);
        assert!(small.limit > 2000 && small.limit < 4096);
    }

    #[test]
    fn detects_overflow_errors() {
        assert!(is_overflow_error("HTTP 400: prompt is too long: 210000 tokens > 200000 maximum"));
        assert!(is_overflow_error("This model's maximum context length is 8192 tokens"));
        assert!(!is_overflow_error("HTTP 401: invalid api key"));
    }
}
