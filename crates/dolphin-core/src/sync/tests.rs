//! Sync behavior across simulated devices sharing one in-memory relay.

use std::collections::BTreeMap;
use std::sync::Arc;

use rand::{Rng, SeedableRng, rngs::StdRng};
use serde_json::json;

use super::backend::MemoryBackend;
use super::engine::SyncEngine;
use super::store::Fields;
use crate::crypto::{DataKey, KdfParams, create_account};
use crate::db::Db;
use crate::model::{Agent, AgentColor, MessageKind, MessageStatus};
use crate::secrets::{MemoryStore, SecretStore, provider_key_name};
use crate::seed;
use crate::util::{new_id, now_ms};

struct Device {
    db: Db,
    secrets: Arc<MemoryStore>,
    engine: SyncEngine,
}

fn pod() -> (MemoryBackend, DataKey) {
    let (_, key, _) = create_account("a passphrase", KdfParams::FAST_FOR_TESTS).unwrap();
    (MemoryBackend::new(), key)
}

fn device(backend: &MemoryBackend, key: &DataKey) -> Device {
    let db = Db::open_in_memory().unwrap();
    seed::seed_if_needed(&db).unwrap();
    seed::seed_roles_if_needed(&db).unwrap();
    let secrets = Arc::new(MemoryStore::default());
    let engine = SyncEngine::new(db.clone(), secrets.clone(), Arc::new(backend.clone()), key.clone(), "test");
    Device { db, secrets, engine }
}

/// Run rounds on every device until nothing moves.
async fn settle(devices: &[&Device]) {
    for _ in 0..4 {
        for d in devices {
            d.engine.cycle().await.unwrap();
        }
    }
}

fn snapshot(d: &Device) -> BTreeMap<(String, String), Fields> {
    d.db.sync_snapshot().unwrap()
}

fn say(d: &Device, chat_id: &str, text: &str) -> String {
    let now = now_ms();
    let m = crate::model::Message {
        id: new_id(),
        chat_id: chat_id.into(),
        agent_id: None,
        kind: MessageKind::Text,
        status: MessageStatus::Done,
        content: text.into(),
        reasoning: None,
        error: None,
        mentions: vec![],
        attachments: vec![],
        meta: json!({}),
        thinking_ms: None,
        working_ms: None,
        tokens_in: None,
        tokens_out: None,
        created_at: now,
        updated_at: now,
    };
    d.db.insert_message(&m).unwrap();
    m.id
}

fn agent(d: &Device, slug: &str) -> Agent {
    d.db.agent_by_slug(slug).unwrap()
}

#[tokio::test]
async fn a_fresh_second_device_adopts_the_pod_instead_of_duplicating_the_defaults() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    let mut rick = agent(&a, "rick");
    rick.color = AgentColor::solid("#112233");
    a.db.upsert_agent(&rick).unwrap();
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    settle(&[&a, &b]).await;
    assert_eq!(b.db.list_agents().unwrap().len(), 10, "stable ids: no second Roger");
    assert_eq!(b.db.list_chats().unwrap().len(), 5);
    assert_eq!(b.db.list_roles().unwrap().len(), 1);
    assert_eq!(agent(&b, "rick").color, AgentColor::solid("#112233"), "the pod's customization wins over B's fresh defaults");
    assert_eq!(snapshot(&a), snapshot(&b));
}

#[tokio::test]
async fn chats_messages_settings_and_profile_reach_the_other_device() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();

    let chat = a.db.list_chats().unwrap().remove(0);
    let msg = say(&a, &chat.id, "hello from the desktop");
    a.db.set_setting_raw("profile.display_name", &json!("Luca")).unwrap();
    settle(&[&a, &b]).await;

    let got = b.db.get_message(&msg).unwrap();
    assert_eq!(got.content, "hello from the desktop");
    assert_eq!(got.chat_id, chat.id);
    assert_eq!(b.db.setting_raw("profile.display_name").unwrap(), Some(json!("Luca")));

    // And back the other way.
    let reply = say(&b, &chat.id, "hello from the phone");
    settle(&[&a, &b]).await;
    assert_eq!(a.db.get_message(&reply).unwrap().content, "hello from the phone");
    assert_eq!(snapshot(&a), snapshot(&b));
}

#[tokio::test]
async fn nothing_readable_reaches_the_relay() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    a.engine.create().await.unwrap();
    let mut chat = a.db.list_chats().unwrap().remove(0);
    chat.name = "Private chat name".into();
    a.db.upsert_chat(&chat).unwrap();
    say(&a, &chat.id, "very secret message text");
    a.db.set_setting_raw("profile.display_name", &json!("Alice Example")).unwrap();
    a.engine.cycle().await.unwrap();

    let ops = backend.dump_ops();
    assert!(ops.len() > 20, "the defaults and the edits are all there");
    for op in &ops {
        let wire = format!("{:?}", op).to_lowercase();
        for needle in ["private chat name", "very secret", "alice", "display_name", "roger", "coding", &chat.id] {
            assert!(!wire.contains(&needle.to_lowercase()), "{needle:?} leaked in {wire}");
        }
        assert!(["provider", "agent", "chat", "chat_agent", "message", "setting", "role", "usage", "attachment", "automation"].contains(&op.kind.as_str()));
    }
    // A device with the wrong key can read none of it.
    let other = device(&backend, &key);
    let stranger = SyncEngine::new(other.db.clone(), other.secrets.clone(), Arc::new(backend.clone()), DataKey::generate(), "stranger");
    let report = stranger.pull().await.unwrap();
    assert_eq!(report.rejected, ops.len(), "every operation fails authentication under the wrong key");
    assert!(report.applied.is_empty());
}

#[tokio::test]
async fn concurrent_edits_to_different_fields_both_survive_and_the_same_field_is_last_writer_wins() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    settle(&[&a, &b]).await;

    // Offline: A changes the desktop model, B the iOS override and both change the job text.
    let mut on_a = agent(&a, "rick");
    on_a.model = Some(crate::model::ModelRef { provider_id: "p".into(), model_id: "big-desktop-model".into(), effort: None, vision: false, tools: true, context_window: None });
    on_a.job = "A's job".into();
    a.db.upsert_agent(&on_a).unwrap();
    a.engine.capture().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let mut on_b = agent(&b, "rick");
    on_b.ios_model = Some(crate::model::ModelRef { provider_id: "p".into(), model_id: "small-phone-model".into(), effort: None, vision: false, tools: true, context_window: None });
    on_b.job = "B's job".into();
    b.db.upsert_agent(&on_b).unwrap();
    settle(&[&a, &b]).await;

    for d in [&a, &b] {
        let r = agent(d, "rick");
        assert_eq!(r.model.as_ref().unwrap().model_id, "big-desktop-model", "iOS edit never clobbers the desktop model");
        assert_eq!(r.ios_model.as_ref().unwrap().model_id, "small-phone-model");
        assert_eq!(r.job, "B's job", "B edited the job later");
    }
    assert_eq!(snapshot(&a), snapshot(&b));
}

#[tokio::test]
async fn cleaning_a_chat_on_one_device_clears_it_everywhere() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    let chat = a.db.list_chats().unwrap().remove(0);
    for i in 0..5 {
        say(&a, &chat.id, &format!("message {i}"));
    }
    settle(&[&a, &b]).await;
    assert_eq!(b.db.list_messages(&chat.id, 50).unwrap().len(), 5);

    a.db.tombstone_all_messages(&chat.id).unwrap();
    settle(&[&a, &b]).await;
    assert!(b.db.list_messages(&chat.id, 50).unwrap().is_empty());
    let content_left: i64 = b.db.conn().query_row("SELECT COUNT(*) FROM messages WHERE content != ''", [], |r| r.get(0)).unwrap();
    assert_eq!(content_left, 0, "the text is gone from the other device, not just hidden");
}

#[tokio::test]
async fn a_delete_beats_an_older_edit_and_a_late_edit_does_not_resurrect() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    let chat = a.db.list_chats().unwrap().remove(0);
    settle(&[&a, &b]).await;

    // B edits the chat's name while offline; A deletes the chat afterwards; then B edits again, later still.
    let mut edited = b.db.get_chat(&chat.id).unwrap();
    edited.name = "renamed on B".into();
    b.db.upsert_chat(&edited).unwrap();
    b.engine.capture().unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    a.db.delete_chat(&chat.id).unwrap();
    a.engine.cycle().await.unwrap();
    std::thread::sleep(std::time::Duration::from_millis(5));
    let mut late = b.db.get_chat(&chat.id).unwrap();
    late.name = "late edit".into();
    b.db.upsert_chat(&late).unwrap();
    settle(&[&a, &b]).await;

    assert!(a.db.get_chat(&chat.id).is_err());
    assert!(b.db.get_chat(&chat.id).is_err(), "B converges to deleted");
    assert_eq!(snapshot(&a), snapshot(&b));
}

#[tokio::test]
async fn usage_is_summed_across_devices_never_overwritten() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    let rick = agent(&a, "rick").id;
    a.db.record_usage(&rick, 100, 50, 1_000, 4_000).unwrap();
    b.db.record_usage(&rick, 7, 3, 10, 40).unwrap();
    a.db.record_usage(&rick, 1, 1, 1, 1).unwrap();
    settle(&[&a, &b]).await;
    for d in [&a, &b] {
        let u = d.db.usage_for(&rick).unwrap();
        assert_eq!((u.tokens_in, u.tokens_out, u.requests), (108, 54, 3));
        assert_eq!((u.thinking_ms, u.working_ms), (1_011, 4_041));
    }
}

#[tokio::test]
async fn a_slow_clock_cannot_lose_an_edit_made_after_seeing_the_other_device() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    settle(&[&a, &b]).await;
    b.db.set_clock_skew_ms(-3_600_000); // B's clock is an hour behind

    a.db.set_setting_raw("appearance.theme", &json!("dark")).unwrap();
    settle(&[&a, &b]).await;
    assert_eq!(b.db.setting_raw("appearance.theme").unwrap(), Some(json!("dark")));
    // B edits after having seen A's value: with plain wall clocks that edit would be "an hour older" and lose.
    b.db.set_setting_raw("appearance.theme", &json!("light")).unwrap();
    settle(&[&a, &b]).await;
    assert_eq!(a.db.setting_raw("appearance.theme").unwrap(), Some(json!("light")));
    assert_eq!(b.db.setting_raw("appearance.theme").unwrap(), Some(json!("light")));
}

#[tokio::test]
async fn a_clock_far_in_the_future_is_refused() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    a.db.set_clock_skew_ms(30 * 24 * 3_600_000); // a month ahead
    a.db.set_setting_raw("appearance.theme", &json!("light")).unwrap();
    a.engine.cycle().await.unwrap();
    b.engine.join().await.unwrap();
    assert_eq!(b.db.setting_raw("appearance.theme").unwrap(), None, "the poisoned clock value is not applied");
}

#[tokio::test]
async fn provider_keys_travel_through_the_keychain_not_the_database() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();

    let cfg = crate::model::ProviderConfig {
        id: new_id(),
        kind: crate::model::ProviderKind::Openrouter,
        name: "OpenRouter".into(),
        base_url: "https://openrouter.ai/api/v1".into(),
        extra: json!({}),
        enabled: true,
        has_key: false,
        updated_at: 0,
    };
    a.secrets.set(&provider_key_name(&cfg.id), "sk-or-secret-value").unwrap();
    a.db.upsert_provider(&cfg).unwrap();
    settle(&[&a, &b]).await;
    assert_eq!(b.secrets.get(&provider_key_name(&cfg.id)).unwrap().as_deref(), Some("sk-or-secret-value"));
    assert_eq!(b.db.get_provider(&cfg.id).unwrap().name, "OpenRouter");
    let in_db: i64 = b.db.conn().query_row("SELECT COUNT(*) FROM provider_configs WHERE extra LIKE '%sk-or%' OR name LIKE '%sk-or%' OR base_url LIKE '%sk-or%'", [], |r| r.get(0)).unwrap();
    assert_eq!(in_db, 0);
    assert!(!format!("{:?}", backend.dump_ops()).contains("sk-or-secret"));

    // Removing the key (or the provider) on one device removes it on the other.
    a.secrets.delete(&provider_key_name(&cfg.id)).unwrap();
    a.db.upsert_provider(&cfg).unwrap();
    settle(&[&a, &b]).await;
    assert_eq!(b.secrets.get(&provider_key_name(&cfg.id)).unwrap(), None);
}

#[tokio::test]
async fn unfinished_local_messages_stay_put_and_finished_text_is_immutable() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    let chat = a.db.list_chats().unwrap().remove(0);

    let mut m = crate::model::Message {
        id: new_id(), chat_id: chat.id.clone(), agent_id: None, kind: MessageKind::Text, status: MessageStatus::Streaming,
        content: "half a thou".into(), reasoning: None, error: None, mentions: vec![], attachments: vec![], meta: json!({}),
        thinking_ms: None, working_ms: None, tokens_in: None, tokens_out: None, created_at: now_ms(), updated_at: now_ms(),
    };
    a.db.insert_message(&m).unwrap();
    settle(&[&a, &b]).await;
    assert!(b.db.get_message(&m.id).is_err(), "a message still being written doesn't leave the device");

    m.status = MessageStatus::Done;
    m.content = "half a thought, finished".into();
    a.db.update_message(&m).unwrap();
    settle(&[&a, &b]).await;
    assert_eq!(b.db.get_message(&m.id).unwrap().content, "half a thought, finished");

    // A remote-triggered message streams to the other device while it is still being written.
    let mut r = m.clone();
    r.id = new_id();
    r.status = MessageStatus::Streaming;
    r.content = "so far".into();
    r.meta = json!({ "remote": true });
    a.db.insert_message(&r).unwrap();
    settle(&[&a, &b]).await;
    assert_eq!(b.db.get_message(&r.id).unwrap().content, "so far");
    r.content = "so far, and more".into();
    a.db.update_message(&r).unwrap();
    settle(&[&a, &b]).await;
    assert_eq!(b.db.get_message(&r.id).unwrap().content, "so far, and more");
}

#[tokio::test]
async fn going_offline_queues_changes_until_the_network_returns() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    let chat = a.db.list_chats().unwrap().remove(0);
    backend.set_offline(true);
    let msg = say(&a, &chat.id, "written offline");
    assert!(a.engine.cycle().await.is_err());
    assert!(a.db.sync_pending_count().unwrap() > 0);
    backend.set_offline(false);
    settle(&[&a, &b]).await;
    assert_eq!(a.db.sync_pending_count().unwrap(), 0);
    assert_eq!(b.db.get_message(&msg).unwrap().content, "written offline");
}

#[tokio::test]
async fn attachments_sync_their_files_lazily() {
    let (backend, key) = pod();
    let a = device(&backend, &key);
    let b = device(&backend, &key);
    a.engine.create().await.unwrap();
    b.engine.join().await.unwrap();
    let chat = a.db.list_chats().unwrap().remove(0);
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("note.txt");
    let payload: Vec<u8> = (0..2_500_000u32).map(|i| (i % 251) as u8).collect(); // spans three chunks
    std::fs::write(&path, &payload).unwrap();
    let att = crate::model::Attachment {
        id: new_id(), name: "note.txt".into(), mime: "text/plain".into(), kind: crate::model::AttachmentKind::Other,
        size: payload.len() as i64, path: path.to_string_lossy().into(),
    };
    let msg = say(&a, &chat.id, "see attached");
    a.db.add_attachment_to_message(&att, &msg, &chat.id).unwrap();
    settle(&[&a, &b]).await;

    let seen = b.db.get_message(&msg).unwrap();
    assert_eq!(seen.attachments.len(), 1);
    assert_eq!(seen.attachments[0].name, "note.txt");
    assert_eq!(seen.attachments[0].path, "", "metadata arrives first; the file is fetched on view");
    let dest = dir.path().join("b").join("note.txt");
    assert!(b.engine.download_blob(&att.id, &dest).await.unwrap());
    assert_eq!(std::fs::read(&dest).unwrap(), payload);
    assert!(!b.engine.download_blob("missing", &dest).await.unwrap());
}

// --------------------------------------------------------------------------------------- convergence

#[derive(Debug)]
enum Step {
    EditAgent(usize),
    EditSetting(usize),
    NewChat(usize),
    RenameChat(usize),
    DeleteChat(usize),
    Say(usize),
    CleanChat(usize),
    Usage(usize),
    Sync(usize),
    PushOnly(usize),
    PullOnly(usize),
}

#[tokio::test]
async fn random_schedules_on_three_replicas_always_converge() {
    for seed in 0..40u64 {
        let mut rng = StdRng::seed_from_u64(seed);
        let (backend, key) = pod();
        let devices = [device(&backend, &key), device(&backend, &key), device(&backend, &key)];
        devices[0].engine.create().await.unwrap();
        devices[1].engine.join().await.unwrap();
        devices[2].engine.join().await.unwrap();
        for d in &devices {
            d.db.set_clock_skew_ms(rng.random_range(-120_000..120_000));
        }
        let mut chats: Vec<String> = devices[0].db.list_chats().unwrap().into_iter().map(|c| c.id).collect();
        let agents: Vec<String> = devices[0].db.list_agents().unwrap().into_iter().map(|a| a.id).collect();
        let mut counter = 0u32;
        let mut script = Vec::new();
        for _ in 0..rng.random_range(30..70) {
            let d = rng.random_range(0..3);
            script.push(match rng.random_range(0..11) {
                0 => Step::EditAgent(d),
                1 => Step::EditSetting(d),
                2 => Step::NewChat(d),
                3 => Step::RenameChat(d),
                4 => Step::DeleteChat(d),
                5 | 6 => Step::Say(d),
                7 => Step::CleanChat(d),
                8 => Step::Usage(d),
                9 => Step::Sync(d),
                _ => if rng.random_bool(0.5) { Step::PushOnly(d) } else { Step::PullOnly(d) },
            });
        }
        for step in &script {
            counter += 1;
            match step {
                Step::EditAgent(d) => {
                    let id = &agents[rng.random_range(0..agents.len())];
                    if let Ok(mut a) = devices[*d].db.get_agent(id) {
                        a.job = format!("job {seed}-{counter}");
                        if rng.random_bool(0.5) {
                            a.color = AgentColor::solid(&format!("#{:06x}", rng.random_range(0..0xffffff)));
                        }
                        devices[*d].db.upsert_agent(&a).unwrap();
                    }
                }
                Step::EditSetting(d) => {
                    let key = ["appearance.theme", "profile.display_name", "web.access"][rng.random_range(0..3)];
                    devices[*d].db.set_setting_raw(key, &json!(format!("v{counter}"))).unwrap();
                }
                Step::NewChat(d) => {
                    let c = devices[*d].db.upsert_chat(&crate::model::Chat {
                        id: new_id(), name: format!("chat {counter}"), icon: "hash".into(), sort: counter as i32,
                        agent_ids: agents[..3].to_vec(), cleanup_enabled: true, cleanup_ttl_hours: 24, workspace_path: None, created_at: 0, updated_at: 0,
                    }).unwrap();
                    chats.push(c.id);
                }
                Step::RenameChat(d) => {
                    let id = &chats[rng.random_range(0..chats.len())];
                    if let Ok(mut c) = devices[*d].db.get_chat(id) {
                        c.name = format!("renamed {counter}");
                        devices[*d].db.upsert_chat(&c).unwrap();
                    }
                }
                Step::DeleteChat(d) => {
                    if rng.random_bool(0.25) {
                        let id = &chats[rng.random_range(0..chats.len())];
                        devices[*d].db.delete_chat(id).unwrap();
                    }
                }
                Step::Say(d) => {
                    let id = chats[rng.random_range(0..chats.len())].clone();
                    if devices[*d].db.get_chat(&id).is_ok() {
                        say(&devices[*d], &id, &format!("msg {counter}"));
                    }
                }
                Step::CleanChat(d) => {
                    let id = &chats[rng.random_range(0..chats.len())];
                    devices[*d].db.tombstone_all_messages(id).unwrap();
                }
                Step::Usage(d) => devices[*d].db.record_usage(&agents[rng.random_range(0..agents.len())], 3, 2, 5, 7).unwrap(),
                Step::Sync(d) => {
                    devices[*d].engine.cycle().await.unwrap();
                }
                Step::PushOnly(d) => {
                    devices[*d].engine.capture().unwrap();
                    devices[*d].engine.push().await.unwrap();
                }
                Step::PullOnly(d) => {
                    devices[*d].engine.pull().await.unwrap();
                }
            }
        }
        let refs: Vec<&Device> = devices.iter().collect();
        settle(&refs).await;
        let (s0, s1, s2) = (snapshot(&devices[0]), snapshot(&devices[1]), snapshot(&devices[2]));
        assert_eq!(s0, s1, "seed {seed}: replicas 0 and 1 diverged\nscript: {script:?}");
        assert_eq!(s0, s2, "seed {seed}: replicas 0 and 2 diverged\nscript: {script:?}");
        let totals: Vec<_> = devices.iter().map(|d| d.db.usage_all().unwrap().iter().map(|u| (u.agent_id.clone(), u.tokens_in, u.requests)).collect::<Vec<_>>()).collect();
        assert_eq!(totals[0], totals[1]);
        assert_eq!(totals[0], totals[2]);
    }
}

// ------------------------------------------------------------------------------------ backend contract

use super::backend::{CommandRow, DeviceRow, RemoteOp, SyncBackend};
use crate::crypto::Sealed;

fn sealed(b: &[u8]) -> Sealed {
    Sealed { nonce: vec![7; 24], ciphertext: b.to_vec() }
}

fn op(id: &str, hlc: &str, dev: &str) -> RemoteOp {
    RemoteOp { seq: 0, kind: "chat".into(), id: id.into(), hlc: hlc.into(), device_id: dev.into(), sealed: sealed(b"x"), deleted: false }
}

/// What every relay must do, run against the in-memory one always and against Postgres when a URL is given.
async fn backend_contract(b: &dyn SyncBackend) {
    b.init().await.unwrap();
    b.init().await.unwrap(); // idempotent

    let (account, _, _) = create_account("pw", KdfParams::FAST_FOR_TESTS).unwrap();
    assert!(b.get_account().await.unwrap().is_none());
    assert!(b.create_account(&account).await.unwrap());
    assert!(!b.create_account(&account).await.unwrap(), "the second device to create loses the race");
    assert_eq!(b.get_account().await.unwrap().unwrap(), account);
    let mut next = account.clone();
    next.generation = 1;
    assert!(!b.replace_account(&next, 5).await.unwrap(), "stale generation");
    assert!(b.replace_account(&next, 0).await.unwrap());
    assert_eq!(b.get_account().await.unwrap().unwrap().generation, 1);

    b.push_ops("dev-a", &[op("r1", "0001", "dev-a"), op("r2", "0002", "dev-a")]).await.unwrap();
    b.push_ops("dev-a", &[op("r1", "0001", "dev-a"), op("r3", "0003", "dev-a")]).await.unwrap(); // retry + new
    let all = b.pull_ops(0, 100).await.unwrap();
    assert_eq!(all.iter().map(|o| o.id.as_str()).collect::<Vec<_>>(), ["r1", "r2", "r3"], "a retried push adds nothing");
    assert!(all.windows(2).all(|w| w[0].seq < w[1].seq));
    let after = b.pull_ops(all[0].seq, 1).await.unwrap();
    assert_eq!(after.len(), 1);
    assert_eq!(after[0].id, "r2");
    assert_eq!(after[0].sealed, sealed(b"x"));

    let dev = |id: &str| DeviceRow { id: id.into(), last_seen_ms: 5, revoked: false, sealed: sealed(b"name") };
    b.upsert_device(&dev("dev-a")).await.unwrap();
    b.upsert_device(&dev("dev-b")).await.unwrap();
    assert_eq!(b.list_devices().await.unwrap().len(), 2);
    b.revoke_device("dev-b").await.unwrap();
    assert!(b.push_ops("dev-b", &[op("r9", "0009", "dev-b")]).await.is_err(), "a removed device can't write");
    b.upsert_device(&dev("dev-b")).await.unwrap();
    assert!(b.list_devices().await.unwrap().iter().find(|d| d.id == "dev-b").unwrap().revoked, "a heartbeat doesn't un-revoke");

    let cmd = |id: &str, ttl: i64| CommandRow {
        id: id.into(), seq: 0, from_device_id: "phone".into(), target_device_id: "desk".into(),
        created_at: now_ms(), expires_at: now_ms() + ttl, sealed: sealed(b"do it"), status: "new".into(),
    };
    b.put_command(&cmd("c1", 60_000)).await.unwrap();
    b.put_command(&cmd("c1", 60_000)).await.unwrap();
    b.put_command(&cmd("c2", -1_000)).await.unwrap(); // already expired
    let pending = b.pull_commands("desk", 10).await.unwrap();
    assert_eq!(pending.iter().map(|c| c.id.as_str()).collect::<Vec<_>>(), ["c1"]);
    assert!(b.pull_commands("someone-else", 10).await.unwrap().is_empty());
    b.set_command_status("c1", "taken").await.unwrap();
    assert!(b.pull_commands("desk", 10).await.unwrap().is_empty());
    assert_eq!(b.get_command("c1").await.unwrap().unwrap().status, "taken");

    assert!(!b.has_blob("att").await.unwrap());
    b.put_blob("att", &[sealed(b"one"), sealed(b"two")]).await.unwrap();
    assert!(b.has_blob("att").await.unwrap());
    assert_eq!(b.get_blob("att").await.unwrap().unwrap(), vec![sealed(b"one"), sealed(b"two")]);
    b.put_blob("att", &[sealed(b"only")]).await.unwrap();
    assert_eq!(b.get_blob("att").await.unwrap().unwrap().len(), 1, "re-upload replaces");
}

#[tokio::test]
async fn the_in_memory_relay_meets_the_contract() {
    backend_contract(&MemoryBackend::new()).await;
}

/// `DOLPHIN_TEST_PG_URL=postgres://user@localhost/db?sslmode=disable cargo test -p dolphin-core postgres -- --ignored`
#[tokio::test]
#[ignore = "needs DOLPHIN_TEST_PG_URL"]
async fn postgres_meets_the_contract_and_two_devices_converge_through_it() {
    let url = std::env::var("DOLPHIN_TEST_PG_URL").expect("DOLPHIN_TEST_PG_URL");
    let fresh = || super::postgres::PostgresBackend::new(&url).unwrap();
    let admin = fresh();
    admin.init().await.unwrap();
    let client = admin.client_for_tests().await;
    client.batch_execute("DROP SCHEMA dolphinpod CASCADE").await.unwrap();
    backend_contract(&fresh()).await;
    client.batch_execute("DROP SCHEMA dolphinpod CASCADE").await.unwrap();

    let backend: Arc<dyn SyncBackend> = Arc::new(fresh());
    backend.init().await.unwrap();
    let (_, key, _) = create_account("pw", KdfParams::FAST_FOR_TESTS).unwrap();
    let mk = || {
        let db = Db::open_in_memory().unwrap();
        seed::seed_if_needed(&db).unwrap();
        let secrets = Arc::new(MemoryStore::default());
        (db.clone(), SyncEngine::new(db, secrets, Arc::new(fresh()), key.clone(), "pg"))
    };
    let (a_db, a) = mk();
    let (b_db, b) = mk();
    a.create().await.unwrap();
    b.join().await.unwrap();
    let chat = a_db.list_chats().unwrap().remove(0);
    a_db.set_setting_raw("profile.display_name", &json!("Pg")).unwrap();
    let now = now_ms();
    let m = crate::model::Message {
        id: new_id(), chat_id: chat.id.clone(), agent_id: None, kind: MessageKind::Text, status: MessageStatus::Done,
        content: "through postgres".into(), reasoning: None, error: None, mentions: vec![], attachments: vec![], meta: json!({}),
        thinking_ms: None, working_ms: None, tokens_in: None, tokens_out: None, created_at: now, updated_at: now,
    };
    a_db.insert_message(&m).unwrap();
    for _ in 0..3 {
        a.cycle().await.unwrap();
        b.cycle().await.unwrap();
    }
    assert_eq!(b_db.get_message(&m.id).unwrap().content, "through postgres");
    assert_eq!(b_db.setting_raw("profile.display_name").unwrap(), Some(json!("Pg")));
    assert_eq!(a_db.sync_snapshot().unwrap(), b_db.sync_snapshot().unwrap());
    client.batch_execute("DROP SCHEMA dolphinpod CASCADE").await.unwrap();
}

/// TLS against a Postgres with a private CA: `DOLPHIN_TEST_PG_TLS_URL=postgres://u@localhost/db?sslmode=require&sslrootcert=/path/ca.pem`.
#[tokio::test]
#[ignore = "needs DOLPHIN_TEST_PG_TLS_URL"]
async fn postgres_over_tls_verifies_the_server_certificate() {
    let url = std::env::var("DOLPHIN_TEST_PG_TLS_URL").expect("DOLPHIN_TEST_PG_TLS_URL");
    let ok = super::postgres::PostgresBackend::new(&url).unwrap();
    ok.init().await.expect("connects with the right CA");
    let client = ok.client_for_tests().await;
    let ssl: bool = client.query_one("SELECT ssl FROM pg_stat_ssl WHERE pid = pg_backend_pid()", &[]).await.unwrap().get(0);
    assert!(ssl, "the connection is encrypted");
    client.batch_execute("DROP SCHEMA IF EXISTS dolphinpod CASCADE").await.unwrap();

    // Without the private CA the same server is refused: certificates are really being verified.
    let bare = url.split("&sslrootcert=").next().unwrap().split("sslrootcert=").next().unwrap().trim_end_matches(['?', '&']).to_string();
    let untrusted = super::postgres::PostgresBackend::new(&bare).unwrap();
    assert!(untrusted.init().await.is_err(), "an unknown CA must not be trusted");
}
