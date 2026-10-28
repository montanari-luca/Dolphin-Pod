//! Per-agent memory end to end with a scripted provider: what each agent is sent, where it sits in the
//! request, which project it applies to, and the Core operations behind the Memory list.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use dolphin_core::db::Db;
use dolphin_core::events::CoreEvent;
use dolphin_core::model::*;
use dolphin_core::providers::{ChatRequest, EventStream, Provider, ProviderEvent, Usage};
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreRef};
use serde_json::json;

#[derive(Default)]
struct Scripted {
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
        self.requests.lock().unwrap().push(req);
        let events = vec![
            ProviderEvent::TextDelta("ok".into()),
            ProviderEvent::Usage(Usage { input_tokens: 10, output_tokens: 1 }),
            ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None },
        ];
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

struct Pod {
    core: CoreRef,
    provider: Arc<Scripted>,
    dir: tempfile::TempDir,
}

fn pod() -> Pod {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let cfg = core
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .unwrap();
    let provider = Arc::new(Scripted::default());
    core.override_provider(&cfg.id, provider.clone());
    for a in core.list_agents().unwrap() {
        if ["roger", "lincoln", "rick"].contains(&a.slug.as_str()) {
            let mut a = a;
            a.model = Some(ModelRef { provider_id: cfg.id.clone(), model_id: "qwen3.5:2b".into(), effort: Some(EffortValue::Toggle(true)), vision: false, tools: true, context_window: None });
            core.update_agent(a).unwrap();
        }
    }
    // These tests are about other things; the council has its own tests in council.rs.
    core.set_setting("council.enabled", None, json!(false)).unwrap();
    Pod { core, provider, dir }
}

fn chat(core: &CoreRef, name: &str) -> Chat {
    core.list_chats().unwrap().into_iter().find(|c| c.name == name).unwrap()
}

fn agent(core: &CoreRef, slug: &str) -> Agent {
    core.list_agents().unwrap().into_iter().find(|a| a.slug == slug).unwrap()
}

fn answered(msgs: &[Message]) -> usize {
    msgs.iter().filter(|m| m.agent_id.is_some() && m.kind == MessageKind::Text && m.status == MessageStatus::Done).count()
}

async fn wait_for_answers(core: &CoreRef, chat_id: &str, n: usize) {
    for _ in 0..300 {
        if answered(&core.list_messages(chat_id, 500).unwrap()) >= n {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out waiting for {n} answers");
}

/// The request an agent was last sent: (system prompt, last message, number of stable messages, message count).
fn last_request(p: &Pod, who: &str) -> (String, String, Option<usize>, usize) {
    let reqs = p.provider.requests.lock().unwrap();
    let r = reqs.iter().rev().find(|r| r.system.starts_with(&format!("You are {who}"))).unwrap_or_else(|| panic!("no request for {who}"));
    (r.system.clone(), r.messages.last().unwrap().text_content(), r.stable_messages, r.messages.len())
}

#[tokio::test]
async fn an_agent_is_sent_its_own_memory_after_the_history_and_never_anothers() {
    let p = pod();
    let (rick, roger) = (agent(&p.core, "rick"), agent(&p.core, "roger"));
    p.core.add_agent_memory(&rick.id, MemoryKind::Style, "Keep answers under three sentences", None).unwrap();
    p.core.add_agent_memory(&roger.id, MemoryKind::Fact, "Roger-only: the user keeps bees", None).unwrap();
    let coding = chat(&p.core, "Coding");

    p.core.send_message(&coding.id, "@Rick how do I reverse a string in Rust?", vec![]).await.unwrap();
    wait_for_answers(&p.core, &coding.id, 1).await;
    let (system, tail, stable, len) = last_request(&p, "Rick");
    assert!(!system.contains("three sentences"), "the system prompt stays identical between turns: memory is not part of it");
    assert!(tail.contains("- Keep answers under three sentences"), "{tail}");
    assert!(!tail.contains("bees"), "Roger's memory is not Rick's");
    assert_eq!(stable, Some(len - 1), "memory is volatile: the history before it stays cacheable");
    assert_eq!(p.core.list_agent_memories(&rick.id).unwrap()[0].hits, 1, "using it counts a hit");

    p.core.send_message(&coding.id, "@Roger hello", vec![]).await.unwrap();
    wait_for_answers(&p.core, &coding.id, 2).await;
    let (_, tail, _, _) = last_request(&p, "Roger");
    assert!(tail.contains("the user keeps bees") && !tail.contains("three sentences"), "{tail}");
}

#[tokio::test]
async fn a_chat_without_memories_sends_nothing_extra() {
    let p = pod();
    let general = chat(&p.core, "General");
    p.core.send_message(&general.id, "@Roger hello", vec![]).await.unwrap();
    wait_for_answers(&p.core, &general.id, 1).await;
    let (_, tail, stable, len) = last_request(&p, "Roger");
    assert!(!tail.contains("learned about this user"), "{tail}");
    assert_eq!(stable, Some(len), "no volatile message was added: everything in the request is cacheable");
}

#[tokio::test]
async fn project_memories_apply_in_that_workspace_and_nowhere_else() {
    let p = pod();
    let roger = agent(&p.core, "roger");
    let ws = p.dir.path().join("proj-alpha");
    std::fs::create_dir(&ws).unwrap();
    let coding = chat(&p.core, "Coding");
    let general = chat(&p.core, "General");
    p.core.set_setting("chat.workspace_path", Some("Coding"), json!(ws.to_string_lossy())).unwrap();
    p.core.add_agent_memory(&roger.id, MemoryKind::Workflow, "Everywhere: run the linter", None).unwrap();
    p.core.add_agent_memory(&roger.id, MemoryKind::Workflow, "Alpha only: use tabs", Some("proj-alpha")).unwrap();
    p.core.add_agent_memory(&roger.id, MemoryKind::Workflow, "Zeta only: use spaces", Some("proj-zeta")).unwrap();

    p.core.send_message(&coding.id, "@Roger hello", vec![]).await.unwrap();
    wait_for_answers(&p.core, &coding.id, 1).await;
    let (_, tail, _, _) = last_request(&p, "Roger");
    assert!(tail.contains("run the linter") && tail.contains("use tabs"), "{tail}");
    assert!(!tail.contains("use spaces"), "another project's rule stays out: {tail}");

    p.core.send_message(&general.id, "@Roger hello", vec![]).await.unwrap();
    wait_for_answers(&p.core, &general.id, 1).await;
    let (_, tail, _, _) = last_request(&p, "Roger");
    assert!(tail.contains("run the linter") && !tail.contains("use tabs") && !tail.contains("use spaces"), "no workspace: only the everywhere rule: {tail}");
}

#[test]
fn the_memory_list_adds_edits_pins_and_forgets() {
    let p = pod();
    let rick = agent(&p.core, "rick");
    let mut events = p.core.subscribe();

    let m = p.core.add_agent_memory(&rick.id, MemoryKind::Avoid, "  Don't rewrite\n the whole file  ", Some("  ")).unwrap();
    assert_eq!((m.text.as_str(), m.project.as_deref(), m.source, m.pinned), ("Don't rewrite the whole file", None, MemorySource::Explicit, false));

    // Pinning alone keeps the source; rewording or re-scoping makes it the user's own.
    let pinned = p.core.update_agent_memory(AgentMemory { pinned: true, ..m.clone() }).unwrap();
    assert!(pinned.pinned);
    assert_eq!(pinned.source, MemorySource::Explicit);
    let reworded = p.core.update_agent_memory(AgentMemory { text: "Don't rewrite whole files".into(), project: Some("api".into()), ..pinned }).unwrap();
    assert_eq!((reworded.source, reworded.project.as_deref()), (MemorySource::Edited, Some("api")));
    assert_eq!(p.core.list_agent_memories(&rick.id).unwrap().len(), 1);

    assert!(p.core.add_agent_memory(&rick.id, MemoryKind::Fact, "   ", None).is_err(), "blank");
    assert!(p.core.add_agent_memory(&rick.id, MemoryKind::Fact, &"x".repeat(301), None).is_err(), "one thought, not a paragraph");
    assert!(p.core.update_agent_memory(AgentMemory { text: String::new(), ..reworded.clone() }).is_err());
    assert!(p.core.add_agent_memory("no-such-agent", MemoryKind::Fact, "x", None).is_err());
    assert_eq!(p.core.list_agent_memories(&rick.id).unwrap()[0].text, "Don't rewrite whole files", "failed edits change nothing");

    p.core.delete_agent_memory(&m.id).unwrap();
    assert!(p.core.list_agent_memories(&rick.id).unwrap().is_empty());
    assert!(p.core.delete_agent_memory(&m.id).is_err(), "already gone");

    let mut changed = 0;
    while let Ok(e) = events.try_recv() {
        if matches!(e, CoreEvent::MemoriesChanged { ref agent_id } if *agent_id == rick.id) {
            changed += 1;
        }
    }
    assert_eq!(changed, 4, "add, pin, reword and delete each tell the UI");
}

#[test]
fn forgetting_everything_touches_only_that_agent_and_the_cap_holds() {
    let p = pod();
    let (rick, roger) = (agent(&p.core, "rick"), agent(&p.core, "roger"));
    p.core.add_agent_memory(&roger.id, MemoryKind::Fact, "Roger's fact", None).unwrap();
    for i in 0..40 {
        p.core.add_agent_memory(&rick.id, MemoryKind::Fact, &format!("fact {i}"), None).unwrap();
    }
    let err = p.core.add_agent_memory(&rick.id, MemoryKind::Fact, "one too many", None).unwrap_err();
    assert!(err.to_string().contains("forget some first"), "{err}");

    assert_eq!(p.core.forget_agent_memories(&rick.id).unwrap(), 40);
    assert_eq!(p.core.forget_agent_memories(&rick.id).unwrap(), 0);
    assert!(p.core.list_agent_memories(&rick.id).unwrap().is_empty());
    assert_eq!(p.core.list_agent_memories(&roger.id).unwrap().len(), 1, "the other agent keeps its own");
    p.core.add_agent_memory(&rick.id, MemoryKind::Fact, "room again", None).unwrap();
}
