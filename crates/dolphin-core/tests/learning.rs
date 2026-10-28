//! Learning end to end with a scripted model that plays both the agents and the learner: which signals queue a
//! lesson, what the learner is shown, what it may write, and that the lesson reaches the agent's next request.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use dolphin_core::db::Db;
use dolphin_core::events::CoreEvent;
use dolphin_core::model::*;
use dolphin_core::providers::{ChatRequest, EventStream, Provider, ProviderEvent, Role, ToolCall, Usage};
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreRef};
use serde_json::json;

const NOTHING: &str = r#"{"ops": []}"#;

fn is_learner(req: &ChatRequest) -> bool {
    req.system.starts_with("You keep the private notes")
}

fn has_tool_results(req: &ChatRequest) -> bool {
    req.messages.iter().any(|m| m.role == Role::Tool)
}

fn tool_results(req: &ChatRequest) -> Vec<String> {
    req.messages.iter().filter(|m| m.role == Role::Tool).map(|m| m.text_content()).collect()
}

type AgentScript = dyn Fn(&ChatRequest) -> Vec<ProviderEvent> + Send + Sync;

struct Scripted {
    /// What the learner answers; changed by a test while it runs.
    learner_reply: Mutex<String>,
    /// What every other request (an agent talking) does. Defaults to a plain "ok".
    agent: Box<AgentScript>,
    requests: Mutex<Vec<ChatRequest>>,
}

#[async_trait]
impl Provider for Scripted {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }
    async fn list_models(&self) -> dolphin_core::Result<Vec<ModelInfo>> {
        Ok(vec![])
    }
    async fn stream_chat(&self, req: ChatRequest) -> dolphin_core::Result<EventStream> {
        self.requests.lock().unwrap().push(req.clone());
        let mut events = if is_learner(&req) {
            vec![ProviderEvent::TextDelta(self.learner_reply.lock().unwrap().clone())]
        } else if req.json_mode && req.system.contains("moderator of \"Dolphin Pod\"") {
            vec![ProviderEvent::TextDelta(json!({"intent": "chat", "mode": "answer", "targets": []}).to_string())]
        } else {
            (self.agent)(&req)
        };
        events.push(ProviderEvent::Usage(Usage { input_tokens: 10, output_tokens: 5 }));
        events.push(ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None });
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

struct Pod {
    core: CoreRef,
    model: Arc<Scripted>,
    _dir: tempfile::TempDir,
}

fn pod_with(agent: impl Fn(&ChatRequest) -> Vec<ProviderEvent> + Send + Sync + 'static) -> Pod {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let cfg = core
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .unwrap();
    let model = Arc::new(Scripted { learner_reply: Mutex::new(NOTHING.into()), agent: Box::new(agent), requests: Mutex::new(vec![]) });
    core.override_provider(&cfg.id, model.clone());
    // Roger's model is also the utility model, so the learner has something to call.
    for a in core.list_agents().unwrap() {
        if ["roger", "lincoln", "rick"].contains(&a.slug.as_str()) {
            let mut a = a;
            a.model = Some(ModelRef { provider_id: cfg.id.clone(), model_id: "qwen3.5:2b".into(), effort: Some(EffortValue::Toggle(true)), vision: false, tools: true, context_window: None });
            core.update_agent(a).unwrap();
        }
    }
    // These tests are about other things; the council has its own tests in council.rs.
    core.set_setting("council.enabled", None, json!(false)).unwrap();
    Pod { core, model, _dir: dir }
}

fn pod() -> Pod {
    pod_with(|_| vec![ProviderEvent::TextDelta("ok".into())])
}

impl Pod {
    fn learner_says(&self, reply: &str) {
        *self.model.learner_reply.lock().unwrap() = reply.to_string();
    }
    fn learner_calls(&self) -> Vec<ChatRequest> {
        self.model.requests.lock().unwrap().iter().filter(|r| is_learner(r)).cloned().collect()
    }
    fn agent_requests(&self, who: &str) -> Vec<ChatRequest> {
        self.model.requests.lock().unwrap().iter().filter(|r| r.system.starts_with(&format!("You are {who}"))).cloned().collect()
    }
    fn memories(&self, slug: &str) -> Vec<AgentMemory> {
        self.core.list_agent_memories(&agent(&self.core, slug).id).unwrap()
    }
    /// Send a message to Rick in Coding and wait until he has answered it (his `n`th answer).
    async fn say_to_rick(&self, text: &str, n: usize) {
        let coding = chat(&self.core, "Coding");
        self.core.send_message(&coding.id, &format!("@Rick {text}"), vec![]).await.unwrap();
        let rick = agent(&self.core, "rick").id;
        until(|| self.core.list_messages(&coding.id, 100).unwrap().iter().filter(|m| m.agent_id.as_deref() == Some(&rick) && m.kind == MessageKind::Text && m.status == MessageStatus::Done).count() >= n).await;
    }
}

fn chat(core: &CoreRef, name: &str) -> Chat {
    core.list_chats().unwrap().into_iter().find(|c| c.name == name).unwrap()
}

fn agent(core: &CoreRef, slug: &str) -> Agent {
    core.list_agents().unwrap().into_iter().find(|a| a.slug == slug).unwrap()
}

async fn until(cond: impl Fn() -> bool) {
    for _ in 0..250 {
        if cond() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting");
}

/// Long enough for a learner call that should NOT happen to have happened.
async fn settle() {
    tokio::time::sleep(Duration::from_millis(400)).await;
}

#[tokio::test]
async fn a_correction_teaches_that_agent_and_the_lesson_reaches_its_next_request() {
    let p = pod();
    p.learner_says(r#"{"ops":[{"op":"add","kind":"style","text":"Use tabs for indentation"}]}"#);
    let mut events = p.core.subscribe();
    p.say_to_rick("format this function", 1).await;
    assert!(p.learner_calls().is_empty(), "an ordinary request is not a signal");

    p.say_to_rick("No, use tabs instead", 2).await;
    until(|| !p.memories("rick").is_empty()).await;
    let m = &p.memories("rick")[0];
    assert_eq!((m.text.as_str(), m.kind, m.source, m.project.as_deref()), ("Use tabs for indentation", MemoryKind::Style, MemorySource::Auto, None));
    assert!(p.memories("roger").is_empty(), "only the agent that was corrected learns");

    // The learner saw the correction, the answer it corrected and the request before it, as a JSON-only task without tools.
    let call = &p.learner_calls()[0];
    let seen = call.messages[0].text_content();
    assert!(call.json_mode && call.tools.is_empty());
    assert!(seen.contains("No, use tabs instead") && seen.contains("format this function") && seen.contains("The assistant's answer"), "{seen}");
    assert!(seen.contains("Assistant: Rick"), "{seen}");

    // The UI is told, and the signal is used up.
    let mut learned = None;
    while let Ok(e) = events.try_recv() {
        if let CoreEvent::MemoryLearned { memory, previous_text } = e {
            learned = Some((memory.text, previous_text));
        }
    }
    assert_eq!(learned, Some(("Use tabs for indentation".to_string(), None)));
    until(|| p.core.db.agents_with_pending_feedback().unwrap().is_empty()).await;

    // Next time Rick is sent the lesson, and he is offered the tool to save more.
    p.say_to_rick("format another one", 3).await;
    let last = p.agent_requests("Rick").pop().unwrap();
    assert!(last.messages.last().unwrap().text_content().contains("- Use tabs for indentation"));
    assert!(last.tools.iter().any(|t| t.name == "remember"));
    assert!(last.system.contains("remember tool"));
}

#[tokio::test]
async fn polite_follow_ups_are_not_signals() {
    let p = pod();
    p.say_to_rick("write hello world", 1).await;
    p.say_to_rick("thanks, now the same in Go", 2).await;
    p.say_to_rick("No problem, I'll take it from here", 3).await;
    settle().await;
    assert!(p.learner_calls().is_empty());
    assert!(p.memories("rick").is_empty());
}

#[tokio::test]
async fn a_learner_with_nothing_lasting_to_say_changes_nothing() {
    let p = pod();
    p.say_to_rick("write hello world", 1).await;
    p.say_to_rick("No, that's not what I asked", 2).await;
    until(|| p.core.db.agents_with_pending_feedback().unwrap().is_empty() && !p.learner_calls().is_empty()).await;
    assert_eq!(p.learner_calls().len(), 1);
    assert!(p.memories("rick").is_empty());
}

#[tokio::test]
async fn remember_that_is_kept_by_the_agent_that_answered_it() {
    let p = pod();
    p.learner_says(r#"{"ops":[{"op":"add","kind":"workflow","text":"Use pnpm, not npm"}]}"#);
    p.say_to_rick("remember that I always use pnpm", 1).await;
    until(|| !p.memories("rick").is_empty()).await;
    let m = &p.memories("rick")[0];
    assert_eq!((m.text.as_str(), m.kind, m.source), ("Use pnpm, not npm", MemoryKind::Workflow, MemorySource::Explicit), "the user asked, so it is theirs");
    assert!(p.learner_calls()[0].system.contains("EXPLICITLY"));
    assert!(p.memories("roger").is_empty());
}

#[tokio::test]
async fn an_explicit_request_is_kept_as_typed_when_the_learner_cannot_help() {
    let p = pod();
    p.learner_says("Sorry, I cannot do that.");
    p.say_to_rick("remember that I prefer Neovim", 1).await;
    until(|| !p.memories("rick").is_empty()).await;
    let m = &p.memories("rick")[0];
    assert_eq!((m.text.as_str(), m.kind, m.source), ("I prefer Neovim", MemoryKind::Fact, MemorySource::Explicit));

    // And a secret is never kept, even when asked.
    p.say_to_rick("remember that the password is hunter2", 2).await;
    settle().await;
    assert_eq!(p.memories("rick").len(), 1, "{:?}", p.memories("rick"));
}

#[tokio::test]
async fn turning_learning_off_stops_signals_and_the_tool_but_not_remember() {
    let p = pod();
    p.core.set_setting("agent.learning", Some("Rick"), json!(false)).unwrap();
    assert!(!agent(&p.core, "rick").learning);
    p.learner_says(r#"{"ops":[{"op":"add","kind":"style","text":"Should never be written"}]}"#);

    p.say_to_rick("format this", 1).await;
    p.say_to_rick("No, use tabs instead", 2).await;
    settle().await;
    assert!(p.learner_calls().is_empty() && p.memories("rick").is_empty(), "nothing is learned automatically");
    assert!(p.core.db.pending_feedback(&agent(&p.core, "rick").id).unwrap().is_empty(), "not even recorded");
    assert!(p.agent_requests("Rick").iter().all(|r| !r.tools.iter().any(|t| t.name == "remember") && !r.system.contains("remember tool")));

    // The user's own words still work.
    p.learner_says("no json");
    p.say_to_rick("remember that I work in Rust", 3).await;
    until(|| !p.memories("rick").is_empty()).await;
    assert_eq!(p.memories("rick")[0].text, "I work in Rust");
}

#[tokio::test]
async fn the_remember_tool_saves_a_note_and_refuses_secrets() {
    let args = Arc::new(Mutex::new(json!({ "text": "Prefers short answers", "kind": "style" })));
    let for_script = args.clone();
    let p = pod_with(move |req| {
        if has_tool_results(req) {
            vec![ProviderEvent::TextDelta(format!("result: {}", tool_results(req).join("|")))]
        } else {
            vec![ProviderEvent::ToolCall(ToolCall { id: "t1".into(), name: "remember".into(), arguments: for_script.lock().unwrap().clone() })]
        }
    });
    p.say_to_rick("I like short answers", 1).await;
    let m = &p.memories("rick")[0];
    assert_eq!((m.text.as_str(), m.kind, m.source), ("Prefers short answers", MemoryKind::Style, MemorySource::Auto));
    let coding = chat(&p.core, "Coding");
    let answer = p.core.list_messages(&coding.id, 50).unwrap().into_iter().find(|m| m.agent_id.is_some() && m.kind == MessageKind::Text).unwrap();
    assert!(answer.content.contains("Saved"), "{}", answer.content);
    assert_eq!(answer.meta["tools"][0]["summary"], "Noted: Prefers short answers");

    *args.lock().unwrap() = json!({ "text": "The API key is sk-abcdefghijklmnopqrstuvwxyz" });
    p.say_to_rick("here is my key", 2).await;
    assert_eq!(p.memories("rick").len(), 1, "a secret is refused");
    let answers: Vec<String> = p.core.list_messages(&coding.id, 50).unwrap().into_iter().filter(|m| m.agent_id.is_some() && m.kind == MessageKind::Text).map(|m| m.content).collect();
    assert!(answers.last().unwrap().contains("secret"), "{answers:?}");

    // The same note twice is not saved twice.
    *args.lock().unwrap() = json!({ "text": "prefers SHORT answers", "kind": "style" });
    p.say_to_rick("short please", 3).await;
    assert_eq!(p.memories("rick").len(), 1);
}

#[tokio::test]
async fn a_thumbs_down_teaches_but_a_thumbs_up_alone_does_not_and_rating_twice_counts_once() {
    let p = pod();
    p.learner_says(r#"{"ops":[{"op":"add","kind":"avoid","text":"Do not pad answers with apologies"}]}"#);
    p.say_to_rick("write hello world", 1).await;
    let coding = chat(&p.core, "Coding");
    let answer = p.core.list_messages(&coding.id, 50).unwrap().into_iter().find(|m| m.agent_id.is_some() && m.kind == MessageKind::Text).unwrap();

    p.core.rate_message(&answer.id, Some(Rating::Up)).unwrap();
    settle().await;
    assert!(p.learner_calls().is_empty(), "praise alone is not worth a model call");

    p.core.rate_message(&answer.id, Some(Rating::Down)).unwrap();
    until(|| !p.memories("rick").is_empty()).await;
    let seen = p.learner_calls()[0].messages[0].text_content();
    assert!(seen.contains("thumbs down") && seen.contains("thumbs up"), "the earlier praise rides along as context: {seen}");
    assert!(seen.contains("write hello world"), "{seen}");
    assert_eq!(p.memories("rick")[0].kind, MemoryKind::Avoid);

    p.core.rate_message(&answer.id, Some(Rating::Down)).unwrap();
    settle().await;
    assert_eq!(p.learner_calls().len(), 1, "the same rating again is not a new signal");
    let stored = p.core.list_messages(&coding.id, 50).unwrap().into_iter().find(|m| m.id == answer.id).unwrap();
    assert_eq!(stored.meta["rating"], "down");
    p.core.rate_message(&answer.id, None).unwrap();
    let stored = p.core.list_messages(&coding.id, 50).unwrap().into_iter().find(|m| m.id == answer.id).unwrap();
    assert!(stored.meta.get("rating").is_none());

    let user_msg = p.core.list_messages(&coding.id, 50).unwrap().into_iter().find(|m| m.agent_id.is_none()).unwrap();
    assert!(p.core.rate_message(&user_msg.id, Some(Rating::Up)).is_err(), "only an agent's message can be rated");
}

#[tokio::test]
async fn the_learner_never_rewrites_what_the_user_wrote() {
    let p = pod();
    let rick = agent(&p.core, "rick");
    let mine = p.core.add_agent_memory(&rick.id, MemoryKind::Style, "British English", None).unwrap();
    // Note 1 is the locked one the user wrote. The model tries to delete and reword it anyway.
    p.learner_says(r#"{"ops":[{"op":"delete","id":1},{"op":"update","id":1,"text":"American English"},{"op":"add","kind":"fact","text":"Works in finance"}]}"#);
    p.say_to_rick("write a summary", 1).await;
    p.say_to_rick("Don't use that word", 2).await;
    until(|| p.memories("rick").len() == 2).await;
    let now = p.memories("rick");
    assert!(now.iter().any(|m| m.id == mine.id && m.text == "British English"), "{now:?}");
    assert!(now.iter().any(|m| m.text == "Works in finance"));
}

/// An agent that runs one shell command and reports what came back.
fn ask_level_pod() -> (Pod, tempfile::TempDir, Chat) {
    let p = pod_with(|req| {
        if has_tool_results(req) {
            vec![ProviderEvent::TextDelta(format!("result: {}", tool_results(req).join("|")))]
        } else {
            vec![ProviderEvent::ToolCall(ToolCall { id: "c1".into(), name: "run_command".into(), arguments: json!({ "command": "echo hi > out.txt" }) })]
        }
    });
    let ws = tempfile::tempdir().unwrap();
    let mut c = chat(&p.core, "Coding");
    c.workspace_path = Some(ws.path().to_string_lossy().into_owned());
    p.core.update_chat(c.clone()).unwrap();
    p.core.grant_permission(&c.id, &agent(&p.core, "rick").id, PermissionLevel::Ask).unwrap();
    (p, ws, c)
}

async fn next_question(rx: &mut tokio::sync::broadcast::Receiver<CoreEvent>) -> String {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(CoreEvent::QuestionRequest { id, .. }) = rx.recv().await {
                return id;
            }
        }
    })
    .await
    .expect("a question was asked")
}

#[tokio::test]
async fn declining_an_action_is_a_signal_but_approving_or_dismissing_it_is_not() {
    let (p, ws, coding) = ask_level_pod();
    p.learner_says(r#"{"ops":[{"op":"avoid","text":"ignored: not a real op"},{"op":"add","kind":"avoid","text":"Do not run shell redirections without asking first"}]}"#);
    let mut rx = p.core.subscribe();

    // Approving, then dismissing a question: no signal.
    p.core.send_message(&coding.id, "@Rick make the file", vec![]).await.unwrap();
    let id = next_question(&mut rx).await;
    p.core.answer_question(&id, "Allow once", false);
    until(|| ws.path().join("out.txt").exists()).await;
    p.core.send_message(&coding.id, "@Rick again", vec![]).await.unwrap();
    let id = next_question(&mut rx).await;
    p.core.answer_question(&id, "", true);
    settle().await;
    assert!(p.learner_calls().is_empty(), "allowing or dismissing is not feedback");

    // Declining is.
    p.core.send_message(&coding.id, "@Rick once more", vec![]).await.unwrap();
    let id = next_question(&mut rx).await;
    p.core.answer_question(&id, "Deny", false);
    until(|| !p.memories("rick").is_empty()).await;
    let seen = p.learner_calls()[0].messages[0].text_content();
    assert!(seen.contains("declined an action") && seen.contains("echo hi > out.txt"), "{seen}");
    assert_eq!(p.memories("rick")[0].text, "Do not run shell redirections without asking first");
}

#[tokio::test]
async fn reverting_a_run_is_a_signal() {
    if std::process::Command::new("git").arg("--version").output().is_err() {
        return;
    }
    let p = pod_with(|req| if has_tool_results(req) { vec![ProviderEvent::TextDelta("done".into())] } else { vec![ProviderEvent::ToolCall(ToolCall { id: "w1".into(), name: "write_file".into(), arguments: json!({ "path": "notes.txt", "content": "changed\n" }) })] });
    p.learner_says(r#"{"ops":[{"op":"add","kind":"workflow","text":"Show a plan before changing files"}]}"#);
    let ws = tempfile::tempdir().unwrap();
    std::fs::write(ws.path().join("notes.txt"), "original\n").unwrap();
    let mut c = chat(&p.core, "Coding");
    c.workspace_path = Some(ws.path().to_string_lossy().into_owned());
    p.core.update_chat(c.clone()).unwrap();
    p.core.grant_permission(&c.id, &agent(&p.core, "rick").id, PermissionLevel::Full).unwrap();

    p.say_to_rick("tidy the notes", 1).await;
    let run = p.core.list_messages(&c.id, 50).unwrap().into_iter().find(|m| m.agent_id.is_some() && m.kind == MessageKind::Text).unwrap();
    p.core.revert_run(&run.id).await.unwrap();
    until(|| !p.memories("rick").is_empty()).await;
    let seen = p.learner_calls()[0].messages[0].text_content();
    assert!(seen.contains("reverted the file changes") && seen.contains("(1 file(s))") && seen.contains("tidy the notes"), "{seen}");
    // A project-less note stays global; the workspace folder name is only the project when the learner asks for it.
    assert_eq!(p.memories("rick")[0].project, None);
    assert!(seen.contains(&format!("Current project: {}", ws.path().file_name().unwrap().to_string_lossy())), "{seen}");
}

// ---------------------------------------------------------------- reflecting on quiet chats

const MINUTE: i64 = 60_000;

fn now() -> i64 {
    dolphin_core::util::now_ms()
}

/// Two more exchanges with Rick, who has already answered `before` times: four more messages in the chat.
async fn talk_with_rick(p: &Pod, before: usize) {
    p.say_to_rick("write the function with tabs", before + 1).await;
    p.say_to_rick("and keep the answers short", before + 2).await;
}

#[tokio::test]
async fn looking_back_is_off_until_the_user_turns_it_on() {
    let p = pod();
    p.learner_says(r#"{"ops":[{"op":"add","kind":"style","text":"Prefers short answers"}]}"#);
    talk_with_rick(&p, 0).await;
    assert_eq!(p.core.reflect_idle(now() + 60 * MINUTE), 0);
    settle().await;
    assert!(p.learner_calls().is_empty(), "nothing is sent to the utility model");
}

#[tokio::test]
async fn a_quiet_chat_is_looked_back_over_once_by_each_agent_that_took_part() {
    let p = pod();
    p.core.set_setting("learning.reflect", None, json!(true)).unwrap();
    p.learner_says(r#"{"ops":[{"op":"add","kind":"style","text":"Prefers short answers"}]}"#);
    assert_eq!(p.core.reflect_idle(now()), 0, "turning it on only starts the clock");
    tokio::time::sleep(Duration::from_millis(5)).await;

    talk_with_rick(&p, 0).await;
    assert_eq!(p.core.reflect_idle(now() + MINUTE), 0, "the chat is not quiet yet");
    assert_eq!(p.core.reflect_idle(now() + 11 * MINUTE), 1, "Rick is the only agent that spoke");
    until(|| !p.memories("rick").is_empty()).await;

    let m = &p.memories("rick")[0];
    assert_eq!((m.text.as_str(), m.source), ("Prefers short answers", MemorySource::Auto));
    let call = &p.learner_calls()[0];
    let seen = call.messages[0].text_content();
    assert!(call.system.contains("finished conversation") && !call.system.contains("EXPLICITLY"));
    assert!(seen.contains("User: @Rick write the function with tabs") && seen.contains("Rick: ok") && seen.contains("User: @Rick and keep the answers short"), "{seen}");
    assert!(p.memories("roger").is_empty(), "agents that said nothing learn nothing");

    // The same messages are not looked at twice; new ones are.
    assert_eq!(p.core.reflect_idle(now() + 30 * MINUTE), 0);
    talk_with_rick(&p, 2).await;
    assert_eq!(p.core.reflect_idle(now() + 60 * MINUTE), 1);
}

#[tokio::test]
async fn only_conversations_after_it_was_turned_on_count_and_short_ones_are_skipped() {
    let p = pod();
    talk_with_rick(&p, 0).await;
    p.core.set_setting("learning.reflect", None, json!(true)).unwrap();
    assert_eq!(p.core.reflect_idle(now()), 0);
    assert_eq!(p.core.reflect_idle(now() + 60 * MINUTE), 0, "what was said before it was turned on stays private");

    tokio::time::sleep(Duration::from_millis(5)).await;
    p.say_to_rick("one more thing", 3).await;
    assert_eq!(p.core.reflect_idle(now() + 60 * MINUTE), 0, "two messages are not worth a model call");
    settle().await;
    assert!(p.learner_calls().is_empty());

    // Turning it off forgets the start, so turning it on later does not reach back over the time in between.
    p.core.set_setting("learning.reflect", None, json!(false)).unwrap();
    p.core.reflect_idle(now());
    assert_eq!(p.core.db.setting_raw("device.reflect_since").unwrap(), Some(serde_json::Value::Null));
}

#[tokio::test]
async fn an_agent_that_is_not_learning_is_not_looked_back_over() {
    let p = pod();
    p.core.set_setting("learning.reflect", None, json!(true)).unwrap();
    p.core.reflect_idle(now());
    tokio::time::sleep(Duration::from_millis(5)).await;
    p.core.set_setting("agent.learning", Some("Rick"), json!(false)).unwrap();
    talk_with_rick(&p, 0).await;
    assert_eq!(p.core.reflect_idle(now() + 60 * MINUTE), 0);
    settle().await;
    assert!(p.learner_calls().is_empty());
}

// ---------------------------------------------------------------- persona suggestions

const PERSONA_OP: &str = r#"{"ops":[{"op":"persona","text":"Dry and terse. Calls the user chief.","reason":"They asked for short answers three times"}]}"#;

fn proposal(p: &Pod) -> Option<PersonaProposal> {
    p.core.persona_proposal(&agent(&p.core, "rick").id).unwrap()
}

/// A correction for Rick, which is what gets the learner to look at his persona.
async fn correct_rick(p: &Pod, answered_before: usize) {
    p.say_to_rick("write hello world", answered_before + 1).await;
    p.say_to_rick("No, shorter. Always shorter", answered_before + 2).await;
}

#[tokio::test]
async fn a_persona_suggestion_waits_for_the_user_and_only_accepting_changes_the_persona() {
    let p = pod();
    p.core.set_setting("agent.suggest_persona", Some("Rick"), json!(true)).unwrap();
    p.learner_says(PERSONA_OP);
    let mut events = p.core.subscribe();
    correct_rick(&p, 0).await;
    until(|| proposal(&p).is_some()).await;

    let s = proposal(&p).unwrap();
    assert_eq!((s.text.as_str(), s.reason.as_str()), ("Dry and terse. Calls the user chief.", "They asked for short answers three times"));
    assert_eq!(agent(&p.core, "rick").persona, None, "a suggestion is not a persona");
    let call = &p.learner_calls()[0];
    assert!(call.system.contains("\"op\":\"persona\"") && call.messages[0].text_content().contains("Current persona: (none)"));
    let mut told = false;
    while let Ok(e) = events.try_recv() {
        told |= matches!(e, CoreEvent::PersonaProposalChanged { proposal: Some(_), .. });
    }
    assert!(told, "the UI hears about it");
    assert!(p.memories("rick").is_empty(), "and it did not turn into a note");

    // Accepting is what changes it, and the same text is not suggested again.
    let saved = p.core.accept_persona_proposal(&s.id).unwrap();
    assert_eq!(saved.persona.as_deref(), Some("Dry and terse. Calls the user chief."));
    assert_eq!(agent(&p.core, "rick").persona, saved.persona);
    assert!(proposal(&p).is_none());
    assert!(p.core.accept_persona_proposal(&s.id).is_err(), "a suggestion is dealt with once");
    correct_rick(&p, 2).await;
    settle().await;
    assert!(proposal(&p).is_none(), "the same text as the persona now is not worth suggesting");
}

#[tokio::test]
async fn a_dismissed_suggestion_does_not_come_back_but_a_different_one_can() {
    let p = pod();
    p.core.set_setting("agent.suggest_persona", Some("Rick"), json!(true)).unwrap();
    p.learner_says(PERSONA_OP);
    correct_rick(&p, 0).await;
    until(|| proposal(&p).is_some()).await;
    p.core.reject_persona_proposal(&proposal(&p).unwrap().id).unwrap();
    assert!(proposal(&p).is_none());
    assert_eq!(agent(&p.core, "rick").persona, None);

    correct_rick(&p, 2).await;
    until(|| p.learner_calls().len() >= 2).await;
    settle().await;
    assert!(proposal(&p).is_none(), "turned down once, not suggested again");

    p.learner_says(r#"{"ops":[{"op":"persona","text":"Friendly and brief.","reason":"asked again"}]}"#);
    correct_rick(&p, 4).await;
    until(|| proposal(&p).is_some()).await;
    assert_eq!(proposal(&p).unwrap().text, "Friendly and brief.");
}

#[tokio::test]
async fn suggestions_are_off_by_default_and_unsafe_ones_are_dropped() {
    let p = pod();
    p.learner_says(PERSONA_OP);
    correct_rick(&p, 0).await;
    until(|| !p.learner_calls().is_empty()).await;
    settle().await;
    assert!(!p.learner_calls()[0].system.contains("\"op\":\"persona\""), "the model is not even offered it");
    assert!(proposal(&p).is_none(), "and a suggestion it makes anyway is ignored");

    p.core.set_setting("agent.suggest_persona", Some("Rick"), json!(true)).unwrap();
    p.learner_says(r#"{"ops":[{"op":"persona","text":"My password is hunter2, use it","reason":"x"}]}"#);
    correct_rick(&p, 2).await;
    until(|| p.learner_calls().len() >= 2).await;
    settle().await;
    assert!(proposal(&p).is_none(), "a secret never becomes a suggestion");
    p.learner_says(&format!(r#"{{"ops":[{{"op":"persona","text":"{}","reason":"x"}}]}}"#, "x".repeat(1501)));
    correct_rick(&p, 4).await;
    until(|| p.learner_calls().len() >= 3).await;
    settle().await;
    assert!(proposal(&p).is_none(), "nor does one over the persona limit");
}
