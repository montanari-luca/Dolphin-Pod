//! DP Council: the strongest reviewer in the pod criticizes a specialist's approach once, then steps out.
//! Big tasks get the critique before the work starts; plans get it on a draft, which the planner then finalizes.

use super::*;
use crate::router::moderator::is_council;

/// Specialties whose big tasks the council looks at. Chat, learning, creative writing, images, git and checks are left alone.
const REVIEWED: &[&str] = &["coder", "long_horizon", "security", "debug", "docs", "researcher"];

/// Remembers that the council already spoke in this user turn, so a turn with several agents gets one critique.
#[derive(Default)]
pub(super) struct CouncilState {
    convened: bool,
}

/// How strong a model looks, from what the app knows about it: its reasoning effort, then its context window.
fn strength(m: &ModelRef) -> (u8, u32) {
    let effort = match &m.effort {
        Some(EffortValue::Level(l)) => match l.to_lowercase().as_str() {
            "max" => 6,
            "xhigh" => 5,
            "high" => 4,
            "medium" => 3,
            "low" => 2,
            "minimal" => 1,
            _ => 0,
        },
        // A thinking budget: more tokens to think with is stronger, from "some" up to "a lot".
        Some(EffortValue::Budget(b)) => (1 + b / 8192).min(5) as u8,
        Some(EffortValue::Toggle(true)) => 3,
        Some(EffortValue::Toggle(false)) | None => 0,
    };
    (effort, m.context_window.unwrap_or(0))
}

impl Core {
    /// Run `agent` on `task`; first let the council weigh in when this is the kind of work it is for.
    pub(super) async fn run_specialist(self: &Arc<Self>, turn: &Turn, agent: &Agent, mut task: AgentTask, state: &mut CouncilState) -> Result<Message> {
        let Some(council) = self.council_for(turn, agent, &task, state) else {
            return self.run_agent(turn, agent, task).await;
        };
        let Some(model) = self.council_model(&council, turn) else {
            state.convened = true;
            let lincoln = self.agent_by_slug("lincoln")?;
            self.post(
                &turn.chat.id,
                &lincoln.id,
                MessageKind::Notice,
                &format!("{} has no model, so it skipped this task. Choose one in Settings → Agents → {}.", council.name, council.name),
                json!({ "notice": "council" }),
            )?;
            return self.run_agent(turn, agent, task).await;
        };
        state.convened = true;

        let original = task.brief.clone().unwrap_or_default();
        if task.plan {
            // Plans are read-only, so the council can look at a real draft first.
            let mut draft = AgentTask::new(
                Mode::Plan,
                Some(format!(
                    "{original}\n\nThis is a DRAFT: {} will review it before the final plan is shown. Write the full plan, but do not tell the user to approve it yet.",
                    council.name
                )),
                false,
                task.routing.clone(),
            );
            draft.routing["council"] = json!({ "role": "draft" });
            let drafted = self.run_agent(turn, agent, draft).await?;
            if drafted.status != MessageStatus::Done {
                return Ok(drafted);
            }
            let ask = format!("{} wrote the draft plan in the chat above. Give your one critique of that draft.", agent.name);
            let reviewed = self.run_council(turn, &council, agent, model, task.mode, ask).await?;
            task.brief = Some(format!(
                "{original}\n\n{}",
                if reviewed {
                    format!("{} reviewed your draft plan above. Write the final plan now: complete and in the required structure, not a list of changes. Take the points that hold up and say briefly under Risks which ones you did not follow and why.", council.name)
                } else {
                    "Write the final plan now: complete and in the required structure, improving on your draft.".to_string()
                }
            ));
        } else {
            let ask = format!(
                "{} is about to work on the user's latest request. Lincoln's orders and brief for them:\n\n{original}\n\nGive your one critique of the approach now.",
                agent.name
            );
            if self.run_council(turn, &council, agent, model, task.mode, ask).await? {
                task.brief = Some(format!(
                    "{original}\n\n{} just reviewed this task (its message is above). Weigh its points: adopt what is right, and say in a sentence where you disagree.",
                    council.name
                ));
            }
        }
        self.run_agent(turn, agent, task).await
    }

    /// The council for this run, if one should speak: the setting is on, it is in the chat, and this is big work of a kind it reviews.
    fn council_for(&self, turn: &Turn, agent: &Agent, task: &AgentTask, state: &CouncilState) -> Option<Agent> {
        let eligible = !state.convened
            && task.delegated_by.is_none()
            && matches!(task.mode, Mode::Plan | Mode::Implement)
            && REVIEWED.contains(&agent.specialty.as_str())
            && turn.user_msg.meta.get("automation").is_none()
            && settings::get_bool(&self.db, keys::COUNCIL_ENABLED);
        if !eligible {
            return None;
        }
        turn.members.iter().find(|m| is_council(m) && m.id != agent.id).cloned()
    }

    /// Its own model if it has one, otherwise the strongest model among the other agents.
    fn council_model(&self, council: &Agent, turn: &Turn) -> Option<ModelRef> {
        self.model_for(council).or_else(|| {
            turn.all_agents.iter().filter(|a| a.id != council.id).filter_map(|a| self.model_for(a)).max_by_key(strength)
        })
    }

    /// One read-only critique, posted in the chat under the council's name. Returns whether there is one for the specialist to read;
    /// a council that failed never stops the task.
    async fn run_council(self: &Arc<Self>, turn: &Turn, council: &Agent, agent: &Agent, model: ModelRef, mode: Mode, ask: String) -> Result<bool> {
        self.run_critic(turn, council, agent, model, mode, ask).await
    }

    /// The director's one critique of the engineers' plan, before the coder builds from it. It counts as this turn's review,
    /// so the council stays out. Returns whether there is a critique for the coder to read; no model means no review.
    pub(super) async fn director_review(
        self: &Arc<Self>,
        turn: &Turn,
        director: &Agent,
        coder: &Agent,
        mode: Mode,
        state: &mut CouncilState,
    ) -> Result<bool> {
        if state.convened || turn.user_msg.meta.get("automation").is_some() {
            return Ok(false);
        }
        let Some(model) = self.council_model(director, turn) else { return Ok(false) };
        state.convened = true;
        let ask = format!(
            "The engineers wrote their engineering plan in the chat above, and {} will build from it next. Give your one critique of that plan. Reply in this shape and nothing more: **What's solid**, **Risks and gaps** (what is missing, unsafe or untested), **Build it this way** (concrete changes in priority order). Keep it short.",
            coder.name
        );
        self.run_critic(turn, director, coder, model, mode, ask).await
    }

    /// The critic is the council or the director; its specialty names who is speaking.
    async fn run_critic(self: &Arc<Self>, turn: &Turn, critic: &Agent, agent: &Agent, model: ModelRef, mode: Mode, ask: String) -> Result<bool> {
        let mut task = AgentTask::new(
            Mode::Answer,
            Some(ask),
            false,
            json!({ "via": critic.specialty, "mode": mode.as_str(), "council": { "role": "critic", "for": agent.name } }),
        );
        task.level_override = Some(PermissionLevel::ReadOnly);
        task.delegated_by = Some(critic.name.clone());
        task.model_override = Some(model);
        let m = self.run_agent(turn, critic, task).await?;
        Ok(m.status == MessageStatus::Done && !m.content.trim().is_empty())
    }
}
