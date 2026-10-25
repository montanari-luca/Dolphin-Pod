//! Roger's routing decision. The moderator model returns JSON, which is validated against the chat's
//! actual members and the routing rules; one repair retry, then deterministic keyword fallback.

use serde::Deserialize;

use super::{Decision, Mode, Rewrite, fallback, prompts::first_sentence};
use crate::model::{Agent, AgentRole, Message, PermissionLevel};
use crate::providers::{ChatMessage, ChatRequest, Provider, Usage, complete};
use crate::util::{extract_json_object, truncate};

#[derive(Debug, Deserialize, Default)]
#[serde(default)]
struct RawDecision {
    intent: Option<String>,
    mode: Option<String>,
    targets: Vec<String>,
    rewrite: Option<String>,
    permission: Option<String>,
    approves_plan: Option<bool>,
    clarification: Option<String>,
    lean_prompt_note: Option<String>,
}

pub struct ModeratorInput<'a> {
    pub chat_name: &'a str,
    pub members: &'a [Agent],
    pub history: &'a [Message],
    pub user_text: &'a str,
    pub preferences: &'a [String],
    pub pending_plan_by: Option<&'a str>,
}

pub fn system_prompt(i: &ModeratorInput, roger: &str) -> String {
    let roster: Vec<String> = i
        .members
        .iter()
        .filter(|a| a.role == AgentRole::Specialist)
        .map(|a| format!("- {} [{}]: {}", a.name, a.specialty, first_sentence(&a.job)))
        .collect();
    let prefs = if i.preferences.is_empty() { "(none)".to_string() } else { i.preferences.join("\n") };
    format!(
        r#"You are {roger}, the moderator of "Dolphin Pod", a group chat of AI agents. You do not answer the user here: you decide how the pod handles their latest message.

Specialists in the chat "{chat}":
{roster}

User preferences:
{prefs}

{pending}

Reply with ONE JSON object and nothing else:
{{"intent": "<5-12 words>", "mode": "quick|learn|plan|implement|answer|internal|clarify", "targets": ["<specialist name>"], "rewrite": "none|light|full", "permission": "read_only|full", "approves_plan": false, "clarification": null, "lean_prompt_note": null}}

Modes:
- answer: greetings, small talk, general knowledge, questions about the pod itself. You ({roger}) answer; targets = [].
- internal: the user asks to change a setting (names, colors, models, theme, cleanup timers, automations) or to clean/clear a chat. targets = [].
- clarify: the request is too vague to act on. Put one short question in "clarification".
- learn: the user asks how to do or understand something. Prefer this over implement unless they want a quick result (in the message or in their preferences). Route to the teacher if one is in the chat.
- quick: they want a fast, direct result.
- plan: they ask for a plan, roadmap or design (not the implementation). Route to the long-horizon agent; rewrite "none" or "light"; permission "read_only".
- implement: they want something built, written, fixed or changed.

Special agents:
- A fun-facts agent (specialty fun_facts) tells fun facts when asked; route "fun fact", trivia and "did you know" requests to it.
- A verification agent (specialty verifier) is used ONLY when the user explicitly asks to double-check, verify or fact-check work. Never add it on your own. When they ask to do something and then check it, targets = [the worker, the verifier], verifier LAST.
- A research agent (specialty researcher) handles requests to research, investigate or compare sources on the web, with many searches and fact-checking. Use it instead of answering yourself when the user wants researched, cited findings.
- A git agent (specialty git) handles git work: status, history, commits, branches, push and pull. Coding agents call it themselves, so route to it when the user's request is about git.

Rules: targets must come from the specialist list above (use their names). Pick the single best fit; use 2-3 only when the task clearly needs several skills. rewrite: "none" if the request is already precise, "light" to tidy it, "full" if it needs structure. approves_plan is true only when the user is approving the pending plan. Set lean_prompt_note to a short tip when the request is actionable but missing details that would help."#,
        chat = i.chat_name,
        roster = if roster.is_empty() { "- (no specialists)".into() } else { roster.join("\n") },
        pending = match i.pending_plan_by {
            Some(n) => format!("{n} has proposed a plan that is waiting for the user's approval."),
            None => "No plan is waiting for approval.".into(),
        },
    )
}

pub fn user_prompt(i: &ModeratorInput, names: &dyn Fn(&Message) -> String) -> String {
    let recent: Vec<String> = i
        .history
        .iter()
        .rev()
        .take(8)
        .rev()
        .map(|m| format!("{}: {}", names(m), truncate(&m.content.replace('\n', " "), 280)))
        .collect();
    format!(
        "Recent conversation:\n{}\n\nLatest user message:\n{}",
        if recent.is_empty() { "(empty)".into() } else { recent.join("\n") },
        i.user_text
    )
}

fn parse_mode(s: &str) -> Option<Mode> {
    Some(match s.trim().to_lowercase().as_str() {
        "quick" | "fast" => Mode::Quick,
        "learn" | "teach" | "explain" => Mode::Learn,
        "plan" | "planning" => Mode::Plan,
        "implement" | "build" | "do" | "code" => Mode::Implement,
        "answer" | "general" | "chat" | "self" => Mode::Answer,
        "internal" | "settings" | "setting" => Mode::Internal,
        "clarify" | "clarification" | "question" => Mode::Clarify,
        _ => return None,
    })
}

pub fn prefers_quick(preferences: &[String]) -> bool {
    preferences.iter().any(|p| {
        let p = p.to_lowercase();
        ["quick implementation", "quick answer", "just the code", "be concise", "skip explanation", "no explanation", "quickly"]
            .iter()
            .any(|w| p.contains(w))
    })
}

/// Validate a model reply into a decision that respects the chat's members and the routing rules.
pub fn parse_decision(reply: &str, members: &[Agent], user_text: &str, preferences: &[String]) -> Option<Decision> {
    let raw: RawDecision = serde_json::from_str(extract_json_object(reply)?).ok()?;
    let mut mode = parse_mode(raw.mode.as_deref()?)?;
    let specialists: Vec<&Agent> = members.iter().filter(|a| a.role == AgentRole::Specialist).collect();
    let mut targets: Vec<String> = Vec::new();
    for t in &raw.targets {
        let t = t.trim().trim_start_matches('@').to_lowercase();
        if let Some(a) = specialists.iter().find(|a| a.name.to_lowercase() == t || a.slug == t || a.specialty == t)
            && !targets.contains(&a.id)
        {
            targets.push(a.id.clone());
        }
    }
    // Amelia only runs when the user explicitly asks for a check, and then she goes last so she sees the work.
    let verifier = specialists.iter().find(|a| a.specialty == "verifier").map(|a| a.id.clone());
    let verify = fallback::wants_verification(user_text);
    if let Some(v) = &verifier {
        targets.retain(|t| t != v);
    }
    targets.truncate(if verify { 2 } else { 3 });
    if verify && let Some(v) = verifier {
        targets.push(v);
    }

    let mut d = Decision {
        intent: raw.intent.unwrap_or_default(),
        mode,
        targets,
        rewrite: match raw.rewrite.as_deref() {
            Some("none") => Rewrite::None,
            Some("full") => Rewrite::Full,
            _ => Rewrite::Light,
        },
        permission: if raw.permission.as_deref() == Some("full") { PermissionLevel::Full } else { PermissionLevel::ReadOnly },
        approves_plan: raw.approves_plan.unwrap_or(false),
        clarification: raw.clarification.filter(|s| !s.trim().is_empty()),
        lean_note: raw.lean_prompt_note.filter(|s| !s.trim().is_empty()),
        source: "model".into(),
    };

    match mode {
        Mode::Plan => {
            // The planner is enforced, the prompt is not heavily rewritten, and planning is read-only.
            if let Some(lh) = specialists.iter().find(|a| a.specialty == "long_horizon") {
                d.targets = vec![lh.id.clone()];
            }
            d.permission = PermissionLevel::ReadOnly;
            if d.rewrite == Rewrite::Full {
                d.rewrite = Rewrite::Light;
            }
        }
        Mode::Clarify if d.clarification.is_none() => mode = Mode::Answer,
        Mode::Answer | Mode::Internal | Mode::Clarify => d.targets.clear(),
        _ => {}
    }
    d.mode = mode;
    if matches!(d.mode, Mode::Quick | Mode::Learn | Mode::Plan | Mode::Implement) && d.targets.is_empty() {
        // The model named nobody valid: let keyword routing pick within the chat.
        let fb = fallback::route(user_text, members, prefers_quick(preferences));
        if fb.targets.is_empty() {
            d.mode = Mode::Answer;
        } else {
            d.targets = fb.targets;
        }
    }
    if d.mode == Mode::Answer {
        d.targets.clear();
    }
    Some(d)
}

/// Run the moderator model; fall back to keywords if it is unavailable or unusable.
/// Returns the decision, the model error (if it fell back) and the tokens the moderator used.
pub async fn decide(provider: Option<(&dyn Provider, ChatRequest)>, input: &ModeratorInput<'_>) -> (Decision, Option<String>, Usage) {
    let fallback = || {
        let mut d = fallback::route(input.user_text, input.members, prefers_quick(input.preferences));
        d.source = "fallback".into();
        d
    };
    let Some((provider, mut req)) = provider else { return (fallback(), None, Usage::default()) };
    let mut last_err = None;
    let mut used = Usage::default();
    for attempt in 0..2 {
        match complete(provider, req.clone()).await {
            Ok((text, u)) => {
                used.input_tokens += u.input_tokens;
                used.output_tokens += u.output_tokens;
                if let Some(d) = parse_decision(&text, input.members, input.user_text, input.preferences) {
                    return (d, None, used);
                }
                if attempt == 0 {
                    req.messages.push(ChatMessage::assistant(truncate(&text, 2000)));
                    req.messages.push(ChatMessage::user("That was not a valid decision. Reply with the JSON object only."));
                }
            }
            Err(e) => {
                last_err = Some(e.to_string());
                break;
            }
        }
    }
    (fallback(), last_err, used)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::db::Db;
    use crate::seed::seed_if_needed;

    fn members(chat: &str) -> Vec<Agent> {
        let db = Db::open_in_memory().unwrap();
        seed_if_needed(&db).unwrap();
        let c = db.list_chats().unwrap().into_iter().find(|c| c.name == chat).unwrap();
        db.list_agents().unwrap().into_iter().filter(|a| c.agent_ids.contains(&a.id)).collect()
    }

    fn name_of(members: &[Agent], id: &str) -> String {
        members.iter().find(|a| a.id == id).unwrap().name.clone()
    }

    #[test]
    fn java_question_routes_to_prof_in_learn_mode() {
        // 7.3: "how do I implement a function in Java" in Coding → Prof teaches.
        let m = members("Coding");
        let reply = r#"{"intent":"learn to write a Java function","mode":"learn","targets":["Prof"],"rewrite":"light","permission":"read_only"}"#;
        let d = parse_decision(reply, &m, "How do I implement a function in Java?", &[]).unwrap();
        assert_eq!(d.mode, Mode::Learn);
        assert_eq!(name_of(&m, &d.targets[0]), "Prof");
        // Fallback reaches the same decision without a model.
        let fb = fallback::route("How do I implement a function in Java?", &m, false);
        assert_eq!((fb.mode, name_of(&m, &fb.targets[0])), (Mode::Learn, "Prof".into()));
        // ...unless the user prefers quick implementations.
        let fb = fallback::route("How do I implement a function in Java?", &m, true);
        assert_eq!(fb.mode, Mode::Quick);
        assert_ne!(name_of(&m, &fb.targets[0]), "Prof");
    }

    #[test]
    fn plan_requests_force_planner_and_read_only() {
        // 7.4: plan in Long Horizon Agentic → Micheal, read-only, light rewrite at most.
        let m = members("Long Horizon Agentic");
        let reply = r#"```json
{"intent":"plan auth feature","mode":"plan","targets":["Rick"],"rewrite":"full","permission":"full"}
```"#;
        let d = parse_decision(reply, &m, "Make a plan to add OAuth to my app", &[]).unwrap();
        assert_eq!(d.mode, Mode::Plan);
        assert_eq!(name_of(&m, &d.targets[0]), "Micheal");
        assert_eq!(d.permission, PermissionLevel::ReadOnly);
        assert_eq!(d.rewrite, Rewrite::Light);
        let fb = fallback::route("Make a plan to add OAuth to my app", &m, false);
        assert_eq!((fb.mode, name_of(&m, &fb.targets[0]), fb.permission), (Mode::Plan, "Micheal".into(), PermissionLevel::ReadOnly));
    }

    #[test]
    fn rejects_targets_outside_the_chat_and_handles_garbage() {
        let m = members("Creative");
        let d = parse_decision(r#"{"mode":"implement","targets":["Rick"]}"#, &m, "write a poem about the sea", &[]).unwrap();
        assert_eq!(name_of(&m, &d.targets[0]), "Emma", "Rick is not in Creative; keyword fallback picks Emma");
        assert!(parse_decision("no idea", &m, "x", &[]).is_none());
        assert!(parse_decision(r#"{"mode":"dance"}"#, &m, "x", &[]).is_none());
        let d = parse_decision(r#"{"mode":"clarify"}"#, &m, "x", &[]).unwrap();
        assert_eq!(d.mode, Mode::Answer, "clarify without a question becomes an answer");
    }

    #[test]
    fn fallback_detects_internal_and_generic() {
        let m = members("General");
        assert_eq!(fallback::route("please clean this chat", &m, false).mode, Mode::Internal);
        assert_eq!(fallback::route("hi roger, how are you?", &m, false).mode, Mode::Answer);
        let d = fallback::route("draw a picture of a dolphin", &m, false);
        assert_eq!(name_of(&m, &d.targets[0]), "Sarah");
    }

    #[test]
    fn verifier_is_explicit_only_and_goes_last() {
        let m = members("General");
        let reply = r#"{"intent":"fix bug","mode":"implement","targets":["Amelia","Jordan"]}"#;
        let d = parse_decision(reply, &m, "this crashes on startup, fix it", &[]).unwrap();
        assert_eq!(d.targets.iter().map(|id| name_of(&m, id)).collect::<Vec<_>>(), ["Jordan"], "no check was asked for");

        let reply = r#"{"intent":"fix bug","mode":"implement","targets":["Amelia","Jordan","Rick"]}"#;
        let d = parse_decision(reply, &m, "fix this bug, then double-check your work", &[]).unwrap();
        assert_eq!(d.targets.iter().map(|id| name_of(&m, id)).collect::<Vec<_>>(), ["Jordan", "Rick", "Amelia"]);

        let fb = fallback::route("fix this bug, then double-check your work", &m, false);
        assert_eq!(fb.targets.iter().map(|id| name_of(&m, id)).collect::<Vec<_>>(), ["Jordan", "Amelia"]);
        let fb = fallback::route("please double check that", &m, false);
        assert_eq!(fb.targets.iter().map(|id| name_of(&m, id)).collect::<Vec<_>>(), ["Amelia"]);
    }

    #[test]
    fn fallback_routes_the_new_specialists() {
        let m = members("General");
        let first = |text: &str| name_of(&m, &fallback::route(text, &m, false).targets[0]);
        assert_eq!(first("tell me a fun fact about octopuses"), "Ruby");
        assert_eq!(first("why do cats purr? any fun facts"), "Ruby", "beats the learn flow");
        assert_eq!(first("research the history of the printing press"), "Luna");
        assert_eq!(first("commit my changes with a good message"), "Robert");
        assert_eq!(first("git status please"), "Robert");
        // Not in the chat: Creative has only Emma, Sarah and Ruby.
        let c = members("Creative");
        assert!(fallback::route("commit my changes", &c, false).targets.iter().all(|id| name_of(&c, id) != "Robert"));
    }
}
