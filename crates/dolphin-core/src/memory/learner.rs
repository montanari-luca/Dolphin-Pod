//! The learner: turns what happened (signals) into small edits of one agent's memory. The model call itself lives in
//! `engine/learning.rs`; this file is everything around it that must be right whatever the model says: the prompt,
//! reading its JSON leniently, and the rules for what it may change.
//!
//! It may only change entries it wrote itself (`source = auto`, not pinned). Entries the user added, edited or pinned
//! are never rewritten or deleted by it, only shown to it so it does not repeat them.

use serde_json::Value;

use super::signals::{Feedback, Signal};
use super::{MAX_ENTRIES, MAX_TEXT_CHARS, clean_text, looks_secret};
use crate::db::Db;
use crate::error::Result;
use crate::model::{Agent, AgentMemory, MemoryKind, MemorySource};
use crate::util::{extract_json_object, new_id, truncate};

/// Most edits one learner call may make.
pub const MAX_OPS: usize = 3;
/// Most recent signals looked at per call; older ones are dropped.
pub const MAX_SIGNALS: usize = 5;
/// Most notes shown to the model, so a small local model's prompt stays small.
const MAX_NOTES_SHOWN: usize = 30;
/// Longest stretch of a reply or request quoted to the model.
const EXCERPT_CHARS: usize = 700;

#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    Add { kind: MemoryKind, text: String, project_only: bool },
    Update { id: usize, text: String, kind: Option<MemoryKind> },
    Merge { ids: Vec<usize>, text: String, kind: Option<MemoryKind> },
    Delete { id: usize },
}

pub fn system_prompt(explicit: bool) -> String {
    let mut s = String::from(
        "You keep the private notes an AI assistant takes about its user so it works better for them next time. \
         You are given the assistant's current notes (numbered) and something that just happened. Decide whether the user \
         showed a LASTING preference, habit, fact about them, or something the assistant should not do again.\n\n\
         Reply with JSON only: {\"ops\": [...]} where each op is one of:\n\
         {\"op\":\"add\",\"kind\":\"style|fact|workflow|avoid\",\"text\":\"...\",\"project_only\":false}\n\
         {\"op\":\"update\",\"id\":N,\"text\":\"...\"}\n\
         {\"op\":\"merge\",\"ids\":[N,M],\"text\":\"...\"}\n\
         {\"op\":\"delete\",\"id\":N}\n\n\
         kinds: style = how to sound or present; fact = something true about the user or their setup; workflow = how they like work \
         done; avoid = something not to do again.\n\
         Rules:\n\
         - A note is ONE short sentence under 200 characters, written as an instruction or fact the assistant can act on \
           (\"Keep answers under three sentences\", \"Never edit tests without asking\", \"The user works in Rust\").\n\
         - Only record what would still matter in a different conversation. Never record one-off task details, code, file \
           contents, other people's private details, or secrets (passwords, keys, tokens, card numbers).\n\
         - If a similar note exists, update or merge it instead of adding another. Notes marked [locked] cannot be changed.\n\
         - Set project_only to true only when it clearly applies to this project alone.\n\
         - A thumbs up, a polite reply or an ordinary follow-up is not a lesson.\n\
         - At most 3 ops. If nothing lasting was shown, reply {\"ops\": []}.",
    );
    if explicit {
        s.push_str("\n\nThe user EXPLICITLY asked the assistant to remember something. Always record it, rephrased as a short note, unless it holds a secret.");
    }
    s
}

fn str_of<'a>(d: &'a Value, key: &str) -> &'a str {
    d.get(key).and_then(Value::as_str).unwrap_or("")
}

/// What happened, in a few plain lines the model can read at a glance.
fn describe(f: &Feedback) -> String {
    let d = &f.detail;
    let quote = |key: &str, label: &str| {
        let t = str_of(d, key).trim();
        if t.is_empty() { String::new() } else { format!("\n  {label}: {}", truncate(t, EXCERPT_CHARS)) }
    };
    match f.signal {
        Signal::Correction => format!("The user replied right after the assistant's answer with what reads like a correction.{}{}{}", quote("request", "The user's earlier request"), quote("reply", "The assistant's answer"), quote("user", "The user's reply")),
        Signal::Revert => {
            let files = d.get("files").and_then(Value::as_u64).map(|n| format!(" ({n} file(s))")).unwrap_or_default();
            format!("The user reverted the file changes the assistant just made{files}.{}{}", quote("request", "The user's request"), quote("reply", "The assistant's message"))
        }
        Signal::Deny => format!("The user declined an action the assistant asked to take: {}{}", str_of(d, "action"), quote("said", "The user said")),
        Signal::ThumbDown => format!("The user gave the assistant's answer a thumbs down.{}{}", quote("request", "The user's request"), quote("reply", "The assistant's answer")),
        Signal::ThumbUp => format!("The user gave the assistant's answer a thumbs up.{}{}", quote("request", "The user's request"), quote("reply", "The assistant's answer")),
        Signal::Remember => format!("The user asked the assistant to remember: \"{}\"", truncate(str_of(d, "text").trim(), EXCERPT_CHARS)),
    }
}

/// The notes shown to the model: locked ones first (so it does not repeat them), then the most recently changed.
/// The position in the returned list, starting at 1, is the number the model uses.
pub fn notes_shown(all: &[AgentMemory]) -> Vec<&AgentMemory> {
    let mut locked: Vec<&AgentMemory> = all.iter().filter(|m| is_locked(m)).collect();
    let mut editable: Vec<&AgentMemory> = all.iter().filter(|m| !is_locked(m)).collect();
    editable.sort_by_key(|m| std::cmp::Reverse(m.updated_at));
    locked.extend(editable);
    locked.truncate(MAX_NOTES_SHOWN);
    locked
}

fn is_locked(m: &AgentMemory) -> bool {
    m.pinned || m.source != MemorySource::Auto
}

pub fn user_prompt(agent: &Agent, shown: &[&AgentMemory], project: Option<&str>, signals: &[Feedback]) -> String {
    let notes = if shown.is_empty() {
        "(none yet)".to_string()
    } else {
        shown
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let scope = m.project.as_deref().map(|p| format!(", project: {p}")).unwrap_or_default();
                let lock = if is_locked(m) { " [locked]" } else { "" };
                format!("[{}] ({}{scope}) {}{lock}", i + 1, super::kind_name(m.kind), m.text)
            })
            .collect::<Vec<_>>()
            .join("\n")
    };
    let what = signals.iter().map(|f| format!("- {}", describe(f))).collect::<Vec<_>>().join("\n");
    format!(
        "Assistant: {} ({})\nCurrent project: {}\n\nCurrent notes:\n{notes}\n\nWhat just happened:\n{what}",
        agent.name,
        agent.specialty,
        project.unwrap_or("none"),
    )
}

fn kind_from(v: Option<&Value>) -> Option<MemoryKind> {
    serde_json::from_value(Value::String(v?.as_str()?.trim().to_lowercase())).ok()
}

fn id_from(v: &Value) -> Option<usize> {
    v.as_u64().map(|n| n as usize).or_else(|| v.as_str()?.trim().trim_matches(['[', ']']).parse().ok())
}

/// Read the model's reply. Anything that is not a well-formed op is skipped, never an error: small models are sloppy.
pub fn parse_ops(reply: &str) -> Vec<Op> {
    let Some(json) = extract_json_object(reply).and_then(|j| serde_json::from_str::<Value>(j).ok()) else { return vec![] };
    let list = json.get("ops").and_then(Value::as_array).cloned().unwrap_or_default();
    let mut out = Vec::new();
    for o in &list {
        let text = o.get("text").and_then(Value::as_str).unwrap_or("").to_string();
        let kind = kind_from(o.get("kind"));
        let op = match o.get("op").and_then(Value::as_str).map(str::to_lowercase).as_deref() {
            Some("add") if !text.trim().is_empty() => {
                Some(Op::Add { kind: kind.unwrap_or(MemoryKind::Fact), text, project_only: o.get("project_only").and_then(Value::as_bool) == Some(true) })
            }
            Some("update") if !text.trim().is_empty() => o.get("id").and_then(id_from).map(|id| Op::Update { id, text, kind }),
            Some("merge") if !text.trim().is_empty() => {
                let ids: Vec<usize> = o.get("ids").and_then(Value::as_array).map(|a| a.iter().filter_map(id_from).collect()).unwrap_or_default();
                (ids.len() >= 2).then_some(Op::Merge { ids, text, kind })
            }
            Some("delete") => o.get("id").and_then(id_from).map(|id| Op::Delete { id }),
            _ => None,
        };
        out.extend(op);
    }
    out.truncate(MAX_OPS);
    out
}

/// What applying the ops did, so the caller can tell the UI.
#[derive(Debug, Default)]
pub struct Outcome {
    pub added: Vec<AgentMemory>,
    /// Entries that were reworded, with the text they had before.
    pub updated: Vec<(AgentMemory, String)>,
    /// Whether anything at all changed (including deletes and expiries).
    pub changed: bool,
}

/// A note as the learner may store it: one line, within the limit, and no secrets.
fn storable(text: &str) -> Option<String> {
    let t = clean_text(text)?;
    (t.chars().count() <= MAX_TEXT_CHARS && !looks_secret(&t)).then_some(t)
}

/// Apply the model's ops to the agent's memory. `shown` is the numbered list the model saw. `explicit` means the user
/// asked for it, so new entries count as the user's own. Everything the rules forbid is skipped quietly.
pub fn apply(db: &Db, agent_id: &str, project: Option<&str>, explicit: bool, shown: &[&AgentMemory], ops: Vec<Op>) -> Result<Outcome> {
    let mut out = Outcome::default();
    // What may be changed: only the learner's own, unpinned entries, looked up by the number the model was given.
    let editable = |n: usize| n.checked_sub(1).and_then(|i| shown.get(i)).copied().filter(|m| !is_locked(m));
    for op in ops {
        match op {
            Op::Add { kind, text, project_only } => {
                let Some(text) = storable(&text) else { continue };
                let existing = db.list_memories(agent_id)?;
                if existing.iter().any(|m| m.text.eq_ignore_ascii_case(&text)) {
                    continue;
                }
                if existing.len() >= MAX_ENTRIES {
                    // Make room by expiring the least-used entry the learner wrote itself; if there is none, drop the add.
                    let victim = existing.iter().filter(|m| !is_locked(m)).min_by_key(|m| (m.hits, m.created_at));
                    let Some(victim) = victim else { continue };
                    db.delete_memory(&victim.id)?;
                    out.changed = true;
                }
                let saved = db.upsert_memory(&AgentMemory {
                    id: new_id(),
                    agent_id: agent_id.to_string(),
                    kind,
                    text,
                    project: if project_only { project.map(str::to_string) } else { None },
                    source: if explicit { MemorySource::Explicit } else { MemorySource::Auto },
                    pinned: false,
                    hits: 0,
                    created_at: 0,
                    updated_at: 0,
                })?;
                out.added.push(saved);
                out.changed = true;
            }
            Op::Update { id, text, kind } => {
                let (Some(m), Some(text)) = (editable(id), storable(&text)) else { continue };
                if text != m.text || kind.is_some_and(|k| k != m.kind) {
                    let saved = db.upsert_memory(&AgentMemory { text, kind: kind.unwrap_or(m.kind), ..m.clone() })?;
                    out.updated.push((saved, m.text.clone()));
                    out.changed = true;
                }
            }
            Op::Merge { ids, text, kind } => {
                let parts: Vec<&AgentMemory> = ids.iter().filter_map(|n| editable(*n)).collect();
                let Some(text) = storable(&text) else { continue };
                // Every number must be one the learner may change; otherwise it is not a merge it is entitled to make.
                if parts.len() != ids.len() || parts.len() < 2 || parts.windows(2).any(|w| w[0].id == w[1].id) {
                    continue;
                }
                let keep = parts[0];
                let saved = db.upsert_memory(&AgentMemory { text, kind: kind.unwrap_or(keep.kind), ..keep.clone() })?;
                for gone in &parts[1..] {
                    db.delete_memory(&gone.id)?;
                }
                out.updated.push((saved, keep.text.clone()));
                out.changed = true;
            }
            Op::Delete { id } => {
                if let Some(m) = editable(id) {
                    db.delete_memory(&m.id)?;
                    out.changed = true;
                }
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn db_with_agent() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        crate::seed::seed_if_needed(&db).unwrap();
        let id = db.agent_by_slug("rick").unwrap().id;
        (db, id)
    }

    fn note(db: &Db, agent: &str, text: &str, source: MemorySource, pinned: bool) -> AgentMemory {
        db.upsert_memory(&AgentMemory {
            id: new_id(),
            agent_id: agent.into(),
            kind: MemoryKind::Style,
            text: text.into(),
            project: None,
            source,
            pinned,
            hits: 0,
            created_at: 0,
            updated_at: 0,
        })
        .unwrap()
    }

    #[test]
    fn replies_are_read_leniently_and_capped() {
        let reply = r#"Sure! ```json
        {"ops":[
          {"op":"add","kind":"STYLE","text":"Be terse","project_only":true},
          {"op":"add","text":"   "},
          {"op":"update","id":"[2]","text":"Use tabs"},
          {"op":"merge","ids":[1],"text":"only one id"},
          {"op":"merge","ids":[1,3],"text":"Merged note","kind":"workflow"},
          {"op":"delete","id":4},
          {"op":"explode"},
          {"op":"add","kind":"fact","text":"A fifth op is cut"}
        ]}
        ```"#;
        let ops = parse_ops(reply);
        assert_eq!(ops.len(), MAX_OPS);
        assert_eq!(ops[0], Op::Add { kind: MemoryKind::Style, text: "Be terse".into(), project_only: true });
        assert_eq!(ops[1], Op::Update { id: 2, text: "Use tabs".into(), kind: None });
        assert_eq!(ops[2], Op::Merge { ids: vec![1, 3], text: "Merged note".into(), kind: Some(MemoryKind::Workflow) });
        assert!(parse_ops("not json at all").is_empty());
        assert!(parse_ops(r#"{"ops": "nope"}"#).is_empty());
        assert!(parse_ops(r#"{"ops": []}"#).is_empty());
    }

    #[test]
    fn it_only_changes_what_it_wrote_and_never_what_the_user_owns() {
        let (db, rick) = db_with_agent();
        let mine = note(&db, &rick, "Keep it short", MemorySource::Auto, false);
        let typed = note(&db, &rick, "I use tabs", MemorySource::Explicit, false);
        let edited = note(&db, &rick, "British English", MemorySource::Edited, false);
        let pinned = note(&db, &rick, "No emojis", MemorySource::Auto, true);
        let all = db.list_memories(&rick).unwrap();
        let shown = notes_shown(&all);
        let n = |id: &str| shown.iter().position(|m| m.id == id).unwrap() + 1;

        let ops = vec![
            Op::Update { id: n(&typed.id), text: "I use spaces".into(), kind: None },
            Op::Delete { id: n(&edited.id) },
            Op::Delete { id: n(&pinned.id) },
            Op::Update { id: n(&mine.id), text: "Keep it very short".into(), kind: None },
        ];
        let out = apply(&db, &rick, None, false, &shown, ops).unwrap();
        let now: Vec<String> = db.list_memories(&rick).unwrap().into_iter().map(|m| m.text).collect();
        assert!(now.contains(&"I use tabs".to_string()) && now.contains(&"British English".to_string()) && now.contains(&"No emojis".to_string()));
        assert!(now.contains(&"Keep it very short".to_string()) && !now.contains(&"Keep it short".to_string()));
        assert_eq!(out.updated.len(), 1);
        assert_eq!(out.updated[0].1, "Keep it short", "the old text is kept for an Undo");
    }

    #[test]
    fn adds_skip_secrets_duplicates_and_overlong_notes_and_scope_to_the_project() {
        let (db, rick) = db_with_agent();
        note(&db, &rick, "Keep answers short", MemorySource::Explicit, false);
        let ops = vec![
            Op::Add { kind: MemoryKind::Fact, text: "The password is hunter2".into(), project_only: false },
            Op::Add { kind: MemoryKind::Style, text: "keep answers SHORT".into(), project_only: false },
            Op::Add { kind: MemoryKind::Fact, text: "x".repeat(MAX_TEXT_CHARS + 1), project_only: false },
            Op::Add { kind: MemoryKind::Workflow, text: "Run cargo test first".into(), project_only: true },
        ];
        let out = apply(&db, &rick, Some("dolphin-pod"), false, &[], ops).unwrap();
        assert_eq!(out.added.len(), 1);
        let a = &out.added[0];
        assert_eq!((a.text.as_str(), a.project.as_deref(), a.source, a.kind), ("Run cargo test first", Some("dolphin-pod"), MemorySource::Auto, MemoryKind::Workflow));

        let explicit = apply(&db, &rick, Some("dolphin-pod"), true, &[], vec![Op::Add { kind: MemoryKind::Fact, text: "I am a beginner".into(), project_only: false }]).unwrap();
        assert_eq!((explicit.added[0].source, explicit.added[0].project.as_deref()), (MemorySource::Explicit, None));
    }

    #[test]
    fn at_the_cap_the_least_used_learned_entry_goes_and_nothing_of_the_users_does() {
        let (db, rick) = db_with_agent();
        for i in 0..MAX_ENTRIES {
            note(&db, &rick, &format!("note {i}"), if i < 3 { MemorySource::Explicit } else { MemorySource::Auto }, false);
        }
        let all = db.list_memories(&rick).unwrap();
        // "note 5" is the least used learned entry; the three explicit ones are never candidates.
        let used: Vec<String> = all.iter().filter(|m| m.text != "note 5").map(|m| m.id.clone()).collect();
        db.bump_memory_hits(&used).unwrap();
        let out = apply(&db, &rick, None, false, &[], vec![Op::Add { kind: MemoryKind::Fact, text: "A new lesson".into(), project_only: false }]).unwrap();
        assert_eq!(out.added.len(), 1);
        let now: Vec<String> = db.list_memories(&rick).unwrap().into_iter().map(|m| m.text).collect();
        assert_eq!(now.len(), MAX_ENTRIES);
        assert!(!now.contains(&"note 5".to_string()) && now.contains(&"A new lesson".to_string()));

        // A memory that is full of the user's own entries has nothing to expire: the add is dropped.
        let (db, rick) = db_with_agent();
        for i in 0..MAX_ENTRIES {
            note(&db, &rick, &format!("mine {i}"), MemorySource::Explicit, false);
        }
        let out = apply(&db, &rick, None, false, &[], vec![Op::Add { kind: MemoryKind::Fact, text: "Dropped".into(), project_only: false }]).unwrap();
        assert!(out.added.is_empty() && !out.changed);
        assert_eq!(db.count_memories(&rick).unwrap(), MAX_ENTRIES);
    }

    #[test]
    fn a_merge_needs_every_note_to_be_one_it_may_change() {
        let (db, rick) = db_with_agent();
        let a = note(&db, &rick, "Short answers", MemorySource::Auto, false);
        let b = note(&db, &rick, "Brief replies", MemorySource::Auto, false);
        let locked = note(&db, &rick, "Mine", MemorySource::Explicit, false);
        let all = db.list_memories(&rick).unwrap();
        let shown = notes_shown(&all);
        let n = |id: &str| shown.iter().position(|m| m.id == id).unwrap() + 1;

        apply(&db, &rick, None, false, &shown, vec![Op::Merge { ids: vec![n(&a.id), n(&locked.id)], text: "Nope".into(), kind: None }]).unwrap();
        assert_eq!(db.count_memories(&rick).unwrap(), 3, "a locked note blocks the whole merge");
        let out = apply(&db, &rick, None, false, &shown, vec![Op::Merge { ids: vec![n(&a.id), n(&b.id)], text: "Keep answers brief".into(), kind: None }]).unwrap();
        assert_eq!(out.updated.len(), 1);
        let texts: Vec<String> = db.list_memories(&rick).unwrap().into_iter().map(|m| m.text).collect();
        assert_eq!(texts.len(), 2);
        assert!(texts.contains(&"Keep answers brief".to_string()) && texts.contains(&"Mine".to_string()));
    }

    #[test]
    fn the_prompt_numbers_the_notes_and_marks_the_locked_ones() {
        let (db, rick) = db_with_agent();
        note(&db, &rick, "Learned note", MemorySource::Auto, false);
        note(&db, &rick, "Typed note", MemorySource::Explicit, false);
        let all = db.list_memories(&rick).unwrap();
        let shown = notes_shown(&all);
        let agent = db.get_agent(&rick).unwrap();
        let sig = Feedback {
            id: "f".into(),
            agent_id: rick.clone(),
            chat_id: "c".into(),
            message_id: None,
            signal: Signal::Correction,
            detail: serde_json::json!({"user": "No, use tabs", "reply": "Here is the code with spaces", "request": "format this"}),
            created_at: 0,
        };
        let p = user_prompt(&agent, &shown, Some("api"), &[sig]);
        assert!(p.contains("[1] (style) Typed note [locked]") && p.contains("[2] (style) Learned note\n"), "{p}");
        assert!(p.contains("Current project: api") && p.contains("No, use tabs") && p.contains("Here is the code with spaces"), "{p}");
        assert!(system_prompt(true).contains("EXPLICITLY") && !system_prompt(false).contains("EXPLICITLY"));
    }
}
