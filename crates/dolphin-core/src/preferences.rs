//! Context efficiency for custom instructions: the instructions are split into chunks and only the
//! chunks relevant to the current request are sent. A small utility model picks them; without one,
//! tag/keyword matching decides. Short instruction sets are sent whole (classifying would cost more).

use std::collections::HashSet;

use serde::Deserialize;

use crate::providers::{ChatMessage, ChatRequest, Provider, complete};
use crate::util::extract_json_object;

/// Below this size everything is included; classification would cost more than it saves.
pub const SEND_ALL_BELOW: usize = 1200;

#[derive(Debug, Clone, PartialEq)]
pub struct Chunk {
    pub id: usize,
    pub text: String,
    /// Phrased as a standing rule ("always…", "never…"): included in every request.
    pub always: bool,
    keywords: HashSet<String>,
}

const STOP: &[&str] = &[
    "about", "after", "also", "always", "because", "before", "being", "could", "every", "from", "have", "into", "just",
    "like", "make", "more", "never", "only", "other", "please", "should", "some", "than", "that", "their", "them",
    "then", "there", "these", "they", "this", "when", "where", "which", "while", "with", "would", "your", "you're",
    "want", "prefer", "using",
];

fn stem(w: &str) -> String {
    w.chars().take(6).collect()
}

fn keywords(text: &str) -> HashSet<String> {
    text.to_lowercase()
        .split(|c: char| !c.is_alphanumeric() && c != '+' && c != '#')
        .filter(|w| w.chars().count() >= 3 && !STOP.contains(w))
        .map(stem)
        .collect()
}

pub fn chunk(text: &str) -> Vec<Chunk> {
    let mut pieces: Vec<String> = Vec::new();
    for block in text.split("\n\n").map(str::trim).filter(|b| !b.is_empty()) {
        let lines: Vec<&str> = block.lines().map(str::trim).filter(|l| !l.is_empty()).collect();
        let bulleted = lines.len() > 1
            && lines.iter().all(|l| l.starts_with(['-', '*', '•']) || l.chars().next().is_some_and(|c| c.is_ascii_digit()));
        if bulleted {
            pieces.extend(lines.iter().map(|l| l.trim_start_matches(['-', '*', '•', ' ']).to_string()));
        } else {
            pieces.push(block.to_string());
        }
    }
    pieces
        .into_iter()
        .enumerate()
        .map(|(id, text)| {
            let lower = text.to_lowercase();
            Chunk {
                id,
                always: ["always", "never", "every answer", "every response", "all responses", "in general"].iter().any(|w| lower.contains(w)),
                keywords: keywords(&text),
                text,
            }
        })
        .collect()
}

/// Keyword fallback: standing rules plus the chunks sharing the most keywords with the request.
pub fn select_by_keywords(chunks: &[Chunk], query: &str, max: usize) -> Vec<String> {
    let q = keywords(query);
    let mut scored: Vec<(usize, &Chunk)> =
        chunks.iter().filter(|c| !c.always).map(|c| (c.keywords.intersection(&q).count(), c)).filter(|(s, _)| *s > 0).collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.id.cmp(&b.1.id)));
    let mut picked: Vec<&Chunk> = chunks.iter().filter(|c| c.always).collect();
    picked.extend(scored.into_iter().take(max).map(|(_, c)| c));
    picked.sort_by_key(|c| c.id);
    picked.into_iter().map(|c| c.text.clone()).collect()
}

/// Selection among ready-made entries that are already one thought each (nothing to split): the flagged ones
/// always, then up to `max` of the others sharing the most keywords with the request. Indexes, in order.
pub fn pick(items: &[(&str, bool)], query: &str, max: usize) -> Vec<usize> {
    let q = keywords(query);
    let mut scored: Vec<(usize, usize)> = items
        .iter()
        .enumerate()
        .filter(|(_, (_, always))| !always)
        .map(|(i, (text, _))| (keywords(text).intersection(&q).count(), i))
        .filter(|(score, _)| *score > 0)
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
    let mut out: Vec<usize> = items.iter().enumerate().filter(|(_, (_, always))| *always).map(|(i, _)| i).collect();
    out.extend(scored.into_iter().take(max).map(|(_, i)| i));
    out.sort_unstable();
    out
}

#[derive(Deserialize)]
struct Ids {
    ids: Vec<usize>,
}

pub async fn select(utility: Option<(&dyn Provider, ChatRequest)>, instructions: &str, query: &str) -> Vec<String> {
    let instructions = instructions.trim();
    if instructions.is_empty() {
        return vec![];
    }
    let chunks = chunk(instructions);
    if instructions.len() < SEND_ALL_BELOW {
        return chunks.into_iter().map(|c| c.text).collect();
    }
    if let Some((provider, mut req)) = utility {
        let list: Vec<String> = chunks.iter().map(|c| format!("[{}] {}", c.id, c.text)).collect();
        req.system = "You select which of the user's saved preferences matter for their current request. Reply with JSON only: {\"ids\": [numbers]}. Include standing rules that always apply. Return [] if none apply.".into();
        req.messages = vec![ChatMessage::user(format!("Preferences:\n{}\n\nCurrent request:\n{query}", list.join("\n")))];
        req.json_mode = true;
        req.tools.clear();
        if let Ok((text, _)) = complete(provider, req).await
            && let Some(ids) = extract_json_object(&text).and_then(|j| serde_json::from_str::<Ids>(j).ok())
        {
            let mut out: Vec<String> = chunks.iter().filter(|c| c.always || ids.ids.contains(&c.id)).map(|c| c.text.clone()).collect();
            out.dedup();
            return out;
        }
    }
    select_by_keywords(&chunks, query, 6)
}

#[cfg(test)]
mod tests {
    use super::*;

    const PREFS: &str = "I prefer quick implementations over long explanations when coding.\n\n\
        - Always answer in British English.\n\
        - For Python, use type hints and pytest.\n\
        - For Java, target Java 21 and use records where possible.\n\
        - When writing stories, keep them under 500 words.\n\n\
        My projects use PostgreSQL and Docker.";

    #[test]
    fn chunks_bullets_separately() {
        let c = chunk(PREFS);
        assert_eq!(c.len(), 6);
        assert!(c[1].always);
        assert_eq!(c[3].text, "For Java, target Java 21 and use records where possible.");
    }

    #[test]
    fn keyword_fallback_picks_relevant_chunks_and_standing_rules() {
        let c = chunk(PREFS);
        let picked = select_by_keywords(&c, "How do I write a Java function that parses dates?", 3);
        assert!(picked.iter().any(|p| p.contains("Java 21")));
        assert!(picked.iter().any(|p| p.contains("British English")), "standing rules always apply");
        assert!(!picked.iter().any(|p| p.contains("stories")));
        let story = select_by_keywords(&c, "Write me stories about dragons", 3);
        assert!(story.iter().any(|p| p.contains("500 words")));
        assert!(!story.iter().any(|p| p.contains("Java")));
    }

    #[test]
    fn pick_keeps_flagged_entries_and_the_best_matches() {
        let items = [("Keep answers short", true), ("Uses PostgreSQL and Docker", false), ("Owns a cat called Miso", false)];
        assert_eq!(pick(&items, "set up a PostgreSQL container", 1), vec![0, 1]);
        assert_eq!(pick(&items, "what should I cook tonight", 1), vec![0], "nothing relevant: only the flagged one");
        assert_eq!(pick(&items, "postgresql docker cat miso", 1), vec![0, 1], "max limits the picked ones, not the flagged ones");
    }

    #[tokio::test]
    async fn short_instructions_are_sent_whole() {
        let out = select(None, "Be concise.\n\nUse metric units.", "anything").await;
        assert_eq!(out, vec!["Be concise.", "Use metric units."]);
    }
}
