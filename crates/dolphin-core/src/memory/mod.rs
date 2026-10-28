//! Per-agent memory: what an agent has learned about the user. Each agent has its own, and an agent only ever
//! sees its own. Entries are short, one thought each; the store itself lives in `db/memory.rs`.
//!
//! They reach the agent as a volatile message after the history, never in the system prompt, so the stable
//! prompt and the history stay identical between turns and provider prompt caches keep hitting.

use std::path::Path;

use crate::model::{AgentMemory, MemoryKind};
use crate::preferences;

/// Most entries one agent keeps. Keeps the block small enough for small local models.
pub const MAX_ENTRIES: usize = 40;
/// Longest entry: a rule or a fact, not a paragraph.
pub const MAX_TEXT_CHARS: usize = 300;
/// How many relevance-picked entries (facts, workflows) ride along with the ones that always apply.
const MAX_RELEVANT: usize = 6;

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
    fn the_block_is_empty_when_nothing_applies() {
        assert_eq!(block(&[]), None);
        let m = mem(MemoryKind::Style, "Be terse", None);
        let b = block(&[&m]).unwrap();
        assert!(b.contains("- Be terse\n") && b.contains("do not mention"));
    }
}
