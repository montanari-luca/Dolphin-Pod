//! Dolphin Pod desktop shell: thin Tauri commands over `dolphin-core`, plus one long-lived event
//! channel that streams core events to the UI.

use std::sync::Arc;

use dolphin_core::events::CoreEvent;
use dolphin_core::model::*;
use dolphin_core::secrets::KeyringStore;
use dolphin_core::settings::{SettingChange, SettingDef};
use dolphin_core::sync::manager::{self as sync, DeviceInfo, SyncSetupResult, SyncStatus};
use dolphin_core::update::{self, UpdateInfo};
use dolphin_core::{Core, CoreError, CoreRef, scheduler};
use serde::Serialize;
use serde_json::Value;
use tauri::ipc::{Channel, InvokeBody, Request};
use tauri::{Manager, State};

type Res<T> = Result<T, CoreError>;

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AppInfo {
    version: &'static str,
    platform: &'static str,
    data_dir: String,
    attachments_dir: String,
    manifest_url: &'static str,
}

#[tauri::command]
fn app_info(core: State<CoreRef>) -> AppInfo {
    AppInfo {
        version: update::CURRENT_VERSION,
        platform: std::env::consts::OS,
        data_dir: core.data_dir().to_string_lossy().into_owned(),
        attachments_dir: core.attachments_dir().to_string_lossy().into_owned(),
        manifest_url: update::UPDATE_MANIFEST_URL,
    }
}

/// Forward every core event to the UI over a single channel.
#[tauri::command]
fn subscribe(core: State<CoreRef>, channel: Channel<CoreEvent>) {
    let mut rx = core.subscribe();
    tauri::async_runtime::spawn(async move {
        loop {
            match rx.recv().await {
                Ok(ev) => {
                    if channel.send(ev).is_err() {
                        break;
                    }
                }
                Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => tracing::warn!("ui lagged {n} events"),
                Err(_) => break,
            }
        }
    });
}

// ------------------------------------------------------------------ agents & chats

#[tauri::command]
fn list_agents(core: State<CoreRef>) -> Res<Vec<Agent>> {
    core.list_agents()
}

#[tauri::command]
fn update_agent(core: State<CoreRef>, agent: Agent) -> Res<Agent> {
    core.update_agent(agent)
}

#[tauri::command]
fn list_chats(core: State<CoreRef>) -> Res<Vec<Chat>> {
    core.list_chats()
}

#[tauri::command]
fn create_chat(core: State<CoreRef>, name: String, icon: String, agent_ids: Vec<String>) -> Res<Chat> {
    core.create_chat(&name, &icon, agent_ids)
}

#[tauri::command]
fn update_chat(core: State<CoreRef>, chat: Chat) -> Res<Chat> {
    core.update_chat(chat)
}

#[tauri::command]
fn delete_chat(core: State<CoreRef>, chat_id: String) -> Res<()> {
    core.delete_chat(&chat_id)
}

#[tauri::command]
fn clean_chat(core: State<CoreRef>, chat_id: String) -> Res<usize> {
    core.clean_chat(&chat_id)
}

#[tauri::command]
fn list_messages(core: State<CoreRef>, chat_id: String, limit: Option<usize>) -> Res<Vec<Message>> {
    core.list_messages(&chat_id, limit.unwrap_or(300))
}

#[tauri::command]
async fn send_message(core: State<'_, CoreRef>, chat_id: String, text: String, attachment_ids: Vec<String>) -> Res<Message> {
    core.send_message(&chat_id, &text, attachment_ids).await
}

#[tauri::command]
fn stop_chat(core: State<CoreRef>, chat_id: String) {
    core.stop(&chat_id)
}

#[tauri::command]
async fn approve_plan(core: State<'_, CoreRef>, message_id: String) -> Res<()> {
    core.approve_plan(&message_id).await
}

#[tauri::command]
fn grant_permission(core: State<CoreRef>, chat_id: String, agent_id: String, level: PermissionLevel) -> Res<PermissionGrant> {
    core.grant_permission(&chat_id, &agent_id, level)
}

#[tauri::command]
fn grants(core: State<CoreRef>, chat_id: String) -> Res<Vec<PermissionGrant>> {
    core.grants(&chat_id)
}

#[tauri::command]
fn action_log(core: State<CoreRef>, chat_id: String) -> Res<Vec<ActionLogEntry>> {
    core.action_log(&chat_id)
}

#[tauri::command]
fn usage_all(core: State<CoreRef>) -> Res<Vec<UsageStats>> {
    core.usage_all()
}

#[tauri::command]
fn resolve_confirmation(core: State<CoreRef>, id: String, accepted: bool) {
    core.resolve_confirmation(&id, accepted)
}

// ------------------------------------------------------------------ the user's roles

#[tauri::command]
fn list_roles(core: State<CoreRef>) -> Res<Vec<UserRole>> {
    core.list_roles()
}

#[tauri::command]
fn save_role(core: State<CoreRef>, role: UserRole) -> Res<UserRole> {
    core.save_role(role)
}

#[tauri::command]
fn delete_role(core: State<CoreRef>, role_id: String) -> Res<()> {
    core.delete_role(&role_id)
}

#[tauri::command]
fn reorder_roles(core: State<CoreRef>, role_ids: Vec<String>) -> Res<Vec<UserRole>> {
    core.reorder_roles(role_ids)
}

// ------------------------------------------------------------------ attachments

/// Raw-bytes upload (paste / drop from the webview). Name and type travel in headers.
#[tauri::command]
fn stage_attachment(core: State<CoreRef>, request: Request<'_>) -> Res<Attachment> {
    let InvokeBody::Raw(bytes) = request.body() else {
        return Err(CoreError::invalid("expected raw bytes"));
    };
    let header = |k: &str| request.headers().get(k).and_then(|v| v.to_str().ok()).map(percent_decode).unwrap_or_default();
    let name = header("x-name");
    core.stage_attachment(if name.is_empty() { "file" } else { &name }, &header("x-mime"), bytes)
}

#[tauri::command]
fn stage_attachment_path(core: State<CoreRef>, path: String) -> Res<Attachment> {
    core.stage_attachment_from_path(&path)
}

fn percent_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%'
            && i + 2 < bytes.len()
            && let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16)
        {
            out.push(b);
            i += 3;
            continue;
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

// ------------------------------------------------------------------ providers & models

#[tauri::command]
fn provider_kinds(core: State<CoreRef>) -> Vec<ProviderKindInfo> {
    core.provider_kinds()
}

#[tauri::command]
fn list_providers(core: State<CoreRef>) -> Res<Vec<ProviderConfig>> {
    core.list_providers()
}

#[tauri::command]
fn save_provider(core: State<CoreRef>, config: ProviderConfig, api_key: Option<String>) -> Res<ProviderConfig> {
    core.save_provider(config, api_key)
}

#[tauri::command]
fn delete_provider(core: State<CoreRef>, provider_id: String) -> Res<()> {
    core.delete_provider(&provider_id)
}

#[tauri::command]
async fn list_models(core: State<'_, CoreRef>, provider_id: String) -> Res<Vec<ModelInfo>> {
    core.list_models(&provider_id).await
}

#[tauri::command]
fn inspect_model(core: State<CoreRef>, model_id: String) -> ModelInfo {
    core.inspect_model(&model_id)
}

// ------------------------------------------------------------------ settings, automations, updates

#[tauri::command]
fn settings_registry(core: State<CoreRef>) -> Vec<SettingDef> {
    core.settings_registry()
}

#[tauri::command]
fn global_settings(core: State<CoreRef>) -> Res<serde_json::Map<String, Value>> {
    core.global_settings()
}

#[tauri::command]
fn set_setting(core: State<CoreRef>, key: String, target: Option<String>, value: Value) -> Res<SettingChange> {
    core.set_setting(&key, target.as_deref(), value)
}

#[tauri::command]
fn list_automations(core: State<CoreRef>) -> Res<Vec<Automation>> {
    core.list_automations()
}

#[tauri::command]
fn save_automation(core: State<CoreRef>, automation: Automation) -> Res<Automation> {
    core.save_automation(automation)
}

#[tauri::command]
fn delete_automation(core: State<CoreRef>, automation_id: String) -> Res<()> {
    core.delete_automation(&automation_id)
}

#[tauri::command]
async fn check_update() -> Res<UpdateInfo> {
    update::check().await
}

fn spawn_update_checks(core: CoreRef) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        loop {
            if dolphin_core::settings::get_bool(&core.db, dolphin_core::settings::keys::AUTO_UPDATE_CHECK)
                && let Ok(info) = update::check().await
                && info.available
            {
                // Toast-style prompt in the UI; the link opens the platform download.
                let url = info.url.unwrap_or_default();
                notify_update(&core, info.latest, url);
            }
            tokio::time::sleep(std::time::Duration::from_secs(6 * 3600)).await;
        }
    });
}

// ------------------------------------------------------------------ sync

#[tauri::command]
fn sync_status(core: State<CoreRef>) -> SyncStatus {
    core.sync_status()
}

#[tauri::command]
async fn sync_setup(core: State<'_, CoreRef>, url: String, passphrase: String) -> Res<SyncSetupResult> {
    core.sync_setup(&url, &passphrase).await
}

#[tauri::command]
async fn sync_recover(core: State<'_, CoreRef>, url: String, recovery_key: String, new_passphrase: String) -> Res<()> {
    core.sync_recover(&url, &recovery_key, &new_passphrase).await
}

#[tauri::command]
async fn sync_unlock(core: State<'_, CoreRef>, passphrase: String) -> Res<()> {
    core.sync_unlock(&passphrase).await
}

#[tauri::command]
fn sync_disable(core: State<CoreRef>) -> Res<()> {
    core.sync_disable()
}

#[tauri::command]
async fn sync_now(core: State<'_, CoreRef>) -> Res<()> {
    core.sync_now().await
}

#[tauri::command]
async fn sync_change_passphrase(core: State<'_, CoreRef>, old: String, new: String) -> Res<()> {
    core.sync_change_passphrase(&old, &new).await
}

#[tauri::command]
async fn sync_new_recovery_key(core: State<'_, CoreRef>, passphrase: String) -> Res<String> {
    core.sync_new_recovery_key(&passphrase).await
}

#[tauri::command]
async fn sync_devices(core: State<'_, CoreRef>) -> Res<Vec<DeviceInfo>> {
    core.sync_devices().await
}

#[tauri::command]
async fn sync_revoke_device(core: State<'_, CoreRef>, device_id: String) -> Res<()> {
    core.sync_revoke_device(&device_id).await
}

#[tauri::command]
fn set_device_name(core: State<CoreRef>, name: String) -> Res<()> {
    core.set_device_name(&name)
}

#[tauri::command]
async fn sync_fetch_attachment(core: State<'_, CoreRef>, attachment_id: String) -> Res<String> {
    core.sync_fetch_attachment(&attachment_id).await
}

fn notify_update(core: &Core, version: String, url: String) {
    core.announce(CoreEvent::UpdateAvailable { version, url });
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tracing_subscriber::fmt().with_env_filter(tracing_subscriber::EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into())).init();

    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, _args, _cwd| {
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .on_window_event(|window, event| {
            // Coming back to the window is a good moment to look for news from the other devices.
            if let tauri::WindowEvent::Focused(true) = event
                && let Some(core) = window.try_state::<CoreRef>()
            {
                core.sync_wake();
            }
        })
        .setup(|app| {
            let dir = app.path().app_data_dir()?;
            let core = Core::open(&dir, Arc::new(KeyringStore::new())).map_err(|e| e.to_string())?;
            let c = core.clone();
            core.set_device_info(None, std::env::consts::OS, "desktop");
            tauri::async_runtime::spawn(async move {
                scheduler::spawn(c.clone());
                sync::spawn(c);
            });
            spawn_update_checks(core.clone());
            app.manage(core);
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            app_info,
            subscribe,
            list_agents,
            update_agent,
            list_chats,
            create_chat,
            update_chat,
            delete_chat,
            clean_chat,
            list_messages,
            send_message,
            stop_chat,
            approve_plan,
            grant_permission,
            grants,
            action_log,
            usage_all,
            resolve_confirmation,
            list_roles,
            save_role,
            delete_role,
            reorder_roles,
            stage_attachment,
            stage_attachment_path,
            provider_kinds,
            list_providers,
            save_provider,
            delete_provider,
            list_models,
            inspect_model,
            settings_registry,
            global_settings,
            set_setting,
            list_automations,
            save_automation,
            delete_automation,
            check_update,
            sync_status,
            sync_setup,
            sync_recover,
            sync_unlock,
            sync_disable,
            sync_now,
            sync_change_passphrase,
            sync_new_recovery_key,
            sync_devices,
            sync_revoke_device,
            set_device_name,
            sync_fetch_attachment,
        ])
        .run(tauri::generate_context!())
        .expect("error while running Dolphin Pod");
}
