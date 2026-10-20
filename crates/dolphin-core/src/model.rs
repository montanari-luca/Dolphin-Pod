//! Domain records shared by the core, the desktop UI (via ts-rs) and later the iOS app.
//!
//! Every syncable record carries `updated_at`, `device_id` and `deleted` so Phase 2 sync
//! (last-writer-wins per field + tombstones) can be added without a migration.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub type Id = String;

// ------------------------------------------------------------------ colours

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct GradientStop {
    pub color: String,
    /// 0..=100
    pub position: f32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum AgentColor {
    Solid { color: String },
    Gradient { angle: f32, stops: Vec<GradientStop> },
}

impl AgentColor {
    pub fn solid(c: &str) -> Self {
        AgentColor::Solid { color: c.into() }
    }
    pub fn gradient(angle: f32, stops: &[(&str, f32)]) -> Self {
        AgentColor::Gradient {
            angle,
            stops: stops.iter().map(|(c, p)| GradientStop { color: (*c).into(), position: *p }).collect(),
        }
    }
}

// ------------------------------------------------------------------ models / providers

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", content = "value", rename_all = "camelCase")]
#[ts(export)]
pub enum EffortValue {
    Level(String),
    Budget(u32),
    Toggle(bool),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelRef {
    pub provider_id: Id,
    pub model_id: String,
    #[serde(default)]
    pub effort: Option<EffortValue>,
    /// Capabilities copied from `ModelInfo` when the model was picked.
    #[serde(default)]
    pub vision: bool,
    #[serde(default = "yes")]
    pub tools: bool,
    /// Context window in tokens, copied from `ModelInfo` when the model was picked (None = unknown).
    #[serde(default)]
    #[ts(type = "number | null")]
    pub context_window: Option<u32>,
}

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ProviderKind {
    Ollama,
    LmStudio,
    CustomOpenai,
    Openrouter,
    Openai,
    Anthropic,
    Gemini,
    Xai,
    Mistral,
    Deepseek,
    Groq,
    Together,
    Fireworks,
    AzureOpenai,
}

impl ProviderKind {
    pub const ALL: [ProviderKind; 14] = [
        ProviderKind::Ollama,
        ProviderKind::LmStudio,
        ProviderKind::CustomOpenai,
        ProviderKind::Openrouter,
        ProviderKind::Openai,
        ProviderKind::Anthropic,
        ProviderKind::Gemini,
        ProviderKind::Xai,
        ProviderKind::Mistral,
        ProviderKind::Deepseek,
        ProviderKind::Groq,
        ProviderKind::Together,
        ProviderKind::Fireworks,
        ProviderKind::AzureOpenai,
    ];

    pub fn as_str(&self) -> &'static str {
        match self {
            ProviderKind::Ollama => "ollama",
            ProviderKind::LmStudio => "lm_studio",
            ProviderKind::CustomOpenai => "custom_openai",
            ProviderKind::Openrouter => "openrouter",
            ProviderKind::Openai => "openai",
            ProviderKind::Anthropic => "anthropic",
            ProviderKind::Gemini => "gemini",
            ProviderKind::Xai => "xai",
            ProviderKind::Mistral => "mistral",
            ProviderKind::Deepseek => "deepseek",
            ProviderKind::Groq => "groq",
            ProviderKind::Together => "together",
            ProviderKind::Fireworks => "fireworks",
            ProviderKind::AzureOpenai => "azure_openai",
        }
    }

    pub fn parse(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == s)
    }

    pub fn label(&self) -> &'static str {
        match self {
            ProviderKind::Ollama => "Ollama",
            ProviderKind::LmStudio => "LM Studio",
            ProviderKind::CustomOpenai => "Custom endpoint",
            ProviderKind::Openrouter => "OpenRouter",
            ProviderKind::Openai => "OpenAI",
            ProviderKind::Anthropic => "Anthropic",
            ProviderKind::Gemini => "Google Gemini",
            ProviderKind::Xai => "xAI",
            ProviderKind::Mistral => "Mistral",
            ProviderKind::Deepseek => "DeepSeek",
            ProviderKind::Groq => "Groq",
            ProviderKind::Together => "Together AI",
            ProviderKind::Fireworks => "Fireworks AI",
            ProviderKind::AzureOpenai => "Azure OpenAI",
        }
    }

    pub fn is_local(&self) -> bool {
        matches!(self, ProviderKind::Ollama | ProviderKind::LmStudio | ProviderKind::CustomOpenai)
    }

    pub fn needs_key(&self) -> bool {
        !self.is_local()
    }

    pub fn default_base_url(&self) -> &'static str {
        match self {
            ProviderKind::Ollama => "http://localhost:11434",
            ProviderKind::LmStudio => "http://localhost:1234/v1",
            ProviderKind::CustomOpenai => "http://localhost:8000/v1",
            ProviderKind::Openrouter => "https://openrouter.ai/api/v1",
            ProviderKind::Openai => "https://api.openai.com/v1",
            ProviderKind::Anthropic => "https://api.anthropic.com",
            ProviderKind::Gemini => "https://generativelanguage.googleapis.com",
            ProviderKind::Xai => "https://api.x.ai/v1",
            ProviderKind::Mistral => "https://api.mistral.ai/v1",
            ProviderKind::Deepseek => "https://api.deepseek.com/v1",
            ProviderKind::Groq => "https://api.groq.com/openai/v1",
            ProviderKind::Together => "https://api.together.xyz/v1",
            ProviderKind::Fireworks => "https://api.fireworks.ai/inference/v1",
            ProviderKind::AzureOpenai => "https://YOUR-RESOURCE.openai.azure.com/openai/v1",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderKindInfo {
    pub kind: ProviderKind,
    pub label: &'static str,
    pub is_local: bool,
    pub needs_key: bool,
    pub default_base_url: &'static str,
}

impl From<ProviderKind> for ProviderKindInfo {
    fn from(kind: ProviderKind) -> Self {
        Self {
            kind,
            label: kind.label(),
            is_local: kind.is_local(),
            needs_key: kind.needs_key(),
            default_base_url: kind.default_base_url(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ProviderConfig {
    pub id: Id,
    pub kind: ProviderKind,
    pub name: String,
    pub base_url: String,
    /// Provider-specific extras (e.g. Azure api-version). Free-form JSON.
    #[serde(default)]
    pub extra: serde_json::Value,
    pub enabled: bool,
    /// Computed: whether an API key is stored in the keychain.
    #[serde(default)]
    pub has_key: bool,
    #[ts(type = "number")]
    pub updated_at: i64,
}

/// How a model exposes reasoning effort. Only what the model supports is shown in the UI.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum EffortControl {
    None,
    Levels { levels: Vec<String>, default: String },
    Budget { min: u32, max: u32, default: u32 },
    Toggle,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ModelInfo {
    pub id: String,
    pub name: String,
    pub reasoning: bool,
    /// Why a model is not selectable (shown in the UI next to disabled entries).
    pub reason: Option<String>,
    pub effort: EffortControl,
    pub vision: bool,
    pub tools: bool,
    pub image_output: bool,
    pub context_window: Option<u32>,
    pub free: bool,
}

// ------------------------------------------------------------------ agents

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AgentRole {
    Moderator,
    Orchestrator,
    Specialist,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Agent {
    pub id: Id,
    /// Stable key ("roger", "rick", ...). Never shown, never renamed.
    pub slug: String,
    pub name: String,
    pub role: AgentRole,
    /// Specialty key: moderator, orchestrator, coder, long_horizon, security, debug, creative, docs, image, teacher.
    pub specialty: String,
    pub job: String,
    pub color: AgentColor,
    pub model: Option<ModelRef>,
    /// Sarah only: the image-generation model used after her reasoning model writes the prompt.
    pub image_model: Option<ModelRef>,
    /// Per-platform override (Phase 2: used by the iOS app).
    pub ios_model: Option<ModelRef>,
    pub sort: i32,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UsageStats {
    pub agent_id: Id,
    #[ts(type = "number")]
    pub tokens_in: i64,
    #[ts(type = "number")]
    pub tokens_out: i64,
    #[ts(type = "number")]
    pub thinking_ms: i64,
    #[ts(type = "number")]
    pub working_ms: i64,
    #[ts(type = "number")]
    pub requests: i64,
    #[ts(type = "number | null")]
    pub last_query_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub last_query_at: Option<i64>,
}

// ------------------------------------------------------------------ chats & messages

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Chat {
    pub id: Id,
    pub name: String,
    pub icon: String,
    pub sort: i32,
    pub agent_ids: Vec<Id>,
    pub cleanup_enabled: bool,
    pub cleanup_ttl_hours: u32,
    pub workspace_path: Option<String>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MessageKind {
    Text,
    Ack,
    Plan,
    Notice,
    Image,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum MessageStatus {
    Pending,
    Streaming,
    Done,
    Error,
    Cancelled,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Message {
    pub id: Id,
    pub chat_id: Id,
    /// None = the user.
    pub agent_id: Option<Id>,
    pub kind: MessageKind,
    pub status: MessageStatus,
    pub content: String,
    pub reasoning: Option<String>,
    pub error: Option<String>,
    pub mentions: Vec<Id>,
    pub attachments: Vec<Attachment>,
    /// Routing decision, Lincoln's brief, plan approval state, tool activity.
    pub meta: serde_json::Value,
    #[ts(type = "number | null")]
    pub thinking_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub working_ms: Option<i64>,
    #[ts(type = "number | null")]
    pub tokens_in: Option<i64>,
    #[ts(type = "number | null")]
    pub tokens_out: Option<i64>,
    #[ts(type = "number")]
    pub created_at: i64,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AttachmentKind {
    Image,
    Text,
    Pdf,
    Other,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Attachment {
    pub id: Id,
    pub name: String,
    pub mime: String,
    pub kind: AttachmentKind,
    #[ts(type = "number")]
    pub size: i64,
    pub path: String,
}

// ------------------------------------------------------------------ permissions

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum PermissionLevel {
    ReadOnly,
    Full,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct PermissionGrant {
    pub chat_id: Id,
    pub agent_id: Id,
    pub level: PermissionLevel,
    pub reason: String,
    #[ts(type = "number")]
    pub updated_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct ActionLogEntry {
    pub id: Id,
    pub chat_id: Id,
    pub agent_id: Id,
    pub tool: String,
    pub summary: String,
    pub allowed: bool,
    pub detail: String,
    #[ts(type = "number")]
    pub created_at: i64,
}

// ------------------------------------------------------------------ automations

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct Automation {
    pub id: Id,
    pub chat_id: Id,
    pub text: String,
    /// "HH:MM", device local time.
    pub time_of_day: String,
    /// Empty = every day. 0 = Monday … 6 = Sunday.
    pub weekdays: Vec<u8>,
    pub enabled: bool,
    pub last_run_date: Option<String>,
    /// The one device that fires this automation. `None` on save means "this device".
    pub runner_device_id: Option<String>,
    #[ts(type = "number")]
    pub updated_at: i64,
}

// ------------------------------------------------------------------ user profile

/// A cosmetic role the user gives themselves. The first role (lowest `sort`) colors the user's name.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct UserRole {
    pub id: Id,
    pub name: String,
    /// Icon key from the app's role icon set (e.g. "crown").
    pub icon: String,
    pub color: AgentColor,
    pub sort: i32,
    #[ts(type = "number")]
    pub updated_at: i64,
}
