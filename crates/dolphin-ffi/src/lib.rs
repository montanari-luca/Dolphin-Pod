//! UniFFI surface of `dolphin-core` for the iOS app.
//!
//! The core owns all logic; this crate only forwards. Structured values cross the boundary as JSON strings
//! (the Swift side decodes them with `Codable` models), which keeps the interface small and lets the core's
//! types evolve without regenerating a wall of foreign records. Streaming events arrive through a callback
//! interface; every async call runs on UniFFI's tokio runtime, which is also what the core's background tasks
//! (sync loop, agent runs, remote control) live on.

use std::sync::Arc;

use async_trait::async_trait;
use dolphin_core::events::CoreEvent;
use dolphin_core::providers::ondevice::{OnDeviceModel, OnDeviceRequest, OnDeviceRuntime, OnDeviceSink};
use dolphin_core::model::{Agent, Automation, Chat, Platform, ProviderConfig, UserRole};
use dolphin_core::remote::LanEndpoint;
use dolphin_core::secrets::KeyringStore;
use dolphin_core::{Core, CoreError, CoreRef};
use serde::Serialize;
use serde_json::Value;

uniffi::setup_scaffolding!();

#[derive(Debug, thiserror::Error, uniffi::Error)]
pub enum FfiError {
    #[error("{message}")]
    Failed { message: String },
}

impl From<CoreError> for FfiError {
    fn from(e: CoreError) -> Self {
        FfiError::Failed { message: e.to_string() }
    }
}

impl From<serde_json::Error> for FfiError {
    fn from(e: serde_json::Error) -> Self {
        FfiError::Failed { message: format!("bad data: {e}") }
    }
}

type R<T> = Result<T, FfiError>;

fn json<T: Serialize>(v: &T) -> R<String> {
    Ok(serde_json::to_string(v)?)
}

/// Core events, delivered as JSON (the same shape the desktop UI receives).
#[uniffi::export(with_foreign)]
pub trait EventListener: Send + Sync {
    fn on_event(&self, event_json: String);
}

// ------------------------------------------------------------------ on-device inference

/// Where the app's inference engine (MLX) writes what it generates.
#[derive(uniffi::Object)]
pub struct GenerationSink {
    inner: Arc<dyn OnDeviceSink>,
}

#[uniffi::export]
impl GenerationSink {
    pub fn text(&self, delta: String) {
        self.inner.text(&delta);
    }

    pub fn reasoning(&self, delta: String) {
        self.inner.reasoning(&delta);
    }

    pub fn finish(&self, input_tokens: i64, output_tokens: i64) {
        self.inner.finish(input_tokens, output_tokens);
    }

    pub fn fail(&self, message: String) {
        self.inner.fail(&message);
    }

    /// True once the user pressed stop; the engine should stop generating.
    pub fn is_cancelled(&self) -> bool {
        self.inner.is_cancelled()
    }
}

/// Implemented in Swift by the app's inference engine.
#[uniffi::export(with_foreign)]
#[async_trait]
pub trait OnDeviceBackend: Send + Sync {
    /// The curated models as JSON: `[{id, name, sizeMb, contextWindow, downloaded}]`.
    fn models_json(&self) -> String;
    /// Run one request (JSON of `OnDeviceRequest`) and stream the reply into `sink`.
    async fn generate(&self, request_json: String, sink: Arc<GenerationSink>);
}

struct BackendRuntime(Arc<dyn OnDeviceBackend>);

#[async_trait]
impl OnDeviceRuntime for BackendRuntime {
    fn models(&self) -> Vec<OnDeviceModel> {
        serde_json::from_str(&self.0.models_json()).unwrap_or_default()
    }

    async fn generate(&self, request: OnDeviceRequest, sink: Arc<dyn OnDeviceSink>) {
        let Ok(json) = serde_json::to_string(&request) else { return };
        self.0.generate(json, Arc::new(GenerationSink { inner: sink })).await;
    }
}

#[derive(uniffi::Object)]
pub struct DolphinCore {
    core: CoreRef,
}

#[uniffi::export(async_runtime = "tokio")]
impl DolphinCore {
    /// Open (or create) the database in `data_dir` and identify this device as an iPhone.
    #[uniffi::constructor]
    pub fn open(data_dir: String, device_name: String) -> R<Arc<Self>> {
        let core = Core::open(std::path::Path::new(&data_dir), Arc::new(KeyringStore::new()))?;
        core.set_platform(Platform::Ios);
        core.set_device_info(Some(device_name).filter(|n| !n.trim().is_empty()), "ios", "phone");
        Ok(Arc::new(Self { core }))
    }

    /// Offer the models this phone can run itself. Agents can then pick them as their iPhone model.
    pub fn set_on_device_backend(&self, backend: Arc<dyn OnDeviceBackend>) {
        self.core.set_on_device_runtime(Arc::new(BackendRuntime(backend)));
    }

    /// Start the background work: housekeeping, sync and the remote-control loop. Call once, after `open`.
    pub async fn start(&self) {
        dolphin_core::scheduler::spawn(self.core.clone());
        dolphin_core::sync::manager::spawn(self.core.clone());
        dolphin_core::remote::spawn(self.core.clone());
    }

    /// Stream core events to `listener` until the app exits.
    pub async fn subscribe(&self, listener: Arc<dyn EventListener>) {
        let mut rx = self.core.subscribe();
        tokio::spawn(async move {
            loop {
                match rx.recv().await {
                    Ok(ev) => {
                        if let Ok(s) = serde_json::to_string(&ev) {
                            listener.on_event(s);
                        }
                    }
                    Err(tokio::sync::broadcast::error::RecvError::Lagged(_)) => continue,
                    Err(_) => break,
                }
            }
        });
    }

    pub fn version(&self) -> String {
        dolphin_core::update::CURRENT_VERSION.to_string()
    }

    pub fn data_dir(&self) -> String {
        self.core.data_dir().to_string_lossy().into_owned()
    }

    // ------------------------------------------------------------------ reading

    pub fn list_chats(&self) -> R<String> {
        json(&self.core.list_chats()?)
    }

    pub fn list_agents(&self) -> R<String> {
        json(&self.core.list_agents()?)
    }

    pub fn list_roles(&self) -> R<String> {
        json(&self.core.list_roles()?)
    }

    pub fn list_messages(&self, chat_id: String, limit: u32) -> R<String> {
        json(&self.core.list_messages(&chat_id, limit as usize)?)
    }

    pub fn usage_all(&self) -> R<String> {
        json(&self.core.usage_all()?)
    }

    pub fn global_settings(&self) -> R<String> {
        json(&self.core.global_settings()?)
    }

    pub fn list_providers(&self) -> R<String> {
        json(&self.core.list_providers()?)
    }

    pub fn provider_kinds(&self) -> R<String> {
        json(&self.core.provider_kinds())
    }

    /// Which model each agent would use on this phone, and whether its computer model is unreachable here.
    pub fn agent_models(&self) -> R<String> {
        let rows: Vec<Value> = self
            .core
            .list_agents()?
            .into_iter()
            .map(|a| {
                let effective = self.core.model_for(&a);
                let desktop_unavailable = a.model.as_ref().is_some_and(|m| !self.core.model_usable_here(m));
                serde_json::json!({ "agentId": a.id, "effective": effective, "desktopModelUnavailable": desktop_unavailable })
            })
            .collect();
        json(&rows)
    }

    // ------------------------------------------------------------------ chats and messages

    pub fn create_chat(&self, name: String, icon: String, agent_ids: Vec<String>) -> R<String> {
        json(&self.core.create_chat(&name, &icon, agent_ids)?)
    }

    pub fn update_chat(&self, chat_json: String) -> R<String> {
        let chat: Chat = serde_json::from_str(&chat_json)?;
        json(&self.core.update_chat(chat)?)
    }

    pub fn delete_chat(&self, chat_id: String) -> R<()> {
        Ok(self.core.delete_chat(&chat_id)?)
    }

    pub fn clean_chat(&self, chat_id: String) -> R<u32> {
        Ok(self.core.clean_chat(&chat_id)? as u32)
    }

    /// Run a message with this phone's own agents (their iPhone models).
    pub async fn send_message(&self, chat_id: String, text: String, attachment_ids: Vec<String>) -> R<String> {
        json(&self.core.send_message(&chat_id, &text, attachment_ids).await?)
    }

    pub fn stop_chat(&self, chat_id: String) {
        self.core.stop(&chat_id);
    }

    pub async fn approve_plan(&self, message_id: String) -> R<()> {
        Ok(self.core.approve_plan(&message_id).await?)
    }

    pub fn stage_attachment(&self, name: String, mime: String, bytes: Vec<u8>) -> R<String> {
        json(&self.core.stage_attachment(&name, &mime, &bytes)?)
    }

    pub fn resolve_confirmation(&self, id: String, accepted: bool) {
        self.core.resolve_confirmation(&id, accepted);
    }

    pub fn answer_question(&self, id: String, answer: String, custom: bool) {
        self.core.answer_question(&id, &answer, custom);
    }

    pub fn skip_question(&self, id: String) {
        self.core.skip_question(&id);
    }

    // ------------------------------------------------------------------ agents, roles, providers, settings

    pub fn update_agent(&self, agent_json: String) -> R<String> {
        let agent: Agent = serde_json::from_str(&agent_json)?;
        json(&self.core.update_agent(agent)?)
    }

    pub fn save_role(&self, role_json: String) -> R<String> {
        let role: UserRole = serde_json::from_str(&role_json)?;
        json(&self.core.save_role(role)?)
    }

    pub fn save_provider(&self, provider_json: String, api_key: Option<String>) -> R<String> {
        let cfg: ProviderConfig = serde_json::from_str(&provider_json)?;
        json(&self.core.save_provider(cfg, api_key)?)
    }

    pub fn delete_provider(&self, provider_id: String) -> R<()> {
        Ok(self.core.delete_provider(&provider_id)?)
    }

    pub async fn list_models(&self, provider_id: String) -> R<String> {
        json(&self.core.list_models(&provider_id).await?)
    }

    pub fn set_setting(&self, key: String, target: Option<String>, value_json: String) -> R<String> {
        let value: Value = serde_json::from_str(&value_json)?;
        json(&self.core.set_setting(&key, target.as_deref(), value)?)
    }

    pub fn list_automations(&self) -> R<String> {
        json(&self.core.list_automations()?)
    }

    pub fn save_automation(&self, automation_json: String) -> R<String> {
        let a: Automation = serde_json::from_str(&automation_json)?;
        json(&self.core.save_automation(a)?)
    }

    pub async fn check_update(&self) -> R<String> {
        json(&dolphin_core::update::check().await?)
    }

    // ------------------------------------------------------------------ sync

    pub fn sync_status(&self) -> R<String> {
        json(&self.core.sync_status())
    }

    pub async fn sync_setup(&self, url: String, passphrase: String) -> R<String> {
        json(&self.core.sync_setup(&url, &passphrase).await?)
    }

    pub async fn sync_recover(&self, url: String, recovery_key: String, new_passphrase: String) -> R<()> {
        Ok(self.core.sync_recover(&url, &recovery_key, &new_passphrase).await?)
    }

    pub async fn sync_unlock(&self, passphrase: String) -> R<()> {
        Ok(self.core.sync_unlock(&passphrase).await?)
    }

    pub fn sync_disable(&self) -> R<()> {
        Ok(self.core.sync_disable()?)
    }

    pub async fn sync_now(&self) -> R<()> {
        Ok(self.core.sync_now().await?)
    }

    /// Ask the sync loop to run soon (the app came to the foreground, a background refresh fired).
    pub fn sync_wake(&self) {
        self.core.sync_wake();
    }

    pub async fn sync_change_passphrase(&self, old: String, new: String) -> R<()> {
        Ok(self.core.sync_change_passphrase(&old, &new).await?)
    }

    pub async fn sync_new_recovery_key(&self, passphrase: String) -> R<String> {
        Ok(self.core.sync_new_recovery_key(&passphrase).await?)
    }

    pub async fn sync_devices(&self) -> R<String> {
        json(&self.core.sync_devices().await?)
    }

    pub async fn sync_revoke_device(&self, device_id: String) -> R<()> {
        Ok(self.core.sync_revoke_device(&device_id).await?)
    }

    pub fn set_device_name(&self, name: String) -> R<()> {
        Ok(self.core.set_device_name(&name)?)
    }

    /// Download an attachment that another device uploaded; returns the local file path.
    pub async fn sync_fetch_attachment(&self, attachment_id: String) -> R<String> {
        Ok(self.core.sync_fetch_attachment(&attachment_id).await?)
    }

    // ------------------------------------------------------------------ remote control

    /// Desktops that accept remote requests and are online, most recently active first.
    pub async fn remote_targets(&self) -> R<String> {
        json(&self.core.remote_targets().await?)
    }

    /// Send a message to run on a desktop. `lan_host`/`lan_port` come from Bonjour discovery when the desktop is
    /// on the same network; without them (or if they fail) the request goes through the cloud.
    pub async fn remote_send(
        &self,
        target_device_id: Option<String>,
        chat_id: String,
        text: String,
        lan_host: Option<String>,
        lan_port: Option<u16>,
    ) -> R<String> {
        let lan = match (lan_host, lan_port) {
            (Some(host), Some(port)) => Some(LanEndpoint { host, port }),
            _ => None,
        };
        json(&self.core.remote_send(target_device_id.as_deref(), &chat_id, &text, lan).await?)
    }
}

/// Convenience for tests and tooling: events as the JSON the apps see.
pub fn event_json(ev: &CoreEvent) -> String {
    serde_json::to_string(ev).unwrap_or_default()
}
