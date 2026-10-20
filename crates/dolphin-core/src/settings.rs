//! The single typed settings registry. The GUI and Roger (chat-driven settings changes) both go
//! through `get` / `set`, so validation, defaults and side effects are identical on both paths.

use serde::Serialize;
use serde_json::{Value, json};
use ts_rs::TS;

use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::model::{Agent, AgentColor, Chat, EffortValue, ModelRef};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum SettingScope {
    Global,
    Chat,
    Agent,
}

#[derive(Debug, Clone, PartialEq, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
pub enum SettingType {
    Bool,
    Int { min: i64, max: i64 },
    Text { max_len: usize },
    Choice { options: Vec<String> },
    Model,
    Effort,
    Color,
    AgentList,
    Path,
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct SettingDef {
    pub key: String,
    pub label: String,
    pub description: String,
    pub scope: SettingScope,
    pub ty: SettingType,
    pub default: Value,
    /// Sensitive changes ask the user for a quick confirmation when requested through Roger.
    pub sensitive: bool,
    pub nullable: bool,
}

fn def(
    key: &str,
    label: &str,
    description: &str,
    scope: SettingScope,
    ty: SettingType,
    default: Value,
) -> SettingDef {
    SettingDef {
        key: key.into(),
        label: label.into(),
        description: description.into(),
        scope,
        ty,
        default,
        sensitive: false,
        nullable: false,
    }
}

impl SettingDef {
    fn sensitive(mut self) -> Self {
        self.sensitive = true;
        self
    }
    fn nullable(mut self) -> Self {
        self.nullable = true;
        self
    }
}

pub mod keys {
    pub const THEME: &str = "appearance.theme";
    pub const REDUCE_MOTION: &str = "appearance.reduce_motion";
    pub const REDUCE_TRANSPARENCY: &str = "appearance.reduce_transparency";
    pub const CLEANUP_ENABLED: &str = "cleanup.enabled";
    pub const CLEANUP_DEFAULT_TTL: &str = "cleanup.default_ttl_hours";
    pub const CUSTOM_INSTRUCTIONS: &str = "preferences.custom_instructions";
    pub const UTILITY_MODEL: &str = "utility.model";
    pub const STALL_SECONDS: &str = "supervisor.stall_seconds";
    pub const AUTO_UPDATE_CHECK: &str = "updates.auto_check";
    pub const HISTORY_MESSAGES: &str = "context.history_messages";
    pub const PASTE_THRESHOLD: &str = "chat.paste_threshold";
    pub const SHOW_BRIEFS: &str = "routing.show_briefs";
    pub const WEB_ACCESS: &str = "web.access";
    pub const ONBOARDED: &str = "app.onboarded";
    pub const DISPLAY_NAME: &str = "profile.display_name";

    pub const CHAT_NAME: &str = "chat.name";
    pub const CHAT_AGENTS: &str = "chat.agents";
    pub const CHAT_CLEANUP_ENABLED: &str = "chat.cleanup_enabled";
    pub const CHAT_CLEANUP_TTL: &str = "chat.cleanup_ttl_hours";
    pub const CHAT_WORKSPACE: &str = "chat.workspace_path";

    pub const AGENT_NAME: &str = "agent.name";
    pub const AGENT_COLOR: &str = "agent.color";
    pub const AGENT_MODEL: &str = "agent.model";
    pub const AGENT_EFFORT: &str = "agent.effort";
    pub const AGENT_IMAGE_MODEL: &str = "agent.image_model";
}

pub fn registry() -> Vec<SettingDef> {
    use SettingScope::*;
    use SettingType::*;
    vec![
        def(keys::THEME, "Theme", "Light, dark, or follow the system.", Global,
            Choice { options: vec!["system".into(), "dark".into(), "light".into()] }, json!("system")),
        def(keys::REDUCE_MOTION, "Reduce motion", "Turn off non-essential animations.", Global, Bool, json!(false)),
        def(keys::REDUCE_TRANSPARENCY, "Reduce transparency", "Use solid surfaces instead of frosted glass.", Global, Bool, json!(false)),
        def(keys::CLEANUP_ENABLED, "Periodic chat cleaning", "Global switch for automatic cleanup of old messages in every chat.", Global, Bool, json!(true)),
        def(keys::CLEANUP_DEFAULT_TTL, "Default cleanup age (hours)", "Messages older than this are removed. Used for new chats.", Global,
            Int { min: 1, max: 24 * 90 }, json!(24)),
        def(keys::CUSTOM_INSTRUCTIONS, "Custom instructions", "Your preferences. Only the parts relevant to each request are sent to the models.", Global,
            Text { max_len: 20_000 }, json!("")),
        def(keys::UTILITY_MODEL, "Utility model", "Small, cheap model for internal tasks (settings changes, classification). A free OpenRouter model or a small local model.", Global,
            Model, Value::Null).nullable(),
        def(keys::STALL_SECONDS, "Stuck-agent timeout (seconds)", "If an agent produces nothing for this long, Roger checks on it.", Global,
            Int { min: 15, max: 1800 }, json!(120)),
        def(keys::AUTO_UPDATE_CHECK, "Check for updates automatically", "Check for a new version on launch and every few hours.", Global, Bool, json!(true)),
        def(keys::HISTORY_MESSAGES, "Context window (messages)", "How many recent chat messages are sent to agents.", Global,
            Int { min: 4, max: 200 }, json!(40)),
        def(keys::PASTE_THRESHOLD, "Long paste threshold (characters)", "Pasted text longer than this becomes a .txt attachment.", Global,
            Int { min: 500, max: 200_000 }, json!(6000)),
        def(keys::SHOW_BRIEFS, "Show orchestration briefs", "Show Lincoln's brief and the routing decision on agent messages.", Global, Bool, json!(true)),
        def(keys::WEB_ACCESS, "Web access for agents", "Let agents search the web and open public pages to answer with current information.", Global, Bool, json!(true)),
        def(keys::ONBOARDED, "Onboarding finished", "Internal.", Global, Bool, json!(false)),
        def(keys::DISPLAY_NAME, "Your name", "How you appear in chats. Agents address you by it.", Global, Text { max_len: 32 }, json!("You")),

        def(keys::CHAT_NAME, "Chat name", "Name of the chat.", Chat, Text { max_len: 60 }, json!("")),
        def(keys::CHAT_AGENTS, "Assigned agents", "Agents that take part in this chat. Roger and Lincoln are always present.", Chat, AgentList, json!([])),
        def(keys::CHAT_CLEANUP_ENABLED, "Clean this chat automatically", "Remove old messages from this chat on a rolling basis.", Chat, Bool, json!(true)),
        def(keys::CHAT_CLEANUP_TTL, "Cleanup age (hours)", "Messages older than this are removed from this chat.", Chat,
            Int { min: 1, max: 24 * 90 }, json!(24)),
        def(keys::CHAT_WORKSPACE, "Workspace folder", "Folder agents may read (and, with full permissions, change) in this chat.", Chat, Path, Value::Null)
            .nullable().sensitive(),

        def(keys::AGENT_NAME, "Agent name", "Display name of the agent.", Agent, Text { max_len: 32 }, json!("")),
        def(keys::AGENT_COLOR, "Agent color", "Solid color or gradient used for the agent's messages and avatar.", Agent, Color, Value::Null),
        def(keys::AGENT_MODEL, "Agent model", "Provider and reasoning model the agent uses.", Agent, Model, Value::Null).nullable(),
        def(keys::AGENT_EFFORT, "Reasoning effort", "Reasoning effort for the agent's model, if the model supports it.", Agent, Effort, Value::Null).nullable(),
        def(keys::AGENT_IMAGE_MODEL, "Image model", "Image-generation model (image specialist only).", Agent, Model, Value::Null).nullable(),
    ]
}

pub fn find_def(key: &str) -> Result<SettingDef> {
    registry().into_iter().find(|d| d.key == key).ok_or_else(|| CoreError::invalid(format!("unknown setting '{key}'")))
}

/// Compact description of every setting for Roger's settings-controller prompt.
pub fn describe_for_llm() -> String {
    registry()
        .into_iter()
        .filter(|d| d.key != keys::ONBOARDED)
        .map(|d| {
            let ty = match &d.ty {
                SettingType::Bool => "bool".to_string(),
                SettingType::Int { min, max } => format!("int {min}..{max}"),
                SettingType::Text { .. } => "string".into(),
                SettingType::Choice { options } => format!("one of {}", options.join("|")),
                SettingType::Model => "{providerId, modelId}".into(),
                SettingType::Effort => "{type: level|budget|toggle, value}".into(),
                SettingType::Color => "{type: solid, color: #hex} or {type: gradient, angle, stops:[{color, position}]}".into(),
                SettingType::AgentList => "array of agent names".into(),
                SettingType::Path => "absolute folder path".into(),
            };
            let target = match d.scope {
                SettingScope::Global => "",
                SettingScope::Chat => " [target: chat name]",
                SettingScope::Agent => " [target: agent name]",
            };
            format!("- {} ({ty}){target}: {}", d.key, d.description)
        })
        .collect::<Vec<_>>()
        .join("\n")
}

// ------------------------------------------------------------------ validation

fn validate(d: &SettingDef, v: &Value) -> Result<Value> {
    if v.is_null() {
        return if d.nullable { Ok(Value::Null) } else { Err(CoreError::invalid(format!("{} cannot be empty", d.key))) };
    }
    let bad = || CoreError::invalid(format!("invalid value for {}: {v}", d.key));
    Ok(match &d.ty {
        SettingType::Bool => match v {
            Value::Bool(_) => v.clone(),
            Value::String(s) if matches!(s.as_str(), "true" | "on" | "yes") => json!(true),
            Value::String(s) if matches!(s.as_str(), "false" | "off" | "no") => json!(false),
            _ => return Err(bad()),
        },
        SettingType::Int { min, max } => {
            let n = v.as_i64().or_else(|| v.as_f64().map(|f| f.round() as i64)).or_else(|| v.as_str()?.trim().parse().ok());
            let n = n.ok_or_else(bad)?;
            if n < *min || n > *max {
                return Err(CoreError::invalid(format!("{} must be between {min} and {max}", d.key)));
            }
            json!(n)
        }
        SettingType::Text { max_len } => {
            let s = v.as_str().ok_or_else(bad)?;
            if s.chars().count() > *max_len {
                return Err(CoreError::invalid(format!("{} is limited to {max_len} characters", d.key)));
            }
            json!(s)
        }
        SettingType::Choice { options } => {
            let s = v.as_str().ok_or_else(bad)?.to_lowercase();
            if !options.contains(&s) {
                return Err(CoreError::invalid(format!("{} must be one of {}", d.key, options.join(", "))));
            }
            json!(s)
        }
        SettingType::Model => {
            serde_json::from_value::<ModelRef>(v.clone()).map_err(|_| bad())?;
            v.clone()
        }
        SettingType::Effort => {
            serde_json::from_value::<EffortValue>(v.clone()).map_err(|_| bad())?;
            v.clone()
        }
        SettingType::Color => {
            let c: AgentColor = serde_json::from_value(v.clone()).map_err(|_| bad())?;
            if !valid_color(&c) {
                return Err(bad());
            }
            v.clone()
        }
        SettingType::AgentList => {
            v.as_array().ok_or_else(bad)?;
            v.clone()
        }
        SettingType::Path => {
            let s = v.as_str().ok_or_else(bad)?;
            let p = std::path::Path::new(s);
            if !p.is_absolute() || !p.is_dir() {
                return Err(CoreError::invalid(format!("'{s}' is not an existing folder")));
            }
            json!(s)
        }
    })
}

/// Hex colors only; gradients need 2-8 stops.
pub fn valid_color(c: &AgentColor) -> bool {
    let ok_hex = |s: &str| {
        let h = s.trim_start_matches('#');
        s.starts_with('#') && matches!(h.len(), 3 | 6 | 8) && h.chars().all(|c| c.is_ascii_hexdigit())
    };
    match c {
        AgentColor::Solid { color } => ok_hex(color),
        AgentColor::Gradient { stops, .. } => (2..=8).contains(&stops.len()) && stops.iter().all(|s| ok_hex(&s.color)),
    }
}

// ------------------------------------------------------------------ targets

pub fn resolve_chat(db: &Db, target: &str) -> Result<Chat> {
    let t = target.trim().trim_start_matches('#').to_lowercase();
    db.list_chats()?
        .into_iter()
        .find(|c| c.id == target || c.name.to_lowercase() == t)
        .ok_or_else(|| CoreError::NotFound(format!("chat '{target}'")))
}

pub fn resolve_agent(db: &Db, target: &str) -> Result<Agent> {
    let t = target.trim().trim_start_matches('@').to_lowercase();
    db.list_agents()?
        .into_iter()
        .find(|a| a.id == target || a.slug == t || a.name.to_lowercase() == t)
        .ok_or_else(|| CoreError::NotFound(format!("agent '{target}'")))
}

// ------------------------------------------------------------------ get / set

pub fn get_global(db: &Db, key: &str) -> Result<Value> {
    let d = find_def(key)?;
    Ok(db.setting_raw(key)?.unwrap_or(d.default))
}

pub fn get_bool(db: &Db, key: &str) -> bool {
    get_global(db, key).ok().and_then(|v| v.as_bool()).unwrap_or(false)
}

pub fn get_int(db: &Db, key: &str) -> i64 {
    get_global(db, key).ok().and_then(|v| v.as_i64()).unwrap_or(0)
}

pub fn get_string(db: &Db, key: &str) -> String {
    get_global(db, key).ok().and_then(|v| v.as_str().map(String::from)).unwrap_or_default()
}

pub fn get_model(db: &Db, key: &str) -> Option<ModelRef> {
    get_global(db, key).ok().and_then(|v| serde_json::from_value(v).ok())
}

/// Every global setting as a key → value map (defaults filled in).
pub fn all_global(db: &Db) -> Result<serde_json::Map<String, Value>> {
    let mut out = serde_json::Map::new();
    for d in registry().into_iter().filter(|d| d.scope == SettingScope::Global) {
        let v = db.setting_raw(&d.key)?.unwrap_or(d.default);
        out.insert(d.key, v);
    }
    Ok(out)
}

#[derive(Debug, Clone, Serialize, TS)]
#[serde(tag = "type", rename_all = "camelCase")]
#[ts(export)]
#[allow(clippy::large_enum_variant)]
pub enum SettingChange {
    Global { key: String, value: Value },
    Chat { chat: Chat },
    Agent { agent: Agent },
}

/// Validate and apply a setting. `target` is a chat or agent (id or name) for scoped keys.
pub fn set(db: &Db, key: &str, target: Option<&str>, value: Value) -> Result<SettingChange> {
    let d = find_def(key)?;
    match d.scope {
        SettingScope::Global => {
            let v = validate(&d, &value)?;
            db.set_setting_raw(key, &v)?;
            Ok(SettingChange::Global { key: key.into(), value: v })
        }
        SettingScope::Chat => {
            let target = target.ok_or_else(|| CoreError::invalid(format!("{key} needs a chat")))?;
            let mut chat = resolve_chat(db, target)?;
            let v = validate(&d, &value)?;
            match key {
                keys::CHAT_NAME => {
                    let name = v.as_str().unwrap_or_default().trim().to_string();
                    if name.is_empty() {
                        return Err(CoreError::invalid("chat name cannot be empty"));
                    }
                    chat.name = name;
                }
                keys::CHAT_CLEANUP_ENABLED => chat.cleanup_enabled = v.as_bool().unwrap_or(true),
                keys::CHAT_CLEANUP_TTL => chat.cleanup_ttl_hours = v.as_i64().unwrap_or(24) as u32,
                keys::CHAT_WORKSPACE => chat.workspace_path = v.as_str().map(String::from),
                keys::CHAT_AGENTS => {
                    let mut ids = Vec::new();
                    for a in db.list_agents()? {
                        if a.role != crate::model::AgentRole::Specialist {
                            ids.push(a.id);
                        }
                    }
                    for item in v.as_array().cloned().unwrap_or_default() {
                        let name = item.as_str().ok_or_else(|| CoreError::invalid("agent list must hold names"))?;
                        let a = resolve_agent(db, name)?;
                        if !ids.contains(&a.id) {
                            ids.push(a.id);
                        }
                    }
                    chat.agent_ids = ids;
                }
                _ => return Err(CoreError::invalid(format!("unhandled chat setting {key}"))),
            }
            Ok(SettingChange::Chat { chat: db.upsert_chat(&chat)? })
        }
        SettingScope::Agent => {
            let target = target.ok_or_else(|| CoreError::invalid(format!("{key} needs an agent")))?;
            let mut agent = resolve_agent(db, target)?;
            let v = validate(&d, &value)?;
            match key {
                keys::AGENT_NAME => {
                    let name = v.as_str().unwrap_or_default().trim().to_string();
                    if name.is_empty() || name.contains(char::is_whitespace) {
                        return Err(CoreError::invalid("agent names must be one word so @mentions work"));
                    }
                    if db.list_agents()?.iter().any(|a| a.id != agent.id && a.name.eq_ignore_ascii_case(&name)) {
                        return Err(CoreError::invalid(format!("another agent is already called {name}")));
                    }
                    agent.name = name;
                }
                keys::AGENT_COLOR => agent.color = serde_json::from_value(v)?,
                keys::AGENT_MODEL => agent.model = serde_json::from_value(v)?,
                keys::AGENT_IMAGE_MODEL => agent.image_model = serde_json::from_value(v)?,
                keys::AGENT_EFFORT => {
                    let m = agent.model.as_mut().ok_or_else(|| CoreError::invalid("set a model before the effort"))?;
                    m.effort = serde_json::from_value(v)?;
                }
                _ => return Err(CoreError::invalid(format!("unhandled agent setting {key}"))),
            }
            Ok(SettingChange::Agent { agent: db.upsert_agent(&agent)? })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::seed::seed_if_needed;

    fn db() -> Db {
        let db = Db::open_in_memory().unwrap();
        seed_if_needed(&db).unwrap();
        db
    }

    #[test]
    fn global_defaults_and_validation() {
        let db = db();
        assert_eq!(get_int(&db, keys::CLEANUP_DEFAULT_TTL), 24);
        assert!(set(&db, keys::THEME, None, json!("purple")).is_err());
        set(&db, keys::THEME, None, json!("Dark")).unwrap();
        assert_eq!(get_string(&db, keys::THEME), "dark");
        assert!(set(&db, keys::CLEANUP_DEFAULT_TTL, None, json!(0)).is_err());
        set(&db, keys::CLEANUP_DEFAULT_TTL, None, json!("48")).unwrap();
        assert_eq!(get_int(&db, keys::CLEANUP_DEFAULT_TTL), 48);
        assert!(set(&db, keys::CLEANUP_ENABLED, None, Value::Null).is_err());
    }

    #[test]
    fn scoped_settings_resolve_targets_by_name() {
        let db = db();
        set(&db, keys::CHAT_CLEANUP_TTL, Some("coding"), json!(12)).unwrap();
        assert_eq!(resolve_chat(&db, "Coding").unwrap().cleanup_ttl_hours, 12);
        set(&db, keys::AGENT_NAME, Some("@rick"), json!("Ricky")).unwrap();
        assert_eq!(db.agent_by_slug("rick").unwrap().name, "Ricky");
        assert!(set(&db, keys::AGENT_NAME, Some("Ricky"), json!("Roger")).is_err());
        assert!(set(&db, keys::AGENT_NAME, Some("Ricky"), json!("Two words")).is_err());
        let bad_color = json!({"type": "solid", "color": "red"});
        assert!(set(&db, keys::AGENT_COLOR, Some("Ricky"), bad_color).is_err());
    }

    #[test]
    fn chat_agent_list_keeps_moderators() {
        let db = db();
        set(&db, keys::CHAT_AGENTS, Some("Creative"), json!(["Emma"])).unwrap();
        let chat = resolve_chat(&db, "Creative").unwrap();
        let roger = db.agent_by_slug("roger").unwrap();
        assert_eq!(chat.agent_ids.len(), 3);
        assert!(chat.agent_ids.contains(&roger.id));
    }

    #[test]
    fn workspace_is_sensitive() {
        assert!(find_def(keys::CHAT_WORKSPACE).unwrap().sensitive);
        assert!(!find_def(keys::THEME).unwrap().sensitive);
    }
}
