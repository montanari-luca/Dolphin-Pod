//! Cheap token estimation. No tokenizer ships with the app and providers disagree anyway, so this
//! errs on the high side (ASCII ≈ 3.6 chars/token, anything else ≈ 0.9 tokens/char) and the engine
//! calibrates it upward with the usage numbers providers report.

use crate::providers::{ChatMessage, Part, ToolSpec};

/// Flat cost of one image part.
const IMAGE_TOKENS: usize = 1200;
/// Framing overhead per message.
const MESSAGE_OVERHEAD: usize = 4;

pub fn text(s: &str) -> usize {
    let mut ascii = 0usize;
    let mut wide = 0usize;
    for c in s.chars() {
        if c.is_ascii() {
            ascii += 1;
        } else {
            wide += 1;
        }
    }
    (ascii as f64 / 3.6 + wide as f64 * 0.9).ceil() as usize
}

/// Characters that fit in roughly `tokens` tokens of typical text.
pub fn chars_for(tokens: usize) -> usize {
    (tokens as f64 * 3.6) as usize
}

pub fn message(m: &ChatMessage) -> usize {
    // A native assistant turn (`raw`) is what actually gets replayed, so it is what costs.
    if let Some(raw) = &m.raw {
        return MESSAGE_OVERHEAD + text(&raw.to_string());
    }
    let parts: usize = m
        .parts
        .iter()
        .map(|p| match p {
            Part::Text(t) => text(t),
            Part::Image { .. } => IMAGE_TOKENS,
        })
        .sum();
    let calls: usize = m.tool_calls.iter().map(|c| text(&c.name) + text(&c.arguments.to_string()) + 8).sum();
    MESSAGE_OVERHEAD + parts + calls
}

pub fn messages(ms: &[ChatMessage]) -> usize {
    ms.iter().map(message).sum()
}

pub fn specs(tools: &[ToolSpec]) -> usize {
    tools.iter().map(|t| text(&t.name) + text(&t.description) + text(&t.parameters.to_string()) + 8).sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn estimates_are_in_the_right_ballpark() {
        assert_eq!(text(""), 0);
        let t = text(&"word ".repeat(1000));
        assert!((1200..1600).contains(&t), "{t}");
        assert!(text("日本語のテキスト") >= 7, "wide characters cost about a token each");
    }
}
