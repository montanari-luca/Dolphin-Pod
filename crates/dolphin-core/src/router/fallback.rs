//! Deterministic keyword routing, used when Roger has no model configured or his reply is unusable.

use super::{Decision, Mode, Rewrite};
use crate::model::{Agent, AgentRole, PermissionLevel};

const RULES: &[(&str, &[&str])] = &[
    ("image", &["image", "picture", "draw", "illustration", "logo", "photo", "render", "wallpaper", "icon of"]),
    ("security", &["security", "vulnerab", "exploit", "cve", "pentest", "xss", "injection", "auth bypass", "threat model", "harden", "malware", "encryption"]),
    ("debug", &["bug", "error", "stack trace", "traceback", "exception", "crash", "doesn't work", "not working", "fails", "failing", "debug", "segfault", "panic"]),
    ("docs", &["readme", "documentation", "docs", "docstring", "api reference", "changelog", "write up"]),
    ("creative", &["story", "poem", "lyrics", "novel", "screenplay", "script for", "slogan", "tagline", "creative", "character", "haiku"]),
    ("long_horizon", &["project", "roadmap", "milestone", "multi-step", "end to end", "architecture", "migrate"]),
    ("teacher", &["learn", "teach", "explain", "understand", "what is", "how does", "why does", "tutorial", "lesson", "course"]),
    ("coder", &["code", "function", "implement", "class", "refactor", "script", "api", "regex", "sql", "typescript", "python", "java", "rust", "swift", "program"]),
];

const INTERNAL: &[&str] = &[
    "setting", "rename", "change the color", "change color", "colour", "clean this chat", "clear this chat", "clean the chat",
    "clear the chat", "cleanup", "clean up", "turn off", "turn on", "dark mode", "light mode", "theme", "timer", "add an automation",
];

const QUICK: &[&str] = &["quick", "quickly", "just give me", "just write", "no explanation", "tl;dr", "asap", "short answer"];
const PLAN: &[&str] = &["plan ", "plan:", "plan,", "plan.", "plan?", "a plan", "roadmap", "design doc", "proposal", "strategy", "outline the steps"];
const LEARN: &[&str] = &["how do i", "how to", "how can i", "explain", "teach me", "help me understand", "what is", "why"];

fn has(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}

pub fn route(text: &str, chat_agents: &[Agent], prefers_quick: bool) -> Decision {
    let t = text.to_lowercase();
    if has(&t, INTERNAL) {
        return Decision { mode: Mode::Internal, intent: "settings or housekeeping".into(), ..Decision::default() };
    }
    let specialists: Vec<&Agent> = chat_agents.iter().filter(|a| a.role == AgentRole::Specialist).collect();
    let padded = format!("{t} ");
    let mode = if has(&padded, PLAN) {
        Mode::Plan
    } else if has(&t, QUICK) || prefers_quick {
        Mode::Quick
    } else if has(&t, LEARN) {
        Mode::Learn
    } else {
        Mode::Implement
    };
    let pick = |spec: &str| specialists.iter().find(|a| a.specialty == spec).map(|a| a.id.clone());
    let mut target = None;
    if mode == Mode::Plan {
        target = pick("long_horizon");
    }
    if target.is_none() && mode == Mode::Learn && !has(&t, &["image", "picture", "draw"]) {
        target = pick("teacher");
    }
    if target.is_none() {
        for (spec, words) in RULES {
            if has(&t, words)
                && let Some(id) = pick(spec)
            {
                target = Some(id);
                break;
            }
        }
    }
    match target {
        Some(id) => Decision {
            mode,
            intent: "keyword routing".into(),
            targets: vec![id],
            rewrite: if mode == Mode::Plan { Rewrite::None } else { Rewrite::Light },
            permission: if mode == Mode::Plan { PermissionLevel::ReadOnly } else { PermissionLevel::Full },
            ..Decision::default()
        },
        // Nothing specific: Roger answers himself.
        None => Decision { mode: Mode::Answer, intent: "general question".into(), ..Decision::default() },
    }
}
