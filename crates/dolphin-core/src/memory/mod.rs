//! Per-agent memory: what an agent has learned about the user. Each agent has its own, and an agent only ever
//! sees its own. Entries are short, one thought each; the store itself lives in `db/memory.rs`.
//!
//! They reach the agent as a volatile message after the history, never in the system prompt, so the stable
//! prompt and the history stay identical between turns and provider prompt caches keep hitting.

use std::path::Path;

pub mod learner;
pub mod signals;

use crate::model::{AgentMemory, MemoryKind};
use crate::preferences;

/// Most entries one agent keeps. Keeps the block small enough for small local models.
pub const MAX_ENTRIES: usize = 40;
/// Longest entry: a rule or a fact, not a paragraph.
pub const MAX_TEXT_CHARS: usize = 300;
/// How many relevance-picked entries (facts, workflows) ride along with the ones that always apply.
const MAX_RELEVANT: usize = 6;

/// The kind as the learner's prompt names it.
pub fn kind_name(k: MemoryKind) -> &'static str {
    match k {
        MemoryKind::Style => "style",
        MemoryKind::Fact => "fact",
        MemoryKind::Workflow => "workflow",
        MemoryKind::Avoid => "avoid",
    }
}

/// The project a workspace belongs to: its folder name. The path itself is device-local and never syncs,
/// so a tag made from it would mean nothing on another device.
pub fn project_of(workspace: Option<&str>) -> Option<String> {
    Path::new(workspace?).file_name()?.to_str().map(str::to_string)
}

/// An entry as stored: one line, single spaces, trimmed. `None` when nothing is left.
pub fn clean_text(s: &str) -> Option<String> {
    let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
    (!s.is_empty()).then_some(s)
}

/// A project tag as stored: trimmed, blank meaning "everywhere".
pub fn clean_project(s: Option<&str>) -> Option<String> {
    s.map(str::trim).filter(|p| !p.is_empty()).map(str::to_string)
}

/// Whether a note looks like it contains a secret (a key, a token, a password, a card number). The learner never saves
/// those: a note is sent to a model with every request.
pub fn looks_secret(text: &str) -> bool {
    let lower = text.to_lowercase();
    if lower.contains("-----begin") {
        return true;
    }
    const LABELS: &[&str] = &["password", "passwd", "passphrase", "secret", "api key", "api_key", "apikey", "token", "pin code", "private key"];
    const LINKS: &[&str] = &[" is ", " was ", ": ", " = ", "=", ":"];
    if LABELS.iter().any(|l| LINKS.iter().any(|k| lower.contains(&format!("{l}{k}")))) {
        // "Never paste a password: use the vault" is a rule about secrets, not a secret.
        let rule = ["never ", "don't ", "do not ", "avoid ", "without "].iter().any(|w| lower.contains(w));
        if !rule {
            return true;
        }
    }
    const PREFIXES: &[&str] = &["sk-", "sk_", "ghp_", "gho_", "ghs_", "github_pat_", "glpat-", "xoxb-", "xoxp-", "akia", "aiza", "eyj"];
    let tokens = text.split(|c: char| c.is_whitespace() || matches!(c, ',' | ';' | '"' | '\'' | '`' | '(' | ')'));
    for t in tokens {
        let tl = t.to_lowercase();
        let keylike = t.len() >= 20 && t.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '+' | '/' | '='));
        if keylike && PREFIXES.iter().any(|p| tl.starts_with(p)) {
            return true;
        }
        // A long unbroken mix of letters and digits is a hash, a key or a token, not a sentence.
        if t.len() >= 32 && keylike && t.chars().any(|c| c.is_ascii_digit()) && t.chars().any(|c| c.is_ascii_alphabetic()) {
            return true;
        }
    }
    // A card or account number: thirteen or more digits in a row once spaces and dashes are ignored.
    let digits: String = text.chars().filter(|c| !matches!(c, ' ' | '-')).collect();
    let mut run = 0;
    for c in digits.chars() {
        run = if c.is_ascii_digit() { run + 1 } else { 0 };
        if run >= 13 {
            return true;
        }
    }
    false
}

fn applies_to(m: &AgentMemory, project: Option<&str>) -> bool {
    match (m.project.as_deref(), project) {
        (None, _) => true,
        (Some(tag), Some(here)) => tag.eq_ignore_ascii_case(here),
        (Some(_), None) => false,
    }
}

/// Small standing rules reach the agent every time; so does anything the user pinned.
fn always(m: &AgentMemory) -> bool {
    m.pinned || matches!(m.kind, MemoryKind::Style | MemoryKind::Avoid)
}

/// What to send with this request: entries for this project (or none), all of them while they are few,
/// otherwise the always-on ones plus the few that share the most words with the request.
pub fn select<'a>(all: &'a [AgentMemory], project: Option<&str>, query: &str) -> Vec<&'a AgentMemory> {
    let candidates: Vec<&AgentMemory> = all.iter().filter(|m| applies_to(m, project)).collect();
    let size: usize = candidates.iter().map(|m| m.text.len()).sum();
    if size < preferences::SEND_ALL_BELOW {
        return candidates;
    }
    let items: Vec<(&str, bool)> = candidates.iter().map(|m| (m.text.as_str(), always(m))).collect();
    preferences::pick(&items, query, MAX_RELEVANT).into_iter().map(|i| candidates[i]).collect()
}

/// The message the agent gets, or `None` when there is nothing to say.
pub fn block(selected: &[&AgentMemory]) -> Option<String> {
    if selected.is_empty() {
        return None;
    }
    let list: String = selected.iter().map(|m| format!("- {}\n", m.text.trim())).collect();
    Some(format!(
        "What you have learned about this user from working with them before. Apply it naturally, and do not mention that you remember it unless asked:\n{list}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::MemorySource;

    fn mem(kind: MemoryKind, text: &str, project: Option<&str>) -> AgentMemory {
        AgentMemory {
            id: text.into(),
            agent_id: "a".into(),
            kind,
            text: text.into(),
            project: project.map(Into::into),
            source: MemorySource::Auto,
            pinned: false,
            share: crate::model::MemoryShare::None,
            hits: 0,
            created_at: 0,
            updated_at: 0,
        }
    }

    #[test]
    fn a_project_is_the_folder_name() {
        assert_eq!(project_of(Some("/Users/luca/Dev/Dolphin Pod")).as_deref(), Some("Dolphin Pod"));
        assert_eq!(project_of(Some("/Users/luca/Dev/api/")).as_deref(), Some("api"));
        assert_eq!(project_of(Some("/")), None);
        assert_eq!(project_of(None), None);
    }

    #[test]
    fn text_is_one_clean_line() {
        assert_eq!(clean_text("  Use tabs,\n not   spaces \n").as_deref(), Some("Use tabs, not spaces"));
        assert_eq!(clean_text(" \n "), None);
        assert_eq!(clean_project(Some("  api ")).as_deref(), Some("api"));
        assert_eq!(clean_project(Some("  ")), None);
    }

    #[test]
    fn project_entries_only_apply_in_their_project() {
        let all = [
            mem(MemoryKind::Style, "Be terse", None),
            mem(MemoryKind::Workflow, "Run cargo test before committing", Some("Dolphin Pod")),
            mem(MemoryKind::Workflow, "Use pnpm", Some("api")),
        ];
        let texts = |sel: Vec<&AgentMemory>| sel.iter().map(|m| m.text.clone()).collect::<Vec<_>>();
        assert_eq!(texts(select(&all, Some("dolphin pod"), "x")), ["Be terse", "Run cargo test before committing"], "folder names match regardless of case");
        assert_eq!(texts(select(&all, None, "x")), ["Be terse"], "no workspace: only entries without a project");
        assert_eq!(texts(select(&all, Some("web"), "x")), ["Be terse"]);
    }

    #[test]
    fn a_big_memory_sends_the_standing_rules_and_the_relevant_entries() {
        let filler = " and some more words to make this entry long enough".repeat(7);
        let mut all = vec![
            mem(MemoryKind::Style, &format!("No emojis{filler}"), None),
            mem(MemoryKind::Avoid, &format!("Never rewrite the whole file{filler}"), None),
            mem(MemoryKind::Fact, &format!("The database is PostgreSQL{filler}"), None),
            mem(MemoryKind::Fact, &format!("Owns a cat called Miso{filler}"), None),
            mem(MemoryKind::Workflow, &format!("Pinned habit about invoices{filler}"), None),
        ];
        all[4].pinned = true;
        let sel = select(&all, None, "why is my PostgreSQL query slow?");
        let texts: Vec<&str> = sel.iter().map(|m| m.text.split(" and ").next().unwrap()).collect();
        assert_eq!(texts, ["No emojis", "Never rewrite the whole file", "The database is PostgreSQL", "Pinned habit about invoices"]);
    }

    #[test]
    fn secrets_are_recognised_and_ordinary_notes_are_not() {
        for secret in [
            "The password is hunter2",
            "my api key: abcdef",
            "Use token=abc123 for the staging API",
            "sk-proj-AbCdEfGhIjKlMnOpQrStUv",
            "ghp_16C7e42F292c6912E7710c838347Ae178B4a",
            "-----BEGIN RSA PRIVATE KEY-----",
            "Card number 4242 4242 4242 4242",
            "The hash is 9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08",
        ] {
            assert!(looks_secret(secret), "{secret}");
        }
        for fine in [
            "Keep answers under three sentences",
            "Never paste a password into the chat: use the vault",
            "The user prefers tokens of at most 4000 per answer",
            "Runs PostgreSQL 16 on port 5432",
            "Call me chief",
            "Don't log the API key",
        ] {
            assert!(!looks_secret(fine), "{fine}");
        }
    }

    #[test]
    fn the_block_is_empty_when_nothing_applies() {
        assert_eq!(block(&[]), None);
        let m = mem(MemoryKind::Style, "Be terse", None);
        let b = block(&[&m]).unwrap();
        assert!(b.contains("- Be terse\n") && b.contains("do not mention"));
    }
}
