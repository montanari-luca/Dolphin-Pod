//! Live end-to-end run against a local Ollama server (no mocks):
//!
//!     cargo run -p dolphin-core --example live_ollama -- qwen3:4b
//!
//! Exercises every default chat plus the two example workflows (Java question → Prof;
//! plan → read-only → approval → full permissions), an @mention and a chat-driven cleanup.

use std::sync::Arc;
use std::time::{Duration, Instant};

use dolphin_core::db::Db;
use dolphin_core::model::*;
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, CoreRef};
use serde_json::json;

fn chat(core: &CoreRef, name: &str) -> Chat {
    core.list_chats().unwrap().into_iter().find(|c| c.name == name).unwrap()
}

fn name_of(core: &CoreRef, id: &Option<String>) -> String {
    match id {
        None => "You".into(),
        Some(id) => core.list_agents().unwrap().into_iter().find(|a| &a.id == id).map(|a| a.name).unwrap_or_default(),
    }
}

/// Wait until no message in the chat is streaming and `pred` holds.
async fn settle(core: &CoreRef, chat_id: &str, timeout: Duration, pred: impl Fn(&[Message]) -> bool) -> Vec<Message> {
    let start = Instant::now();
    loop {
        let msgs = core.list_messages(chat_id, 200).unwrap();
        let streaming = msgs.iter().any(|m| matches!(m.status, MessageStatus::Streaming | MessageStatus::Pending));
        if !streaming && pred(&msgs) {
            return msgs;
        }
        if start.elapsed() > timeout {
            println!("  !! timed out after {:?}", timeout);
            return msgs;
        }
        tokio::time::sleep(Duration::from_millis(300)).await;
    }
}

fn report(core: &CoreRef, msgs: &[Message], since: usize) {
    for m in &msgs[since.min(msgs.len())..] {
        let who = name_of(core, &m.agent_id);
        let routing = &m.meta["routing"];
        let extra = match m.kind {
            MessageKind::Ack => format!(" → {} [{}] via {}", routing["targets"], routing["mode"], routing["source"]),
            MessageKind::Plan => format!(" [PLAN approved={}]", m.meta["plan"]["approved"]),
            _ => String::new(),
        };
        let tools: Vec<String> = m.meta["tools"].as_array().map(|a| a.iter().map(|t| format!("{} ok={}", t["summary"], t["ok"])).collect()).unwrap_or_default();
        println!(
            "  [{:?}/{:?}] {who}{extra}: {}",
            m.kind,
            m.status,
            m.content.replace('\n', " ").chars().take(220).collect::<String>()
        );
        if let Some(r) = &m.reasoning {
            println!("      thinking: {} chars, {:?} ms; tokens in/out {:?}/{:?}; working {:?} ms", r.len(), m.thinking_ms, m.tokens_in, m.tokens_out, m.working_ms);
        }
        if !tools.is_empty() {
            println!("      tools: {}", tools.join(" | "));
        }
        if let Some(e) = &m.error {
            println!("      error: {e}");
        }
    }
}

#[tokio::main]
async fn main() {
    let model = std::env::args().nth(1).unwrap_or_else(|| "qwen3:4b".into());
    let dir = tempfile::tempdir().unwrap();
    let core = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    dolphin_core::scheduler::spawn(core.clone());

    let cfg = core
        .save_provider(ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Ollama".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 }, None)
        .unwrap();
    let models = core.list_models(&cfg.id).await.expect("Ollama reachable");
    println!("== models");
    for m in &models {
        println!("  {} reasoning={} effort={:?} tools={} vision={} ctx={:?}", m.id, m.reasoning, m.effort, m.tools, m.vision, m.context_window);
    }
    let info = models.iter().find(|m| m.id == model).unwrap_or_else(|| panic!("{model} is not pulled"));
    assert!(info.reasoning, "{model} must be a reasoning model");
    for a in core.list_agents().unwrap() {
        let mut a = a;
        a.model = Some(ModelRef { provider_id: cfg.id.clone(), model_id: model.clone(), effort: Some(EffortValue::Toggle(true)), vision: info.vision, tools: info.tools });
        core.update_agent(a).unwrap();
    }

    let long = Duration::from_secs(600);

    println!("\n== 7.3 Coding: Java question");
    let coding = chat(&core, "Coding");
    core.send_message(&coding.id, "How do I implement a function in Java?", vec![]).await.unwrap();
    let msgs = settle(&core, &coding.id, long, |m| m.iter().filter(|m| m.agent_id.is_some() && m.kind != MessageKind::Ack).count() >= 1).await;
    report(&core, &msgs, 0);

    println!("\n== @mention: Ducky in Coding");
    let n = msgs.len();
    core.send_message(&coding.id, "@Ducky write a one-line docstring for a Python function add(a, b)", vec![]).await.unwrap();
    let msgs = settle(&core, &coding.id, long, |m| m.len() >= n + 2).await;
    report(&core, &msgs, n);

    println!("\n== 7.4 Long Horizon: plan → approve");
    let lh = chat(&core, "Long Horizon Agentic");
    let ws = dir.path().join("project");
    std::fs::create_dir_all(&ws).unwrap();
    std::fs::write(ws.join("main.py"), "def main():\n    print('hello')\n\nif __name__ == '__main__':\n    main()\n").unwrap();
    let mut c = lh.clone();
    c.workspace_path = Some(ws.to_string_lossy().into_owned());
    core.update_chat(c).unwrap();
    core.send_message(&lh.id, "Make a plan to add a --name command line flag to the project in my workspace", vec![]).await.unwrap();
    let msgs = settle(&core, &lh.id, long, |m| m.iter().any(|m| m.kind == MessageKind::Plan || (m.agent_id.is_some() && m.kind == MessageKind::Text))).await;
    report(&core, &msgs, 0);
    println!("  grants: {:?}", core.grants(&lh.id).unwrap().iter().map(|g| (&g.level, &g.reason)).collect::<Vec<_>>());
    if let Some(plan) = msgs.iter().find(|m| m.kind == MessageKind::Plan) {
        let n = msgs.len();
        core.send_message(&lh.id, "Looks good, I approve the plan. Go ahead.", vec![]).await.unwrap();
        let msgs = settle(&core, &lh.id, Duration::from_secs(900), |m| m.len() >= n + 3).await;
        report(&core, &msgs, n);
        println!("  grants: {:?}", core.grants(&lh.id).unwrap().iter().map(|g| (&g.level, &g.reason)).collect::<Vec<_>>());
        println!("  plan approved: {}", core.list_messages(&lh.id, 200).unwrap().iter().find(|m| m.id == plan.id).map(|m| m.meta["plan"]["approved"].clone()).unwrap_or_default());
        println!("  main.py now:\n{}", std::fs::read_to_string(ws.join("main.py")).unwrap());
    }

    for (name, prompt) in [
        ("General", "Hi Roger! What can this pod do?"),
        ("Learning", "Teach me what recursion is"),
        ("Creative", "Write a four-line poem about dolphins"),
    ] {
        println!("\n== {name}");
        let c = chat(&core, name);
        core.send_message(&c.id, prompt, vec![]).await.unwrap();
        let msgs = settle(&core, &c.id, long, |m| m.iter().any(|m| m.agent_id.is_some() && m.kind == MessageKind::Text)).await;
        report(&core, &msgs, 0);
    }

    println!("\n== Roger cleans General on request");
    let g = chat(&core, "General");
    core.send_message(&g.id, "Roger, clean this chat please", vec![]).await.unwrap();
    let msgs = settle(&core, &g.id, long, |m| m.iter().any(|m| m.meta["settings"] == true)).await;
    report(&core, &msgs, 0);

    println!("\n== usage");
    for u in core.usage_all().unwrap() {
        println!("  {}: {} req, {} in / {} out, thinking {} ms, working {} ms", name_of(&core, &Some(u.agent_id.clone())), u.requests, u.tokens_in, u.tokens_out, u.thinking_ms, u.working_ms);
    }
}
