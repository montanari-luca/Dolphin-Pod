//! DP Council: one critique before a big task (or on a plan's draft), on the strongest model, and never routed to.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use dolphin_core::db::Db;
use dolphin_core::model::*;
use dolphin_core::providers::{ChatRequest, EventStream, Provider, ProviderEvent};
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreRef};
use serde_json::json;

struct Scripted {
    requests: Mutex<Vec<ChatRequest>>,
    /// What the moderator decides, from the user's text.
    route: Box<dyn Fn(&str) -> serde_json::Value + Send + Sync>,
    council_fails: bool,
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
        if self.council_fails && req.system.starts_with("You are DP Council") {
            return Err(dolphin_core::CoreError::invalid("the council's provider is down"));
        }
        let text = if req.json_mode && req.system.contains("moderator of \"Dolphin Pod\"") {
            let user = req.messages[0].text_content();
            (self.route)(user.rsplit("Latest user message:").next().unwrap_or(&user)).to_string()
        } else if req.system.starts_with("You are Lincoln") {
            "Do the task well.".to_string()
        } else {
            let who = req.system.trim_start_matches("You are ").split(',').next().unwrap_or("?").to_string();
            if who == "DP Council" { "Solid: the idea. Risks: no tests. Do this instead: add tests.".to_string() } else { format!("{who} here. Done.") }
        };
        Ok(Box::pin(futures::stream::iter(vec![
            ProviderEvent::TextDelta(text),
            ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None },
        ])))
    }
}

struct Pod {
    core: CoreRef,
    scripted: Arc<Scripted>,
    provider_id: String,
    _dir: tempfile::TempDir,
}

fn model(provider_id: &str, id: &str, effort: &str) -> ModelRef {
    ModelRef { provider_id: provider_id.into(), model_id: id.into(), effort: Some(EffortValue::Level(effort.into())), vision: false, tools: true, context_window: None }
}

/// `models`: slug → (model id, effort). The council gets a model only if it is listed.
fn pod(models: &[(&str, &str, &str)], route: impl Fn(&str) -> serde_json::Value + Send + Sync + 'static, council_fails: bool) -> Pod {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let cfg = core
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .unwrap();
    let scripted = Arc::new(Scripted { requests: Mutex::new(vec![]), route: Box::new(route), council_fails });
    core.override_provider(&cfg.id, scripted.clone());
    for a in core.list_agents().unwrap() {
        if let Some((_, id, effort)) = models.iter().find(|(slug, _, _)| *slug == a.slug) {
            let mut a = a;
            a.model = Some(model(&cfg.id, id, effort));
            core.update_agent(a).unwrap();
        }
    }
    Pod { core, scripted, provider_id: cfg.id, _dir: dir }
}

fn chat(core: &CoreRef, name: &str) -> Chat {
    core.list_chats().unwrap().into_iter().find(|c| c.name == name).unwrap()
}

fn agent(core: &CoreRef, slug: &str) -> Agent {
    core.list_agents().unwrap().into_iter().find(|a| a.slug == slug).unwrap()
}

async fn wait_for(core: &CoreRef, chat_id: &str, pred: impl Fn(&[Message]) -> bool) -> Vec<Message> {
    for _ in 0..300 {
        let msgs = core.list_messages(chat_id, 100).unwrap();
        if pred(&msgs) {
            return msgs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out: {:#?}", core.list_messages(chat_id, 100).unwrap().iter().map(|m| (&m.agent_id, &m.kind, &m.status, &m.content)).collect::<Vec<_>>());
}

fn done_by(msgs: &[Message], id: &str) -> usize {
    msgs.iter().filter(|m| m.agent_id.as_deref() == Some(id) && m.status == MessageStatus::Done && m.kind != MessageKind::Notice).count()
}

fn council_requests(p: &Pod) -> Vec<ChatRequest> {
    p.scripted.requests.lock().unwrap().iter().filter(|r| r.system.starts_with("You are DP Council")).cloned().collect()
}

fn implement(targets: &[&str]) -> impl Fn(&str) -> serde_json::Value + Send + Sync + 'static {
    let targets: Vec<String> = targets.iter().map(|t| t.to_string()).collect();
    move |_| json!({"intent": "build it", "mode": "implement", "targets": targets, "rewrite": "light", "permission": "read_only"})
}

const BASE: &[(&str, &str, &str)] = &[("roger", "weak", "low"), ("lincoln", "weak", "low"), ("rick", "strong", "high"), ("viktor", "medium", "medium")];

#[tokio::test]
async fn a_coding_task_gets_one_critique_before_the_specialist_works() {
    let p = pod(BASE, implement(&["Rick"]), false);
    let coding = chat(&p.core, "Coding");
    let (rick, council) = (agent(&p.core, "rick"), agent(&p.core, "council"));
    p.core.send_message(&coding.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;

    let critique = msgs.iter().find(|m| m.agent_id.as_deref() == Some(&council.id)).expect("the council speaks in the chat");
    assert_eq!(critique.meta["council"]["role"], "critic");
    assert_eq!(critique.meta["council"]["for"], "Rick");
    let answer = msgs.iter().find(|m| m.agent_id.as_deref() == Some(&rick.id)).unwrap();
    assert!(critique.created_at <= answer.created_at, "the critique comes first");

    let reqs = p.scripted.requests.lock().unwrap();
    let rick_req = reqs.iter().find(|r| r.system.starts_with("You are Rick")).unwrap();
    let seen: String = rick_req.messages.iter().map(|m| m.text_content()).collect::<Vec<_>>().join("\n");
    assert!(seen.contains("[DP Council]: Solid: the idea."), "Rick reads the critique in the history");
    assert!(rick_req.messages.last().unwrap().text_content().contains("just reviewed this task"));
    drop(reqs);
    assert_eq!(council_requests(&p).len(), 1, "once, then it is out");
}

#[tokio::test]
async fn the_council_reads_but_never_writes_asks_or_delegates() {
    let p = pod(BASE, implement(&["Rick"]), false);
    let coding = chat(&p.core, "Coding");
    let ws = tempfile::tempdir().unwrap();
    let mut c = coding.clone();
    c.workspace_path = Some(ws.path().to_string_lossy().into_owned());
    p.core.update_chat(c).unwrap();
    p.core.grant_permission(&coding.id, &agent(&p.core, "rick").id, PermissionLevel::Full).unwrap();
    let rick = agent(&p.core, "rick");
    p.core.send_message(&coding.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;

    let req = &council_requests(&p)[0];
    let names: Vec<&str> = req.tools.iter().map(|t| t.name.as_str()).collect();
    assert!(names.contains(&"read_file"), "it can look at the code: {names:?}");
    for banned in ["write_file", "edit_file", "run_command", "ask_user", "remember", "delegate_to_agent"] {
        assert!(!names.contains(&banned), "{banned} must not be offered to the council: {names:?}");
    }
}

#[tokio::test]
async fn without_a_model_of_its_own_it_uses_the_strongest_model_in_the_pod() {
    let p = pod(BASE, implement(&["Rick"]), false);
    let coding = chat(&p.core, "Coding");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&coding.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;
    assert_eq!(council_requests(&p)[0].model, "strong");

    // Its own model wins when the user picked one.
    let p = pod(&[BASE, &[("council", "frontier", "max")]].concat(), implement(&["Rick"]), false);
    let coding = chat(&p.core, "Coding");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&coding.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;
    assert_eq!(council_requests(&p)[0].model, "frontier");
    let _ = &p.provider_id;
}

#[tokio::test]
async fn creative_writing_and_the_off_switch_keep_the_council_out() {
    let mut models = BASE.to_vec();
    models.push(("emma", "medium", "medium"));
    let p = pod(&models, implement(&["Emma"]), false);
    let creative = chat(&p.core, "General");
    let emma = agent(&p.core, "emma");
    p.core.send_message(&creative.id, "Write a poem about the sea", vec![]).await.unwrap();
    wait_for(&p.core, &creative.id, |m| done_by(m, &emma.id) == 1).await;
    assert!(council_requests(&p).is_empty(), "creative writing is not the council's job");

    let p = pod(BASE, implement(&["Rick"]), false);
    p.core.set_setting("council.enabled", None, json!(false)).unwrap();
    let coding = chat(&p.core, "Coding");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&coding.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;
    assert!(council_requests(&p).is_empty(), "the switch turns it off");
}

#[tokio::test]
async fn a_turn_with_several_agents_gets_one_critique() {
    let p = pod(BASE, implement(&["Rick", "Viktor"]), false);
    let coding = chat(&p.core, "Coding");
    let (rick, viktor) = (agent(&p.core, "rick"), agent(&p.core, "viktor"));
    p.core.send_message(&coding.id, "Build a login page and review its security", vec![]).await.unwrap();
    wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1 && done_by(m, &viktor.id) == 1).await;
    assert_eq!(council_requests(&p).len(), 1);
}

#[tokio::test]
async fn a_mentioned_specialist_is_reviewed_and_the_council_itself_is_not() {
    let p = pod(BASE, implement(&["Rick"]), false);
    let coding = chat(&p.core, "Coding");
    let (rick, council) = (agent(&p.core, "rick"), agent(&p.core, "council"));
    p.core.send_message(&coding.id, "@Rick write a hello world in Rust", vec![]).await.unwrap();
    wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;
    assert_eq!(council_requests(&p).len(), 1);

    // Asking the council directly is just a conversation with it: no second review layered on top.
    p.core.send_message(&coding.id, "@DP Council what do you think of this?", vec![]).await.unwrap();
    wait_for(&p.core, &coding.id, |m| done_by(m, &council.id) == 2).await;
    assert_eq!(council_requests(&p).len(), 2);
}

#[tokio::test]
async fn roger_never_routes_to_the_council() {
    let p = pod(BASE, implement(&["DP Council"]), false);
    let coding = chat(&p.core, "Coding");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&coding.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    // The moderator named the council; it is ignored and the keyword fallback picks a real specialist.
    wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;
    let reqs = p.scripted.requests.lock().unwrap();
    let moderator = reqs.iter().find(|r| r.json_mode).unwrap();
    assert!(!moderator.system.contains("[council]") && !moderator.system.contains("DP Council"), "the roster hides the council");
}

#[tokio::test]
async fn a_failing_council_never_blocks_the_task() {
    let p = pod(BASE, implement(&["Rick"]), true);
    let coding = chat(&p.core, "Coding");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&coding.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;
    let council = agent(&p.core, "council");
    assert!(msgs.iter().any(|m| m.agent_id.as_deref() == Some(&council.id) && m.status == MessageStatus::Error), "the failure is visible");
    let reqs = p.scripted.requests.lock().unwrap();
    let rick_req = reqs.iter().find(|r| r.system.starts_with("You are Rick")).unwrap();
    assert!(!rick_req.messages.last().unwrap().text_content().contains("just reviewed"), "no critique to weigh");
}

#[tokio::test]
async fn a_plan_is_drafted_criticized_and_then_finalized() {
    let route = |_: &str| json!({"intent": "plan oauth", "mode": "plan", "targets": ["Micheal"], "rewrite": "none", "permission": "read_only"});
    let p = pod(&[BASE, &[("micheal", "medium", "medium")]].concat(), route, false);
    let lh = chat(&p.core, "Long Horizon Agentic");
    let (micheal, council) = (agent(&p.core, "micheal"), agent(&p.core, "council"));
    p.core.send_message(&lh.id, "Make a plan to add OAuth to my app", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &lh.id, |m| m.iter().any(|m| m.kind == MessageKind::Plan && m.status == MessageStatus::Done)).await;

    let mut ordered: Vec<&Message> = msgs.iter().filter(|m| m.kind != MessageKind::Ack && m.kind != MessageKind::Notice && m.agent_id.is_some()).collect();
    ordered.sort_by_key(|m| m.created_at);
    let who: Vec<&str> = ordered.iter().map(|m| if m.agent_id.as_deref() == Some(&council.id) { "council" } else { "micheal" }).collect();
    assert_eq!(who, ["micheal", "council", "micheal"], "draft, critique, final plan");
    assert_eq!(ordered[0].kind, MessageKind::Text, "the draft cannot be approved");
    assert_eq!(ordered[0].meta["council"]["role"], "draft");
    assert_eq!(ordered[2].kind, MessageKind::Plan);
    assert!(ordered[2].meta["plan"]["hash"].is_string(), "the final plan is pinned");
    assert_eq!(msgs.iter().filter(|m| m.kind == MessageKind::Plan).count(), 1);
    let _ = &micheal;

    // Approving implements the final plan, without asking the council again.
    p.core.approve_plan(&ordered[2].id).await.unwrap();
    wait_for(&p.core, &lh.id, |m| m.iter().filter(|m| m.agent_id.as_deref() == Some(&micheal.id) && m.status == MessageStatus::Done).count() == 3).await;
    assert_eq!(council_requests(&p).len(), 1);
    let reqs = p.scripted.requests.lock().unwrap();
    let finalizing = reqs.iter().filter(|r| r.system.starts_with("You are Micheal")).nth(1).unwrap();
    assert!(finalizing.messages.last().unwrap().text_content().contains("reviewed your draft plan"));
}
