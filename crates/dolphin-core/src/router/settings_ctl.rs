//! Roger as settings controller: "rename Rick to Ricky", "turn off cleanup in Creative", "clean this chat".
//! Every change goes through the same settings registry as the GUI. Sensitive changes ask for confirmation.

use serde::Deserialize;
use serde_json::Value;

use crate::core::CoreRef;
use crate::model::Chat;
use crate::providers::{ChatMessage, ChatRequest, Provider, complete};
use crate::settings::{self, describe_for_llm};
use crate::util::extract_json_object;

#[derive(Debug, Deserialize, PartialEq)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum Action {
    Set {
        key: String,
        #[serde(default)]
        target: Option<String>,
        value: Value,
    },
    CleanChat {
        #[serde(default)]
        chat: Option<String>,
    },
    CleanAllChats,
    DeleteChat {
        chat: String,
    },
}

#[derive(Debug, Deserialize, Default)]
struct Plan {
    #[serde(default)]
    actions: Vec<Action>,
    #[serde(default)]
    reply: String,
}

/// Requests that need no model at all.
pub fn quick_actions(text: &str) -> Option<Vec<Action>> {
    let t = text.to_lowercase();
    let cleanish = ["clean", "clear", "wipe", "empty"].iter().any(|w| t.contains(w));
    if !cleanish || t.split_whitespace().count() > 10 {
        return None;
    }
    if ["all chats", "every chat", "all the chats", "everything"].iter().any(|w| t.contains(w)) {
        return Some(vec![Action::CleanAllChats]);
    }
    if ["this chat", "the chat", "chat history", "this conversation", "up here"].iter().any(|w| t.contains(w)) {
        return Some(vec![Action::CleanChat { chat: None }]);
    }
    None
}

fn system_prompt(core: &CoreRef, chat: &Chat) -> String {
    let chats: Vec<String> = core.list_chats().unwrap_or_default().into_iter().map(|c| c.name).collect();
    let agents: Vec<String> = core.list_agents().unwrap_or_default().into_iter().map(|a| format!("{} ({})", a.name, a.specialty)).collect();
    format!(
        r##"You turn the user's request into settings actions for the Dolphin Pod app. Current chat: "{current}".
Chats: {chats}
Agents: {agents}

Settings:
{settings}

Reply with ONE JSON object only:
{{"actions": [ ... ], "reply": "<one friendly sentence confirming what you did, written as Roger>"}}
Action shapes:
{{"op": "set", "key": "<setting key>", "target": "<chat or agent name, omit for global>", "value": <value>}}
{{"op": "clean_chat", "chat": "<chat name, omit for the current chat>"}}
{{"op": "clean_all_chats"}}
{{"op": "delete_chat", "chat": "<chat name>"}}
Colors are hex strings (e.g. "#22c55e"). Durations in hours are integers. If the request is unclear or not about settings, return no actions and explain in "reply"."##,
        current = chat.name,
        chats = chats.join(", "),
        agents = agents.join(", "),
        settings = describe_for_llm(),
    )
}

pub fn parse_plan(text: &str) -> Option<(Vec<Action>, String)> {
    let p: Plan = serde_json::from_str(extract_json_object(text)?).ok()?;
    Some((p.actions, p.reply))
}

async fn apply(core: &CoreRef, chat: &Chat, action: Action) -> Result<String, String> {
    match action {
        Action::Set { key, target, value } => {
            let def = settings::find_def(&key).map_err(|e| e.to_string())?;
            let target = target.or_else(|| (def.scope == settings::SettingScope::Chat).then(|| chat.id.clone()));
            if def.sensitive {
                let body = format!("Roger wants to set \"{}\" to {}{}.", def.label, value, target.as_deref().map(|t| format!(" for {t}")).unwrap_or_default());
                if !core.confirm("Confirm setting change", &body).await {
                    return Err(format!("{}: not changed (not confirmed)", def.label));
                }
            }
            core.set_setting(&key, target.as_deref(), value).map_err(|e| e.to_string())?;
            Ok(format!("{} updated", def.label))
        }
        Action::CleanChat { chat: target } => {
            let c = match target {
                Some(t) if !t.eq_ignore_ascii_case("this") && !t.eq_ignore_ascii_case(&chat.name) => {
                    settings::resolve_chat(&core.db, &t).map_err(|e| e.to_string())?
                }
                _ => chat.clone(),
            };
            let n = core.clean_chat(&c.id).map_err(|e| e.to_string())?;
            Ok(format!("cleaned {} ({n} messages)", c.name))
        }
        Action::CleanAllChats => {
            if !core.confirm("Clean every chat?", "Roger will remove all messages from every chat.").await {
                return Err("chats not cleaned (not confirmed)".into());
            }
            let mut total = 0;
            for c in core.list_chats().map_err(|e| e.to_string())? {
                total += core.clean_chat(&c.id).map_err(|e| e.to_string())?;
            }
            Ok(format!("cleaned every chat ({total} messages)"))
        }
        Action::DeleteChat { chat: target } => {
            let c = settings::resolve_chat(&core.db, &target).map_err(|e| e.to_string())?;
            if !core.confirm("Delete chat?", &format!("Roger wants to delete the chat \"{}\" and all its messages.", c.name)).await {
                return Err(format!("{} not deleted (not confirmed)", c.name));
            }
            core.delete_chat(&c.id).map_err(|e| e.to_string())?;
            Ok(format!("deleted {}", c.name))
        }
    }
}

/// Handle an internal request and return Roger's reply.
pub async fn handle(core: &CoreRef, chat: &Chat, user_text: &str, model: Option<(&dyn Provider, ChatRequest)>) -> String {
    let (actions, mut reply) = if let Some(a) = quick_actions(user_text) {
        (a, String::new())
    } else if let Some((provider, mut req)) = model {
        req.system = system_prompt(core, chat);
        req.messages = vec![ChatMessage::user(user_text)];
        req.json_mode = true;
        req.tools.clear();
        match complete(provider, req).await {
            Ok((text, _)) => match parse_plan(&text) {
                Some(p) => p,
                None => return "I couldn't work out which setting you meant. Could you rephrase it, or change it in Settings?".into(),
            },
            Err(e) => return format!("I couldn't reach the utility model to change that ({e}). You can change it in Settings."),
        }
    } else {
        return "I need a utility model (or a model for me) to change settings from chat. Set one in Settings → General, or use the Settings screen directly.".into();
    };

    let mut done = Vec::new();
    let mut failed = Vec::new();
    for a in actions {
        match apply(core, chat, a).await {
            Ok(s) => done.push(s),
            Err(e) => failed.push(e),
        }
    }
    if reply.trim().is_empty() || !failed.is_empty() {
        reply = match (done.is_empty(), failed.is_empty()) {
            (true, true) => "There was nothing to change.".into(),
            (false, true) => format!("Done: {}.", done.join(", ")),
            (true, false) => format!("I couldn't do that: {}.", failed.join("; ")),
            (false, false) => format!("Done: {}. But: {}.", done.join(", "), failed.join("; ")),
        };
    }
    reply
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn quick_paths() {
        assert_eq!(quick_actions("Roger, clean this chat please"), Some(vec![Action::CleanChat { chat: None }]));
        assert_eq!(quick_actions("clear all chats"), Some(vec![Action::CleanAllChats]));
        assert_eq!(quick_actions("rename Rick to Ricky"), None);
    }

    #[test]
    fn parses_model_plan() {
        let (a, r) = parse_plan(r##"{"actions":[{"op":"set","key":"agent.name","target":"Rick","value":"Ricky"},{"op":"set","key":"chat.cleanup_enabled","value":false}],"reply":"Done!"}"##).unwrap();
        assert_eq!(a[0], Action::Set { key: "agent.name".into(), target: Some("Rick".into()), value: json!("Ricky") });
        assert_eq!(a[1], Action::Set { key: "chat.cleanup_enabled".into(), target: None, value: json!(false) });
        assert_eq!(r, "Done!");
    }
}
