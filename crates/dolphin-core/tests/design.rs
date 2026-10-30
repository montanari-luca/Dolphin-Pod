//! Two designers work side by side; the run pauses on a card, and the user's pick or merge carries the team on.

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

const TEAM: &[&str] = &["roger", "lincoln", "vera", "nolan", "priya", "mira", "kai", "rick"];

fn requests_of(p: &Pod, who: &str) -> Vec<ChatRequest> {
    p.scripted.requests.lock().unwrap().iter().filter(|r| r.system.starts_with(&format!("You are {who},"))).cloned().collect()
}

fn text_of(r: &ChatRequest) -> String {
    r.messages.iter().map(|m| m.text_content()).collect::<Vec<_>>().join("\n")
}

fn card(msgs: &[Message]) -> Option<&Message> {
    msgs.iter().find(|m| m.meta.get("designChoice").is_some())
}

/// Names of everyone who spoke, in order: agents by name, the user as "You".
fn order(core: &CoreRef, msgs: &[Message]) -> Vec<String> {
    let agents = core.list_agents().unwrap();
    let mut all: Vec<&Message> = msgs.iter().filter(|m| m.status == MessageStatus::Done && m.kind == MessageKind::Text).collect();
    all.sort_by_key(|m| m.created_at);
    all.iter().map(|m| m.agent_id.as_ref().and_then(|id| agents.iter().find(|a| &a.id == id)).map_or("You".to_string(), |a| a.name.clone())).collect()
}

#[tokio::test]
async fn two_designers_work_blind_to_each_other_and_the_run_pauses_on_a_card() {
    let p = pod(TEAM, implement(&["Mira", "Rick"]));
    let dev = chat(&p.core, "Web Development");
    p.core.send_message(&dev.id, "Design a landing page for a coffee shop and build it", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| card(m).is_some()).await;

    let c = card(&msgs).unwrap();
    assert_eq!(c.meta["designChoice"]["state"], "pending");
    let options = c.meta["designChoice"]["options"].as_array().unwrap();
    assert_eq!(options.len(), 2);
    let (mira, kai) = (agent(&p.core, "mira"), agent(&p.core, "kai"));
    assert_eq!(options[0]["agentId"], mira.id.as_str());
    assert_eq!(options[1]["agentId"], kai.id.as_str());
    for (i, label) in ["A", "B"].iter().enumerate() {
        let m = msgs.iter().find(|m| m.id == options[i]["messageId"].as_str().unwrap()).unwrap();
        assert_eq!(m.meta["designOption"]["label"], *label);
        assert_eq!(m.meta["designOption"]["group"], c.meta["designChoice"]["group"]);
        assert_eq!(m.status, MessageStatus::Done);
    }

    assert!(!text_of(&requests_of(&p, "Kai")[0]).contains("[Mira]"), "Kai never sees Mira's idea");
    assert!(!text_of(&requests_of(&p, "Mira")[0]).contains("[Kai]"), "Mira never sees Kai's idea");
    assert!(requests_of(&p, "Rick").is_empty() && requests_of(&p, "Nolan").is_empty(), "the rest of the team waits for the choice");
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(requests_of(&p, "Rick").is_empty());
    let resume = &c.meta["designChoice"]["resume"]["decision"]["targets"];
    assert_eq!(resume.as_array().unwrap().len(), 3, "Nolan, Priya and Rick are waiting: {resume}");
}

#[tokio::test]
async fn picking_an_idea_hands_it_to_the_rest_of_the_team() {
    let p = pod(TEAM, implement(&["Mira", "Rick"]));
    let dev = chat(&p.core, "Web Development");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&dev.id, "Design a landing page for a coffee shop and build it", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| card(m).is_some()).await;
    let id = card(&msgs).unwrap().id.clone();

    p.core.choose_design(&id, "b", None).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| done_by(m, &rick.id) == 1).await;

    let who = order(&p.core, &msgs);
    assert_eq!(who[who.len() - 5..], ["You", "Nolan", "Priya", "Vera", "Rick"], "the pick, the engineers, the director's review, the coder: {who:?}");
    assert!(msgs.iter().any(|m| m.agent_id.is_none() && m.content == "I pick design B (Kai's)."), "the choice is a message from the user");
    let c = msgs.iter().find(|m| m.id == id).unwrap();
    assert_eq!((c.meta["designChoice"]["state"].as_str(), c.meta["designChoice"]["picked"].as_str()), (Some("decided"), Some("b")));

    let rick_req = requests_of(&p, "Rick").pop().unwrap();
    let last = rick_req.messages.last().unwrap().text_content();
    assert!(last.contains("The user picked design B, by Kai (above)"), "{last}");
    assert!(text_of(&rick_req).contains("[Kai]: Kai here."), "Rick reads the designs in the history");
}

#[tokio::test]
async fn merging_has_a_designer_write_one_design_from_the_note() {
    let p = pod(TEAM, implement(&["Mira", "Rick"]));
    let dev = chat(&p.core, "Web Development");
    let rick = agent(&p.core, "rick");
    p.core.send_message(&dev.id, "Design a landing page for a coffee shop and build it", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| card(m).is_some()).await;
    let id = card(&msgs).unwrap().id.clone();

    p.core.choose_design(&id, "merge", Some("A's layout with B's colors")).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| done_by(m, &rick.id) == 1).await;

    let mira = agent(&p.core, "mira");
    let merged: Vec<&Message> = msgs.iter().filter(|m| m.meta["designOption"]["label"] == "merged").collect();
    assert_eq!(merged.len(), 1);
    assert_eq!(merged[0].agent_id.as_deref(), Some(mira.id.as_str()), "the designer of idea A writes the merge");
    let mira_req = requests_of(&p, "Mira").pop().unwrap();
    let ask = mira_req.messages.last().unwrap().text_content();
    assert!(ask.contains("merged into ONE design") && ask.contains("A's layout with B's colors"), "{ask}");
    assert!(text_of(&mira_req).contains("[Kai]: Kai here."), "the merge reads both ideas");

    let c = msgs.iter().find(|m| m.id == id).unwrap();
    assert_eq!(c.meta["designChoice"]["mergedMessageId"], merged[0].id.as_str());
    let last = requests_of(&p, "Rick").pop().unwrap().messages.last().unwrap().text_content();
    assert!(last.contains("chose the merged design above, written by Mira"), "{last}");
}

#[tokio::test]
async fn a_choice_counts_once_and_only_a_design_card_can_be_answered() {
    let p = pod(TEAM, implement(&["Mira"]));
    let dev = chat(&p.core, "Web Development");
    p.core.send_message(&dev.id, "Design a landing page for a coffee shop", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &dev.id, |m| card(m).is_some()).await;
    let id = card(&msgs).unwrap().id.clone();
    assert!(p.core.choose_design(&id, "c", None).await.is_err(), "only a, b or merge");
    let not_a_card = msgs.iter().find(|m| m.agent_id.is_none()).unwrap().id.clone();
    assert!(p.core.choose_design(&not_a_card, "a", None).await.is_err());

    p.core.choose_design(&id, "a", None).await.unwrap();
    p.core.choose_design(&id, "b", None).await.unwrap();
    wait_for(&p.core, &dev.id, |m| m.iter().any(|m| m.agent_id.is_none() && m.content.starts_with("I pick design"))).await;
    tokio::time::sleep(Duration::from_millis(100)).await;
    let msgs = p.core.list_messages(&dev.id, 100).unwrap();
    let picks: Vec<&Message> = msgs.iter().filter(|m| m.agent_id.is_none() && m.content.starts_with("I pick design")).collect();
    assert_eq!(picks.len(), 1);
    assert_eq!(picks[0].content, "I pick design A (Mira's).", "the first answer stands");
}

#[tokio::test]
async fn without_the_director_designers_just_run_one_after_the_other() {
    let p = pod(&["roger", "lincoln", "mira", "kai"], implement(&["Mira", "Kai"]));
    let creative = chat(&p.core, "Creative");
    let mut c = creative.clone();
    let (mira, kai) = (agent(&p.core, "mira"), agent(&p.core, "kai"));
    c.agent_ids.extend([mira.id.clone(), kai.id.clone()]);
    p.core.update_chat(c).unwrap();
    p.core.send_message(&creative.id, "Design a logo", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &creative.id, |m| done_by(m, &kai.id) == 1).await;
    assert!(card(&msgs).is_none());
    assert!(text_of(&requests_of(&p, "Kai")[0]).contains("[Mira]"), "in a chat without the director the second designer sees the first");
}
