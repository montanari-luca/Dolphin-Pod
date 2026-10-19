//! Routing: Roger (moderator) decides, Lincoln (orchestrator) briefs and dispatches, `@mentions` bypass both.

pub mod ack;
pub mod fallback;
pub mod mention;
pub mod moderator;
pub mod orchestrator;
pub mod prompts;
pub mod settings_ctl;

use serde::{Deserialize, Serialize};

use crate::model::PermissionLevel;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Mode {
    /// Direct, concise answer or implementation.
    Quick,
    /// The user probably wants to understand: teach.
    Learn,
    /// Produce a plan only; read-only permissions are enforced until it is approved.
    Plan,
    /// Do the work.
    #[default]
    Implement,
    /// Generic question: Roger answers himself.
    Answer,
    /// Settings changes or housekeeping, handled by Roger with the utility model.
    Internal,
    /// The request is too vague: Roger asks a question.
    Clarify,
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::Quick => "quick",
            Mode::Learn => "learn",
            Mode::Plan => "plan",
            Mode::Implement => "implement",
            Mode::Answer => "answer",
            Mode::Internal => "internal",
            Mode::Clarify => "clarify",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Rewrite {
    None,
    #[default]
    Light,
    Full,
}

/// The moderator's validated decision. `targets` are agent ids that belong to the chat.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Decision {
    pub intent: String,
    pub mode: Mode,
    pub targets: Vec<String>,
    pub rewrite: Rewrite,
    pub permission: PermissionLevel,
    pub approves_plan: bool,
    pub clarification: Option<String>,
    /// Roger's heads-up when the prompt is thin but still actionable.
    pub lean_note: Option<String>,
    /// "model" or "fallback".
    pub source: String,
}

impl Default for Decision {
    fn default() -> Self {
        Self {
            intent: String::new(),
            mode: Mode::Answer,
            targets: vec![],
            rewrite: Rewrite::Light,
            permission: PermissionLevel::ReadOnly,
            approves_plan: false,
            clarification: None,
            lean_note: None,
            source: "fallback".into(),
        }
    }
}
