//! Two `Core`s sharing a pod through the in-process relay, driven only through the public API.

use std::sync::Arc;
use std::time::Duration;

use dolphin_core::crypto::KdfParams;
use dolphin_core::db::Db;
use dolphin_core::events::CoreEvent;
use dolphin_core::model::{Message, MessageKind, MessageStatus};
use dolphin_core::secrets::MemoryStore;
use dolphin_core::sync::manager::{self, SyncState};
use dolphin_core::{Core, CoreRef};
use serde_json::json;

fn core(dir: &tempfile::TempDir) -> CoreRef {
    let c = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    c.set_sync_kdf_for_tests(KdfParams::FAST_FOR_TESTS);
    c
}

async fn until<F: Fn() -> bool>(what: &str, f: F) {
    for _ in 0..200 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

fn text(chat_id: &str, content: &str) -> Message {
    Message {
        id: dolphin_core::util::new_id(), chat_id: chat_id.into(), agent_id: None, kind: MessageKind::Text, status: MessageStatus::Done,
        content: content.into(), reasoning: None, error: None, mentions: vec![], attachments: vec![], meta: json!({}),
        thinking_ms: None, working_ms: None, tokens_in: None, tokens_out: None, created_at: dolphin_core::util::now_ms(), updated_at: 0,
    }
}

#[tokio::test]
async fn a_chat_started_on_one_core_continues_on_another() {
    let (da, db_) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (core(&da), core(&db_));
    manager::spawn(a.clone());
    manager::spawn(b.clone());
    let url = "memory://core-test-1";

    assert_eq!(a.sync_status().state, SyncState::Off);
    let made = a.sync_setup(url, "hunter2 hunter2").await.unwrap();
    assert!(made.created);
    assert_eq!(made.recovery_key.unwrap().split(' ').count(), 24, "recovery key is shown once, as 24 words");

    assert!(b.sync_setup(url, "wrong passphrase").await.is_err(), "wrong passphrase");
    assert_eq!(b.sync_status().state, SyncState::Off);
    assert!(b.sync_setup(url, "short").await.is_err(), "too short");
    let joined = b.sync_setup(url, "hunter2 hunter2").await.unwrap();
    assert!(!joined.created && joined.recovery_key.is_none());

    let mut events = b.subscribe();
    let chat = a.list_chats().unwrap().remove(0);
    let m = text(&chat.id, "started on A");
    a.db.insert_message(&m).unwrap();
    until("B receives A's message", || b.db.get_message(&m.id).is_ok()).await;
    let mut saw = false;
    while let Ok(ev) = events.try_recv() {
        saw |= matches!(ev, CoreEvent::MessageCreated { message } if message.id == m.id);
    }
    assert!(saw, "the UI is told about the new message");

    let reply = text(&chat.id, "continued on B");
    b.db.insert_message(&reply).unwrap();
    // A would notice within its 30 s poll; focusing the window asks it to look right now.
    until("B pushes its reply", || b.sync_status().pending == 0).await;
    a.sync_wake();
    until("A receives B's reply", || a.db.get_message(&reply.id).is_ok()).await;

    until("both settle", || a.sync_status().state == SyncState::Idle && b.sync_status().state == SyncState::Idle && a.sync_status().pending == 0).await;
    let devices = a.sync_devices().await.unwrap();
    assert_eq!(devices.len(), 2);
    assert_eq!(devices.iter().filter(|d| d.this_device).count(), 1);
}

#[tokio::test]
async fn the_recovery_key_restores_access_and_sets_a_new_passphrase() {
    let (da, db_) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (core(&da), core(&db_));
    let url = "memory://core-test-2";
    let made = a.sync_setup(url, "the first passphrase").await.unwrap();
    let words = made.recovery_key.unwrap();
    assert!(b.sync_recover(url, "abandon abandon abandon", "a brand new one").await.is_err());
    b.sync_recover(url, &words, "a brand new one").await.unwrap();
    // The new passphrase now works for a third device; the old one no longer does.
    let dc = tempfile::tempdir().unwrap();
    let c = core(&dc);
    assert!(c.sync_setup(url, "the first passphrase").await.is_err());
    assert!(c.sync_setup(url, "a brand new one").await.is_ok());
}

#[tokio::test]
async fn changing_the_passphrase_and_removing_a_device() {
    let (da, db_) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (core(&da), core(&db_));
    manager::spawn(a.clone());
    manager::spawn(b.clone());
    let url = "memory://core-test-3";
    a.sync_setup(url, "original passphrase").await.unwrap();
    b.sync_setup(url, "original passphrase").await.unwrap();

    assert!(a.sync_change_passphrase("not the one", "next passphrase!").await.is_err());
    a.sync_change_passphrase("original passphrase", "next passphrase!").await.unwrap();
    let fresh_dir = tempfile::tempdir().unwrap();
    let fresh = core(&fresh_dir);
    assert!(fresh.sync_setup(url, "original passphrase").await.is_err());
    fresh.sync_setup(url, "next passphrase!").await.unwrap();

    let mut listed = Vec::new();
    for _ in 0..100 {
        listed = a.sync_devices().await.unwrap();
        if listed.len() >= 2 {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    let other = listed.iter().find(|d| !d.this_device).expect("another device").id.clone();
    assert!(a.sync_revoke_device(a.db.device_id()).await.is_err(), "can't remove yourself");
    let victim = if other == b.db.device_id() { &b } else { &fresh };
    a.sync_revoke_device(&other).await.unwrap();
    victim.sync_now().await.ok();
    until("the removed device notices", || victim.sync_status().state == SyncState::Revoked).await;
    assert!(victim.secrets_for_tests().get(manager::KEY_DATA_KEY).unwrap().is_none(), "its cached key is wiped");

    a.sync_disable().unwrap();
    assert_eq!(a.sync_status().state, SyncState::Off);
}

#[tokio::test]
async fn pointing_sync_somewhere_else_needs_a_confirmation() {
    let da = tempfile::tempdir().unwrap();
    let a = core(&da);
    a.sync_setup("memory://core-test-4a", "some passphrase").await.unwrap();
    let mut events = a.subscribe();
    let a2 = a.clone();
    let task = tokio::spawn(async move { a2.sync_setup("memory://core-test-4b", "some passphrase").await });
    let id = loop {
        if let Ok(CoreEvent::ConfirmRequest { id, title, .. }) = events.recv().await {
            assert!(title.contains("sync database"));
            break id;
        }
    };
    a.resolve_confirmation(&id, false);
    assert!(task.await.unwrap().is_err(), "declined: nothing changes");
    assert_eq!(a.sync_status().provider.as_deref(), Some("memory (core-test-4a)"));
}

#[test]
fn only_postgres_urls_are_accepted() {
    assert!(manager::make_backend("https://example.com").is_err());
    assert!(manager::make_backend("mysql://x").is_err());
    assert!(manager::make_backend("postgres://u:p@ep-cool-123.us-east-2.aws.neon.tech/neondb?sslmode=require").is_ok());
}

#[tokio::test]
async fn edits_made_while_the_first_sync_is_running_still_reach_a_device_that_joins_later() {
    use dolphin_core::model::{ModelRef, ProviderConfig, ProviderKind};
    let (da, db_) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (a, b) = (core(&da), core(&db_));
    manager::spawn(a.clone());
    manager::spawn(b.clone());
    let url = "memory://core-test-race";
    a.sync_setup(url, "race passphrase").await.unwrap();
    // Straight away, while the creating sync is still running: add a provider and give every agent a model.
    let p = a
        .save_provider(ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 }, None)
        .unwrap();
    for mut agent in a.list_agents().unwrap() {
        agent.model = Some(ModelRef { provider_id: p.id.clone(), model_id: "m".into(), effort: None, vision: false, tools: true, context_window: None });
        a.update_agent(agent).unwrap();
    }
    until("A has pushed everything", || a.sync_status().pending == 0 && a.sync_status().state == SyncState::Idle).await;
    b.sync_setup(url, "race passphrase").await.unwrap();
    until("B shows the models", || b.list_agents().unwrap().iter().all(|x| x.model.is_some())).await;
    until("B settles", || b.sync_status().pending == 0).await;
    // And B's own join must not have pushed anything that takes them away again.
    a.sync_wake();
    tokio::time::sleep(Duration::from_secs(3)).await;
    assert!(a.list_agents().unwrap().iter().all(|x| x.model.is_some()), "A still has its models");
    assert!(b.list_agents().unwrap().iter().all(|x| x.model.is_some()), "B still has the models");
}

#[tokio::test]
async fn a_device_keeps_its_own_edits_made_during_its_first_sync() {
    use dolphin_core::model::{ModelRef, ProviderConfig, ProviderKind};
    let da = tempfile::tempdir().unwrap();
    let a = core(&da);
    manager::spawn(a.clone());
    a.sync_setup("memory://core-test-alone", "alone passphrase").await.unwrap();
    let p = a
        .save_provider(ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 }, None)
        .unwrap();
    for mut agent in a.list_agents().unwrap() {
        agent.model = Some(ModelRef { provider_id: p.id.clone(), model_id: "m".into(), effort: None, vision: false, tools: true, context_window: None });
        a.update_agent(agent).unwrap();
    }
    until("A idle", || a.sync_status().pending == 0 && a.sync_status().state == SyncState::Idle).await;
    tokio::time::sleep(Duration::from_secs(1)).await;
    let with = a.list_agents().unwrap().iter().filter(|x| x.model.is_some()).count();
    assert_eq!(with, 24, "only {with} of 24 agents kept their model");
}
