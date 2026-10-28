//! Live check of agent learning against a local Ollama server (no mocks):
//!
//!     cargo run -p dolphin-core --example live_learning -- qwen3.5:2b
//!     cargo run -p dolphin-core --example live_learning -- qwen3.5:2b control
//!
//! `control` turns learning off and only runs the first request: the same model without the remember tool, to tell
//! whether a model's trouble comes from the model or from what learning adds to its prompt.
//!
//! Rick answers a request, the user corrects him, then asks him to remember something. Prints what the learner
//! wrote into his memory, so you can see whether the model you picked produces usable notes.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dolphin_core::db::Db;
use dolphin_core::events::CoreEvent;
use dolphin_core::model::*;
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreRef};
use serde_json::json;

fn rick_done(core: &CoreRef, chat_id: &str, n: usize) -> bool {
    let rick = core.list_agents().unwrap().into_iter().find(|a| a.slug == "rick").unwrap().id;
    core.list_messages(chat_id, 100).unwrap().iter().filter(|m| m.agent_id.as_deref() == Some(&rick) && m.kind == MessageKind::Text && m.status == MessageStatus::Done).count() >= n
}

async fn wait(what: &str, timeout: Duration, cond: impl Fn() -> bool) -> bool {
    let start = Instant::now();
    while start.elapsed() < timeout {
        if cond() {
            println!("  ({what}: {:.1}s)", start.elapsed().as_secs_f32());
            return true;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
    println!("  !! gave up waiting for {what} after {timeout:?}");
    false
}

/// What Rick said and which signals are still queued, to see why nothing was learned.
fn debug(core: &CoreRef, chat_id: &str, rick: &str) {
    for m in core.list_messages(chat_id, 100).unwrap() {
        let who = if m.agent_id.is_none() { "You" } else { "Agent" };
        println!("    {who} [{:?}/{:?}] {}", m.kind, m.status, m.content.replace('\n', " ").chars().take(160).collect::<String>());
    }
    let pending = core.db.pending_feedback(rick).unwrap();
    println!("    pending signals for Rick: {:?}", pending.iter().map(|f| f.signal).collect::<Vec<_>>());
}

fn show_memory(core: &CoreRef, rick: &str) {
    let all = core.list_agent_memories(rick).unwrap();
    if all.is_empty() {
        println!("  Rick's memory: (empty)");
    }
    for m in all {
        println!("  Rick's memory: [{:?}/{:?}] {}", m.kind, m.source, m.text);
    }
}

#[tokio::main]
async fn main() {
    let model = std::env::args().nth(1).unwrap_or_else(|| "qwen3.5:2b".into());
    let control = std::env::args().nth(2).as_deref() == Some("control");
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    let cfg = core
        .save_provider(ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Ollama".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 }, None)
        .unwrap();
    let models = core.list_models(&cfg.id).await.expect("Ollama reachable");
    let info = models.iter().find(|m| m.id == model).unwrap_or_else(|| panic!("{model} is not pulled"));
    for a in core.list_agents().unwrap() {
        let mut a = a;
        a.model = Some(ModelRef { provider_id: cfg.id.clone(), model_id: model.clone(), effort: Some(EffortValue::Toggle(true)), vision: info.vision, tools: info.tools, context_window: info.context_window });
        a.learning = !control;
        core.update_agent(a).unwrap();
    }
    let rick = core.list_agents().unwrap().into_iter().find(|a| a.slug == "rick").unwrap().id;
    let coding = core.list_chats().unwrap().into_iter().find(|c| c.name == "Coding").unwrap();
    let mut events = core.subscribe();
    let long = Duration::from_secs(120);
    let learn_wait = Duration::from_secs(45);

    println!("model: {model} (tools: {})", info.tools);
    println!("\n== 1. an ordinary request");
    core.send_message(&coding.id, "@Rick write a Python function add(a, b)", vec![]).await.unwrap();
    wait("Rick's answer", long, || rick_done(&core, &coding.id, 1)).await;
    if control {
        debug(&core, &coding.id, &rick);
        return;
    }

    println!("\n== 2. a correction");
    core.send_message(&coding.id, "@Rick No, don't add docstrings or comments. Code only, from now on.", vec![]).await.unwrap();
    wait("Rick's second answer", long, || rick_done(&core, &coding.id, 2)).await;
    if !wait("a learned note", learn_wait, || !core.list_agent_memories(&rick).unwrap().is_empty()).await {
        debug(&core, &coding.id, &rick);
    }
    show_memory(&core, &rick);

    println!("\n== 3. remember that");
    let before = core.list_agent_memories(&rick).unwrap().len();
    core.send_message(&coding.id, "@Rick remember that I always use pnpm, never npm", vec![]).await.unwrap();
    wait("Rick's third answer", long, || rick_done(&core, &coding.id, 3)).await;
    if !wait("the explicit note", learn_wait, || core.list_agent_memories(&rick).unwrap().len() > before).await {
        debug(&core, &coding.id, &rick);
    }
    show_memory(&core, &rick);

    println!("\n== learner events");
    while let Ok(e) = events.try_recv() {
        if let CoreEvent::MemoryLearned { memory, previous_text } = e {
            println!("  learned: {:?} (was {previous_text:?})", memory.text);
        }
    }
}
