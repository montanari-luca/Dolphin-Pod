//! A second device for manual testing: joins a pod (creating it if the database has none), turns on remote
//! control, authorizes every phone it sees, and runs requests with a local Ollama model.
//!
//!   cargo run -p dolphin-core --example pod_peer -- "postgres://dolphin@localhost:54329/dptest?sslmode=disable" "my passphrase" [model]
//!
//! For testing only: it authorizes phones automatically, which a real desktop never does.

use std::sync::Arc;
use std::time::Duration;

use dolphin_core::events::CoreEvent;
use dolphin_core::model::{EffortValue, ModelRef, ProviderConfig, ProviderKind};
use dolphin_core::secrets::MemoryStore;
use dolphin_core::{Core, sync};
use serde_json::json;

#[tokio::main]
async fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let (url, pass) = (args.first().expect("database url"), args.get(1).expect("passphrase"));
    let model = args.get(2).cloned().unwrap_or_else(|| "qwen3.5:2b".into());

    let dir = tempfile::tempdir().expect("temp dir");
    let core = Core::open(dir.path(), Arc::new(MemoryStore::default())).expect("open");
    core.set_device_info(Some("Test Desktop".into()), "macos", "desktop");
    sync::manager::spawn(core.clone());
    dolphin_core::remote::spawn(core.clone());

    let made = core.sync_setup(url, pass).await.expect("sync setup");
    println!("pod {} (recovery key shown once: {})", if made.created { "created" } else { "joined" }, made.recovery_key.is_some());

    let provider = core
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local Ollama".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .expect("provider");
    for a in core.list_agents().expect("agents") {
        let mut a = a;
        a.model = Some(ModelRef { provider_id: provider.id.clone(), model_id: model.clone(), effort: Some(EffortValue::Toggle(false)), vision: false, tools: true, context_window: Some(8192) });
        core.update_agent(a).expect("agent model");
    }
    core.set_remote_enabled(true).expect("remote on");
    println!("remote control on, model {model}; waiting for phones…");

    let mut events = core.subscribe();
    let watcher = core.clone();
    tokio::spawn(async move {
        loop {
            match events.recv().await {
                Ok(CoreEvent::MessageCreated { message }) => println!("[message] {:?} {:?}: {}", message.agent_id.is_some(), message.kind, message.content.chars().take(80).collect::<String>()),
                Ok(CoreEvent::MessageUpdated { message }) if message.status == dolphin_core::model::MessageStatus::Done && message.agent_id.is_some() => {
                    println!("[done] {}", message.content.chars().take(160).collect::<String>())
                }
                Ok(CoreEvent::Toast { text, .. }) => println!("[toast] {text}"),
                Ok(CoreEvent::SyncStatusChanged { status }) => println!("[sync] {:?} pending={}", status.state, status.pending),
                Ok(_) => {}
                Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => {}
                Err(_) => break,
            }
        }
    });
    let mut authorized = std::collections::HashSet::new();
    for _ in 0..3600 {
        if let Ok(devices) = watcher.sync_devices().await {
            let fresh: Vec<_> = devices.iter().filter(|d| !d.this_device && !d.revoked && d.kind == "phone" && !authorized.contains(&d.id)).collect();
            for d in fresh {
                match watcher.remote_authorize(&d.id, true).await {
                    Ok(()) => {
                        println!("authorized {} ({})", d.name, d.id);
                        authorized.insert(d.id.clone());
                    }
                    Err(e) => println!("could not authorize {}: {e}", d.name),
                }
            }
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}
