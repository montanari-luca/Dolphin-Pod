//! Asking the user in the middle of a run: the pause holds no provider slot, and the answer is checked here.

use tokio::sync::OwnedSemaphorePermit;

use super::*;
use crate::events::QuestionOption;

impl Core {
    /// Ask the user a question and wait for the answer. The run's provider slot is released while it waits and
    /// taken again afterwards.
    pub(super) async fn ask_releasing(
        &self,
        turn: &Turn,
        agent: &Agent,
        cfg: &ProviderConfig,
        permit: &mut Option<OwnedSemaphorePermit>,
        question: &str,
        options: Vec<QuestionOption>,
    ) -> Result<QuestionAnswer> {
        self.activity(&turn.chat.id, &agent.id, AgentActivity::UsingTool, Some("Waiting for your approval".into()));
        *permit = None;
        let answer = self.ask(&turn.chat.id, &agent.id, question, options, &turn.token).await;
        *permit = Some(tokio::select! {
            p = self.providers.semaphore(cfg).acquire_owned() => p.map_err(|_| CoreError::Cancelled)?,
            _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
        });
        answer
    }

    /// A plain "allow this once or not" approval. The inner result is the decision; the outer one is a failure to ask.
    pub(super) async fn approve_once(
        &self,
        turn: &Turn,
        agent: &Agent,
        cfg: &ProviderConfig,
        permit: &mut Option<OwnedSemaphorePermit>,
        can_ask: bool,
        question: &str,
    ) -> Result<Result<()>> {
        if !can_ask {
            return Ok(Err(CoreError::Permission(format!(
                "{question} needs the user's approval because this run has read web content, and nobody can answer here (automations, delegated and remote runs cannot ask). Do not retry it."
            ))));
        }
        let options = vec![
            QuestionOption { label: tools::APPROVE_ONCE.into(), description: None },
            QuestionOption { label: tools::DENY.into(), description: None },
        ];
        Ok(match self.ask_releasing(turn, agent, cfg, permit, &truncate(question, 400), options).await? {
            QuestionAnswer::Choice(c) if c == tools::APPROVE_ONCE => Ok(()),
            QuestionAnswer::Custom(t) => Err(CoreError::Permission(format!("the user did not approve this and said: \"{t}\". Do not run it; adjust your approach."))),
            _ => Err(CoreError::Permission("the user denied this action. Do not retry it; ask what they would like instead.".into())),
        })
    }
}
