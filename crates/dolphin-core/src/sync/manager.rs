//! The sync service inside `Core`: setup and unlock, the background loop, device presence and the events
//! that tell the UI what changed. Business logic stays here so the desktop and iPhone apps stay thin.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use ts_rs::TS;

use super::backend::{DeviceRow, MemoryBackend, SyncBackend};
use super::engine::{CycleReport, SyncEngine};
use super::kinds;
use super::postgres::PostgresBackend;
use super::store::Applied;
use crate::core::{Core, CoreRef};
use crate::crypto::{self, DataKey, KdfParams, RecoveryKey, device_aad};
use crate::error::{CoreError, Result};
use crate::events::CoreEvent;
use crate::util::now_ms;

pub const KEY_URL: &str = "sync:url";
pub const KEY_DATA_KEY: &str = "sync:data_key";
const META_ENABLED: &str = "sync.enabled";
const META_INITIALIZED: &str = "sync.initialized";
const META_DEVICE_NAME: &str = "device.name";
/// How often a device with nothing to say still asks the remote for news.
const POLL: Duration = Duration::from_secs(30);
const TICK: Duration = Duration::from_secs(2);
const MIN_PASSPHRASE: usize = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SyncState {
    /// Sync is not set up on this device.
    Off,
    /// Set up, but the key is not on this device (the keychain entry is gone): enter the passphrase again.
    Locked,
    Idle,
    Syncing,
    /// The remote can't be reached right now; changes queue up locally.
    Offline,
    Error,
    /// Another device removed this one from the pod.
    Revoked,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncStatus {
    pub state: SyncState,
    /// Host of the database, never the full URL (it holds the password).
    pub provider: Option<String>,
    #[ts(type = "number | null")]
    pub last_sync_at: Option<i64>,
    #[ts(type = "number")]
    pub pending: i64,
    pub error: Option<String>,
    pub device_id: String,
    pub device_name: String,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SyncSetupResult {
    /// Shown to the user exactly once, when the pod is created.
    pub recovery_key: Option<String>,
    pub created: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct DeviceInfo {
    pub id: String,
    pub name: String,
    pub platform: String,
    /// "desktop" or "phone".
    pub kind: String,
    /// The device accepts remote-control requests.
    pub remote: bool,
    /// This device lets that device control it (only meaningful on the device that decides).
    pub authorized: bool,
    #[ts(type = "number")]
    pub last_seen: i64,
    pub revoked: bool,
    pub this_device: bool,
}

#[derive(Default)]
struct Inner {
    engine: Option<Arc<SyncEngine>>,
    backend: Option<Arc<dyn SyncBackend>>,
    host: Option<String>,
    last_sync_at: Option<i64>,
    error: Option<String>,
    syncing: bool,
    offline: bool,
    revoked: bool,
    locked: bool,
    /// The first sync (join or create) has finished. Until then no ordinary round may run: it would push this
    /// device's fresh defaults before it has read the pod, and they would win over the pod's own values.
    initialized: bool,
    next_attempt_ms: i64,
    /// While a remote request is running, sync every tick instead of every 30 s.
    hurry_until_ms: i64,
    name: Option<String>,
    platform: Option<String>,
    kind: Option<String>,
    kdf: Option<KdfParams>,
}

/// Runtime state of sync, owned by `Core`.
#[derive(Default)]
pub struct SyncRuntime {
    inner: Mutex<Inner>,
    wake: tokio::sync::Notify,
    cycle: tokio::sync::Mutex<()>,
}

impl SyncRuntime {
    pub fn engine(&self) -> Option<Arc<SyncEngine>> {
        self.inner.lock().unwrap().engine.clone()
    }

    pub fn backend(&self) -> Option<Arc<dyn SyncBackend>> {
        self.inner.lock().unwrap().backend.clone()
    }

    pub(crate) fn wake(&self) {
        self.wake.notify_one();
    }

    /// Sync at the fastest pace for the next `ms`.
    pub(crate) fn hurry_for(&self, ms: i64) {
        self.inner.lock().unwrap().hurry_until_ms = now_ms() + ms;
        self.wake();
    }

    fn hurrying(&self) -> bool {
        now_ms() < self.inner.lock().unwrap().hurry_until_ms
    }
}

static MEMORY_REMOTES: Mutex<Option<HashMap<String, MemoryBackend>>> = Mutex::new(None);

/// `memory://name` is an in-process remote for tests and demos; everything else must be Postgres.
pub fn make_backend(url: &str) -> Result<(Arc<dyn SyncBackend>, String)> {
    let url = url.trim();
    if let Some(name) = url.strip_prefix("memory://") {
        let mut guard = MEMORY_REMOTES.lock().unwrap();
        let remote = guard.get_or_insert_with(HashMap::new).entry(name.to_string()).or_default().clone();
        return Ok((Arc::new(remote), format!("memory ({name})")));
    }
    if !(url.starts_with("postgres://") || url.starts_with("postgresql://")) {
        return Err(CoreError::invalid("sync needs a Postgres connection URL (postgres://…). NeonDB gives you one for free."));
    }
    let host = url::Url::parse(url).ok().and_then(|u| u.host_str().map(String::from)).unwrap_or_else(|| "database".into());
    Ok((Arc::new(PostgresBackend::new(url)?), host))
}

fn remote_name(url: &str) -> String {
    let digest = Sha256::digest(url.trim().as_bytes());
    digest.iter().take(8).map(|b| format!("{b:02x}")).collect()
}

fn network_error(e: &CoreError) -> bool {
    matches!(e, CoreError::Provider(_) | CoreError::Http(_) | CoreError::Io(_))
}

impl Core {
    // ------------------------------------------------------------------ status

    pub fn sync_enabled(&self) -> bool {
        self.db.meta_get(META_ENABLED).ok().flatten().as_deref() == Some("1")
    }

    /// Whether other devices of the pod may send this one requests (Settings → Remote control).
    pub fn remote_enabled(&self) -> bool {
        self.db.meta_get("remote.enabled").ok().flatten().as_deref() == Some("1")
    }

    /// Cheaper key derivation for tests (never use outside them).
    #[doc(hidden)]
    pub fn set_sync_kdf_for_tests(&self, kdf: KdfParams) {
        self.sync.inner.lock().unwrap().kdf = Some(kdf);
    }

    pub fn set_device_info(&self, name: Option<String>, platform: &str, kind: &str) {
        let mut i = self.sync.inner.lock().unwrap();
        i.name = name;
        i.platform = Some(platform.into());
        i.kind = Some(kind.into());
    }

    pub fn device_name(&self) -> String {
        if let Some(n) = self.sync.inner.lock().unwrap().name.clone() {
            return n;
        }
        if let Ok(Some(n)) = self.db.meta_get(META_DEVICE_NAME) {
            return n;
        }
        hostname::get().ok().and_then(|h| h.into_string().ok()).filter(|h| !h.is_empty()).unwrap_or_else(|| "This device".into())
    }

    pub fn set_device_name(&self, name: &str) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || name.chars().count() > 40 {
            return Err(CoreError::invalid("give this device a name of 1-40 characters"));
        }
        self.db.meta_set(META_DEVICE_NAME, name)?;
        self.sync.inner.lock().unwrap().name = Some(name.into());
        self.sync.wake();
        Ok(())
    }

    pub fn sync_status(&self) -> SyncStatus {
        let enabled = self.sync_enabled();
        let i = self.sync.inner.lock().unwrap();
        let state = if !enabled {
            SyncState::Off
        } else if i.revoked {
            SyncState::Revoked
        } else if i.locked || i.engine.is_none() {
            SyncState::Locked
        } else if i.syncing {
            SyncState::Syncing
        } else if i.offline {
            SyncState::Offline
        } else if i.error.is_some() {
            SyncState::Error
        } else {
            SyncState::Idle
        };
        let provider = i.host.clone();
        let (last, error) = (i.last_sync_at, i.error.clone());
        drop(i);
        SyncStatus {
            state,
            provider,
            last_sync_at: last,
            pending: if enabled { self.db.sync_pending_count().unwrap_or(0) } else { 0 },
            error,
            device_id: self.db.device_id().to_string(),
            device_name: self.device_name(),
        }
    }

    fn announce_status(&self) {
        self.emit(CoreEvent::SyncStatusChanged { status: self.sync_status() });
    }

    // ------------------------------------------------------------------ setup

    fn install(self: &Arc<Self>, url: &str, key: DataKey) -> Result<()> {
        let (backend, host) = make_backend(url)?;
        let engine = Arc::new(SyncEngine::new(self.db.clone(), self.secrets.clone(), backend.clone(), key, &remote_name(url)));
        let mut i = self.sync.inner.lock().unwrap();
        i.engine = Some(engine);
        i.backend = Some(backend);
        i.host = Some(host);
        i.locked = false;
        i.revoked = false;
        i.initialized = self.db.meta_get(META_INITIALIZED).ok().flatten().as_deref() == Some(remote_name(url).as_str());
        i.error = None;
        i.offline = false;
        i.next_attempt_ms = 0;
        Ok(())
    }

    fn persist(&self, url: &str, key: &DataKey) -> Result<()> {
        self.secrets.set(KEY_URL, url.trim())?;
        self.secrets.set(KEY_DATA_KEY, &key.to_base64())?;
        self.db.meta_set("sync.remote", &remote_name(url))?;
        self.db.meta_set(META_ENABLED, "1")?;
        Ok(())
    }

    /// Changing the database a pod syncs through is sensitive: it asks first when sync is already on.
    async fn confirm_repoint(&self) -> Result<()> {
        if self.sync_enabled()
            && !self.confirm("Change the sync database?", "This device will stop syncing with the current database and start with the new one. Your local data stays.").await
        {
            return Err(CoreError::Permission("changing the sync database was not confirmed".into()));
        }
        Ok(())
    }

    fn check_passphrase(passphrase: &str) -> Result<()> {
        if passphrase.chars().count() < MIN_PASSPHRASE {
            return Err(CoreError::invalid(format!("use a passphrase of at least {MIN_PASSPHRASE} characters")));
        }
        Ok(())
    }

    /// Turn sync on: create the pod if the database has none, otherwise join it with the passphrase.
    pub async fn sync_setup(self: &Arc<Self>, url: &str, passphrase: &str) -> Result<SyncSetupResult> {
        Self::check_passphrase(passphrase)?;
        self.confirm_repoint().await?;
        let (backend, _) = make_backend(url)?;
        backend.init().await?;
        let (key, recovery, created) = match backend.get_account().await? {
            Some(account) => (crypto::unlock_with_passphrase(&account, passphrase)?, None, false),
            None => {
                let kdf = self.sync.inner.lock().unwrap().kdf.unwrap_or(KdfParams::DEFAULT);
                let (account, key, recovery) = crypto::create_account(passphrase, kdf)?;
                if backend.create_account(&account).await? {
                    (key, Some(recovery.words()), true)
                } else {
                    // Another device created the pod a moment ago: join it instead.
                    let account = backend.get_account().await?.ok_or_else(|| CoreError::provider("the sync account vanished"))?;
                    (crypto::unlock_with_passphrase(&account, passphrase)?, None, false)
                }
            }
        };
        self.start_with(url, key, created)?;
        Ok(SyncSetupResult { recovery_key: recovery, created })
    }

    /// Forgot the passphrase: the recovery key unlocks the pod and a new passphrase replaces the old one.
    pub async fn sync_recover(self: &Arc<Self>, url: &str, recovery_words: &str, new_passphrase: &str) -> Result<()> {
        Self::check_passphrase(new_passphrase)?;
        self.confirm_repoint().await?;
        let (backend, _) = make_backend(url)?;
        backend.init().await?;
        let account = backend.get_account().await?.ok_or_else(|| CoreError::invalid("there is no pod in that database to recover"))?;
        let key = crypto::unlock_with_recovery(&account, &RecoveryKey::parse(recovery_words)?)?;
        let next = crypto::change_passphrase(&account, &key, new_passphrase)?;
        if !backend.replace_account(&next, account.generation).await? {
            return Err(CoreError::provider("another device changed the passphrase at the same moment; try again"));
        }
        self.start_with(url, key, false)
    }

    fn start_with(self: &Arc<Self>, url: &str, key: DataKey, created: bool) -> Result<()> {
        if self.sync_enabled() {
            self.db.sync_reset()?;
        }
        self.db.meta_set(META_INITIALIZED, "")?;
        self.persist(url, &key)?;
        self.install(url, key)?;
        self.announce_status();
        let core = self.clone();
        let name = remote_name(url);
        tokio::spawn(async move {
            core.run_initial(created, &name).await;
        });
        Ok(())
    }

    async fn run_initial(self: &Arc<Self>, create: bool, remote: &str) {
        let Some(engine) = self.sync.engine() else { return };
        let _guard = self.sync.cycle.lock().await;
        self.set_syncing(true);
        let result = if create { engine.create().await } else { engine.join().await };
        self.finish_cycle(result, Some(remote)).await;
    }

    /// Turn sync off on this device. Nothing is deleted locally or remotely.
    pub fn sync_disable(&self) -> Result<()> {
        let _ = self.secrets.delete(KEY_DATA_KEY);
        let _ = self.secrets.delete(KEY_URL);
        self.db.meta_set(META_ENABLED, "0")?;
        self.db.meta_set(META_INITIALIZED, "")?;
        self.db.sync_reset()?;
        {
            let mut i = self.sync.inner.lock().unwrap();
            *i = Inner { name: i.name.take(), platform: i.platform.take(), kind: i.kind.take(), kdf: i.kdf, ..Inner::default() };
        }
        self.announce_status();
        Ok(())
    }

    /// Change the passphrase. Records are not re-encrypted; only the wrapped data key changes.
    pub async fn sync_change_passphrase(&self, old: &str, new: &str) -> Result<()> {
        Self::check_passphrase(new)?;
        let backend = self.sync.backend().ok_or_else(|| CoreError::invalid("sync is not set up"))?;
        let account = backend.get_account().await?.ok_or_else(|| CoreError::provider("the pod has no account"))?;
        let key = crypto::unlock_with_passphrase(&account, old)?;
        let next = crypto::change_passphrase(&account, &key, new)?;
        if !backend.replace_account(&next, account.generation).await? {
            return Err(CoreError::provider("another device changed the passphrase at the same moment; try again"));
        }
        Ok(())
    }

    /// Replace the recovery key (the old one stops working). Needs the passphrase.
    pub async fn sync_new_recovery_key(&self, passphrase: &str) -> Result<String> {
        let backend = self.sync.backend().ok_or_else(|| CoreError::invalid("sync is not set up"))?;
        let account = backend.get_account().await?.ok_or_else(|| CoreError::provider("the pod has no account"))?;
        let key = crypto::unlock_with_passphrase(&account, passphrase)?;
        let (next, recovery) = crypto::rotate_recovery_key(&account, &key)?;
        if !backend.replace_account(&next, account.generation).await? {
            return Err(CoreError::provider("another device changed the account at the same moment; try again"));
        }
        Ok(recovery.words())
    }

    /// Re-enter the passphrase on a device whose key cache is gone.
    pub async fn sync_unlock(self: &Arc<Self>, passphrase: &str) -> Result<()> {
        let url = self.secrets.get(KEY_URL)?.ok_or_else(|| CoreError::invalid("the sync database address is missing; set sync up again"))?;
        let (backend, _) = make_backend(&url)?;
        backend.init().await?;
        let account = backend.get_account().await?.ok_or_else(|| CoreError::provider("the pod has no account"))?;
        let key = crypto::unlock_with_passphrase(&account, passphrase)?;
        self.persist(&url, &key)?;
        self.install(&url, key)?;
        self.announce_status();
        self.sync.wake();
        Ok(())
    }

    // ------------------------------------------------------------------ devices

    fn heartbeat_row(&self, engine: &SyncEngine) -> Result<DeviceRow> {
        let (platform, kind) = {
            let i = self.sync.inner.lock().unwrap();
            (i.platform.clone().unwrap_or_else(|| std::env::consts::OS.into()), i.kind.clone().unwrap_or_else(|| "desktop".into()))
        };
        let pk = self.identity().map(|i| i.public_key()).unwrap_or_default();
        let info = json!({ "name": self.device_name(), "platform": platform, "kind": kind, "remote": self.remote_enabled(), "pk": pk });
        let id = self.db.device_id().to_string();
        let sealed = engine.key().seal(&device_aad(&id), info.to_string().as_bytes())?;
        Ok(DeviceRow { id, last_seen_ms: now_ms(), revoked: false, sealed })
    }

    /// Every device row with its decrypted description.
    pub(crate) async fn sync_devices_raw(&self) -> Result<Vec<(String, Value)>> {
        let (engine, backend) = (self.sync.engine(), self.sync.backend());
        let (Some(engine), Some(backend)) = (engine, backend) else { return Ok(vec![]) };
        Ok(backend
            .list_devices()
            .await?
            .into_iter()
            .map(|row| {
                let info: Value = engine.key().open(&device_aad(&row.id), &row.sealed).ok().and_then(|b| serde_json::from_slice(&b).ok()).unwrap_or(Value::Null);
                (row.id, info)
            })
            .collect())
    }

    pub async fn sync_devices(&self) -> Result<Vec<DeviceInfo>> {
        let Some(backend) = self.sync.backend() else { return Ok(vec![]) };
        let infos: HashMap<String, Value> = self.sync_devices_raw().await?.into_iter().collect();
        let authorized = self.remote_authorized_ids();
        let mut out = Vec::new();
        for row in backend.list_devices().await? {
            let info = infos.get(&row.id).cloned().unwrap_or(Value::Null);
            out.push(DeviceInfo {
                this_device: row.id == self.db.device_id(),
                name: info["name"].as_str().unwrap_or("Unknown device").into(),
                platform: info["platform"].as_str().unwrap_or("").into(),
                kind: info["kind"].as_str().unwrap_or("desktop").into(),
                remote: info["remote"].as_bool().unwrap_or(false),
                authorized: authorized.contains(&row.id),
                last_seen: row.last_seen_ms,
                revoked: row.revoked,
                id: row.id,
            });
        }
        out.sort_by(|a, b| b.last_seen.cmp(&a.last_seen));
        Ok(out)
    }

    pub(crate) async fn sync_device_name(&self, id: &str) -> String {
        self.sync_devices().await.ok().and_then(|d| d.into_iter().find(|d| d.id == id)).map_or_else(|| "A paired device".into(), |d| d.name)
    }

    /// Remove a device from the pod. It stops receiving and sending updates the next time it syncs.
    /// (Removing is cooperative: someone holding both the database URL and the passphrase could still read
    /// the data. To lock out a lost device for good, change the database password and the passphrase.)
    pub async fn sync_revoke_device(&self, id: &str) -> Result<()> {
        if id == self.db.device_id() {
            return Err(CoreError::invalid("use \"Turn off sync\" to remove this device"));
        }
        self.sync.backend().ok_or_else(|| CoreError::invalid("sync is not set up"))?.revoke_device(id).await
    }

    // ------------------------------------------------------------------ the loop

    fn set_syncing(&self, on: bool) {
        self.sync.inner.lock().unwrap().syncing = on;
        self.announce_status();
    }

    async fn finish_cycle(self: &Arc<Self>, result: Result<CycleReport>, initialized: Option<&str>) {
        let mut revoked = false;
        match result {
            Ok(report) => {
                self.emit_applied(&report.applied);
                if let Some(remote) = initialized {
                    let _ = self.db.meta_set(META_INITIALIZED, remote);
                    self.sync.inner.lock().unwrap().initialized = true;
                }
                if let (Some(engine), Some(backend)) = (self.sync.engine(), self.sync.backend()) {
                    if let Ok(row) = self.heartbeat_row(&engine) {
                        let _ = backend.upsert_device(&row).await;
                    }
                    if let Ok(devices) = backend.list_devices().await {
                        revoked = devices.iter().any(|d| d.id == self.db.device_id() && d.revoked);
                    }
                }
                let mut i = self.sync.inner.lock().unwrap();
                i.last_sync_at = Some(now_ms());
                i.error = None;
                i.offline = false;
                i.next_attempt_ms = 0;
            }
            Err(e) => {
                tracing::warn!("sync failed: {e}");
                let mut i = self.sync.inner.lock().unwrap();
                if matches!(e, CoreError::Permission(_)) {
                    revoked = true;
                } else if network_error(&e) {
                    i.offline = true;
                    i.error = Some(e.to_string());
                } else {
                    i.error = Some(e.to_string());
                }
                i.next_attempt_ms = now_ms() + 15_000;
            }
        }
        self.sync.inner.lock().unwrap().syncing = false;
        if revoked {
            self.handle_revoked();
        }
        self.announce_status();
    }

    /// This device was removed from the pod: forget the key, stop syncing, keep the local data.
    fn handle_revoked(&self) {
        let _ = self.secrets.delete(KEY_DATA_KEY);
        let mut i = self.sync.inner.lock().unwrap();
        i.engine = None;
        i.revoked = true;
        i.locked = true;
        drop(i);
        self.toast(crate::events::ToastLevel::Warning, "This device was removed from the pod, so sync is off. Your local data is untouched.");
    }

    /// Run one round now (also what "Sync now" and window focus call).
    pub async fn sync_now(self: &Arc<Self>) -> Result<()> {
        let engine = self.sync.engine().ok_or_else(|| CoreError::invalid("sync is not set up on this device"))?;
        let _guard = self.sync.cycle.lock().await;
        self.set_syncing(true);
        // A device that hasn't finished its first sync reads the pod before it writes anything.
        let first = !self.sync.inner.lock().unwrap().initialized;
        let result = if first { engine.join().await } else { engine.cycle().await };
        let failed = result.as_ref().err().map(|e| e.to_string());
        let mark = first.then(|| self.db.meta_get("sync.remote").ok().flatten().unwrap_or_default());
        self.finish_cycle(result, mark.as_deref()).await;
        failed.map_or(Ok(()), |e| Err(CoreError::provider(e)))
    }

    /// Ask the loop to run soon (a window got focus, the app came to the foreground).
    pub fn sync_wake(&self) {
        self.sync.wake();
    }

    fn emit_applied(&self, applied: &[Applied]) {
        if applied.is_empty() {
            return;
        }
        let (mut providers, mut automations) = (false, false);
        for a in applied {
            match a.kind.as_str() {
                "message" => match self.db.get_message(&a.id) {
                    Ok(message) if a.created => self.emit(CoreEvent::MessageCreated { message }),
                    Ok(message) => self.emit(CoreEvent::MessageUpdated { message }),
                    Err(_) => {
                        if let Some(chat_id) = self.db.message_chat_id(&a.id) {
                            self.emit(CoreEvent::MessagesRemoved { chat_id, message_ids: vec![a.id.clone()], all: false });
                        }
                    }
                },
                "attachment" => {
                    if let Some(mid) = self.db.attachment_message_id(&a.id)
                        && let Ok(message) = self.db.get_message(&mid)
                    {
                        self.emit(CoreEvent::MessageUpdated { message });
                    }
                }
                "chat" => match self.db.get_chat(&a.id) {
                    Ok(chat) => self.emit(CoreEvent::ChatChanged { chat }),
                    Err(_) => self.emit(CoreEvent::ChatDeleted { chat_id: a.id.clone() }),
                },
                "chat_agent" => {
                    if let Some((chat_id, _)) = a.id.split_once('|')
                        && let Ok(chat) = self.db.get_chat(chat_id)
                    {
                        self.emit(CoreEvent::ChatChanged { chat });
                    }
                }
                "agent" => {
                    if let Ok(agent) = self.db.get_agent(&a.id) {
                        self.emit(CoreEvent::AgentChanged { agent });
                    }
                }
                "agent_memory" => {
                    if let Ok(Some(agent_id)) = self.db.memory_agent_id(&a.id) {
                        self.emit(CoreEvent::MemoriesChanged { agent_id });
                    }
                }
                "role" => {
                    if let Ok(roles) = self.db.list_roles() {
                        self.emit(CoreEvent::RolesChanged { roles });
                    }
                }
                "setting" => {
                    if let Ok(Some(value)) = self.db.setting_raw(&a.id) {
                        self.emit(CoreEvent::SettingChanged { change: crate::settings::SettingChange::Global { key: a.id.clone(), value } });
                    }
                }
                "usage" => {
                    if let Some((agent_id, _)) = a.id.split_once('|')
                        && let Ok(usage) = self.db.usage_for(agent_id)
                    {
                        self.emit(CoreEvent::UsageUpdated { usage });
                    }
                }
                kinds::PROVIDER => {
                    providers = true;
                    self.providers.invalidate(&a.id);
                }
                "automation" => automations = true,
                _ => {}
            }
        }
        if providers || automations {
            self.emit(CoreEvent::SyncApplied { providers, automations });
        }
    }

    /// Attachment bytes live remotely until first viewed; fetch them into the attachments folder.
    pub async fn sync_fetch_attachment(&self, attachment_id: &str) -> Result<String> {
        let att = self.db.get_attachment(attachment_id)?;
        if !att.path.is_empty() && std::path::Path::new(&att.path).exists() {
            return Ok(att.path);
        }
        let engine = self.sync.engine().ok_or_else(|| CoreError::invalid("sync is not set up on this device"))?;
        let safe: String = att.name.chars().map(|c| if c.is_alphanumeric() || c == '.' || c == '-' || c == '_' { c } else { '_' }).collect();
        let dest = self.attachments_dir().join(format!("{attachment_id}-{safe}"));
        if !engine.download_blob(attachment_id, &dest).await? {
            return Err(CoreError::NotFound("the file has not reached the sync database yet".into()));
        }
        self.db.set_attachment_path(attachment_id, &dest.to_string_lossy())?;
        Ok(dest.to_string_lossy().into_owned())
    }
}

/// Start the sync loop. Safe to call whether or not sync is set up: it idles until it is.
pub fn spawn(core: CoreRef) {
    tokio::spawn(async move {
        restore(&core).await;
        let mut last_full = 0i64;
        loop {
            tokio::select! {
                _ = tokio::time::sleep(TICK) => {}
                _ = core.sync.wake.notified() => { last_full = 0; }
            }
            let Some(engine) = core.sync.engine() else { continue };
            let (retry_after, syncing, initialized) = {
                let i = core.sync.inner.lock().unwrap();
                (i.next_attempt_ms, i.syncing, i.initialized)
            };
            if !initialized {
                // The first sync failed or was interrupted: try it again before anything else.
                if !syncing && now_ms() >= retry_after && core.sync.cycle.try_lock().is_ok() {
                    let remote = core.db.meta_get("sync.remote").ok().flatten().unwrap_or_default();
                    let core2 = core.clone();
                    tokio::spawn(async move { core2.run_initial(false, &remote).await });
                }
                continue;
            }
            if syncing || now_ms() < retry_after {
                continue;
            }
            let local_changes = core.db.sync_pending_count().unwrap_or(0) > 0;
            if !local_changes && !core.sync.hurrying() && now_ms() - last_full < POLL.as_millis() as i64 {
                continue;
            }
            last_full = now_ms();
            let Ok(_guard) = core.sync.cycle.try_lock() else { continue };
            core.set_syncing(true);
            let result = engine.cycle().await;
            core.finish_cycle(result, None).await;
        }
    });
}

/// On launch: bring back the engine from the keychain.
async fn restore(core: &CoreRef) {
    if !core.sync_enabled() {
        return;
    }
    let url = core.secrets.get(KEY_URL).ok().flatten();
    let key = core.secrets.get(KEY_DATA_KEY).ok().flatten().and_then(|k| DataKey::from_base64(&k).ok());
    match (url, key) {
        (Some(url), Some(key)) => {
            if let Err(e) = core.install(&url, key) {
                core.sync.inner.lock().unwrap().error = Some(e.to_string());
            }
            // A pod that never finished its first sync (the app quit mid-way) finishes it now.
            let remote = remote_name(&url);
            let done = core.db.meta_get(META_INITIALIZED).ok().flatten().unwrap_or_default();
            if done != remote {
                let core = core.clone();
                tokio::spawn(async move { core.run_initial(false, &remote).await });
            }
        }
        _ => {
            core.sync.inner.lock().unwrap().locked = true;
        }
    }
    core.announce_status();
}
