//! Remote control across simulated devices that share a pod through the in-process relay.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use async_trait::async_trait;
use serde_json::json;

use super::{LanEndpoint, RemotePath, RemoteRequest};
use crate::core::{Core, CoreRef};
use crate::crypto::KdfParams;
use crate::db::Db;
use crate::error::CoreError;
use crate::events::CoreEvent;
use crate::model::*;
use crate::providers::{ChatRequest, EventStream, Provider, ProviderEvent, Usage};
use crate::secrets::MemoryStore;
use crate::sync::manager;
use crate::util::{new_id, now_ms};

struct Scripted {
    seen: Mutex<Vec<String>>,
}

#[async_trait]
impl Provider for Scripted {
    fn kind(&self) -> ProviderKind {
        ProviderKind::Ollama
    }
    async fn list_models(&self) -> crate::Result<Vec<ModelInfo>> {
        Ok(vec![])
    }
    async fn stream_chat(&self, req: ChatRequest) -> crate::Result<EventStream> {
        self.seen.lock().unwrap().push(req.system.chars().take(30).collect());
        let mut events = if req.json_mode {
            vec![ProviderEvent::TextDelta(json!({"intent": "greeting", "mode": "answer", "targets": []}).to_string())]
        } else {
            vec![
                ProviderEvent::TextDelta("Roger here, ".into()),
                ProviderEvent::TextDelta("running on the desktop.".into()),
                ProviderEvent::Usage(Usage { input_tokens: 10, output_tokens: 5 }),
            ]
        };
        events.push(ProviderEvent::Done { stop_reason: Some("stop".into()), raw_assistant: None });
        Ok(Box::pin(futures::stream::iter(events)))
    }
}

fn core(dir: &tempfile::TempDir) -> CoreRef {
    let c = Core::with_db(Db::open_in_memory().unwrap(), dir.path(), Arc::new(MemoryStore::default())).unwrap();
    c.set_sync_kdf_for_tests(KdfParams::FAST_FOR_TESTS);
    c
}

/// A desktop whose agents run on a scripted model, and a phone, in one pod.
struct Pair {
    desktop: CoreRef,
    phone: CoreRef,
    provider: Arc<Scripted>,
    _dirs: (tempfile::TempDir, tempfile::TempDir),
}

async fn pair(url: &str) -> Pair {
    let dirs = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    let (desktop, phone) = (core(&dirs.0), core(&dirs.1));
    desktop.set_device_info(Some("Studio Mac".into()), "macos", "desktop");
    phone.set_device_info(Some("Luca's iPhone".into()), "ios", "phone");
    let cfg = desktop
        .save_provider(
            ProviderConfig { id: String::new(), kind: ProviderKind::Ollama, name: "Local".into(), base_url: String::new(), extra: json!({}), enabled: true, has_key: false, updated_at: 0 },
            None,
        )
        .unwrap();
    let provider = Arc::new(Scripted { seen: Mutex::new(vec![]) });
    desktop.override_provider(&cfg.id, provider.clone());
    for a in desktop.list_agents().unwrap() {
        if ["roger", "lincoln"].contains(&a.slug.as_str()) {
            let mut a = a;
            a.model = Some(ModelRef { provider_id: cfg.id.clone(), model_id: "qwen3.5:2b".into(), effort: None, vision: false, tools: true, context_window: None });
            desktop.update_agent(a).unwrap();
        }
    }
    desktop.sync_setup(url, "a long passphrase").await.unwrap();
    phone.sync_setup(url, "a long passphrase").await.unwrap();
    desktop.set_remote_fast_poll_for_tests();
    manager::spawn(desktop.clone());
    manager::spawn(phone.clone());
    super::spawn(desktop.clone());
    super::spawn(phone.clone());
    Pair { desktop, phone, provider, _dirs: dirs }
}

async fn until<F: Fn() -> bool>(what: &str, f: F) {
    for _ in 0..300 {
        if f() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("timed out waiting for: {what}");
}

async fn wait_online(p: &Pair) {
    p.desktop.set_remote_enabled(true).unwrap();
    for _ in 0..300 {
        p.desktop.sync_wake();
        p.phone.sync_wake();
        if p.phone.remote_targets().await.map(|t| !t.is_empty()).unwrap_or(false) {
            return;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    panic!("the desktop never showed up as a remote target");
}

fn first_chat(c: &CoreRef) -> Chat {
    c.list_chats().unwrap().remove(0)
}

fn signed(from: &CoreRef, to: &CoreRef, chat: &str, text: &str) -> (RemoteRequest, String) {
    let now = now_ms();
    let req = RemoteRequest {
        id: new_id(),
        from_device_id: from.db.device_id().into(),
        target_device_id: to.db.device_id().into(),
        chat_id: chat.into(),
        message_id: new_id(),
        text: text.into(),
        issued_at: now,
        expires_at: now + 60_000,
    };
    let sig = from.identity().unwrap().sign(&req.signing_bytes());
    (req, sig)
}

// ------------------------------------------------------------------------------------------ admission

#[tokio::test]
async fn the_desktop_only_obeys_authorized_fresh_signed_unrepeated_requests() {
    let p = pair("memory://remote-admit").await;
    wait_online(&p).await;
    let chat = first_chat(&p.desktop).id;
    let (phone_id, desktop_id) = (p.phone.db.device_id().to_string(), p.desktop.db.device_id().to_string());

    // Nothing is allowed until the user allows this phone on the desktop.
    let (req, sig) = signed(&p.phone, &p.desktop, &chat, "hello");
    assert!(matches!(p.desktop.remote_admit(&req, &sig), Err(CoreError::Permission(m)) if m.contains("not allowed")));
    p.desktop.remote_authorize(&phone_id, true).await.unwrap();
    assert!(p.desktop.remote_authorize(&desktop_id, true).await.is_err(), "no self-authorization");

    p.desktop.remote_admit(&req, &sig).expect("authorized, fresh, signed");
    // The same request again: a replay.
    assert!(matches!(p.desktop.remote_admit(&req, &sig), Err(CoreError::Permission(m)) if m.contains("replay")));
    // A new request id for the same message (a retry over the other path) does not run twice either.
    let mut dup = req.clone();
    dup.id = new_id();
    let dup_sig = p.phone.identity().unwrap().sign(&dup.signing_bytes());
    assert!(p.desktop.remote_admit(&dup, &dup_sig).is_err());

    // Tampering with the text breaks the signature.
    let (mut forged, sig) = signed(&p.phone, &p.desktop, &chat, "hello");
    forged.text = "delete everything".into();
    assert!(matches!(p.desktop.remote_admit(&forged, &sig), Err(CoreError::Permission(m)) if m.contains("signature")));
    // Someone with the pod key but not the phone's signing key can't pose as the phone.
    let (req, _) = signed(&p.phone, &p.desktop, &chat, "hi");
    let stranger = super::Identity::load_or_create(&MemoryStore::default()).unwrap();
    assert!(p.desktop.remote_admit(&req, &stranger.sign(&req.signing_bytes())).is_err());
    // Expired, addressed elsewhere, or implausible windows.
    let (mut old, _) = signed(&p.phone, &p.desktop, &chat, "late");
    old.issued_at -= 120_000;
    old.expires_at -= 120_000;
    assert!(matches!(p.desktop.remote_admit(&old, &p.phone.identity().unwrap().sign(&old.signing_bytes())), Err(CoreError::Permission(m)) if m.contains("expired")));
    let (mut elsewhere, _) = signed(&p.phone, &p.desktop, &chat, "x");
    elsewhere.target_device_id = "someone-else".into();
    assert!(p.desktop.remote_admit(&elsewhere, &p.phone.identity().unwrap().sign(&elsewhere.signing_bytes())).is_err());
    let (mut forever, _) = signed(&p.phone, &p.desktop, &chat, "x");
    forever.expires_at += 3_600_000;
    assert!(p.desktop.remote_admit(&forever, &p.phone.identity().unwrap().sign(&forever.signing_bytes())).is_err());

    // Revoking the permission, or turning remote control off, stops everything at once.
    let (req, sig) = signed(&p.phone, &p.desktop, &chat, "after revoke");
    p.desktop.remote_authorize(&phone_id, false).await.unwrap();
    assert!(p.desktop.remote_admit(&req, &sig).is_err());
    p.desktop.remote_authorize(&phone_id, true).await.unwrap();
    p.desktop.set_remote_enabled(false).unwrap();
    assert!(matches!(p.desktop.remote_admit(&req, &sig), Err(CoreError::Permission(m)) if m.contains("off")));
}

// ------------------------------------------------------------------------------------------ cloud path

#[tokio::test]
async fn a_request_from_the_phone_runs_on_the_desktop_through_the_cloud_and_streams_back() {
    let p = pair("memory://remote-cloud").await;
    wait_online(&p).await;
    p.desktop.remote_authorize(p.phone.db.device_id(), true).await.unwrap();
    let chat = first_chat(&p.phone);
    let mut desktop_events = p.desktop.subscribe();

    let receipt = p.phone.remote_send(None, &chat.id, "hello from my phone", None).await.unwrap();
    assert_eq!(receipt.path, RemotePath::Cloud);
    assert_eq!(receipt.target_name, "Studio Mac");

    // The phone shows its own message at once.
    let mine = p.phone.db.get_message(&receipt.message_id).unwrap();
    assert_eq!(mine.content, "hello from my phone");
    // The desktop ran it: the same message, marked remote, then Roger's answer from the desktop's model.
    until("desktop runs the request", || p.provider.seen.lock().unwrap().len() >= 2).await;
    let on_desktop = p.desktop.db.get_message(&receipt.message_id).unwrap();
    assert_eq!(on_desktop.meta["remote"], json!(true));
    until("Roger answers on the desktop", || {
        p.desktop.list_messages(&chat.id, 50).unwrap().iter().any(|m| m.content.contains("running on the desktop"))
    })
    .await;
    // And the answer reaches the phone through sync, marked remote.
    until("the phone receives the answer", || {
        p.phone.list_messages(&chat.id, 50).unwrap().iter().any(|m| m.content.contains("running on the desktop") && m.meta["remote"] == json!(true))
    })
    .await;
    // The desktop logged it and told the user.
    let log = p.desktop.action_log(&chat.id).unwrap();
    assert!(log.iter().any(|e| e.tool == "remote" && e.summary.contains("Luca's iPhone asked: hello from my phone")), "{log:?}");
    let mut toasted = false;
    while let Ok(ev) = desktop_events.try_recv() {
        toasted |= matches!(ev, CoreEvent::Toast { text, .. } if text.contains("Luca's iPhone"));
    }
    assert!(toasted);
}

#[tokio::test]
async fn an_unauthorized_phone_is_refused_and_nothing_runs() {
    let p = pair("memory://remote-refused").await;
    wait_online(&p).await;
    let chat = first_chat(&p.phone);
    let receipt = p.phone.remote_send(None, &chat.id, "do something", None).await.unwrap();
    p.desktop.remote_wake();
    tokio::time::sleep(Duration::from_millis(800)).await;
    assert!(p.provider.seen.lock().unwrap().is_empty(), "no model call happened");
    let backend = p.desktop.sync.backend().unwrap();
    until("the command is marked rejected", || {
        futures::executor::block_on(backend.get_command(&receipt.command_id)).unwrap().is_some_and(|c| c.status == "rejected")
    })
    .await;
}

#[tokio::test]
async fn nothing_is_offered_when_no_desktop_has_remote_control_on() {
    let p = pair("memory://remote-off").await;
    for _ in 0..40 {
        p.phone.sync_wake();
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(p.phone.remote_targets().await.unwrap().is_empty());
    let chat = first_chat(&p.phone);
    assert!(matches!(p.phone.remote_send(None, &chat.id, "hi", None).await, Err(CoreError::NotFound(_))));
    assert!(p.phone.list_messages(&chat.id, 10).unwrap().is_empty(), "no phantom message is left behind");
}

// ------------------------------------------------------------------------------------------ nearby path & fallback

#[tokio::test]
async fn nearby_requests_stream_straight_to_the_phone_and_unreachable_desktops_fall_back_to_the_cloud() {
    let p = pair("memory://remote-lan").await;
    wait_online(&p).await;
    p.desktop.remote_authorize(p.phone.db.device_id(), true).await.unwrap();
    let port = p.desktop.remote_start_lan_for_tests().await.unwrap();
    let chat = first_chat(&p.phone);
    let mut events = p.phone.subscribe();

    let lan = LanEndpoint { host: "127.0.0.1".into(), port };
    let receipt = p.phone.remote_send(None, &chat.id, "hello over wifi", Some(lan)).await.unwrap();
    assert_eq!(receipt.path, RemotePath::Lan, "the fast path is preferred");
    // Roger's answer arrives on the phone over the nearby channel, without waiting for a sync round.
    let mut streamed = false;
    for _ in 0..100 {
        while let Ok(ev) = events.try_recv() {
            streamed |= matches!(&ev, CoreEvent::MessageCreated { message } | CoreEvent::MessageUpdated { message } if message.content.contains("running on the desktop"));
        }
        if streamed {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(streamed, "events streamed over the LAN");
    assert!(p.phone.list_messages(&chat.id, 50).unwrap().iter().any(|m| m.content.contains("running on the desktop")), "and were stored locally");

    // Desktop not reachable on that address: the same call quietly uses the cloud.
    let dead = LanEndpoint { host: "127.0.0.1".into(), port: 9 };
    let receipt = p.phone.remote_send(None, &chat.id, "now over the internet", Some(dead)).await.unwrap();
    assert_eq!(receipt.path, RemotePath::Cloud);
    until("the cloud request runs", || p.desktop.db.get_message(&receipt.message_id).is_ok_and(|m| m.meta["remote"] == json!(true))).await;

    // A desktop that refuses over the LAN is final: the cloud would say the same, so it isn't tried.
    p.desktop.remote_authorize(p.phone.db.device_id(), false).await.unwrap();
    let lan = LanEndpoint { host: "127.0.0.1".into(), port };
    let refused = p.phone.remote_send(None, &chat.id, "should be refused", Some(lan)).await;
    assert!(matches!(refused, Err(CoreError::Permission(_))));
    assert!(!p.phone.list_messages(&chat.id, 100).unwrap().iter().any(|m| m.content == "should be refused"), "the unsent message is withdrawn");
}
