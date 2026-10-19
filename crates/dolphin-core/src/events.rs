//! Events pushed from the core to the UI over one long-lived channel.

use serde::Serialize;
use ts_rs::TS;

use crate::model::{ActionLogEntry, Agent, Chat, Message, PermissionGrant, UsageStats, UserRole};
use crate::settings::SettingChange;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
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
    UpdateAvailable { version: String, url: String },
    Toast { level: ToastLevel, text: String },
}
