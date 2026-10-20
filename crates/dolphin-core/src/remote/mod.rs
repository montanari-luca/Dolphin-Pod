//! Remote control: a message written on one device (the iPhone) can start work on another (a desktop that
//! has the agents, the files and the models).
//!
//! Two paths carry the same signed request, and the sender tries the fast one first:
//! - **nearby (LAN)**: the desktop advertises `_dolphinpod._tcp`; the phone opens a WebSocket carrying a Noise
//!   `NNpsk0` channel keyed from the pod key, and the desktop streams the run back as it happens (`lan.rs`).
//! - **cloud**: the request is sealed into a `commands` record in the sync database; the desktop polls for
//!   it, runs it, and the results come back as ordinary (streaming) message records.
//!
//! Every request is signed by the sending device's Ed25519 key. The desktop only obeys devices the user
//! authorized *on that desktop*, whose public key it pinned at that moment, so someone who merely holds the
//! database login and the passphrase still can't drive the machine. Requests expire, carry a unique id, and a
//! request id is acted on at most once.

pub mod lan;

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use ts_rs::TS;

use crate::core::{Core, CoreRef};
use crate::crypto::record_aad;
use crate::error::{CoreError, Result};
use crate::events::{CoreEvent, ToastLevel};
use crate::model::{MessageKind, MessageStatus};
use crate::secrets::SecretStore;
use crate::sync::backend::CommandRow;
use crate::sync::manager::DeviceInfo;
use crate::util::{new_id, now_ms};

const META_ENABLED: &str = "remote.enabled";
const META_AUTHORIZED: &str = "remote.authorized";
const KEY_SIGNING: &str = "device:signing_key";
/// How long a request stays valid. Long enough for the cloud path's polling, short enough to limit replays.
pub const REQUEST_TTL_MS: i64 = 5 * 60_000;
/// A device that hasn't checked in for this long is not offered as a target.
const ONLINE_WINDOW_MS: i64 = 3 * 60_000;
const CLOUD_POLL: Duration = Duration::from_secs(6);

// ---------------------------------------------------------------------------------------------- identity

/// The device's long-lived signing key, kept in the OS keychain.
pub struct Identity {
    key: SigningKey,
}

impl Identity {
    pub fn load_or_create(secrets: &dyn SecretStore) -> Result<Self> {
        if let Some(b64) = secrets.get(KEY_SIGNING)?
            && let Ok(bytes) = B64.decode(b64.trim())
            && let Ok(seed) = <[u8; 32]>::try_from(bytes.as_slice())
        {
            return Ok(Self { key: SigningKey::from_bytes(&seed) });
        }
        let mut seed = [0u8; 32];
        rand::rng().fill_bytes(&mut seed);
        secrets.set(KEY_SIGNING, &B64.encode(seed))?;
        Ok(Self { key: SigningKey::from_bytes(&seed) })
    }

    pub fn public_key(&self) -> String {
        B64.encode(self.key.verifying_key().to_bytes())
    }

    pub fn sign(&self, msg: &[u8]) -> String {
        B64.encode(self.key.sign(msg).to_bytes())
    }
}

pub fn verify(public_key: &str, msg: &[u8], signature: &str) -> bool {
    let (Ok(pk), Ok(sig)) = (B64.decode(public_key), B64.decode(signature)) else { return false };
    let (Ok(pk), Ok(sig)) = (<[u8; 32]>::try_from(pk.as_slice()), <[u8; 64]>::try_from(sig.as_slice())) else { return false };
    VerifyingKey::from_bytes(&pk).is_ok_and(|k| k.verify(msg, &Signature::from_bytes(&sig)).is_ok())
}

// ---------------------------------------------------------------------------------------------- types

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RemoteRequest {
    /// Unique per request; the host acts on an id once.
    pub id: String,
    pub from_device_id: String,
    pub target_device_id: String,
    pub chat_id: String,
    /// The id of the user message the sender already shows locally.
    pub message_id: String,
    pub text: String,
    pub issued_at: i64,
    pub expires_at: i64,
}

impl RemoteRequest {
    /// The exact bytes that are signed.
    pub fn signing_bytes(&self) -> Vec<u8> {
        let mut out = b"dolphinpod/request/v1".to_vec();
        for part in [&self.id, &self.from_device_id, &self.target_device_id, &self.chat_id, &self.message_id, &self.text] {
            out.extend_from_slice(&(part.len() as u32).to_be_bytes());
            out.extend_from_slice(part.as_bytes());
        }
        out.extend_from_slice(&self.issued_at.to_be_bytes());
        out.extend_from_slice(&self.expires_at.to_be_bytes());
        out
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum RemotePath {
    /// Straight to the desktop over the local network.
    Lan,
    /// Through the sync database.
    Cloud,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteReceipt {
    pub command_id: String,
    pub message_id: String,
    pub path: RemotePath,
    pub target_device_id: String,
    pub target_name: String,
}

/// Where a desktop can be reached on the local network (found by Bonjour on the platform side).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct LanEndpoint {
    pub host: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RemoteSettings {
    pub enabled: bool,
    /// Whether the nearby-network listener is up.
    pub lan_port: Option<u16>,
}

// ---------------------------------------------------------------------------------------------- runtime

#[derive(Default)]
pub struct RemoteRuntime {
    /// Chats with a remote-triggered run in flight; everything written there is marked remote so it syncs while streaming.
    runs: Mutex<HashMap<String, u32>>,
    wake: tokio::sync::Notify,
    lan: Mutex<Option<lan::LanServer>>,
    identity: Mutex<Option<Arc<Identity>>>,
    /// Test hook: shorter cloud polling.
    fast_poll: Mutex<bool>,
}

impl RemoteRuntime {
    pub(crate) fn is_active(&self, chat_id: &str) -> bool {
        self.runs.lock().unwrap().get(chat_id).is_some_and(|n| *n > 0)
    }

    pub(crate) fn begin(&self, chat_id: &str) {
        *self.runs.lock().unwrap().entry(chat_id.into()).or_insert(0) += 1;
    }

    pub(crate) fn end(&self, chat_id: &str) {
        let mut runs = self.runs.lock().unwrap();
        if let Some(n) = runs.get_mut(chat_id) {
            *n = n.saturating_sub(1);
        }
    }
}

fn clip(s: &str, n: usize) -> String {
    crate::util::truncate(s.trim(), n)
}

impl Core {
    // ------------------------------------------------------------------ host settings

    pub fn remote_settings(&self) -> RemoteSettings {
        RemoteSettings { enabled: self.remote_enabled(), lan_port: self.remote.lan.lock().unwrap().as_ref().map(|l| l.port()) }
    }

    /// Opt this device in or out of accepting remote requests.
    pub fn set_remote_enabled(&self, on: bool) -> Result<()> {
        self.db.meta_set(META_ENABLED, if on { "1" } else { "0" })?;
        self.remote.wake.notify_one();
        self.sync_wake(); // the device list advertises whether remote control is on
        Ok(())
    }

    fn authorized_map(&self) -> HashMap<String, String> {
        self.db.meta_get(META_AUTHORIZED).ok().flatten().and_then(|s| serde_json::from_str(&s).ok()).unwrap_or_default()
    }

    pub fn remote_authorized_ids(&self) -> HashSet<String> {
        self.authorized_map().into_keys().collect()
    }

    /// Allow or stop allowing one device to control this one. Allowing pins the device's current public key.
    pub async fn remote_authorize(&self, device_id: &str, allow: bool) -> Result<()> {
        let mut map = self.authorized_map();
        if allow {
            if device_id == self.db.device_id() {
                return Err(CoreError::invalid("this device is already in charge of itself"));
            }
            let devices = self.sync_devices_raw().await?;
            let pk = devices
                .into_iter()
                .find(|(id, _)| id == device_id)
                .and_then(|(_, info)| info["pk"].as_str().map(String::from))
                .ok_or_else(|| CoreError::NotFound("that device hasn't published a key yet; open the app on it and sync".into()))?;
            map.insert(device_id.into(), pk);
        } else {
            map.remove(device_id);
        }
        self.db.meta_set(META_AUTHORIZED, &serde_json::to_string(&map)?)?;
        self.sync.wake();
        Ok(())
    }

    pub(crate) fn identity(&self) -> Result<Arc<Identity>> {
        let mut slot = self.remote.identity.lock().unwrap();
        if let Some(i) = slot.as_ref() {
            return Ok(i.clone());
        }
        let i = Arc::new(Identity::load_or_create(self.secrets.as_ref())?);
        *slot = Some(i.clone());
        Ok(i)
    }

    // ------------------------------------------------------------------ host: accepting a request

    /// Check a request against everything the host demands. Marks the id as seen, so call once per request.
    pub(crate) fn remote_admit(&self, req: &RemoteRequest, signature: &str) -> Result<String> {
        if !self.remote_enabled() {
            return Err(CoreError::Permission("remote control is off on this device".into()));
        }
        if req.target_device_id != self.db.device_id() {
            return Err(CoreError::Permission("the request is addressed to another device".into()));
        }
        let now = now_ms();
        if req.expires_at <= now {
            return Err(CoreError::Permission("the request expired".into()));
        }
        if req.expires_at - req.issued_at > REQUEST_TTL_MS + 5_000 || req.issued_at > now + 60_000 {
            return Err(CoreError::Permission("the request has an implausible time window".into()));
        }
        let pinned = self.authorized_map();
        let Some(pk) = pinned.get(&req.from_device_id) else {
            return Err(CoreError::Permission(format!("device {} is not allowed to control this one", &req.from_device_id)));
        };
        if !verify(pk, &req.signing_bytes(), signature) {
            return Err(CoreError::Permission("the request's signature does not match the authorized device".into()));
        }
        // Once per request id, and once per message: a request retried over the other path never runs twice.
        if !self.db.remote_mark_seen(&req.id)? || !self.db.remote_mark_seen(&format!("message:{}", req.message_id))? {
            return Err(CoreError::Permission("this request was already handled (replay)".into()));
        }
        Ok(pk.clone())
    }

    /// Run an admitted request through the normal pipeline, marked remote, and log it.
    pub(crate) async fn remote_execute(self: &Arc<Self>, req: &RemoteRequest, from_name: &str) -> Result<tokio::task::JoinHandle<()>> {
        let chat = self.db.get_chat(&req.chat_id)?;
        let meta = json!({ "remote": true, "remoteDevice": from_name });
        let roger = self.agent_by_slug("roger")?;
        let entry = self.db.log_action(
            &chat.id,
            &roger.id,
            "remote",
            &format!("{from_name} asked: {}", clip(&req.text, 80)),
            true,
            &format!("remote request {} from device {}", req.id, req.from_device_id),
        )?;
        self.emit(CoreEvent::ActionLogged { entry });
        self.toast(ToastLevel::Info, format!("{from_name} sent a request in \"{}\"", chat.name));
        self.send_message_remote(&chat.id, &req.text, &req.message_id, meta).await
    }

    // ------------------------------------------------------------------ host: polling the cloud path

    async fn poll_cloud_commands(self: &Arc<Self>) -> Result<()> {
        let (Some(engine), Some(backend)) = (self.sync.engine(), self.sync.backend()) else { return Ok(()) };
        for row in backend.pull_commands(self.db.device_id(), 10).await? {
            let outcome = self.admit_cloud_command(&engine, &row).await;
            match outcome {
                Ok(()) => backend.set_command_status(&row.id, "done").await?,
                Err(e) => {
                    tracing::warn!("remote request {} refused: {e}", row.id);
                    backend.set_command_status(&row.id, "rejected").await?;
                    if matches!(e, CoreError::Permission(_)) {
                        self.toast(ToastLevel::Warning, format!("A remote request was refused: {e}"));
                    }
                }
            }
        }
        Ok(())
    }

    async fn admit_cloud_command(self: &Arc<Self>, engine: &crate::sync::SyncEngine, row: &CommandRow) -> Result<()> {
        if row.target_device_id != self.db.device_id() {
            return Err(CoreError::Permission("addressed to another device".into()));
        }
        let aad = command_aad(&row.id, &row.from_device_id, &row.target_device_id, row.expires_at);
        let plain = engine.key().open(&aad, &row.sealed)?;
        let v: Value = serde_json::from_slice(&plain)?;
        let req: RemoteRequest = serde_json::from_value(v["request"].clone())?;
        let sig = v["sig"].as_str().unwrap_or_default();
        if req.id != row.id || req.from_device_id != row.from_device_id || req.expires_at != row.expires_at {
            return Err(CoreError::Permission("the request does not match its envelope".into()));
        }
        self.remote_admit(&req, sig)?;
        let from = self.sync_device_name(&req.from_device_id).await;
        self.remote_execute(&req, &from).await?;
        Ok(())
    }

    // ------------------------------------------------------------------ client: choosing a target and sending

    /// Desktops of the pod that accept remote requests and have checked in recently, most recently active first.
    pub async fn remote_targets(&self) -> Result<Vec<DeviceInfo>> {
        let now = now_ms();
        Ok(self
            .sync_devices()
            .await?
            .into_iter()
            .filter(|d| !d.this_device && !d.revoked && d.kind == "desktop" && d.remote && now - d.last_seen < ONLINE_WINDOW_MS)
            .collect())
    }

    /// Send `text` to run in `chat_id` on a desktop. Tries the nearby-network path first when `lan` is given,
    /// then the cloud, and reports which one carried it.
    pub async fn remote_send(
        self: &Arc<Self>,
        target: Option<&str>,
        chat_id: &str,
        text: &str,
        lan: Option<LanEndpoint>,
    ) -> Result<RemoteReceipt> {
        if text.trim().is_empty() {
            return Err(CoreError::invalid("the message is empty"));
        }
        let engine = self.sync.engine().ok_or_else(|| CoreError::invalid("turn on sync to control another device"))?;
        let targets = self.remote_targets().await?;
        let target = match target {
            Some(id) => targets.into_iter().find(|d| d.id == id),
            None => targets.into_iter().next(),
        }
        .ok_or_else(|| CoreError::NotFound("no desktop with remote control turned on is online".into()))?;
        let chat = self.db.get_chat(chat_id)?;

        // The sender shows its own message at once; the desktop reuses the same id instead of duplicating it.
        let now = now_ms();
        let mut msg = self.new_message(&chat.id, None, MessageKind::Text, MessageStatus::Done, text.trim_end());
        msg.meta = json!({ "remote": true, "remoteTarget": target.name });
        self.insert_message(&msg)?;

        let req = RemoteRequest {
            id: new_id(),
            from_device_id: self.db.device_id().into(),
            target_device_id: target.id.clone(),
            chat_id: chat.id.clone(),
            message_id: msg.id.clone(),
            text: text.trim_end().into(),
            issued_at: now,
            expires_at: now + REQUEST_TTL_MS,
        };
        let sig = self.identity()?.sign(&req.signing_bytes());
        let receipt = |path| RemoteReceipt {
            command_id: req.id.clone(),
            message_id: msg.id.clone(),
            path,
            target_device_id: target.id.clone(),
            target_name: target.name.clone(),
        };

        if let Some(ep) = lan {
            match lan::send(self, &engine, &ep, &req, &sig).await {
                Ok(()) => return Ok(receipt(RemotePath::Lan)),
                // The desktop heard us and said no: the cloud would get the same answer.
                Err(e @ CoreError::Permission(_)) => {
                    let _ = self.db.tombstone_message(&msg.id);
                    return Err(e);
                }
                Err(e) => tracing::info!("nearby path unavailable ({e}); using the cloud"),
            }
        }
        let backend = self.sync.backend().ok_or_else(|| CoreError::invalid("turn on sync to control another device"))?;
        let aad = command_aad(&req.id, &req.from_device_id, &req.target_device_id, req.expires_at);
        let sealed = engine.key().seal(&aad, json!({ "request": req, "sig": sig }).to_string().as_bytes())?;
        let sent = backend
            .put_command(&CommandRow {
                id: req.id.clone(),
                seq: 0,
                from_device_id: req.from_device_id.clone(),
                target_device_id: req.target_device_id.clone(),
                created_at: now,
                expires_at: req.expires_at,
                sealed,
                status: "new".into(),
            })
            .await;
        if let Err(e) = sent {
            let _ = self.db.tombstone_message(&msg.id);
            return Err(e);
        }
        // The results stream back through sync; look for them often while this request is running.
        self.sync.hurry_for(REQUEST_TTL_MS);
        Ok(receipt(RemotePath::Cloud))
    }

    /// Ask the remote loop to look for requests now.
    pub fn remote_wake(&self) {
        self.remote.wake.notify_one();
    }

    #[doc(hidden)]
    pub fn set_remote_fast_poll_for_tests(&self) {
        *self.remote.fast_poll.lock().unwrap() = true;
    }

    /// The listener and polling follow the opt-in switch and the sync state.
    async fn remote_reconcile(self: &Arc<Self>) {
        let want_lan = self.remote_enabled() && self.sync.engine().is_some();
        let have_lan = self.remote.lan.lock().unwrap().is_some();
        if want_lan && !have_lan {
            if let Some(engine) = self.sync.engine() {
                match lan::LanServer::start(self.clone(), engine.key().link_key(), true).await {
                    Ok(server) => *self.remote.lan.lock().unwrap() = Some(server),
                    Err(e) => tracing::warn!("nearby listener failed to start: {e}"),
                }
            }
        } else if !want_lan && have_lan {
            *self.remote.lan.lock().unwrap() = None; // dropping stops it
        }
    }

    /// Start the LAN listener on a given pod key without advertising (tests).
    #[doc(hidden)]
    pub async fn remote_start_lan_for_tests(self: &Arc<Self>) -> Result<u16> {
        let engine = self.sync.engine().ok_or_else(|| CoreError::invalid("sync is not set up"))?;
        let server = lan::LanServer::start(self.clone(), engine.key().link_key(), false).await?;
        let port = server.port();
        *self.remote.lan.lock().unwrap() = Some(server);
        Ok(port)
    }
}

pub fn command_aad(id: &str, from: &str, target: &str, expires_at: i64) -> Vec<u8> {
    record_aad("command", id, &expires_at.to_string(), &format!("{from}>{target}"))
}

/// The remote-control loop: keeps the listener in step with the settings and polls for cloud requests.
pub fn spawn(core: CoreRef) {
    tokio::spawn(async move {
        loop {
            core.remote_reconcile().await;
            if core.remote_enabled() {
                if let Err(e) = core.poll_cloud_commands().await {
                    tracing::debug!("remote poll: {e}");
                }
            }
            let wait = if *core.remote.fast_poll.lock().unwrap() { Duration::from_millis(100) } else { CLOUD_POLL };
            tokio::select! {
                _ = tokio::time::sleep(wait) => {}
                _ = core.remote.wake.notified() => {}
            }
        }
    });
}

#[cfg(test)]
mod tests;
