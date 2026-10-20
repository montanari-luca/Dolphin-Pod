//! Compaction: turn a stretch of conversation into a short memory. A model writes it when one is
//! available; otherwise (or if the call fails) a mechanical outline keeps the context from ever
//! being the thing that breaks a run.

use std::sync::Arc;
use std::time::Duration;

use super::compress::head_tail;
use super::tokens;
use crate::providers::{ChatMessage, ChatRequest, Part, Provider, Role, Usage, complete};
use crate::util::truncate;

/// First line of every summary message, so later passes can recognise and extend it.
pub const SUMMARY_MARK: &str = "[Summary of the earlier conversation";
pub const PROGRESS_MARK: &str = "[Progress so far on this task";

/// A model that writes summaries, with the room it has to do so.
#[derive(Clone)]
pub struct Summarizer {
    pub provider: Arc<dyn Provider>,
    pub base: ChatRequest,
    /// Input tokens this model can take.
    pub limit: usize,
}

const SYSTEM: &str = "You maintain the running memory of a group chat between one user and several AI agents. \
Fold the existing memory (if any) and the new transcript into ONE updated memory that replaces both.\n\
Keep: the user's goals, preferences and constraints; decisions and the reasons for them; facts, names, numbers, \
file paths, commands, identifiers, error messages and their fixes; what each agent did, found or promised; \
open questions and unfinished work. Drop pleasantries, repetition and anything superseded. \
Write compact Markdown bullets grouped by topic, naming who said or did what when it matters. \
Never invent details. Output only the memory, at most {words} words.";

fn clip_entry(s: &str, max_chars: usize) -> String {
    head_tail(s, max_chars)
}

/// Render messages as transcript lines for a summarizer.
pub fn render_entries(msgs: &[ChatMessage]) -> Vec<String> {
    msgs.iter()
        .map(|m| {
            let text = m.text_content();
            let images = m.parts.iter().filter(|p| matches!(p, Part::Image { .. })).count();
            let mut s = match m.role {
                Role::User if text.starts_with('[') => text,
                Role::User => format!("User: {text}"),
                Role::Assistant => format!("Assistant: {text}"),
                Role::Tool => format!("Result of {}: {}", m.tool_name.as_deref().unwrap_or("tool"), strip_footer(&text)),
            };
            for c in &m.tool_calls {
                s.push_str(&format!("\n(called {} {})", c.name, truncate(&c.arguments.to_string(), 160)));
            }
            if images > 0 {
                s.push_str(&format!("\n({images} image(s) attached)"));
            }
            clip_entry(&s, if m.role == Role::Tool { 1500 } else { 3000 })
        })
        .collect()
}

fn strip_footer(text: &str) -> &str {
    text.rfind("\n[compressed ").map(|i| &text[..i]).unwrap_or(text)
}

/// Mechanical fallback: one clipped line per entry, newest entries win when space runs out.
pub fn outline(prev: Option<&str>, entries: &[String], max_tokens: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    if let Some(p) = prev.filter(|p| !p.trim().is_empty()) {
        lines.push(truncate(p.trim(), tokens::chars_for(max_tokens / 3)));
    }
    let mut budget = tokens::chars_for(max_tokens).saturating_sub(lines.iter().map(|l| l.len()).sum::<usize>());
    let mut tail: Vec<String> = Vec::new();
    for e in entries.iter().rev() {
        let line = format!("- {}", truncate(&e.replace('\n', " "), 220));
        if line.len() + 1 > budget {
            break;
        }
        budget -= line.len() + 1;
        tail.push(line);
    }
    tail.reverse();
    let dropped = entries.len() - tail.len();
    if dropped > 0 {
        lines.push(format!("- … {dropped} earlier messages not shown (automatic outline)"));
    }
    lines.extend(tail);
    lines.join("\n")
}

/// Result of summarizing: the text, how it was made, and what the model calls cost.
pub struct Summary {
    pub text: String,
    pub by_model: bool,
    pub usage: Usage,
}

/// Summarize `entries` (oldest first) on top of an optional previous summary. Long transcripts are
/// folded in chunks that fit the summarizer's window.
pub async fn summarize(sum: Option<&Summarizer>, prev: Option<&str>, entries: &[String], max_summary_tokens: usize) -> Summary {
    let mut usage = Usage::default();
    let fallback = |usage: Usage| Summary { text: outline(prev, entries, max_summary_tokens), by_model: false, usage };
    let Some(sum) = sum else { return fallback(usage) };

    let overhead = max_summary_tokens * 2 + 500;
    let chunk_tokens = (sum.limit * 6 / 10).saturating_sub(overhead).max(400);
    let mut chunks: Vec<String> = vec![String::new()];
    for e in entries {
        let e = clip_entry(e, tokens::chars_for(chunk_tokens / 2).max(400));
        if tokens::text(chunks.last().unwrap()) + tokens::text(&e) > chunk_tokens && !chunks.last().unwrap().is_empty() {
            chunks.push(String::new());
        }
        let cur = chunks.last_mut().unwrap();
        cur.push_str(&e);
        cur.push_str("\n\n");
    }
    // Bounded cost: a transcript that needs many passes is outlined instead.
    if chunks.len() > 8 {
        return fallback(usage);
    }

    let mut memory = prev.map(str::to_string).unwrap_or_default();
    for chunk in chunks.iter().filter(|c| !c.trim().is_empty()) {
        let mut req = sum.base.clone();
        req.system = SYSTEM.replace("{words}", &((max_summary_tokens as f64 * 0.7) as usize).to_string());
        req.messages = vec![ChatMessage::user(format!(
            "Existing memory:\n{}\n\nNew transcript to fold in:\n{}",
            if memory.trim().is_empty() { "(none yet)" } else { memory.trim() },
            chunk.trim_end()
        ))];
        req.tools.clear();
        req.json_mode = false;
        req.want_reasoning = false;
        req.web_search = false;
        req.cache_key = None;
        req.stable_messages = None;
        match tokio::time::timeout(Duration::from_secs(180), complete(sum.provider.as_ref(), req)).await {
            Ok(Ok((text, u))) if !text.trim().is_empty() => {
                usage.input_tokens += u.input_tokens;
                usage.output_tokens += u.output_tokens;
                memory = text.trim().to_string();
            }
            _ => return fallback(usage),
        }
    }
    if memory.trim().is_empty() {
        return fallback(usage);
    }
    // A runaway summary defeats the point.
    Summary { text: truncate(&memory, tokens::chars_for(max_summary_tokens * 2)), by_model: true, usage }
}

/// Text of a summary message with its marker line removed (for extending it).
pub fn summary_body(text: &str) -> Option<&str> {
    let t = text.trim_start();
    if !(t.starts_with(SUMMARY_MARK) || t.starts_with(PROGRESS_MARK)) {
        return None;
    }
    Some(t.split_once('\n').map(|(_, rest)| rest.trim()).unwrap_or(""))
}

pub fn summary_message(body: &str) -> String {
    format!("{SUMMARY_MARK}. Older messages were compacted to save space; this is what happened in them.]\n{}", body.trim())
}

pub fn progress_message(body: &str) -> String {
    format!("{PROGRESS_MARK}. Earlier steps were compacted to save space; this is what was done and found.]\n{}", body.trim())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outline_keeps_the_newest_entries_within_budget() {
        let entries: Vec<String> = (0..200).map(|i| format!("User: message number {i} with some words in it")).collect();
        let o = outline(None, &entries, 300);
        assert!(o.contains("message number 199") && !o.contains("message number 3 "));
        assert!(o.contains("earlier messages not shown"));
        assert!(tokens::text(&o) < 400);
    }

    #[test]
    fn summary_markers_round_trip() {
        let m = summary_message("- user wants X");
        assert_eq!(summary_body(&m), Some("- user wants X"));
        assert_eq!(summary_body("hello"), None);
    }

    #[tokio::test]
    async fn without_a_model_the_outline_is_used() {
        let s = summarize(None, Some("- earlier"), &["User: hi".into()], 300).await;
        assert!(!s.by_model && s.text.contains("earlier") && s.text.contains("hi"));
    }
}
