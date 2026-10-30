//! Vera, the software director: she routes the development chats in place of Roger and Lincoln.
//! The decision has the moderator's shape, but the team is bigger and the order matters: designers first,
//! then the other creative specialists, then the engineers, then the coder, then the check.

use super::moderator::{ModeratorInput, decide_with, is_unrouted, parse_decision_with, prefers_quick};
use super::prompts::first_sentence;
use super::{Decision, Mode, fallback};
use crate::model::{Agent, AgentRole};
use crate::providers::{ChatRequest, Provider, Usage};

/// Designers, engineers, a coder and a check, plus a couple of game specialists.
const MAX_TARGETS: usize = 7;

pub fn is_director(a: &Agent) -> bool {
    a.specialty == "director"
}

pub fn system_prompt(i: &ModeratorInput, director: &str) -> String {
    let roster: Vec<String> = i
        .members
        .iter()
        .filter(|a| a.role == AgentRole::Specialist && !is_unrouted(a))
        .map(|a| format!("- {} [{}]: {}", a.name, a.specialty, first_sentence(&a.job)))
        .collect();
    let prefs = if i.preferences.is_empty() { "(none)".to_string() } else { i.preferences.join("\n") };
    format!(
        r#"You are {director}, the software director of "Dolphin Pod", a group chat of AI agents. You do not answer the user here: you decide who works on their latest message in the development chat "{chat}", and in what order.

Team:
{roster}

User preferences:
{prefs}

{pending}

Reply with ONE JSON object and nothing else:
{{"intent": "<5-12 words>", "mode": "quick|learn|plan|implement|answer|internal|clarify", "targets": ["<name>"], "rewrite": "none|light|full", "permission": "read_only|full", "approves_plan": false, "clarification": null, "lean_prompt_note": null}}

Modes:
- answer: greetings, small talk, general questions about the pod. The moderator answers; targets = [].
- internal: the user asks to change a setting or to clean a chat. targets = [].
- clarify: the request is too vague to act on. Put one short question in "clarification".
- learn: the user wants to understand something. Route to the teacher if there is one.
- quick: they want a fast, direct result from one specialist.
- plan: they ask for a plan or roadmap. Route to the long-horizon agent; permission "read_only".
- implement: they want something designed, built, fixed or changed.

How the team works:
- Engineers (specialty engineer) engineer the software BEFORE the coder writes code: for anything the coder builds, name the engineers too (both) and they will be put in front of the coder. Engineers are not needed for a one-line fix or a question.
- Designers (specialty designer) each produce one design idea in parallel and the user picks or merges them. Name both designers when the user wants something designed (an interface, a page, a look, a system); do not name them for pure code work.
- Game specialists (level designer, 3D artist, audio designer, story designer) take only the part of a game that is theirs.
- A verification agent is used ONLY when the user explicitly asks to double-check or verify work. Never add it on your own.
- A research agent handles web research with sources; a git agent handles git work.

Rules: targets must come from the team above (use their names). Pick the smallest team that does the job. rewrite: "none" if the request is already precise, "light" to tidy it, "full" if it needs structure. approves_plan is true only when the user is approving the pending plan. Set lean_prompt_note to a short tip when the request is actionable but missing details that would help."#,
        chat = i.chat_name,
        roster = if roster.is_empty() { "- (no specialists)".into() } else { roster.join("\n") },
        pending = match i.pending_plan_by {
            Some(n) => format!("{n} has proposed a plan that is waiting for the user's approval."),
            None => "No plan is waiting for approval.".into(),
        },
    )
}

/// Where a specialty works in the order of a job.
fn phase(a: &Agent) -> u8 {
    match a.specialty.as_str() {
        "designer" => 0,
        "level_designer" | "artist_3d" | "audio_designer" | "story_designer" => 1,
        "engineer" => 2,
        "coder" => 3,
        "verifier" => 5,
        _ => 4,
    }
}

fn add_all<'a>(team: &mut Vec<&'a Agent>, members: &'a [Agent], spec: &str) {
    for a in members.iter().filter(|a| a.role == AgentRole::Specialist && a.specialty == spec) {
        if !team.iter().any(|t| t.id == a.id) {
            team.push(a);
        }
    }
}

/// Put the team in working order and fill it out: both engineers or neither, both designers or neither,
/// and engineers in front of the coder whenever the coder builds something.
pub fn arrange(d: &mut Decision, members: &[Agent]) {
    // The planner alone plans; answers and settings have no team.
    if !matches!(d.mode, Mode::Quick | Mode::Learn | Mode::Implement) || d.targets.is_empty() {
        return;
    }
    let mut team: Vec<&Agent> = d.targets.iter().filter_map(|id| members.iter().find(|m| &m.id == id)).collect();
    let has = |team: &[&Agent], spec: &str| team.iter().any(|a| a.specialty == spec);
    for spec in ["engineer", "designer"] {
        if has(&team, spec) {
            add_all(&mut team, members, spec);
        }
    }
    if d.mode == Mode::Implement && has(&team, "coder") && !has(&team, "engineer") {
        add_all(&mut team, members, "engineer");
    }
    // Stable, so agents of one phase keep the order the model gave them.
    team.sort_by_key(|a| phase(a));
    let verifier = team.iter().position(|a| a.specialty == "verifier");
    let mut targets: Vec<String> = team.iter().map(|a| a.id.clone()).collect();
    if let Some(v) = verifier {
        // Never let the cap drop the check the user asked for.
        let id = targets.remove(v);
        targets.truncate(MAX_TARGETS - 1);
        targets.push(id);
    } else {
        targets.truncate(MAX_TARGETS);
    }
    d.targets = targets;
}

/// Keyword routing, then the same team arrangement; used when the director has no model or her reply is unusable.
pub fn fallback_decision(input: &ModeratorInput) -> Decision {
    let mut d = fallback::route(input.user_text, input.members, prefers_quick(input.preferences));
    d.source = "fallback".into();
    arrange(&mut d, input.members);
    d
}

/// Run the director's model; fall back to keywords. Returns the decision, the model error (if it fell back) and the tokens used.
pub async fn decide(provider: Option<(&dyn Provider, ChatRequest)>, input: &ModeratorInput<'_>) -> (Decision, Option<String>, Usage) {
    let parse = |text: &str| {
        let mut d = parse_decision_with(text, input.members, input.user_text, input.preferences, MAX_TARGETS)?;
        arrange(&mut d, input.members);
        Some(d)
    };
    let fallback = || fallback_decision(input);
    decide_with(provider, &parse, &fallback).await
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

    fn names(m: &[Agent], d: &Decision) -> Vec<String> {
        d.targets.iter().map(|id| m.iter().find(|a| &a.id == id).unwrap().name.clone()).collect()
    }

    fn input<'a>(m: &'a [Agent], text: &'a str) -> ModeratorInput<'a> {
        ModeratorInput { chat_name: "Software Development", members: m, history: &[], user_text: text, preferences: &[], pending_plan_by: None }
    }

    #[test]
    fn engineers_come_before_the_coder() {
        let m = members("Software Development");
        let reply = r#"{"intent":"build a parser","mode":"implement","targets":["Rick"]}"#;
        let mut d = parse_decision_with(reply, &m, "Build a CSV parser in Rust", &[], MAX_TARGETS).unwrap();
        arrange(&mut d, &m);
        assert_eq!(names(&m, &d), ["Nolan", "Priya", "Rick"]);
    }

    #[test]
    fn both_engineers_and_both_designers_or_neither() {
        let m = members("Web Development");
        let mut d = parse_decision_with(r#"{"mode":"implement","targets":["Rick","Kai"]}"#, &m, "Build and design a landing page", &[], MAX_TARGETS).unwrap();
        arrange(&mut d, &m);
        assert_eq!(names(&m, &d), ["Kai", "Mira", "Nolan", "Priya", "Rick"], "designers, engineers, then the coder");

        let mut d = parse_decision_with(r#"{"mode":"implement","targets":["Nolan"]}"#, &m, "Review the architecture", &[], MAX_TARGETS).unwrap();
        arrange(&mut d, &m);
        assert_eq!(names(&m, &d), ["Nolan", "Priya"]);
    }

    #[test]
    fn quick_fixes_plans_and_answers_get_no_extra_team() {
        let m = members("Software Development");
        let mut d = parse_decision_with(r#"{"mode":"quick","targets":["Rick"]}"#, &m, "quickly rename this variable", &[], MAX_TARGETS).unwrap();
        arrange(&mut d, &m);
        assert_eq!(names(&m, &d), ["Rick"]);

        let mut d = parse_decision_with(r#"{"mode":"plan","targets":["Rick"]}"#, &m, "Make a plan to add OAuth", &[], MAX_TARGETS).unwrap();
        arrange(&mut d, &m);
        assert_eq!(names(&m, &d), ["Micheal"], "the planner plans alone");
        let mut d = parse_decision_with(r#"{"mode":"answer","targets":[]}"#, &m, "hi", &[], MAX_TARGETS).unwrap();
        arrange(&mut d, &m);
        assert!(d.targets.is_empty());
    }

    #[test]
    fn the_director_is_never_a_target_and_the_check_stays_last() {
        let m = members("Software Development");
        let reply = r#"{"mode":"implement","targets":["Vera","Amelia","Rick"]}"#;
        let mut d = parse_decision_with(reply, &m, "Build a CLI, then double-check your work", &[], MAX_TARGETS).unwrap();
        arrange(&mut d, &m);
        assert_eq!(names(&m, &d), ["Nolan", "Priya", "Rick", "Amelia"]);
    }

    #[test]
    fn game_specialists_work_before_the_engineers() {
        let m = members("Game Development");
        let mut d = parse_decision_with(r#"{"mode":"implement","targets":["Rick","Tilda"]}"#, &m, "Build the first level of my platformer", &[], MAX_TARGETS).unwrap();
        arrange(&mut d, &m);
        assert_eq!(names(&m, &d), ["Tilda", "Nolan", "Priya", "Rick"]);
    }

    #[test]
    fn keyword_fallback_builds_the_same_team() {
        let m = members("Software Development");
        let d = fallback_decision(&input(&m, "implement a function that parses dates"));
        assert_eq!(d.source, "fallback");
        assert_eq!(names(&m, &d), ["Nolan", "Priya", "Rick"]);

        let d = fallback_decision(&input(&m, "give me a mockup for the profile screen"));
        assert_eq!(names(&m, &d), ["Mira", "Kai"]);

        let g = members("Game Development");
        let d = fallback_decision(&input(&g, "write the dialogue for the blacksmith quest"));
        assert_eq!(names(&g, &d), ["Wren"]);
    }

    #[test]
    fn the_prompt_lists_the_team_but_not_the_director_or_the_council() {
        let m = members("Game Development");
        let p = system_prompt(&input(&m, "x"), "Vera");
        assert!(p.contains("- Tilda [level_designer]") && p.contains("- Nolan [engineer]"));
        assert!(!p.contains("- Vera [") && !p.contains("[council]"));
    }
}
