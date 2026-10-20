//! Context handling end to end with scripted providers: persisted compaction, tool-output
//! compression with retrieval, and recovery from a provider rejecting an over-long prompt.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use dolphin_core::context::SUMMARY_MARK;
use dolphin_core::db::Db;
use dolphin_core::model::*;
use dolphin_core::providers::{ChatRequest, EventStream, Provider, ProviderEvent, Role, ToolCall, Usage};
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreError, CoreRef};
use serde_json::{Value, json};

type Script = Box<dyn Fn(&ChatRequest, usize) -> dolphin_core::Result<Vec<ProviderEvent>> + Send + Sync>;

struct Scripted {
    requests: Mutex<Vec<ChatRequest>>,
    script: Script,
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
        let n = {
            let mut r = self.requests.lock().unwrap();
            r.push(req.clone());
            r.len()
        };
        let mut events = (self.script)(&req, n)?;
        events.push(ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None });
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

fn is_summary_request(req: &ChatRequest) -> bool {
    req.system.contains("running memory of a group chat")
}

fn say(text: &str) -> Vec<ProviderEvent> {
    vec![ProviderEvent::TextDelta(text.into()), ProviderEvent::Usage(Usage { input_tokens: 100, output_tokens: 10 })]
}

struct Pod {
    core: CoreRef,
    provider: Arc<Scripted>,
    _dir: tempfile::TempDir,
}

fn pod(script: Script) -> Pod {
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let cfg = core
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .unwrap();
    let provider = Arc::new(Scripted { requests: Mutex::new(vec![]), script });
    core.override_provider(&cfg.id, provider.clone());
    for a in core.list_agents().unwrap() {
        if ["roger", "lincoln", "rick"].contains(&a.slug.as_str()) {
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

async fn wait_for(core: &CoreRef, chat_id: &str, pred: impl Fn(&[Message]) -> bool) -> Vec<Message> {
    for _ in 0..300 {
        let msgs = core.list_messages(chat_id, 500).unwrap();
        if pred(&msgs) {
            return msgs;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("timed out: {:#?}", core.list_messages(chat_id, 500).unwrap().iter().map(|m| (&m.kind, &m.status, &m.content)).collect::<Vec<_>>());
}

fn answered(msgs: &[Message]) -> usize {
    msgs.iter().filter(|m| m.agent_id.is_some() && m.kind == MessageKind::Text && m.status == MessageStatus::Done).count()
}

#[tokio::test]
async fn long_chats_are_compacted_into_a_saved_summary() {
    let p = pod(Box::new(|req, _| {
        Ok(if is_summary_request(req) { say("- The user has been greeting Roger and asking small questions.") } else { say("Hello again.") })
    }));
    p.core.set_setting("context.history_messages", None, json!(4)).unwrap();
    let general = chat(&p.core, "General");

    for i in 1..=5 {
        p.core.send_message(&general.id, &format!("@Roger question number {i}"), vec![]).await.unwrap();
        wait_for(&p.core, &general.id, |m| answered(m) == i).await;
    }

    let msgs = p.core.list_messages(&general.id, 500).unwrap();
    let notice = msgs.iter().find(|m| m.meta["notice"] == "compaction").expect("a compaction notice is posted");
    assert!(notice.content.contains("Compacted"));
    assert!(notice.meta["compaction"]["summary"].as_str().unwrap().contains("greeting Roger"));

    // The last request carries the summary instead of the whole chat, and still ends with the question.
    let reqs = p.provider.requests.lock().unwrap();
    let last = reqs.iter().rev().find(|r| !is_summary_request(r) && r.system.starts_with("You are Roger")).unwrap();
    assert!(last.messages[0].text_content().starts_with(SUMMARY_MARK), "{}", last.messages[0].text_content());
    assert!(last.messages[0].text_content().contains("greeting Roger"));
    assert!(last.messages.len() <= 6, "{} messages", last.messages.len());
    assert!(last.messages.iter().any(|m| m.text_content().contains("question number 5")));
    assert!(!last.messages.iter().any(|m| m.text_content().contains("question number 1")), "folded into the summary");
    // The stable prefix (summary + history) is marked for provider caches; the task text is not part of it.
    assert_eq!(last.stable_messages, Some(last.messages.len()));
    assert!(last.cache_key.as_deref().unwrap().contains(&general.id));
    assert_eq!(last.context_window, Some(32_768), "Ollama is asked for a fixed context length");
}

#[tokio::test]
async fn compaction_can_be_turned_off() {
    let p = pod(Box::new(|_, _| Ok(say("ok"))));
    p.core.set_setting("context.history_messages", None, json!(4)).unwrap();
    p.core.set_setting("context.auto_compact", None, json!(false)).unwrap();
    let general = chat(&p.core, "General");
    for i in 1..=4 {
        p.core.send_message(&general.id, &format!("@Roger number {i}"), vec![]).await.unwrap();
        wait_for(&p.core, &general.id, |m| answered(m) == i).await;
    }
    assert!(!p.core.list_messages(&general.id, 500).unwrap().iter().any(|m| m.meta["notice"] == "compaction"));
    let reqs = p.provider.requests.lock().unwrap();
    assert!(reqs.last().unwrap().messages.len() <= 5, "plain sliding window");
}

#[tokio::test]
async fn big_tool_output_is_compressed_and_can_be_retrieved() {
    let ws = tempfile::tempdir().unwrap();
    let body: String = (0..450).map(|i| format!("entry {i}: {} token_{}\n", ["alpha", "beta", "gamma", "delta"][i % 4], i * 31)).collect();
    std::fs::write(ws.path().join("big.log"), &body).unwrap();

    let p = pod(Box::new(|req, _| {
        let last = req.messages.last().unwrap();
        if last.role == Role::Tool && !last.text_content().contains("Re-read") && last.tool_name.as_deref() == Some("read_file") {
            // Step 2: the model sees the compressed file and asks for the original.
            return Ok(vec![ProviderEvent::ToolCall(ToolCall { id: "r2".into(), name: "context_retrieve".into(), arguments: json!({ "id": "c1", "query": "entry 449:" }) })]);
        }
        if last.role == Role::Tool {
            return Ok(say("Found it."));
        }
        Ok(vec![ProviderEvent::ToolCall(ToolCall { id: "r1".into(), name: "read_file".into(), arguments: json!({ "path": "big.log", "max_lines": 1000 }) })])
    }));
    let coding = chat(&p.core, "Coding");
    let mut c = coding.clone();
    c.workspace_path = Some(ws.path().to_string_lossy().into_owned());
    p.core.update_chat(c).unwrap();

    p.core.send_message(&coding.id, "@Rick read big.log and find the last entry", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &coding.id, |m| answered(m) == 1).await;
    let reply = msgs.iter().find(|m| m.agent_id.is_some() && m.kind == MessageKind::Text).unwrap();
    assert_eq!(reply.content, "Found it.");
    assert!(reply.meta["context"]["savedTokens"].as_i64().unwrap() > 2000, "{}", reply.meta["context"]);

    let reqs = p.provider.requests.lock().unwrap();
    assert_eq!(reqs.len(), 3);
    let first_result = reqs[1].messages.iter().find(|m| m.role == Role::Tool).unwrap().text_content();
    assert!(first_result.contains("context_retrieve id=\"c1\""), "{}", &first_result[first_result.len().saturating_sub(300)..]);
    assert!(first_result.len() < body.len() * 3 / 5, "compressed: {} vs {}", first_result.len(), body.len());
    assert!(reqs[0].tools.iter().any(|t| t.name == "context_retrieve"));
    let retrieved = reqs[2].messages.last().unwrap().text_content();
    assert!(retrieved.contains("entry 449: beta token_13919"), "{retrieved}");
    // Retrieval is not logged as a workspace action.
    assert!(!p.core.action_log(&coding.id).unwrap().iter().any(|a| a.tool == "context_retrieve"));
}

#[tokio::test]
async fn an_overlong_prompt_error_triggers_a_squeeze_and_a_retry() {
    let p = pod(Box::new(|_, n| {
        if n == 1 {
            return Err(CoreError::provider("HTTP 400: prompt is too long: 250000 tokens > 200000 maximum"));
        }
        Ok(say("Recovered."))
    }));
    let general = chat(&p.core, "General");
    p.core.send_message(&general.id, "@Roger hello", vec![]).await.unwrap();
    let msgs = wait_for(&p.core, &general.id, |m| answered(m) == 1).await;
    assert_eq!(msgs.iter().find(|m| m.agent_id.is_some() && m.kind == MessageKind::Text).unwrap().content, "Recovered.");
    assert_eq!(p.provider.requests.lock().unwrap().len(), 2);

    // Other errors are still errors.
    let p = pod(Box::new(|_, _| Err(CoreError::provider("HTTP 401: invalid api key"))));
    let general = chat(&p.core, "General");
    p.core.send_message(&general.id, "@Roger hello", vec![]).await.unwrap();
    wait_for(&p.core, &general.id, |m| m.iter().any(|m| m.status == MessageStatus::Error)).await;
    assert_eq!(p.provider.requests.lock().unwrap().len(), 1);
}

#[tokio::test]
async fn preferences_travel_after_the_history_not_in_the_system_prompt() {
    let p = pod(Box::new(|_, _| Ok(say("ok"))));
    p.core.set_setting("preferences.custom_instructions", None, Value::String("Always answer in British English.".into())).unwrap();
    let general = chat(&p.core, "General");
    p.core.send_message(&general.id, "@Roger colour or color?", vec![]).await.unwrap();
    wait_for(&p.core, &general.id, |m| answered(m) == 1).await;
    let reqs = p.provider.requests.lock().unwrap();
    let r = reqs.iter().find(|r| r.system.starts_with("You are Roger")).unwrap();
    assert!(!r.system.contains("British English"), "the system prompt stays identical between turns");
    let tail = r.messages.last().unwrap().text_content();
    assert!(tail.contains("British English"), "{tail}");
    assert_eq!(r.stable_messages, Some(r.messages.len() - 1));
}
