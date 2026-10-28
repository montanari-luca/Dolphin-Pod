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
use crate::events::{AgentActivity, CoreEvent, QuestionOption, ToastLevel};
use crate::model::*;
use crate::providers::{Provider, ProviderRegistry};
use crate::secrets::{SecretStore, provider_key_name};
use crate::settings::{self, SettingChange};
use crate::util::{new_id, now_ms};
use crate::{catalog, memory, seed};

pub struct Core {
    pub db: Db,
    pub(crate) secrets: Arc<dyn SecretStore>,
    pub(crate) providers: ProviderRegistry,
    events: broadcast::Sender<CoreEvent>,
    data_dir: PathBuf,
    /// Cancellation per chat: stopping a chat cancels every run in it.
    pub(crate) chat_tokens: Mutex<HashMap<String, CancellationToken>>,
    pending_confirms: Mutex<HashMap<String, oneshot::Sender<bool>>>,
    pending_questions: Mutex<HashMap<String, oneshot::Sender<QuestionAnswer>>>,
    web_override: Mutex<Option<WebFn>>,
    pub(crate) sync: crate::sync::manager::SyncRuntime,
    pub(crate) remote: crate::remote::RemoteRuntime,
    platform: Mutex<Platform>,
    on_device: Mutex<Option<Arc<dyn crate::providers::ondevice::OnDeviceRuntime>>>,
    /// The learner runs one at a time; a signal that arrives while it works makes it go round once more.
    pub(crate) learning_busy: std::sync::atomic::AtomicBool,
    pub(crate) learning_again: std::sync::atomic::AtomicBool,
}

pub type CoreRef = Arc<Core>;

type WebFn = Arc<dyn Fn(&str, &Value) -> Result<String> + Send + Sync>;

/// What the user did with a question an agent asked.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QuestionAnswer {
    Choice(String),
    Custom(String),
    Skipped,
}

impl QuestionAnswer {
    /// The tool result the asking agent reads.
    pub fn for_agent(&self) -> String {
        match self {
            Self::Choice(a) => format!("The user chose: \"{a}\""),
            Self::Custom(a) => format!("The user answered in their own words: \"{a}\""),
            Self::Skipped => "The user skipped this question; use your best judgment.".into(),
        }
    }
}

impl Core {
    pub fn open(data_dir: &Path, secrets: Arc<dyn SecretStore>) -> Result<CoreRef> {
        let db = Db::open(&data_dir.join("dolphin-pod.db"))?;
        Self::with_db(db, data_dir, secrets)
    }

    pub fn with_db(db: Db, data_dir: &Path, secrets: Arc<dyn SecretStore>) -> Result<CoreRef> {
        std::fs::create_dir_all(data_dir.join("attachments"))?;
        seed::seed_if_needed(&db)?;
        seed::seed_roles_if_needed(&db)?;
        seed::seed_new_agents_if_needed(&db)?;
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
            pending_questions: Mutex::new(HashMap::new()),
            web_override: Mutex::new(None),
            sync: Default::default(),
            remote: Default::default(),
            platform: Mutex::new(Platform::Desktop),
            on_device: Mutex::new(None),
            learning_busy: Default::default(),
            learning_again: Default::default(),
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

    /// Pause a run and ask the user a question. There is no timeout: it resolves when the user answers or
    /// skips, and fails with `Cancelled` when the chat is stopped. The panel is always closed afterwards.
    pub(crate) async fn ask(
        &self,
        chat_id: &str,
        agent_id: &str,
        question: &str,
        options: Vec<QuestionOption>,
        token: &CancellationToken,
    ) -> Result<QuestionAnswer> {
        let id = new_id();
        let (tx, rx) = oneshot::channel();
        self.pending_questions.lock().unwrap().insert(id.clone(), tx);
        self.emit(CoreEvent::QuestionRequest { id: id.clone(), chat_id: chat_id.into(), agent_id: agent_id.into(), question: question.into(), options });
        let res = tokio::select! {
            r = rx => Some(r.unwrap_or(QuestionAnswer::Skipped)),
            _ = token.cancelled() => None,
        };
        self.pending_questions.lock().unwrap().remove(&id);
        self.emit(CoreEvent::QuestionClosed { id });
        res.ok_or(CoreError::Cancelled)
    }

    /// The user's answer to a pending question: one of the offered labels, or their own words when `custom`.
    pub fn answer_question(&self, id: &str, answer: &str, custom: bool) {
        let answer = answer.trim();
        if answer.is_empty() {
            return self.skip_question(id);
        }
        let answer = if custom { QuestionAnswer::Custom(answer.into()) } else { QuestionAnswer::Choice(answer.into()) };
        if let Some(tx) = self.pending_questions.lock().unwrap().remove(id) {
            let _ = tx.send(answer);
        }
    }

    pub fn skip_question(&self, id: &str) {
        if let Some(tx) = self.pending_questions.lock().unwrap().remove(id) {
            let _ = tx.send(QuestionAnswer::Skipped);
        }
    }

    // ---------------------------------------------------------------- platform

    pub fn set_platform(&self, platform: Platform) {
        *self.platform.lock().unwrap() = platform;
    }

    /// Plug in the runtime that runs small models on this device (the iOS app does this at launch).
    pub fn set_on_device_runtime(&self, runtime: Arc<dyn crate::providers::ondevice::OnDeviceRuntime>) {
        *self.on_device.lock().unwrap() = Some(runtime);
    }

    fn on_device_runtime(&self) -> Option<Arc<dyn crate::providers::ondevice::OnDeviceRuntime>> {
        self.on_device.lock().unwrap().clone()
    }

    /// The virtual "On this iPhone" provider, present only while a runtime is registered.
    fn on_device_config(&self) -> Option<ProviderConfig> {
        self.on_device_runtime()?;
        Some(ProviderConfig {
            id: crate::providers::ondevice::ON_DEVICE_PROVIDER_ID.into(),
            kind: ProviderKind::OnDevice,
            name: ProviderKind::OnDevice.label().into(),
            base_url: String::new(),
            extra: Value::Null,
            enabled: true,
            has_key: false,
            updated_at: 0,
        })
    }

    /// A provider's config, including the virtual on-device one.
    fn provider_config(&self, id: &str) -> Result<ProviderConfig> {
        if id == crate::providers::ondevice::ON_DEVICE_PROVIDER_ID {
            return self.on_device_config().ok_or_else(|| CoreError::NotFound("on-device models are not available here".into()));
        }
        self.db.get_provider(id)
    }

    pub fn platform(&self) -> Platform {
        *self.platform.lock().unwrap()
    }

    /// Whether a model's provider can be reached from this device. A PC's Ollama or LM Studio (or any
    /// localhost endpoint) is unreachable from a phone; cloud providers work everywhere.
    pub fn model_usable_here(&self, m: &ModelRef) -> bool {
        let Ok(cfg) = self.provider_config(&m.provider_id) else { return false };
        if self.platform() == Platform::Desktop {
            return cfg.kind != ProviderKind::OnDevice;
        }
        match cfg.kind {
            ProviderKind::Ollama | ProviderKind::LmStudio => false,
            ProviderKind::CustomOpenai => !is_loopback(&cfg.base_url),
            _ => true,
        }
    }

    /// The model an agent uses on this device. On the desktop that is its model; on iOS the agent's iOS override
    /// comes first and its desktop model is used only if it can be reached from a phone.
    pub fn model_for(&self, agent: &Agent) -> Option<ModelRef> {
        if self.platform() == Platform::Desktop {
            return agent.model.clone();
        }
        [&agent.ios_model, &agent.model].into_iter().flatten().find(|m| self.model_usable_here(m)).cloned()
    }

    #[doc(hidden)]
    pub fn no_model_reason_for_tests(&self, agent: &Agent) -> String {
        self.no_model_reason(agent)
    }

    /// Why an agent has no model on this device, in words for the user.
    pub(crate) fn no_model_reason(&self, agent: &Agent) -> String {
        if self.platform() == Platform::Ios && agent.model.is_some() && agent.ios_model.is_none() {
            return format!("{}'s model runs on your computer. Pick an iPhone model for {} in Settings → Agents.", agent.name, agent.name);
        }
        format!("{} has no model yet", agent.name)
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
        merged.persona = a.persona.as_deref().and_then(settings::clean_persona);
        merged.learning = a.learning;
        merged.suggest_persona = a.suggest_persona;
        if merged.persona.as_ref().is_some_and(|p| p.chars().count() > settings::PERSONA_MAX_CHARS) {
            return Err(CoreError::invalid(format!("the persona is limited to {} characters", settings::PERSONA_MAX_CHARS)));
        }
        merged.job = if a.job.trim().is_empty() { current.job } else { a.job };
        let saved = self.db.upsert_agent(&merged)?;
        self.emit(CoreEvent::AgentChanged { agent: saved.clone() });
        Ok(saved)
    }

    // ---------------------------------------------------------------- agent memory

    pub fn list_agent_memories(&self, agent_id: &str) -> Result<Vec<AgentMemory>> {
        self.db.get_agent(agent_id)?;
        self.db.list_memories(agent_id)
    }

    /// Something the user wants an agent to remember (written by hand in Settings).
    pub fn add_agent_memory(&self, agent_id: &str, kind: MemoryKind, text: &str, project: Option<&str>) -> Result<AgentMemory> {
        self.db.get_agent(agent_id)?;
        let text = Self::memory_text(text)?;
        if self.db.count_memories(agent_id)? >= memory::MAX_ENTRIES {
            return Err(CoreError::invalid(format!("this agent already remembers {} things; forget some first", memory::MAX_ENTRIES)));
        }
        let saved = self.db.upsert_memory(&AgentMemory {
            id: new_id(),
            agent_id: agent_id.to_string(),
            kind,
            text,
            project: memory::clean_project(project),
            source: MemorySource::Explicit,
            pinned: false,
            hits: 0,
            created_at: 0,
            updated_at: 0,
        })?;
        self.emit(CoreEvent::MemoriesChanged { agent_id: agent_id.to_string() });
        Ok(saved)
    }

    /// Change an entry's text, kind, scope or pin. Rewording it (or re-scoping it) marks it as the user's own.
    pub fn update_agent_memory(&self, m: AgentMemory) -> Result<AgentMemory> {
        let current = self.db.get_memory(&m.id)?;
        let text = Self::memory_text(&m.text)?;
        let project = memory::clean_project(m.project.as_deref());
        let reworked = text != current.text || m.kind != current.kind || project != current.project;
        let saved = self.db.upsert_memory(&AgentMemory {
            text,
            kind: m.kind,
            project,
            pinned: m.pinned,
            source: if reworked { MemorySource::Edited } else { current.source },
            ..current
        })?;
        self.emit(CoreEvent::MemoriesChanged { agent_id: saved.agent_id.clone() });
        Ok(saved)
    }

    pub fn delete_agent_memory(&self, id: &str) -> Result<()> {
        let current = self.db.get_memory(id)?;
        self.db.delete_memory(id)?;
        self.emit(CoreEvent::MemoriesChanged { agent_id: current.agent_id });
        Ok(())
    }

    /// Wipe everything one agent remembers. How many entries went.
    pub fn forget_agent_memories(&self, agent_id: &str) -> Result<usize> {
        self.db.get_agent(agent_id)?;
        let n = self.db.forget_memories(agent_id)?;
        if n > 0 {
            self.emit(CoreEvent::MemoriesChanged { agent_id: agent_id.to_string() });
        }
        Ok(n)
    }

    /// The persona change an agent has suggested for itself, if one is waiting for the user.
    pub fn persona_proposal(&self, agent_id: &str) -> Result<Option<PersonaProposal>> {
        self.db.get_agent(agent_id)?;
        self.db.pending_persona_proposal(agent_id)
    }

    /// The user accepts a suggestion: it becomes the agent's persona. This is the only way a suggestion takes effect.
    pub fn accept_persona_proposal(&self, id: &str) -> Result<Agent> {
        let proposal = self.db.get_persona_proposal(id)?;
        let mut agent = self.db.get_agent(&proposal.agent_id)?;
        agent.persona = Some(proposal.text.clone());
        let saved = self.db.upsert_agent(&agent)?;
        self.db.resolve_persona_proposal(id, true)?;
        self.emit(CoreEvent::AgentChanged { agent: saved.clone() });
        self.emit(CoreEvent::PersonaProposalChanged { agent_id: saved.id.clone(), proposal: None });
        Ok(saved)
    }

    /// The user turns a suggestion down. The same text is not suggested again.
    pub fn reject_persona_proposal(&self, id: &str) -> Result<()> {
        let proposal = self.db.get_persona_proposal(id)?;
        self.db.resolve_persona_proposal(id, false)?;
        self.emit(CoreEvent::PersonaProposalChanged { agent_id: proposal.agent_id, proposal: None });
        Ok(())
    }

    fn memory_text(text: &str) -> Result<String> {
        let text = memory::clean_text(text).ok_or_else(|| CoreError::invalid("write what the agent should remember"))?;
        if text.chars().count() > memory::MAX_TEXT_CHARS {
            return Err(CoreError::invalid(format!("a memory is limited to {} characters; keep it to one thought", memory::MAX_TEXT_CHARS)));
        }
        Ok(text)
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
        // A folder that was already set stays usable; only a newly chosen one has to pass the safety check.
        if let Some(p) = &c.workspace_path
            && self.db.get_chat(&c.id).ok().and_then(|old| old.workspace_path).as_deref() != Some(p.as_str())
        {
            crate::permissions::check_workspace(Path::new(p))?;
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
        let _ = std::fs::remove_dir_all(crate::checkpoint::repo_dir(&self.data_dir, id));
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
        let m = self.mark_if_remote(m);
        self.db.insert_message(&m)?;
        self.emit(CoreEvent::MessageCreated { message: m.into_owned() });
        Ok(())
    }

    pub(crate) fn save_message(&self, m: &Message) -> Result<()> {
        let m = self.mark_if_remote(m);
        self.db.update_message(&m)?;
        self.emit(CoreEvent::MessageUpdated { message: m.into_owned() });
        Ok(())
    }

    /// Everything written in a chat while a remote request runs there is marked remote, which is what lets it
    /// sync to the requesting device while it is still streaming.
    fn mark_if_remote<'a>(&self, m: &'a Message) -> std::borrow::Cow<'a, Message> {
        if !self.remote.is_active(&m.chat_id) || m.meta.get("remote").and_then(Value::as_bool) == Some(true) {
            return std::borrow::Cow::Borrowed(m);
        }
        let mut marked = m.clone();
        match marked.meta.as_object_mut() {
            Some(o) => {
                o.insert("remote".into(), Value::Bool(true));
            }
            None => marked.meta = json!({ "remote": true }),
        }
        std::borrow::Cow::Owned(marked)
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
        let mut all: Vec<ProviderConfig> = self.db.list_providers()?.into_iter().map(|p| self.with_key_flag(p)).collect();
        all.extend(self.on_device_config());
        Ok(all)
    }

    pub fn provider_kinds(&self) -> Vec<ProviderKindInfo> {
        // On-device models are not something to add by hand: the iPhone app offers them when it can run them.
        ProviderKind::ALL.into_iter().filter(|k| *k != ProviderKind::OnDevice).map(ProviderKindInfo::from).collect()
    }

    /// Save a provider config. `api_key`: None = unchanged, Some("") = remove, Some(k) = store.
    pub fn save_provider(&self, cfg: ProviderConfig, api_key: Option<String>) -> Result<ProviderConfig> {
        let mut cfg = cfg;
        if cfg.kind == ProviderKind::OnDevice || cfg.id == crate::providers::ondevice::ON_DEVICE_PROVIDER_ID {
            return Err(CoreError::invalid("on-device models are built in; there is nothing to add or change"));
        }
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
        if provider_id == crate::providers::ondevice::ON_DEVICE_PROVIDER_ID {
            let cfg = self.provider_config(provider_id)?;
            let runtime = self.on_device_runtime().ok_or_else(|| CoreError::invalid("on-device models are not available here"))?;
            return Ok((cfg, Arc::new(crate::providers::ondevice::OnDeviceProvider::new(runtime))));
        }
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
    /// Replace the web tools with a canned answer (tests have no network).
    #[doc(hidden)]
    pub fn override_web(&self, f: impl Fn(&str, &Value) -> Result<String> + Send + Sync + 'static) {
        *self.web_override.lock().unwrap() = Some(Arc::new(f));
    }

    /// Run a web tool: the real one, unless a test replaced it.
    pub(crate) async fn run_web(&self, name: &str, args: &Value) -> Result<String> {
        let canned = self.web_override.lock().unwrap().clone();
        match canned {
            Some(f) => f(name, args),
            None => crate::tools::execute_web(name, args).await,
        }
    }

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

    pub fn shell_policy(&self, chat_id: &str) -> Result<ShellPolicy> {
        self.db.get_chat(chat_id)?;
        self.db.shell_policy(chat_id)
    }

    /// Set the chat's command policy. Pre-approved commands are cleaned up: one plain command prefix per entry.
    pub fn set_shell_policy(&self, chat_id: &str, policy: ShellPolicy) -> Result<ShellPolicy> {
        self.db.get_chat(chat_id)?;
        let mut allow: Vec<String> = Vec::new();
        for entry in policy.allow_commands {
            let entry = entry.split_whitespace().collect::<Vec<_>>().join(" ");
            if entry.is_empty() {
                continue;
            }
            if !crate::permissions::command_allowed(&entry, std::slice::from_ref(&entry)) {
                return Err(CoreError::invalid(format!("'{entry}' is not a plain command prefix (no ; & | < > $ ` ( ) or backslashes)")));
            }
            if !allow.contains(&entry) {
                allow.push(entry);
            }
        }
        let policy = ShellPolicy { network: policy.network, allow_commands: allow };
        self.db.set_shell_policy(chat_id, &policy)?;
        Ok(policy)
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

fn is_loopback(url: &str) -> bool {
    url::Url::parse(url).ok().and_then(|u| u.host_str().map(str::to_lowercase)).is_some_and(|h| {
        matches!(h.as_str(), "localhost" | "127.0.0.1" | "::1" | "[::1]" | "0.0.0.0") || h.ends_with(".local")
    })
}
