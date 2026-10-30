//! Two designers, two ideas: the director's designers work side by side on the same request, and the run
//! pauses on a card in the chat. The user picks an idea or asks for a merge, and the rest of the team carries on from there.

use futures::future::join_all;

use super::*;
use crate::util::new_id;

/// Positions of the first two designers in the team, if there are two.
pub(super) fn designer_pair(team: &[Agent]) -> Option<(usize, usize)> {
    let mut it = team.iter().enumerate().filter(|(_, a)| a.specialty == "designer").map(|(i, _)| i);
    Some((it.next()?, it.next()?))
}

const LABELS: [&str; 2] = ["A", "B"];

impl Core {
    /// Both designers brief and work in parallel, each on their own idea. When both finish, the director posts the choice card and
    /// the run stops (`true`); the team behind them resumes from `choose_design`. If a designer failed there is nothing to choose
    /// between, so the run goes on with whatever there is (`false`).
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn design_round(
        self: &Arc<Self>,
        turn: &Turn,
        d: &Decision,
        team: &[Agent],
        pair: (usize, usize),
        director: &Agent,
        model: Option<&ModelHandle>,
        extra: Option<&str>,
    ) -> Result<bool> {
        let group = new_id();
        let designers = [&team[pair.0], &team[pair.1]];
        let mut tasks = Vec::new();
        for (i, designer) in designers.iter().enumerate() {
            let (brief, brief_text) = self.brief_for(turn, d, designer, team, director, model, extra).await?;
            let mut task = AgentTask::new(
                d.mode,
                Some(brief),
                false,
                json!({ "via": "director", "mode": d.mode.as_str(), "rewrite": d.rewrite, "brief": brief_text }),
            );
            task.meta_extra = Some(json!({ "designOption": { "group": group, "label": LABELS[i] } }));
            tasks.push(task);
        }
        let results = join_all(designers.iter().zip(tasks).map(|(designer, task)| self.run_agent(turn, designer, task))).await;
        let mut options = Vec::new();
        for (i, result) in results.into_iter().enumerate() {
            match result {
                Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                Ok(m) if m.status == MessageStatus::Done => {
                    options.push(json!({ "label": LABELS[i], "agentId": designers[i].id, "messageId": m.id }));
                }
                _ => {}
            }
        }
        if options.len() < 2 {
            return Ok(false);
        }

        let mut resume = d.clone();
        resume.targets = team.iter().enumerate().filter(|(i, _)| *i > pair.0 && *i != pair.1).map(|(_, a)| a.id.clone()).collect();
        let text = format!("Two design ideas are ready: **{}** (A) and **{}** (B). Pick one, or merge them.", designers[0].name, designers[1].name);
        self.post(
            &turn.chat.id,
            &director.id,
            MessageKind::Text,
            &text,
            json!({ "designChoice": { "group": group, "state": "pending", "options": options, "resume": { "decision": resume, "userMessageId": turn.user_msg.id } } }),
        )?;
        Ok(true)
    }

    /// The user's answer to a design card: `a` or `b` picks that idea, `merge` has a designer write one design from both
    /// (following `note`, if there is one). The team behind the designers then carries on with the chosen design.
    pub async fn choose_design(self: &Arc<Self>, message_id: &str, choice: &str, note: Option<&str>) -> Result<()> {
        let mut card = self.db.get_message(message_id)?;
        let choice = choice.trim().to_lowercase();
        if !matches!(choice.as_str(), "a" | "b" | "merge") {
            return Err(CoreError::invalid("choose a, b or merge"));
        }
        let state = card.meta["designChoice"]["state"].as_str().ok_or_else(|| CoreError::invalid("that message is not a design choice"))?;
        if state != "pending" {
            return Ok(());
        }
        let note = note.map(str::trim).filter(|n| !n.is_empty()).map(String::from);
        card.meta["designChoice"]["state"] = json!("decided");
        card.meta["designChoice"]["picked"] = json!(choice);
        card.meta["designChoice"]["note"] = json!(note);
        self.save_message(&card)?;

        let chat = self.db.get_chat(&card.chat_id)?;
        let core = self.clone();
        tokio::spawn(async move {
            let token = core.chat_token(&chat.id);
            if let Err(e) = core.resume_after_design(&chat, card, &choice, note, &token).await {
                if matches!(e, CoreError::Cancelled) {
                    core.settle_cancelled(&chat.id);
                } else {
                    core.pipeline_error(&chat.id, &e);
                }
            }
        });
        Ok(())
    }

    async fn resume_after_design(self: &Arc<Self>, chat: &Chat, card: Message, choice: &str, note: Option<String>, token: &CancellationToken) -> Result<()> {
        let dc = &card.meta["designChoice"];
        let group = dc["group"].as_str().unwrap_or_default().to_string();
        let options = dc["options"].as_array().cloned().unwrap_or_default();
        let decision: Decision = serde_json::from_value(dc["resume"]["decision"].clone()).map_err(|_| CoreError::invalid("this design choice cannot be resumed"))?;

        let all_agents = self.db.list_agents()?;
        let members: Vec<Agent> = all_agents.iter().filter(|a| chat.agent_ids.contains(&a.id)).cloned().collect();
        let director = members.iter().find(|a| a.specialty == "director").cloned().ok_or_else(|| CoreError::invalid("the director is not in this chat any more"))?;
        let roger = self.agent_by_slug("roger")?;
        let lincoln = self.agent_by_slug("lincoln")?;
        let user_msg = dc["resume"]["userMessageId"].as_str().and_then(|id| self.db.get_message(id).ok()).or_else(|| {
            self.db.list_messages(&chat.id, 50).ok().and_then(|m| m.into_iter().rev().find(|m| m.agent_id.is_none()))
        });
        let user_msg = user_msg.ok_or_else(|| CoreError::invalid("the request behind these designs is gone"))?;
        let prefs = tokio::select! {
            p = self.select_preferences(&roger, &user_msg.content) => p,
            _ = token.cancelled() => return Err(CoreError::Cancelled),
        };
        let turn = Turn { chat: chat.clone(), members, all_agents, user_msg, prefs, token: token.clone() };

        let option = |label: &str| options.iter().find(|o| o["label"].as_str() == Some(label)).cloned();
        let name_of = |o: &Value| turn.all_agents.iter().find(|a| o["agentId"].as_str() == Some(a.id.as_str())).map(|a| a.name.clone()).unwrap_or_default();

        let extra = if choice == "merge" {
            let first = option("A").ok_or_else(|| CoreError::invalid("design A is gone"))?;
            let merger = turn.all_agents.iter().find(|a| first["agentId"].as_str() == Some(a.id.as_str())).cloned().ok_or_else(|| CoreError::invalid("the designer is gone"))?;
            let wish = note.clone().unwrap_or_else(|| "take the best of both".into());
            self.record_choice(&chat.id, &format!("Merge the two designs: {wish}"), &group)?;
            let mut task = AgentTask::new(
                Mode::Answer,
                Some(format!(
                    "The user wants the two design ideas above merged into ONE design. Their note: {wish}\n\nWrite the merged design in the same shape as your own idea (Concept, Structure, Look and feel, Key details, Trade-offs), and say in one line what you took from each."
                )),
                false,
                json!({ "via": "director", "mode": "answer" }),
            );
            task.meta_extra = Some(json!({ "designOption": { "group": group, "label": "merged" } }));
            let merged = self.run_agent(&turn, &merger, task).await?;
            if merged.status != MessageStatus::Done {
                return Ok(());
            }
            if let Ok(mut c) = self.db.get_message(&card.id) {
                c.meta["designChoice"]["mergedMessageId"] = json!(merged.id);
                self.save_message(&c)?;
            }
            format!("The user chose the merged design above, written by {}. Build on that design only.", merger.name)
        } else {
            let label = choice.to_uppercase();
            let picked = option(&label).ok_or_else(|| CoreError::invalid("that design is gone"))?;
            let who = name_of(&picked);
            self.record_choice(&chat.id, &format!("I pick design {label} ({who}'s)."), &group)?;
            format!("The user picked design {label}, by {who} (above). Build on that design and leave the other one aside.")
        };

        if decision.targets.is_empty() {
            return Ok(());
        }
        self.dispatch(&turn, &decision, &lincoln, &roger, Some(&director), Some(&extra)).await
    }

    /// The user's choice, as a message from them in the chat, so everyone who works next reads it in the history.
    fn record_choice(&self, chat_id: &str, text: &str, group: &str) -> Result<()> {
        let mut m = self.new_message(chat_id, None, MessageKind::Text, MessageStatus::Done, text);
        m.meta = json!({ "designPick": group });
        self.insert_message(&m)
    }
}
