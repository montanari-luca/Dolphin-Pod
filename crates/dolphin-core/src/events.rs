//! Events pushed from the core to the UI over one long-lived channel.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::model::{ActionLogEntry, Agent, Chat, Message, PermissionGrant, UsageStats, UserRole};
use crate::settings::SettingChange;
use crate::sync::manager::SyncStatus;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum AgentActivity {
    Idle,
    Routing,
    Thinking,
    Writing,
    UsingTool,
    Stuck,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ToastLevel {
    Info,
    Success,
    Warning,
    Error,
}

/// One choice an agent offers when it asks the user a question.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct QuestionOption {
    pub label: String,
    pub description: Option<String>,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase", rename_all_fields = "camelCase")]
#[ts(export)]
pub enum CoreEvent {
    MessageCreated { message: Message },
    MessageDelta { chat_id: String, message_id: String, text: Option<String>, reasoning: Option<String> },
    MessageUpdated { message: Message },
    MessagesRemoved { chat_id: String, message_ids: Vec<String>, all: bool },
    AgentActivity { chat_id: String, agent_id: String, activity: AgentActivity, detail: Option<String> },
    ChatChanged { chat: Chat },
    ChatDeleted { chat_id: String },
    AgentChanged { agent: Agent },
    RolesChanged { roles: Vec<UserRole> },
    SettingChanged { change: SettingChange },
    UsageUpdated { usage: UsageStats },
    GrantChanged { grant: PermissionGrant },
    ActionLogged { entry: ActionLogEntry },
    ConfirmRequest { id: String, title: String, body: String },
    /// An agent paused its run to ask the user a question (the options exclude the UI's own "Other" field).
    QuestionRequest { id: String, chat_id: String, agent_id: String, question: String, options: Vec<QuestionOption> },
    /// The question was answered, skipped or cancelled; every client drops its panel.
    QuestionClosed { id: String },
    UpdateAvailable { version: String, url: String },
    SyncStatusChanged { status: SyncStatus },
    /// Records that other lists need to reload after a sync (providers and automations have no finer events).
    SyncApplied { providers: bool, automations: bool },
    Toast { level: ToastLevel, text: String },
}
