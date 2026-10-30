//! The software director: she routes the development chats in Roger's place, the engineers work before the coder,
//! and she reviews the engineers' plan once before the coder builds from it.

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
    /// What the director (or the moderator) decides, from the user's text.
    route: Box<dyn Fn(&str) -> serde_json::Value + Send + Sync>,
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
        let text = if req.json_mode && (req.system.contains("software director of") || req.system.contains("moderator of \"Dolphin Pod\"")) {
            let user = req.messages[0].text_content();
            (self.route)(user.rsplit("Latest user message:").next().unwrap_or(&user)).to_string()
        } else if req.system.starts_with("You are Lincoln") {
            "Do the task well.".to_string()
        } else {
            let who = req.system.trim_start_matches("You are ").split(',').next().unwrap_or("?").to_string();
            format!("{who} here. Done.")
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
    _dir: tempfile::TempDir,
}

fn model(provider_id: &str, id: &str) -> ModelRef {
    ModelRef { provider_id: provider_id.into(), model_id: id.into(), effort: None, vision: false, tools: true, context_window: None }
}

/// Every agent in `slugs` gets a model.
fn pod(slugs: &[&str], route: impl Fn(&str) -> serde_json::Value + Send + Sync + 'static) -> Pod {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let cfg = core
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .unwrap();
    let scripted = Arc::new(Scripted { requests: Mutex::new(vec![]), route: Box::new(route) });
    core.override_provider(&cfg.id, scripted.clone());
    for a in core.list_agents().unwrap() {
        if slugs.contains(&a.slug.as_str()) {
            let mut a = a;
            a.model = Some(model(&cfg.id, "m"));
            core.update_agent(a).unwrap();
        }
    }
    Pod { core, scripted, _dir: dir }
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

fn implement(targets: &[&str]) -> impl Fn(&str) -> serde_json::Value + Send + Sync + 'static {
    let targets: Vec<String> = targets.iter().map(|t| t.to_string()).collect();
    move |_| json!({"intent": "build it", "mode": "implement", "targets": targets, "rewrite": "none", "permission": "read_only"})
}

const TEAM: &[&str] = &["roger", "lincoln", "vera", "nolan", "priya", "rick"];

/// Authors of the finished agent messages, in the order they appeared.
fn speakers(core: &CoreRef, msgs: &[Message]) -> Vec<String> {
    let mut done: Vec<&Message> = msgs.iter().filter(|m| m.agent_id.is_some() && m.status == MessageStatus::Done && m.kind == MessageKind::Text).collect();
    done.sort_by_key(|m| m.created_at);
    done.iter().map(|m| core.list_agents().unwrap().into_iter().find(|a| Some(&a.id) == m.agent_id.as_ref()).unwrap().name).collect()
}

#[tokio::test]
async fn the_engineers_work_before_the_coder_and_the_director_reviews_their_plan() {
    let p = pod(TEAM, implement(&["Rick"]));
    let dev = chat(&p.core, "Software Development");
    let (vera, rick) = (agent(&p.core, "vera"), agent(&p.core, "rick"));
    p.core.send_message(&dev.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| done_by(m, &rick.id) == 1).await;

    assert_eq!(speakers(&p.core, &msgs), ["Nolan", "Priya", "Vera", "Rick"], "engineers, the director's review, then the coder");
    let review = msgs.iter().find(|m| m.agent_id.as_deref() == Some(&vera.id)).unwrap();
    assert_eq!(review.meta["council"]["for"], "Rick");
    assert_eq!(review.meta["routing"]["via"], "director");

    let reqs = p.scripted.requests.lock().unwrap();
    let rick_req = reqs.iter().find(|r| r.system.starts_with("You are Rick")).unwrap();
    let seen: String = rick_req.messages.iter().map(|m| m.text_content()).collect::<Vec<_>>().join("\n");
    assert!(seen.contains("[Nolan]: Nolan here.") && seen.contains("[Priya]: Priya here."), "Rick reads the engineers' plans");
    let last = rick_req.messages.last().unwrap().text_content();
    assert!(last.contains("Vera (director) → Rick") && last.contains("The engineers' plan is in the chat above"), "{last}");
    assert!(last.contains("just reviewed the engineers' plan"));
}

#[tokio::test]
async fn the_director_routes_instead_of_roger_and_is_never_a_target() {
    let p = pod(TEAM, implement(&["Vera", "Rick"]));
    let dev = chat(&p.core, "Software Development");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&dev.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    wait_for(&p.core, &dev.id, |m| done_by(m, &rick.id) == 1).await;
    let reqs = p.scripted.requests.lock().unwrap();
    let routing: Vec<&ChatRequest> = reqs.iter().filter(|r| r.json_mode).collect();
    assert_eq!(routing.len(), 1);
    assert!(routing[0].system.starts_with("You are Vera, the software director"));
    assert!(!routing[0].system.contains("- Vera ["), "the director does not route to herself");
}

#[tokio::test]
async fn without_a_director_the_moderator_still_routes_and_no_engineers_are_added() {
    let p = pod(&["roger", "lincoln", "rick"], implement(&["Rick"]));
    let coding = chat(&p.core, "Coding");
    let rick = agent(&p.core, "rick");
    p.core.set_setting("council.enabled", None, json!(false)).unwrap();
    p.core.send_message(&coding.id, "Build a CSV parser in Rust", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &coding.id, |m| done_by(m, &rick.id) == 1).await;
    assert_eq!(speakers(&p.core, &msgs), ["Rick"]);
    assert!(p.scripted.requests.lock().unwrap().iter().all(|r| !r.system.contains("software director")));
}

#[tokio::test]
async fn a_quick_request_skips_the_engineers() {
    let p = pod(TEAM, |_| json!({"intent": "rename", "mode": "quick", "targets": ["Rick"], "rewrite": "none"}));
    let dev = chat(&p.core, "Software Development");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&dev.id, "quickly rename this variable", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| done_by(m, &rick.id) == 1).await;
    assert_eq!(speakers(&p.core, &msgs), ["Rick"]);
}

#[tokio::test]
async fn when_the_model_names_nobody_the_director_falls_back_to_keywords_and_still_orders_the_team() {
    let p = pod(&["roger", "lincoln", "nolan", "priya", "rick"], implement(&[]));
    let dev = chat(&p.core, "Software Development");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&dev.id, "implement a function that parses dates", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| done_by(m, &rick.id) == 1).await;
    assert_eq!(speakers(&p.core, &msgs), ["Nolan", "Priya", "Vera", "Rick"], "keyword routing builds the same team; the director reviews on the strongest model there is");
}
