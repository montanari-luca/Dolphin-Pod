//! `Core`: the service object the apps talk to. Owns the database, secrets, provider registry,
//! the event bus and in-flight runs. The chat pipeline lives in `engine.rs`.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::sync::{broadcast, oneshot};
use tokio_util::sync::CancellationToken;

use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::events::{AgentActivity, CoreEvent, ToastLevel};
use crate::model::*;
use crate::providers::{Provider, ProviderRegistry};
use crate::secrets::{SecretStore, provider_key_name};
use crate::settings::{self, SettingChange};
use crate::util::{new_id, now_ms};
use crate::{catalog, seed};

pub struct Core {
    pub db: Db,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) providers: ProviderRegistry,
    events: broadcast::Sender<CoreEvent>,
    data_dir: PathBuf,
    /// Cancellation per chat: stopping a chat cancels every run in it.
    pub(crate) chat_tokens: Mutex<HashMap<String, CancellationToken>>,
    pending_confirms: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    pub(crate) sync: crate::sync::manager::SyncRuntime,
}

pub type CoreRef = Arc<Core>;

impl Core {
    pub fn open(data_dir: &Path, secrets: Arc<dyn SecretStore>) -> Result<CoreRef> {
        let db = Db::open(&data_dir.join("dolphin-pod.db"))?;
        Self::with_db(db, data_dir, secrets)
    }

    pub fn with_db(db: Db, data_dir: &Path, secrets: Arc<dyn SecretStore>) -> Result<CoreRef> {
        std::fs::create_dir_all(data_dir.join("attachments"))?;
        seed::seed_if_needed(&db)?;
        seed::seed_roles_if_needed(&db)?;
        seed::canonicalize_seed_ids(&db)?;
        db.recover_interrupted_messages()?;
        let (events, _) = broadcast::channel(4096);
        Ok(Arc::new(Self {
            db,
            secrets,
            providers: ProviderRegistry::new(),
            events,
            data_dir: data_dir.to_path_buf(),
            chat_tokens: Mutex::new(HashMap::new()),
            pending_confirms: Mutex::new(HashMap::new()),
            sync: Default::default(),
        }))
    }

    #[doc(hidden)]
    pub fn secrets_for_tests(&self) -> Arc<dyn SecretStore> {
        self.secrets.clone()
    }

    pub fn data_dir(&self) -> &Path {
        &self.data_dir
    }

    pub fn attachments_dir(&self) -> PathBuf {
        self.data_dir.join("attachments")
    }

    // ---------------------------------------------------------------- events

    pub fn subscribe(&self) -> broadcast::Receiver<CoreEvent> {
        self.events.subscribe()
    }

    pub(crate) fn emit(&self, ev: CoreEvent) {
        let _ = self.events.send(ev);
    }

    /// Publish an event from outside the core (e.g. the shell's update checker).
    pub fn announce(&self, ev: CoreEvent) {
        self.emit(ev);
    }

    pub(crate) fn activity(&self, chat_id: &str, agent_id: &str, activity: AgentActivity, detail: Option<String>) {
        self.emit(CoreEvent::AgentActivity { chat_id: chat_id.into(), agent_id: agent_id.into(), activity, detail });
    }

    pub fn toast(&self, level: ToastLevel, text: impl Into<String>) {
        self.emit(CoreEvent::Toast { level, text: text.into() });
    }

    /// Ask the user to confirm a sensitive action. Resolves to false on timeout or dismissal.
    pub(crate) async fn confirm(&self, title: &str, body: &str) -> bool {
        let id = new_id();
        let (tx, rx) = oneshot::channel();
        self.pending_confirms.lock().unwrap().insert(id.clone(), tx);
        self.emit(CoreEvent::ConfirmRequest { id: id.clone(), title: title.into(), body: body.into() });
        let res = tokio::time::timeout(Duration::from_secs(180), rx).await;
        self.pending_confirms.lock().unwrap().remove(&id);
        matches!(res, Ok(Ok(true)))
    }

    pub fn resolve_confirmation(&self, id: &str, accepted: bool) {
        if let Some(tx) = self.pending_confirms.lock().unwrap().remove(id) {
            let _ = tx.send(accepted);
        }
    }

    // ---------------------------------------------------------------- agents

    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        self.db.list_agents()
    }

    /// Update an agent from the GUI. Name/color/model changes go through the settings registry rules.
    pub fn update_agent(&self, a: Agent) -> Result<Agent> {
        let current = self.db.get_agent(&a.id)?;
        if a.name != current.name {
            settings::set(&self.db, settings::keys::AGENT_NAME, Some(&a.id), json!(a.name))?;
        }
        let mut merged = self.db.get_agent(&a.id)?;
        merged.color = a.color;
        merged.model = a.model;
        merged.image_model = a.image_model;
        merged.ios_model = a.ios_model;
        merged.job = if a.job.trim().is_empty() { current.job } else { a.job };
        let saved = self.db.upsert_agent(&merged)?;
        self.emit(CoreEvent::AgentChanged { agent: saved.clone() });
        Ok(saved)
    }

    pub(crate) fn agent_by_slug(&self, slug: &str) -> Result<Agent> {
        self.db.agent_by_slug(slug)
    }

    // ---------------------------------------------------------------- chats & messages

    pub fn list_chats(&self) -> Result<Vec<Chat>> {
        self.db.list_chats()
    }

    pub fn create_chat(&self, name: &str, icon: &str, agent_ids: Vec<String>) -> Result<Chat> {
        let name = name.trim();
        if name.is_empty() {
            return Err(CoreError::invalid("give the chat a name"));
        }
        let mut ids = Vec::new();
        for a in self.db.list_agents()? {
            if a.role != AgentRole::Specialist || agent_ids.contains(&a.id) {
                ids.push(a.id);
            }
        }
        let sort = self.db.list_chats()?.iter().map(|c| c.sort).max().unwrap_or(0) + 1;
        let chat = self.db.upsert_chat(&Chat {
            id: new_id(),
            name: name.into(),
            icon: if icon.is_empty() { "hash".into() } else { icon.into() },
            sort,
            agent_ids: ids,
            cleanup_enabled: true,
            cleanup_ttl_hours: settings::get_int(&self.db, settings::keys::CLEANUP_DEFAULT_TTL) as u32,
            workspace_path: None,
            created_at: 0,
            updated_at: 0,
        })?;
        self.emit(CoreEvent::ChatChanged { chat: chat.clone() });
        Ok(chat)
    }

    pub fn update_chat(&self, c: Chat) -> Result<Chat> {
        if c.name.trim().is_empty() {
            return Err(CoreError::invalid("chat name cannot be empty"));
        }
        if let Some(p) = &c.workspace_path
            && !Path::new(p).is_dir()
        {
            return Err(CoreError::invalid(format!("'{p}' is not an existing folder")));
        }
        let mut c = c;
        // Moderator and orchestrator are always members.
        for a in self.db.list_agents()? {
            if a.role != AgentRole::Specialist && !c.agent_ids.contains(&a.id) {
                c.agent_ids.insert(0, a.id);
            }
        }
        let chat = self.db.upsert_chat(&c)?;
        self.emit(CoreEvent::ChatChanged { chat: chat.clone() });
        Ok(chat)
    }

    pub fn delete_chat(&self, id: &str) -> Result<()> {
        self.stop(id);
        self.db.delete_chat(id)?;
        self.emit(CoreEvent::ChatDeleted { chat_id: id.into() });
        Ok(())
    }

    pub fn list_messages(&self, chat_id: &str, limit: usize) -> Result<Vec<Message>> {
        self.db.list_messages(chat_id, limit)
    }

    /// Manual clean ("Roger, clean this chat" or the chat menu).
    pub fn clean_chat(&self, chat_id: &str) -> Result<usize> {
        let ids = self.db.tombstone_all_messages(chat_id)?;
        let n = ids.len();
        self.emit(CoreEvent::MessagesRemoved { chat_id: chat_id.into(), message_ids: ids, all: true });
        Ok(n)
    }

    pub(crate) fn new_message(&self, chat_id: &str, agent_id: Option<&str>, kind: MessageKind, status: MessageStatus, content: &str) -> Message {
        let now = now_ms();
        Message {
            id: new_id(),
            chat_id: chat_id.into(),
            agent_id: agent_id.map(String::from),
            kind,
            status,
            content: content.into(),
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
        }
    }

    pub(crate) fn insert_message(&self, m: &Message) -> Result<()> {
        self.db.insert_message(m)?;
        self.emit(CoreEvent::MessageCreated { message: m.clone() });
        Ok(())
    }

    pub(crate) fn save_message(&self, m: &Message) -> Result<()> {
        self.db.update_message(m)?;
        self.emit(CoreEvent::MessageUpdated { message: m.clone() });
        Ok(())
    }

    /// Post a finished message from an agent (Roger's replies, Lincoln's notices).
    pub(crate) fn post(&self, chat_id: &str, agent_id: &str, kind: MessageKind, content: &str, meta: Value) -> Result<Message> {
        let mut m = self.new_message(chat_id, Some(agent_id), kind, MessageStatus::Done, content);
        m.meta = meta;
        self.insert_message(&m)?;
        Ok(m)
    }

    // ---------------------------------------------------------------- providers

    fn with_key_flag(&self, mut p: ProviderConfig) -> ProviderConfig {
        p.has_key = self.secrets.get(&provider_key_name(&p.id)).ok().flatten().is_some_and(|k| !k.is_empty());
        p
    }

    pub fn list_providers(&self) -> Result<Vec<ProviderConfig>> {
        Ok(self.db.list_providers()?.into_iter().map(|p| self.with_key_flag(p)).collect())
    }

    pub fn provider_kinds(&self) -> Vec<ProviderKindInfo> {
        ProviderKind::ALL.into_iter().map(ProviderKindInfo::from).collect()
    }

    /// Save a provider config. `api_key`: None = unchanged, Some("") = remove, Some(k) = store.
    pub fn save_provider(&self, cfg: ProviderConfig, api_key: Option<String>) -> Result<ProviderConfig> {
        let mut cfg = cfg;
        if cfg.id.is_empty() {
            cfg.id = new_id();
        }
        if cfg.name.trim().is_empty() {
            cfg.name = cfg.kind.label().into();
        }
        if cfg.base_url.trim().is_empty() {
            cfg.base_url = cfg.kind.default_base_url().into();
        }
        if !cfg.base_url.starts_with("http://") && !cfg.base_url.starts_with("https://") {
            return Err(CoreError::invalid("the base URL must start with http:// or https://"));
        }
        match api_key.as_deref() {
            Some("") => self.secrets.delete(&provider_key_name(&cfg.id))?,
            Some(k) => self.secrets.set(&provider_key_name(&cfg.id), k.trim())?,
            None => {}
        }
        let saved = self.db.upsert_provider(&cfg)?;
        self.providers.invalidate(&saved.id);
        Ok(self.with_key_flag(saved))
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        self.db.delete_provider(id)?;
        self.secrets.delete(&provider_key_name(id))?;
        self.providers.invalidate(id);
        Ok(())
    }

    pub(crate) fn provider(&self, provider_id: &str) -> Result<(ProviderConfig, Arc<dyn Provider>)> {
        let cfg = self.db.get_provider(provider_id)?;
        if !cfg.enabled {
            return Err(CoreError::invalid(format!("the provider '{}' is turned off", cfg.name)));
        }
        let key = self.secrets.get(&provider_key_name(&cfg.id))?;
        if cfg.kind.needs_key() && key.as_deref().is_none_or(str::is_empty) {
            return Err(CoreError::invalid(format!("add an API key for '{}' in Settings → Providers", cfg.name)));
        }
        let p = self.providers.get(&cfg, key);
        Ok((cfg, p))
    }

    #[doc(hidden)]
    pub fn override_provider(&self, provider_id: &str, provider: Arc<dyn Provider>) {
        self.providers.set_override(provider_id, provider);
    }

    pub(crate) fn provider_key(&self, provider_id: &str) -> Result<Option<String>> {
        self.secrets.get(&provider_key_name(provider_id))
    }

    pub async fn list_models(&self, provider_id: &str) -> Result<Vec<ModelInfo>> {
        let (_, p) = self.provider(provider_id)?;
        p.list_models().await
    }

    /// Check a model id that is not in the provider's list (e.g. an Azure deployment name).
    pub fn inspect_model(&self, model_id: &str) -> ModelInfo {
        catalog::info_from_catalog(model_id, None)
    }

    // ---------------------------------------------------------------- settings

    pub fn settings_registry(&self) -> Vec<settings::SettingDef> {
        settings::registry()
    }

    pub fn global_settings(&self) -> Result<serde_json::Map<String, Value>> {
        settings::all_global(&self.db)
    }

    pub fn set_setting(&self, key: &str, target: Option<&str>, value: Value) -> Result<SettingChange> {
        let change = settings::set(&self.db, key, target, value)?;
        self.emit_change(&change);
        Ok(change)
    }

    pub(crate) fn emit_change(&self, change: &SettingChange) {
        match change {
            SettingChange::Chat { chat } => self.emit(CoreEvent::ChatChanged { chat: chat.clone() }),
            SettingChange::Agent { agent } => self.emit(CoreEvent::AgentChanged { agent: agent.clone() }),
            SettingChange::Global { .. } => {}
        }
        self.emit(CoreEvent::SettingChanged { change: change.clone() });
    }

    // ---------------------------------------------------------------- permissions & usage

    pub fn grants(&self, chat_id: &str) -> Result<Vec<PermissionGrant>> {
        self.db.grants_for_chat(chat_id)
    }

    pub fn action_log(&self, chat_id: &str) -> Result<Vec<ActionLogEntry>> {
        self.db.action_log(chat_id, 200)
    }

    pub fn usage_all(&self) -> Result<Vec<UsageStats>> {
        self.db.usage_all()
    }

    // ---------------------------------------------------------------- the user's roles (cosmetic)

    pub fn list_roles(&self) -> Result<Vec<UserRole>> {
        self.db.list_roles()
    }

    pub fn save_role(&self, role: UserRole) -> Result<UserRole> {
        let mut role = role;
        role.name = role.name.trim().to_string();
        if role.name.is_empty() || role.name.chars().count() > 24 {
            return Err(CoreError::invalid("role names need 1-24 characters"));
        }
        if role.icon.trim().is_empty() || role.icon.len() > 32 {
            return Err(CoreError::invalid("pick an icon for the role"));
        }
        if !settings::valid_color(&role.color) {
            return Err(CoreError::invalid("the role color must be a hex color or a gradient of 2-8 stops"));
        }
        let existing = self.db.list_roles()?;
        if existing.iter().any(|r| r.id != role.id && r.name.eq_ignore_ascii_case(&role.name)) {
            return Err(CoreError::invalid(format!("you already have a role called {}", role.name)));
        }
        if role.id.is_empty() {
            role.id = new_id();
            role.sort = existing.iter().map(|r| r.sort).max().map_or(0, |m| m + 1);
        }
        let saved = self.db.upsert_role(&role)?;
        self.emit_roles()?;
        Ok(saved)
    }

    pub fn delete_role(&self, id: &str) -> Result<()> {
        self.db.delete_role(id)?;
        self.emit_roles()
    }

    /// Reorder roles; the first one colors the user's name.
    pub fn reorder_roles(&self, ids: Vec<String>) -> Result<Vec<UserRole>> {
        let roles = self.db.list_roles()?;
        for (i, id) in ids.iter().enumerate() {
            if let Some(r) = roles.iter().find(|r| &r.id == id) {
                let mut r = r.clone();
                r.sort = i as i32;
                self.db.upsert_role(&r)?;
            }
        }
        self.emit_roles()?;
        self.db.list_roles()
    }

    fn emit_roles(&self) -> Result<()> {
        self.emit(CoreEvent::RolesChanged { roles: self.db.list_roles()? });
        Ok(())
    }

    // ---------------------------------------------------------------- automations

    pub fn list_automations(&self) -> Result<Vec<Automation>> {
        self.db.list_automations()
    }

    pub fn save_automation(&self, a: Automation) -> Result<Automation> {
        let mut a = a;
        if a.id.is_empty() {
            a.id = new_id();
        }
        if a.text.trim().is_empty() {
            return Err(CoreError::invalid("the automation needs a message"));
        }
        crate::scheduler::parse_time(&a.time_of_day).ok_or_else(|| CoreError::invalid("use a time like 08:30"))?;
        if a.weekdays.iter().any(|d| *d > 6) {
            return Err(CoreError::invalid("weekdays go from 0 (Monday) to 6 (Sunday)"));
        }
        self.db.get_chat(&a.chat_id)?;
        self.db.upsert_automation(&a)
    }

    pub fn delete_automation(&self, id: &str) -> Result<()> {
        self.db.delete_automation(id)
    }

    // ---------------------------------------------------------------- cancellation

    pub(crate) fn chat_token(&self, chat_id: &str) -> CancellationToken {
        let mut map = self.chat_tokens.lock().unwrap();
        let t = map.entry(chat_id.to_string()).or_default();
        if t.is_cancelled() {
            *t = CancellationToken::new();
        }
        t.clone()
    }

    /// Stop every running agent in a chat.
    pub fn stop(&self, chat_id: &str) {
        if let Some(t) = self.chat_tokens.lock().unwrap().remove(chat_id) {
            t.cancel();
        }
    }
}
