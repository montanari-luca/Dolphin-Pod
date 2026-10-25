//! Lincoln: turns Roger's decision into a brief for each specialist. He never talks to the user;
//! his brief travels with the specialist's request and is shown as a collapsible chip in the UI.

use super::{Decision, Mode, Rewrite};
use crate::model::Agent;
use crate::providers::{ChatMessage, ChatRequest, Provider, Usage, complete};
use crate::util::truncate;

pub fn rewrite_system(level: Rewrite) -> &'static str {
    match level {
        Rewrite::Full => "You are Lincoln, the orchestrator of a team of AI agents. Rewrite the user's request into a complete, well-structured brief for the specialist who will handle it: the goal, relevant context from the conversation, constraints, what a great answer contains, and the expected output format. Keep the user's intent and wording where it matters. Do not answer the request yourself. Output only the brief.",
        _ => "You are Lincoln, the orchestrator of a team of AI agents. Lightly tidy the user's request into a clear brief for the specialist: fix ambiguity, add the missing context from the conversation in one or two sentences, keep everything else as the user wrote it. Do not answer the request yourself. Output only the brief.",
    }
}

/// Lincoln's orders, prefixed to the brief.
pub fn orders(d: &Decision, agent: &Agent, others: &[&Agent]) -> String {
    let mut s = format!("Lincoln (orchestrator) → {}: you are handling this ({} mode).", agent.name, d.mode.as_str());
    if d.mode == Mode::Plan {
        s.push_str(" Plan only: read-only until the user approves.");
    }
    if agent.specialty == "verifier" {
        s.push_str(" The user asked you to double-check the work above: verify what the previous agents did and claimed, with evidence; do not redo it.");
    }
    if !others.is_empty() {
        let names: Vec<&str> = others.iter().map(|a| a.name.as_str()).collect();
        s.push_str(&format!(" {} will also contribute; focus on your part.", names.join(", ")));
    }
    s
}

/// Build the brief (and the tokens it cost). With `Rewrite::None` (or no model) the user's words pass through unchanged.
pub async fn brief(
    provider: Option<(&dyn Provider, ChatRequest)>,
    d: &Decision,
    agent: &Agent,
    user_text: &str,
    recent_context: &str,
) -> (String, Usage) {
    if d.rewrite == Rewrite::None {
        return (user_text.to_string(), Usage::default());
    }
    let Some((provider, mut req)) = provider else { return (user_text.to_string(), Usage::default()) };
    req.system = rewrite_system(d.rewrite).to_string();
    req.messages = vec![ChatMessage::user(format!(
        "Specialist: {} ({})\nIntent: {}\n\nRecent conversation:\n{}\n\nUser request:\n{}",
        agent.name,
        agent.specialty,
        d.intent,
        if recent_context.is_empty() { "(none)" } else { recent_context },
        user_text
    ))];
    req.tools.clear();
    req.json_mode = false;
    match complete(provider, req).await {
        Ok((text, u)) if !text.trim().is_empty() => (truncate(text.trim(), 6000), u),
        Ok((_, u)) => (user_text.to_string(), u),
        Err(_) => (user_text.to_string(), Usage::default()),
    }
}
