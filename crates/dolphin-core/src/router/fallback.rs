//! Deterministic keyword routing, used when Roger has no model configured or his reply is unusable.

use super::{Decision, Mode, Rewrite};
use crate::model::{Agent, AgentRole, PermissionLevel};

const RULES: &[(&str, &[&str])] = &[
    ("image", &["image", "picture", "draw", "illustration", "logo", "photo", "render", "wallpaper", "icon of"]),
    ("fun_facts", &["fun fact", "did you know", "trivia", "random fact", "something interesting", "interesting fact"]),
    ("researcher", &["research", "deep dive", "investigate", "look into", "compare sources", "state of the art", "literature review"]),
    ("security", &["security", "vulnerab", "exploit", "cve", "pentest", "xss", "injection", "auth bypass", "threat model", "harden", "malware", "encryption"]),
    ("debug", &["bug", "error", "stack trace", "traceback", "exception", "crash", "doesn't work", "not working", "fails", "failing", "debug", "segfault", "panic"]),
    ("git", &["git ", "commit", "rebase", "stash", "branch", "merge conflict", "push to", "push my", "push the", "pull from", "pull the latest", "git history"]),
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

/// Specialties whose keywords beat "the user wants to learn", so "how did X start, any fun facts?" still reaches Ruby.
const PRIORITY: &[&str] = &["fun_facts", "researcher", "git"];

/// Phrases that explicitly ask for the work to be checked. Amelia only ever runs on these (or an @mention).
const VERIFY: &[&str] = &[
    "double-check", "double check", "doublecheck", "fact-check", "fact check", "check your work", "check the work", "check my work",
    "verify your", "verify the work", "verify the result", "verify this", "verify it", "verify everything", "and verify", "then verify",
    "make sure it's correct", "make sure it is correct",
];

const QUICK: &[&str] = &["quick", "quickly", "just give me", "just write", "no explanation", "tl;dr", "asap", "short answer"];
const PLAN: &[&str] = &["plan ", "plan:", "plan,", "plan.", "plan?", "a plan", "roadmap", "design doc", "proposal", "strategy", "outline the steps"];
const LEARN: &[&str] = &["how do i", "how to", "how can i", "explain", "teach me", "help me understand", "what is", "why"];

/// Whether the user explicitly asked for a verification pass.
pub fn wants_verification(text: &str) -> bool {
    has(&text.to_lowercase(), VERIFY)
}

fn has(text: &str, words: &[&str]) -> bool {
    words.iter().any(|w| text.contains(w))
}

pub fn route(text: &str, chat_agents: &[Agent], prefers_quick: bool) -> Decision {
    let t = text.to_lowercase();
    if has(&t, INTERNAL) {
        return Decision { mode: Mode::Internal, intent: "settings or housekeeping".into(), ..Decision::default() };
    }
    let specialists: Vec<&Agent> = chat_agents.iter().filter(|a| a.role == AgentRole::Specialist && a.specialty != "council").collect();
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
    let priority = RULES.iter().any(|(spec, words)| PRIORITY.contains(spec) && has(&t, words) && pick(spec).is_some());
    let mut target = None;
    if mode == Mode::Plan {
        target = pick("long_horizon");
    }
    if target.is_none() && mode == Mode::Learn && !priority && !has(&t, &["image", "picture", "draw"]) {
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
    // "Do X, then double-check it": the worker first, then Amelia, who sees the worker's answer.
    let mut targets: Vec<String> = target.into_iter().collect();
    if let Some(v) = pick("verifier")
        && mode != Mode::Plan
        && wants_verification(text)
        && !targets.contains(&v)
    {
        targets.push(v);
    }
    match targets.first() {
        Some(_) => Decision {
            mode,
            intent: "keyword routing".into(),
            targets,
            rewrite: if mode == Mode::Plan { Rewrite::None } else { Rewrite::Light },
            permission: if mode == Mode::Plan { PermissionLevel::ReadOnly } else { PermissionLevel::Full },
            ..Decision::default()
        },
        // Nothing specific: Roger answers himself.
        None => Decision { mode: Mode::Answer, intent: "general question".into(), ..Decision::default() },
    }
}
