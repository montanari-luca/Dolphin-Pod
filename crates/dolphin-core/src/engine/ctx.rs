//! Engine side of context handling: choosing budgets, building the chat history (with persisted
//! compaction) and keeping a running request inside the model's window. The algorithms live in
//! `crate::context`; this file connects them to the database, events and providers.

use super::*;
use crate::context::{self, ContextSettings, RunContext, Summarizer, compact, tokens, window};

/// Summary state stored on the latest `compaction` notice of a chat.
struct SavedSummary {
    text: String,
    up_to_created_at: i64,
    up_to_id: String,
}

impl Core {
    /// The model that writes summaries: the utility model if one is set, else the agent's own.
    /// Summaries need no thinking, so effort stays off.
    pub(super) fn summarizer_for(&self, agent: &Agent, cfg: &ContextSettings) -> Option<Summarizer> {
        let m = settings::get_model(&self.db, keys::UTILITY_MODEL)
            .filter(|m| self.provider(&m.provider_id).is_ok())
            .or_else(|| agent.model.clone())?;
        let (pcfg, provider) = self.provider(&m.provider_id).ok()?;
        let win = window::resolve(&m, pcfg.kind, cfg.local_window);
        Some(Summarizer {
            provider,
            base: ChatRequest { model: m.model_id, effort: None, want_reasoning: false, context_window: Some(win), ..Default::default() },
            limit: context::Budget::new(win, 90).limit,
        })
    }

    fn saved_summary(&self, chat_id: &str) -> Option<SavedSummary> {
        let id = self.db.find_notice(chat_id, "compaction").ok()??;
        let m = self.db.get_message(&id).ok()?;
        let c = &m.meta["compaction"];
        Some(SavedSummary {
            text: c["summary"].as_str()?.to_string(),
            up_to_created_at: c["upToCreatedAt"].as_i64()?,
            up_to_id: c["upToId"].as_str()?.to_string(),
        })
    }

    /// One history message as `agent` sees it: its own turns are assistant turns; everyone else is quoted.
    fn history_message(&self, turn: &Turn, agent: &Agent, m: &Message, send_files: bool, vision: bool) -> ChatMessage {
        match &m.agent_id {
            Some(id) if id == &agent.id => {
                let mut text = m.content.clone();
                for a in &m.attachments {
                    text.push_str(&format!("\n[generated image: {}]", a.name));
                }
                ChatMessage::assistant(text)
            }
            None => {
                let mut parts = vec![Part::Text(m.content.clone())];
                for a in &m.attachments {
                    if send_files {
                        parts.extend(attachments::to_parts(a, vision));
                    } else {
                        parts.push(Part::Text(format!("[earlier attachment: {}]", a.name)));
                    }
                }
                ChatMessage { role: Role::User, parts, tool_calls: vec![], tool_call_id: None, tool_name: None, raw: None }
            }
            Some(_) => ChatMessage::user(format!("[{}]: {}", self.author_name(&turn.all_agents, m), m.content)),
        }
    }

    /// Chat history as seen by `agent`. Messages already folded into the chat's summary are replaced
    /// by it; when the rest outgrows the model's window (or the message limit) the older part is
    /// summarized, the summary is saved, and the chat gets a notice.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn prepare_history(
        self: &Arc<Self>,
        turn: &Turn,
        agent: &Agent,
        rc: &mut RunContext,
        fixed_tokens: usize,
        exclude: &str,
        vision: bool,
        sum: Option<&Summarizer>,
    ) -> Result<Vec<ChatMessage>> {
        let cfg = rc.settings;
        let mut saved = self.saved_summary(&turn.chat.id);
        let boundary = saved.as_ref().map(|s| (s.up_to_created_at, s.up_to_id.clone()));
        let fetch = if cfg.auto_compact { 600 } else { cfg.history_messages };
        let mut msgs: Vec<Message> = self
            .db
            .list_messages(&turn.chat.id, fetch)?
            .into_iter()
            .filter(|m| m.id != exclude && !matches!(m.kind, MessageKind::Ack | MessageKind::Notice))
            .filter(|m| !(m.content.trim().is_empty() && m.attachments.is_empty()))
            .filter(|m| boundary.as_ref().is_none_or(|(at, id)| (m.created_at, m.id.as_str()) > (*at, id.as_str())))
            .collect();
        if !cfg.auto_compact && msgs.len() > cfg.history_messages {
            msgs.drain(..msgs.len() - cfg.history_messages);
        }

        let recent_users: Vec<String> = msgs.iter().filter(|m| m.agent_id.is_none()).rev().take(3).map(|m| m.id.clone()).collect();
        let mut built: Vec<ChatMessage> = msgs.iter().map(|m| self.history_message(turn, agent, m, recent_users.contains(&m.id), vision)).collect();

        if cfg.auto_compact && msgs.len() >= 2 {
            let summary_tokens = saved.as_ref().map_or(0, |s| tokens::text(&s.text));
            let total = fixed_tokens + summary_tokens + tokens::messages(&built);
            let over_count = msgs.len() > cfg.history_messages;
            if over_count || total > rc.budget.trigger {
                // Keep the newest messages that fit the target, but at most half the message limit.
                let max_summary = (rc.budget.target / 4).clamp(300, 2500);
                let room = rc.budget.target.saturating_sub(fixed_tokens + max_summary).max(1000);
                let mut keep = 0usize;
                let mut used = 0usize;
                for b in built.iter().rev() {
                    let t = tokens::message(b);
                    if keep >= 1 && used + t > room || keep >= (cfg.history_messages / 2).max(2) {
                        break;
                    }
                    used += t;
                    keep += 1;
                }
                let cut = msgs.len().saturating_sub(keep.max(1));
                if cut >= 1 {
                    let before = total;
                    let started = Instant::now();
                    self.activity(&turn.chat.id, &agent.id, AgentActivity::Thinking, Some("Compacting the conversation".into()));
                    let entries = compact::render_entries(&built[..cut]);
                    let prev = saved.as_ref().map(|s| s.text.clone());
                    let s = tokio::select! {
                        s = compact::summarize(sum, prev.as_deref(), &entries, max_summary) => s,
                        _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
                    };
                    self.activity(&turn.chat.id, &agent.id, AgentActivity::Thinking, None);
                    if s.usage.input_tokens + s.usage.output_tokens > 0 {
                        self.record_internal_usage(&agent.id, &s.usage, started)?;
                    }
                    let last = &msgs[cut - 1];
                    let (up_to_at, up_to_id) = (last.created_at, last.id.clone());
                    built.drain(..cut);
                    let after = fixed_tokens + tokens::text(&s.text) + tokens::messages(&built);
                    let roger = self.agent_by_slug("roger")?;
                    self.post(
                        &turn.chat.id,
                        &roger.id,
                        MessageKind::Notice,
                        &format!("🗜️ Compacted {cut} earlier message{} into a summary to keep {}'s context within its window (~{}k → ~{}k tokens).",
                            if cut == 1 { "" } else { "s" }, agent.name, before / 1000, after / 1000),
                        json!({ "notice": "compaction", "compaction": {
                            "summary": s.text, "upToCreatedAt": up_to_at, "upToId": up_to_id,
                            "messages": cut, "byModel": s.by_model, "tokensBefore": before, "tokensAfter": after,
                        }}),
                    )?;
                    rc.stats.compactions += 1;
                    rc.stats.compacted_messages += cut;
                    saved = Some(SavedSummary { text: s.text, up_to_created_at: up_to_at, up_to_id });
                }
            }
        }

        let mut out: Vec<ChatMessage> = Vec::with_capacity(built.len() + 1);
        if let Some(s) = &saved {
            out.push(ChatMessage::user(compact::summary_message(&s.text)));
        }
        out.extend(built);
        if out.first().is_some_and(|m| m.role != Role::User) {
            out.insert(0, ChatMessage::user("(earlier conversation)"));
        }
        Ok(out)
    }

    /// Keep a running request inside the window, announcing compactions. `force` after a provider
    /// said the prompt was too long.
    #[allow(clippy::too_many_arguments)]
    pub(super) async fn fit_context(
        &self,
        turn: &Turn,
        agent: &Agent,
        rc: &mut RunContext,
        req: &mut ChatRequest,
        run_start: &mut usize,
        volatile: usize,
        sum: Option<&Summarizer>,
        force: bool,
    ) -> Result<()> {
        let started = Instant::now();
        let progress = |what: &str| self.activity(&turn.chat.id, &agent.id, AgentActivity::Thinking, Some(what.to_string()));
        let out = tokio::select! {
            o = context::fit(rc, req, run_start, sum, force, &progress) => o,
            _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
        };
        req.stable_messages = Some(run_start.saturating_sub(volatile));
        if out.compactions.is_empty() {
            return Ok(());
        }
        self.activity(&turn.chat.id, &agent.id, AgentActivity::Thinking, None);
        if out.usage.input_tokens + out.usage.output_tokens > 0 {
            self.record_internal_usage(&agent.id, &out.usage, started)?;
        }
        let roger = self.agent_by_slug("roger")?;
        for c in &out.compactions {
            let what = if c.scope == "history" { "the earlier conversation" } else { "its earlier steps" };
            self.post(
                &turn.chat.id,
                &roger.id,
                MessageKind::Notice,
                &format!("🗜️ {}'s context was getting full, so {what} ({} messages) {} summarized (~{}k → ~{}k tokens).",
                    agent.name, c.messages, if c.messages == 1 { "was" } else { "were" }, c.tokens_before / 1000, c.tokens_after / 1000),
                json!({ "notice": "context", "scope": c.scope, "byModel": c.by_model }),
            )?;
        }
        Ok(())
    }
}
