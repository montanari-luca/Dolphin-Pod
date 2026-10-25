//! Tool-using runs with a scripted provider: parallel web lookups, Robert's git tool and delegation.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use dolphin_core::db::Db;
use dolphin_core::model::*;
use dolphin_core::providers::{ChatRequest, EventStream, Provider, ProviderEvent, Role, ToolCall, Usage};
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreRef};
use serde_json::{Value, json};

type Script = dyn Fn(&ChatRequest) -> Vec<ProviderEvent> + Send + Sync;

/// A provider whose replies come from a closure over the request.
struct FnProvider {
    script: Box<Script>,
    requests: Mutex<Vec<ChatRequest>>,
}

#[async_trait]
impl Provider for FnProvider {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }
    async fn list_models(&self) -> dolphin_core::Result<Vec<ModelInfo>> {
        Ok(vec![])
    }
    async fn stream_chat(&self, req: ChatRequest) -> dolphin_core::Result<EventStream> {
        self.requests.lock().unwrap().push(req.clone());
        let mut events = (self.script)(&req);
        events.push(ProviderEvent::Usage(Usage { input_tokens: 10, output_tokens: 5 }));
        events.push(ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None });
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

pub fn call(id: &str, name: &str, args: Value) -> ProviderEvent {
    ProviderEvent::ToolCall(ToolCall { id: id.into(), name: name.into(), arguments: args })
}

pub fn text(t: &str) -> ProviderEvent {
    ProviderEvent::TextDelta(t.into())
}

/// Who a request is for: "You are Rick, ..." → "Rick".
pub fn who(req: &ChatRequest) -> String {
    req.system.trim_start_matches("You are ").split(',').next().unwrap_or("?").to_string()
}

/// Whether the request already carries tool results (so the scripted agent should now answer in text).
pub fn has_tool_results(req: &ChatRequest) -> bool {
    req.messages.iter().any(|m| m.role == Role::Tool)
}

struct Pod {
    core: CoreRef,
    provider: Arc<FnProvider>,
    _dir: tempfile::TempDir,
}

fn pod(configure: &[&str], script: impl Fn(&ChatRequest) -> Vec<ProviderEvent> + Send + Sync + 'static) -> Pod {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let cfg = core
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .unwrap();
    let provider = Arc::new(FnProvider { script: Box::new(script), requests: Mutex::new(vec![]) });
    core.override_provider(&cfg.id, provider.clone());
    for a in core.list_agents().unwrap() {
        if configure.contains(&a.slug.as_str()) {
            let mut a = a;
            a.model = Some(ModelRef { provider_id: cfg.id.clone(), model_id: "qwen3.5:2b".into(), effort: Some(EffortValue::Toggle(true)), vision: false, tools: true, context_window: None });
            core.update_agent(a).unwrap();
        }
    }
    Pod { core, provider, _dir: dir }
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
    panic!("timed out; messages: {:#?}", core.list_messages(chat_id, 100).unwrap().iter().map(|m| (&m.agent_id, &m.kind, &m.status, &m.content)).collect::<Vec<_>>());
}

fn done_by(core: &CoreRef, slug: &str) -> impl Fn(&[Message]) -> bool {
    let id = agent(core, slug).id;
    move |m: &[Message]| m.iter().any(|m| m.agent_id.as_deref() == Some(&id) && m.status == MessageStatus::Done)
}

#[tokio::test]
async fn parallel_web_calls_in_one_step_come_back_in_order() {
    // Luna asks for three lookups at once; local addresses fail fast, each with its own error.
    let p = pod(&["luna"], |req| {
        if has_tool_results(req) {
            vec![text("Report done.")]
        } else {
            vec![
                call("c1", "fetch_page", json!({ "url": "http://127.0.0.1/one" })),
                call("c2", "fetch_page", json!({ "url": "http://localhost/two" })),
                call("c3", "fetch_page", json!({ "url": "not a url" })),
            ]
        }
    });
    let general = chat(&p.core, "General");
    p.core.send_message(&general.id, "@Luna look this up", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &general.id, done_by(&p.core, "luna")).await;
    let luna = msgs.iter().find(|m| m.agent_id.as_deref() == Some(&agent(&p.core, "luna").id)).unwrap();
    assert_eq!(luna.content, "Report done.");
    let tools = luna.meta["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 3);
    assert!(tools.iter().all(|t| t["ok"] == false));

    // The second request replayed the three results in the order they were asked for.
    let reqs = p.provider.requests.lock().unwrap();
    let second = reqs.iter().find(|r| has_tool_results(r)).unwrap();
    let ids: Vec<String> = second.messages.iter().filter(|m| m.role == Role::Tool).filter_map(|m| m.tool_call_id.clone()).collect();
    assert_eq!(ids, ["c1", "c2", "c3"]);
}
