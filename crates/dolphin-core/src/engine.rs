//! The chat pipeline: user message → (@mention | Roger's ack → moderator → Lincoln → specialists),
//! agent runs with streaming, tool loops, permissions, supervision and usage accounting.

use std::sync::Arc;
use std::time::{Duration, Instant};

use futures::StreamExt;
use serde_json::{Value, json};
use tokio_util::sync::CancellationToken;

use crate::attachments;
use crate::context::{self, ContextSettings, RETRIEVE_TOOL, RunContext, tokens, window};
use crate::core::{Core, QuestionAnswer};
use crate::error::{CoreError, Result};
use crate::events::{AgentActivity, CoreEvent};
use crate::model::*;
use crate::permissions;
use crate::providers::{ChatMessage, ChatRequest, Part, Provider, ProviderEvent, Role, ToolCall, image};
use crate::router::moderator::{self, ModeratorInput};
use crate::router::{Decision, Mode, ack, mention, orchestrator, prompts, settings_ctl};
use crate::settings::{self, keys};
use crate::tools;
use crate::util::{now_ms, truncate};

mod changes;
mod ctx;
mod delegate;
mod git;
mod guard;
mod learning;

const MAX_TOOL_STEPS: usize = 40;
/// How often one run may recover from a provider rejecting the prompt as too long.
const MAX_OVERFLOW_RETRIES: u32 = 2;
const FLUSH_EVERY: Duration = Duration::from_millis(40);

/// Everything one user turn needs, gathered once.
struct Turn {
    chat: Chat,
    members: Vec<Agent>,
    all_agents: Vec<Agent>,
    user_msg: Message,
    prefs: Vec<String>,
    token: CancellationToken,
}

struct AgentTask {
    mode: Mode,
    /// Lincoln's orders + brief, appended after the history.
    brief: Option<String>,
    plan: bool,
    routing: Value,
    /// Set for a nested run: the permission level of the agent that delegated (the delegate never exceeds it).
    level_override: Option<PermissionLevel>,
    /// Set for a nested run: the name of the delegating agent. Delegates cannot delegate further.
    delegated_by: Option<String>,
}

impl AgentTask {
    fn new(mode: Mode, brief: Option<String>, plan: bool, routing: Value) -> Self {
        Self { mode, brief, plan, routing, level_override: None, delegated_by: None }
    }
}

/// Specialties that may hand work to a colleague, and the specialties they may hand it to.
const DELEGATORS: &[&str] = &["coder", "long_horizon", "debug", "security", "docs"];
const DELEGATES: &[&str] = &["git", "verifier", "researcher"];
const MAX_DELEGATIONS: usize = 3;

type ModelHandle = (Arc<dyn Provider>, ChatRequest);

impl Core {
    // ---------------------------------------------------------------- entry points

    pub async fn send_message(self: &Arc<Self>, chat_id: &str, text: &str, attachment_ids: Vec<String>) -> Result<Message> {
        self.send_message_with_meta(chat_id, text, attachment_ids, json!({})).await
    }

    pub async fn send_message_with_meta(
        self: &Arc<Self>,
        chat_id: &str,
        text: &str,
        attachment_ids: Vec<String>,
        meta: Value,
    ) -> Result<Message> {
        let chat = self.db.get_chat(chat_id)?;
        if text.trim().is_empty() && attachment_ids.is_empty() {
            return Err(CoreError::invalid("the message is empty"));
        }
        let agents = self.db.list_agents()?;
        let mut msg = self.new_message(&chat.id, None, MessageKind::Text, MessageStatus::Done, text.trim_end());
        msg.mentions = mention::parse(text, &agents);
        msg.meta = meta;
        for id in &attachment_ids {
            msg.attachments.push(self.db.get_attachment(id)?);
        }
        self.insert_message(&msg)?;
        let core = self.clone();
        let user_msg = msg.clone();
        tokio::spawn(async move { core.pipeline(chat, user_msg).await });
        Ok(msg)
    }

    /// Approve a plan (button). Lincoln escalates the planner to full permissions and starts the work.
    /// Run a message that another device asked for. The sender already shows the message under `message_id`
    /// (possibly already synced here), so that id is reused. The returned handle finishes with the pipeline.
    pub(crate) async fn send_message_remote(
        self: &Arc<Self>,
        chat_id: &str,
        text: &str,
        message_id: &str,
        meta: Value,
    ) -> Result<tokio::task::JoinHandle<()>> {
        let chat = self.db.get_chat(chat_id)?;
        let agents = self.db.list_agents()?;
        self.remote.begin(&chat.id);
        let msg = match self.db.get_message(message_id) {
            Ok(m) => m,
            Err(_) => {
                let mut m = self.new_message(&chat.id, None, MessageKind::Text, MessageStatus::Done, text.trim_end());
                m.id = message_id.to_string();
                m.mentions = mention::parse(text, &agents);
                m.meta = meta;
                if let Err(e) = self.insert_message(&m) {
                    self.remote.end(&chat.id);
                    return Err(e);
                }
                m
            }
        };
        let core = self.clone();
        Ok(tokio::spawn(async move {
            core.clone().pipeline(chat.clone(), msg).await;
            // Let the last updates sync before the flag drops.
            tokio::time::sleep(Duration::from_secs(3)).await;
            core.remote.end(&chat.id);
        }))
    }

    pub async fn approve_plan(self: &Arc<Self>, message_id: &str) -> Result<()> {
        let plan = self.db.get_message(message_id)?;
        if plan.kind != MessageKind::Plan {
            return Err(CoreError::invalid("that message is not a plan"));
        }
        if plan.meta["plan"]["approved"].as_bool() == Some(true) {
            return Ok(());
        }
        // What gets implemented is what was shown: the hash was taken when the plan finished.
        if let Some(h) = plan.meta["plan"]["hash"].as_str()
            && h != plan_hash(&plan.content)
        {
            return Err(CoreError::invalid("this plan changed after it was shown, so it cannot be approved. Ask for the plan again."));
        }
        let chat = self.db.get_chat(&plan.chat_id)?;
        let core = self.clone();
        tokio::spawn(async move {
            let token = core.chat_token(&chat.id);
            if let Err(e) = core.run_approved_plan(&chat, plan, &token).await {
                if matches!(e, CoreError::Cancelled) {
                    core.settle_cancelled(&chat.id);
                } else {
                    core.pipeline_error(&chat.id, &e);
                }
            }
        });
        Ok(())
    }

    /// Explicit grant from the UI (permission badge).
    pub fn grant_permission(&self, chat_id: &str, agent_id: &str, level: PermissionLevel) -> Result<PermissionGrant> {
        let reason = if level == PermissionLevel::Full { "granted by you" } else { "set by you" };
        self.change_grant(chat_id, agent_id, level, reason)
    }

    // ---------------------------------------------------------------- pipeline

    async fn pipeline(self: Arc<Self>, chat: Chat, user_msg: Message) {
        let token = self.chat_token(&chat.id);
        // A correction is judged against the answer it replies to, so it is noted before this turn adds new ones.
        self.note_correction(&chat, &user_msg);
        if let Err(e) = self.pipeline_inner(chat.clone(), user_msg.clone(), token).await {
            if matches!(e, CoreError::Cancelled) {
                self.settle_cancelled(&chat.id);
            } else {
                self.pipeline_error(&chat.id, &e);
            }
        }
        // Whoever answered a "remember that..." learns it; then the learner gets to work in the background.
        self.note_remember(&chat, &user_msg);
        self.schedule_learning();
    }

    /// After a stop: every agent in the chat goes idle and a half-finished routing ack is closed,
    /// so the UI stops showing the chat as busy.
    pub(crate) fn settle_cancelled(&self, chat_id: &str) {
        if let Ok(agents) = self.db.list_agents() {
            for a in agents {
                self.activity(chat_id, &a.id, AgentActivity::Idle, None);
            }
        }
        if let Ok(msgs) = self.db.list_messages(chat_id, 20) {
            for mut m in msgs {
                if m.kind == MessageKind::Ack && m.meta["routing"]["status"] == "routing" {
                    m.meta["routing"] = json!({ "status": "done", "stopped": true });
                    let _ = self.save_message(&m);
                }
            }
        }
    }

    fn pipeline_error(&self, chat_id: &str, e: &CoreError) {
        if let Ok(roger) = self.agent_by_slug("roger") {
            let _ = self.post(chat_id, &roger.id, MessageKind::Notice, &format!("Something went wrong: {e}"), json!({ "notice": "error" }));
        }
    }

    async fn pipeline_inner(self: &Arc<Self>, chat: Chat, user_msg: Message, token: CancellationToken) -> Result<()> {
        let all_agents = self.db.list_agents()?;
        let members: Vec<Agent> = all_agents.iter().filter(|a| chat.agent_ids.contains(&a.id)).cloned().collect();
        let roger = self.agent_by_slug("roger")?;
        let lincoln = self.agent_by_slug("lincoln")?;
        let prefs = tokio::select! {
            p = self.select_preferences(&roger, &user_msg.content) => p,
            _ = token.cancelled() => return Err(CoreError::Cancelled),
        };
        let turn = Turn { chat, members, all_agents, user_msg, prefs, token };

        // @mentions: straight to the agent, no moderator. Permissions still apply.
        let mentioned: Vec<Agent> =
            turn.user_msg.mentions.iter().filter_map(|id| turn.all_agents.iter().find(|a| &a.id == id).cloned()).collect();
        let mentioned: Vec<Agent> = mentioned.into_iter().filter(|a| a.id != lincoln.id).collect();
        if !mentioned.is_empty() {
            for agent in mentioned {
                if !turn.chat.agent_ids.contains(&agent.id) {
                    self.post(&turn.chat.id, &roger.id, MessageKind::Notice,
                        &format!("{} isn't in this chat. Add them in the chat settings to talk to them here.", agent.name), json!({}))?;
                    continue;
                }
                let mode = if agent.id == roger.id { Mode::Answer } else { Mode::Implement };
                let task = AgentTask::new(mode, None, false, json!({ "via": "mention" }));
                self.run_agent(&turn, &agent, task).await?;
            }
            return Ok(());
        }

        // 1. Instant acknowledgment, no model call.
        let mut ack_msg = self.post(&turn.chat.id, &roger.id, MessageKind::Ack, ack::random_ack(), json!({ "routing": { "status": "routing" } }))?;
        self.activity(&turn.chat.id, &roger.id, AgentActivity::Routing, None);

        // 2. Moderator reasoning.
        let history = self.db.list_messages(&turn.chat.id, 12)?;
        let pending_plan = history.iter().rev().find(|m| m.kind == MessageKind::Plan && m.status == MessageStatus::Done && m.meta["plan"]["approved"].as_bool() != Some(true)).cloned();
        let pending_by = pending_plan.as_ref().and_then(|p| p.agent_id.as_ref()).and_then(|id| turn.all_agents.iter().find(|a| &a.id == id)).map(|a| a.name.clone());
        let history_wo_ack: Vec<Message> = history.into_iter().filter(|m| m.kind != MessageKind::Ack && m.id != turn.user_msg.id).collect();
        let input = ModeratorInput {
            chat_name: &turn.chat.name,
            members: &turn.members,
            history: &history_wo_ack,
            user_text: &turn.user_msg.content,
            preferences: &turn.prefs,
            pending_plan_by: pending_by.as_deref(),
        };
        let roger_model = self.model_handle(&roger, false).ok().map(|(p, mut req)| {
            req.system = moderator::system_prompt(&input, &roger.name);
            let names = |m: &Message| self.author_name(&turn.all_agents, m);
            req.messages = vec![ChatMessage::user(moderator::user_prompt(&input, &names))];
            req.json_mode = true;
            (p, req)
        });
        let started = Instant::now();
        let (decision, err, used) = tokio::select! {
            r = moderator::decide(roger_model.as_ref().map(|(p, r)| (p.as_ref(), r.clone())), &input) => r,
            _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
        };
        self.activity(&turn.chat.id, &roger.id, AgentActivity::Idle, None);
        if roger_model.is_some() {
            self.record_internal_usage(&roger.id, &used, started)?;
        }

        let approving = decision.approves_plan && pending_plan.is_some();
        let target_names: Vec<String> = if approving {
            // The plan's author implements it, whoever the moderator named.
            pending_by.iter().cloned().collect()
        } else {
            decision.targets.iter().filter_map(|id| turn.members.iter().find(|a| &a.id == id)).map(|a| a.name.clone()).collect()
        };
        ack_msg.meta = json!({ "routing": {
            "status": "done",
            "mode": if approving { "implement" } else { decision.mode.as_str() },
            "intent": if approving { "plan approved".to_string() } else { decision.intent.clone() },
            "targets": target_names,
            "source": decision.source, "needsSetup": self.model_for(&roger).is_none(), "error": err,
        }});
        self.save_message(&ack_msg)?;

        if approving && let Some(plan) = pending_plan {
            return self.run_approved_plan(&turn.chat, plan, &turn.token).await;
        }
        if let Some(note) = &decision.lean_note {
            self.post(&turn.chat.id, &roger.id, MessageKind::Text, &format!("💡 {note}"), json!({ "leanPrompt": true }))?;
        }

        match decision.mode {
            Mode::Clarify => {
                let q = decision.clarification.clone().unwrap_or_else(|| "Could you tell me a bit more about what you need?".into());
                self.post(&turn.chat.id, &roger.id, MessageKind::Text, &q, json!({ "clarification": true }))?;
            }
            Mode::Internal => {
                self.activity(&turn.chat.id, &roger.id, AgentActivity::Writing, Some("Updating settings".into()));
                let utility = self.utility_handle(&roger);
                let reply = settings_ctl::handle(self, &turn.chat, &turn.user_msg.content, utility.as_ref().map(|(p, r)| (p.as_ref(), r.clone()))).await;
                self.activity(&turn.chat.id, &roger.id, AgentActivity::Idle, None);
                if self.db.get_chat(&turn.chat.id).is_ok() {
                    self.post(&turn.chat.id, &roger.id, MessageKind::Text, &reply, json!({ "settings": true }))?;
                }
            }
            Mode::Answer => {
                let task = AgentTask::new(Mode::Answer, None, false, json!({ "via": "moderator", "mode": "answer" }));
                self.run_agent(&turn, &roger, task).await?;
            }
            mode => self.dispatch(&turn, &decision, mode, &lincoln, &roger).await?,
        }
        Ok(())
    }

    /// Lincoln: brief each target and run them in order (each sees the previous answers).
    async fn dispatch(self: &Arc<Self>, turn: &Turn, d: &Decision, mode: Mode, lincoln: &Agent, roger: &Agent) -> Result<()> {
        let targets: Vec<Agent> = d.targets.iter().filter_map(|id| turn.members.iter().find(|a| &a.id == id).cloned()).collect();
        let lincoln_model = self.model_handle(lincoln, false).or_else(|_| self.model_handle(roger, false)).ok();
        for agent in &targets {
            if turn.token.is_cancelled() {
                return Err(CoreError::Cancelled);
            }
            if mode == Mode::Plan {
                self.change_grant(&turn.chat.id, &agent.id, PermissionLevel::ReadOnly, "planning")?;
            }
            self.activity(&turn.chat.id, &lincoln.id, AgentActivity::Routing, Some(format!("Briefing {}", agent.name)));
            let recent = self.recent_context(turn, 6);
            let started = Instant::now();
            let (brief_text, used) = tokio::select! {
                b = orchestrator::brief(lincoln_model.as_ref().map(|(p, r)| (p.as_ref(), r.clone())), d, agent, &turn.user_msg.content, &recent) => b,
                _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
            };
            self.activity(&turn.chat.id, &lincoln.id, AgentActivity::Idle, None);
            if used.input_tokens + used.output_tokens > 0 {
                self.record_internal_usage(&lincoln.id, &used, started)?;
            }
            let others: Vec<&Agent> = targets.iter().filter(|a| a.id != agent.id).collect();
            let orders = orchestrator::orders(d, agent, &others);
            let task = AgentTask::new(
                mode,
                Some(format!("{orders}\n\nBrief:\n{brief_text}")),
                mode == Mode::Plan,
                json!({ "via": "lincoln", "mode": mode.as_str(), "rewrite": d.rewrite, "brief": brief_text }),
            );
            self.run_agent(turn, agent, task).await?;
        }
        Ok(())
    }

    async fn run_approved_plan(self: &Arc<Self>, chat: &Chat, mut plan: Message, token: &CancellationToken) -> Result<()> {
        let agent_id = plan.agent_id.clone().ok_or_else(|| CoreError::invalid("plan has no author"))?;
        let agent = self.db.get_agent(&agent_id)?;
        plan.meta["plan"]["approved"] = json!(true);
        plan.meta["plan"]["approvedAt"] = json!(now_ms());
        self.save_message(&plan)?;
        self.change_grant(&chat.id, &agent.id, PermissionLevel::Full, "plan approved")?;

        let all_agents = self.db.list_agents()?;
        let members = all_agents.iter().filter(|a| chat.agent_ids.contains(&a.id)).cloned().collect();
        let roger = self.agent_by_slug("roger")?;
        let prefs = tokio::select! {
            p = self.select_preferences(&roger, &plan.content) => p,
            _ = token.cancelled() => return Err(CoreError::Cancelled),
        };
        let user_msg = self.db.list_messages(&chat.id, 50)?.into_iter().rev().find(|m| m.agent_id.is_none()).unwrap_or_else(|| plan.clone());
        let turn = Turn { chat: chat.clone(), members, all_agents, user_msg, prefs, token: token.clone() };
        let brief = format!(
            "Lincoln (orchestrator) → {}: the user approved your plan above. You now have full agentic permissions in this chat. Implement the plan step by step, verify your work, then summarize what you changed.",
            agent.name
        );
        let task = AgentTask::new(Mode::Implement, Some(brief), false, json!({ "via": "lincoln", "mode": "implement", "planApproved": plan.id, "planHash": plan_hash(&plan.content) }));
        self.run_agent(&turn, &agent, task).await.map(|_| ())
    }

    /// Change a grant; every change is announced in the chat by Lincoln.
    fn change_grant(&self, chat_id: &str, agent_id: &str, level: PermissionLevel, reason: &str) -> Result<PermissionGrant> {
        let before = self.db.grant(chat_id, agent_id)?;
        let g = self.db.set_grant(chat_id, agent_id, level, reason)?;
        self.emit(CoreEvent::GrantChanged { grant: g.clone() });
        if before.level != level || before.updated_at == 0 && level >= PermissionLevel::Ask {
            let agent = self.db.get_agent(agent_id)?;
            let lincoln = self.agent_by_slug("lincoln")?;
            let text = match level {
                PermissionLevel::Full => format!("🔓 {} now has **full agentic permissions** in this chat ({reason}).", agent.name),
                PermissionLevel::Ask => format!("🙋 {} will **ask before** changing files or running commands in this chat ({reason}).", agent.name),
                PermissionLevel::ReadOnly => format!("🔒 {} is **read-only** in this chat ({reason}).", agent.name),
            };
            let chat = self.db.get_chat(chat_id)?;
            let extra = if level >= PermissionLevel::Ask && chat.workspace_path.is_none() {
                " Set a workspace folder in the chat settings so they can work on your files."
            } else {
                ""
            };
            self.post(chat_id, &lincoln.id, MessageKind::Notice, &format!("{text}{extra}"), json!({ "notice": "permission", "level": level }))?;
        }
        Ok(g)
    }

    /// Usage for internal work (Roger's routing, Lincoln's briefs) counts toward those agents too.
    fn record_internal_usage(&self, agent_id: &str, used: &crate::providers::Usage, started: Instant) -> Result<()> {
        let ms = started.elapsed().as_millis() as i64;
        self.db.record_usage(agent_id, used.input_tokens, used.output_tokens, 0, ms)?;
        self.emit(CoreEvent::UsageUpdated { usage: self.db.usage_for(agent_id)? });
        Ok(())
    }

    // ---------------------------------------------------------------- model plumbing

    fn model_handle(&self, agent: &Agent, want_reasoning: bool) -> Result<ModelHandle> {
        let m = self.model_for(agent).ok_or_else(|| CoreError::invalid(self.no_model_reason(agent)))?;
        let (_, provider) = self.provider(&m.provider_id)?;
        Ok((provider, ChatRequest { model: m.model_id.clone(), effort: m.effort.clone(), want_reasoning, ..Default::default() }))
    }

    /// The utility model (cheap internal tasks), falling back to Roger's model.
    fn utility_handle(&self, roger: &Agent) -> Option<ModelHandle> {
        if let Some(m) = settings::get_model(&self.db, keys::UTILITY_MODEL)
            && let Ok((_, p)) = self.provider(&m.provider_id)
        {
            return Some((p, ChatRequest { model: m.model_id, effort: m.effort, ..Default::default() }));
        }
        self.model_handle(roger, false).ok()
    }

    async fn select_preferences(&self, roger: &Agent, query: &str) -> Vec<String> {
        let text = settings::get_string(&self.db, keys::CUSTOM_INSTRUCTIONS);
        if text.trim().is_empty() {
            return vec![];
        }
        let utility = if text.len() >= crate::preferences::SEND_ALL_BELOW { self.utility_handle(roger) } else { None };
        crate::preferences::select(utility.as_ref().map(|(p, r)| (p.as_ref(), r.clone())), &text, query).await
    }

    /// What this agent remembers about the user that applies to this request, ready to send after the history.
    /// Counting the entries as used is best effort: a failure here must never stop a run.
    fn memory_message(&self, agent: &Agent, chat: &Chat, query: &str) -> Option<String> {
        let all = self.db.list_memories(&agent.id).ok()?;
        let project = crate::memory::project_of(chat.workspace_path.as_deref());
        let picked = crate::memory::select(&all, project.as_deref(), query);
        let text = crate::memory::block(&picked)?;
        let ids: Vec<String> = picked.iter().map(|m| m.id.clone()).collect();
        let _ = self.db.bump_memory_hits(&ids);
        Some(text)
    }

    fn author_name(&self, agents: &[Agent], m: &Message) -> String {
        match &m.agent_id {
            None => {
                let n = settings::get_string(&self.db, keys::DISPLAY_NAME);
                if n.trim().is_empty() || n.eq_ignore_ascii_case("you") { "User".into() } else { n }
            }
            Some(id) => agents.iter().find(|a| &a.id == id).map(|a| a.name.clone()).unwrap_or_else(|| "Agent".into()),
        }
    }

    fn recent_context(&self, turn: &Turn, n: usize) -> String {
        let msgs = self.db.list_messages(&turn.chat.id, n + 2).unwrap_or_default();
        let kept: Vec<&Message> =
            msgs.iter().filter(|m| m.kind != MessageKind::Ack && m.kind != MessageKind::Notice && m.id != turn.user_msg.id).collect();
        kept[kept.len().saturating_sub(n)..]
            .iter()
            .map(|m| format!("{}: {}", self.author_name(&turn.all_agents, m), truncate(&m.content.replace('\n', " "), 400)))
            .collect::<Vec<_>>()
            .join("\n")
    }

    // ---------------------------------------------------------------- agent run

    async fn run_agent(self: &Arc<Self>, turn: &Turn, agent: &Agent, task: AgentTask) -> Result<Message> {
        let kind = if task.plan { MessageKind::Plan } else { MessageKind::Text };
        let mut msg = self.new_message(&turn.chat.id, Some(&agent.id), kind, MessageStatus::Streaming, "");
        let show_brief = settings::get_bool(&self.db, keys::SHOW_BRIEFS);
        msg.meta = json!({ "routing": if show_brief { task.routing.clone() } else { json!({ "mode": task.mode.as_str() }) }, "tools": [] });
        if task.plan {
            msg.meta["plan"] = json!({ "approved": false });
        }
        self.insert_message(&msg)?;
        self.activity(&turn.chat.id, &agent.id, AgentActivity::Thinking, None);

        let started = Instant::now();
        let mut stats = RunStats::default();
        let result = self.run_agent_inner(turn, agent, &task, &mut msg, &mut stats).await;
        self.finish_checkpoint(&turn.chat.id, &stats, &mut msg).await;
        msg.working_ms = Some(started.elapsed().as_millis() as i64);
        msg.thinking_ms = stats.thinking_ms(started);
        match &result {
            Ok(()) => msg.status = MessageStatus::Done,
            Err(CoreError::Cancelled) => {
                msg.status = MessageStatus::Cancelled;
                if msg.content.is_empty() {
                    msg.content = "_Stopped._".into();
                }
            }
            Err(e) => {
                msg.status = MessageStatus::Error;
                msg.error = Some(e.to_string());
                if matches!(e, CoreError::Invalid(s) if s.contains("has no model yet")) {
                    msg.meta["needsSetup"] = json!(true);
                    msg.error = Some(format!("I'm not set up yet. Choose a provider and model for me in Settings → Agents → {}.", agent.name));
                }
            }
        }
        if msg.status == MessageStatus::Done && msg.content.trim().is_empty() && msg.attachments.is_empty() {
            msg.content = "_(no response)_".into();
        }
        if task.plan && msg.status == MessageStatus::Done {
            msg.meta["plan"]["hash"] = json!(plan_hash(&msg.content));
        }
        let (tin, tout) = stats.tokens(&msg);
        msg.tokens_in = Some(tin);
        msg.tokens_out = Some(tout);
        self.save_message(&msg)?;
        self.activity(&turn.chat.id, &agent.id, AgentActivity::Idle, None);
        if stats.requests > 0 {
            self.db.record_usage(&agent.id, tin, tout, msg.thinking_ms.unwrap_or(0), msg.working_ms.unwrap_or(0))?;
            self.emit(CoreEvent::UsageUpdated { usage: self.db.usage_for(&agent.id)? });
        }
        match result {
            Err(CoreError::Cancelled) => Err(CoreError::Cancelled),
            _ => Ok(msg),
        }
    }

    async fn run_agent_inner(self: &Arc<Self>, turn: &Turn, agent: &Agent, task: &AgentTask, msg: &mut Message, stats: &mut RunStats) -> Result<()> {
        let model = self.model_for(agent).ok_or_else(|| CoreError::invalid(self.no_model_reason(agent)))?;
        let (cfg, provider) = self.provider(&model.provider_id)?;
        let workspace = turn.chat.workspace_path.clone().filter(|p| std::path::Path::new(p).is_dir());
        let level = match task.level_override {
            Some(l) => l,
            None => self.db.grant(&turn.chat.id, &agent.id)?.level,
        };
        let is_image_agent = agent.specialty == "image" && agent.image_model.is_some();

        let mut tool_specs = if model.tools { tools::specs(workspace.is_some(), level) } else { vec![] };
        // Web access for every agent (Roger included): OpenRouter runs its own search server-side,
        // everyone else gets the built-in web_search engine. Both can open pages with fetch_page.
        let web = model.tools && settings::get_bool(&self.db, keys::WEB_ACCESS);
        let native_search = web && cfg.kind == ProviderKind::Openrouter;
        if web {
            tool_specs.extend(tools::web_specs(native_search));
        }
        if is_image_agent && model.tools {
            tool_specs.push(tools::image_tool());
        }
        // Coding and agentic agents can hand work to Robert, Amelia and Luna (one level deep).
        let delegates: Vec<Agent> = if model.tools && task.delegated_by.is_none() && DELEGATORS.contains(&agent.specialty.as_str()) {
            turn.members.iter().filter(|m| DELEGATES.contains(&m.specialty.as_str()) && m.id != agent.id).cloned().collect()
        } else {
            vec![]
        };
        if !delegates.is_empty() {
            tool_specs.push(tools::delegate_spec(&delegates));
        }
        // Robert's git tool: needs a repository folder to work in.
        let has_git = agent.specialty == "git" && workspace.is_some() && model.tools;
        if has_git {
            tool_specs.push(crate::gittool::spec());
        }
        // The question tool needs someone to answer: not in automations, delegated runs or runs another device asked for.
        let can_ask = model.tools
            && task.delegated_by.is_none()
            && turn.user_msg.meta.get("automation").is_none()
            && !self.remote.is_active(&turn.chat.id);
        if can_ask {
            tool_specs.push(tools::ask_spec());
        }
        // Saving notes about the user is something the agent does while talking to them: not in automations or delegated runs.
        let can_remember = model.tools && agent.learning && task.delegated_by.is_none() && turn.user_msg.meta.get("automation").is_none();
        if can_remember {
            tool_specs.push(tools::remember_spec());
        }
        let ctx_cfg = ContextSettings::load(&self.db);
        let mut rc = RunContext::new(context::Budget::new(window::resolve(&model, cfg.kind, ctx_cfg.local_window), ctx_cfg.compact_at), ctx_cfg);
        if ctx_cfg.compress_tools && model.tools {
            tool_specs.push(context::compress::retrieve_spec());
        }
        let user_name = settings::get_string(&self.db, keys::DISPLAY_NAME).trim().to_string();
        let system = prompts::agent_system(&prompts::AgentPromptInput {
            agent,
            chat: &turn.chat,
            members: &turn.members,
            mode: task.mode,
            permission: level,
            has_image_tool: is_image_agent,
            has_web: web,
            has_ask: can_ask,
            has_remember: can_remember,
            delegates: &delegates,
            // Preferences change with every request; they go after the history so the system prompt and
            // the history stay identical between turns and provider prompt caches keep hitting.
            preferences: &[],
            user_name: Some(user_name.as_str()).filter(|n| !n.is_empty() && !n.eq_ignore_ascii_case("you")),
        });
        let summarizer = if ctx_cfg.auto_compact { self.summarizer_for(agent, &ctx_cfg) } else { None };
        let fixed_tokens = tokens::text(&system) + tokens::specs(&tool_specs);
        let mut messages = self.prepare_history(turn, agent, &mut rc, fixed_tokens, &msg.id, model.vision, summarizer.as_ref()).await?;
        let mut volatile = 0usize;
        if let Some(b) = &task.brief {
            messages.push(ChatMessage::user(b.clone()));
            volatile += 1;
        }
        if !turn.prefs.is_empty() {
            let list: String = turn.prefs.iter().map(|p| format!("- {}\n", p.trim())).collect();
            messages.push(ChatMessage::user(format!("The user's preferences that apply to this request:\n{list}")));
            volatile += 1;
        }
        if let Some(m) = self.memory_message(agent, &turn.chat, &turn.user_msg.content) {
            messages.push(ChatMessage::user(m));
            volatile += 1;
        }
        let mut run_start = messages.len();
        let mut req = ChatRequest {
            model: model.model_id.clone(),
            system,
            messages,
            tools: tool_specs,
            effort: model.effort.clone(),
            max_tokens: None,
            json_mode: false,
            web_search: native_search,
            want_reasoning: true,
            cache_key: Some(format!("dolphin-pod:{}:{}", turn.chat.id, agent.id)),
            stable_messages: Some(run_start.saturating_sub(volatile)),
            context_window: Some(rc.budget.window),
        };
        stats.input_chars = req.system.len() + req.messages.iter().map(|m| m.text_content().len()).sum::<usize>();

        let mut _permit = Some(tokio::select! {
            p = self.providers.semaphore(&cfg).acquire_owned() => p.map_err(|_| CoreError::Cancelled)?,
            _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
        });
        let mut delegations = 0usize;
        let stall = Duration::from_secs(settings::get_int(&self.db, keys::STALL_SECONDS).max(15) as u64);
        let roger = self.agent_by_slug("roger")?;
        let mut stalled_once = false;
        let mut last_sig: Option<(String, usize)> = None;
        let mut generated_image = false;
        let mut overflow_retries = 0u32;
        let mut finished = false;
        // "Allow file changes for this run": file writes and edits stop asking until the run ends.
        let mut edits_approved = false;
        // Set once the run has read web content: from then on changes need the user's approval (see settings TAINT_GUARD).
        let taint_guard = settings::get_bool(&self.db, keys::TAINT_GUARD);
        let mut tainted = false;

        for _step in 0..MAX_TOOL_STEPS {
            self.fit_context(turn, agent, &mut rc, &mut req, &mut run_start, volatile, summarizer.as_ref(), false).await?;
            stats.requests += 1;
            let opened = tokio::select! {
                s = provider.stream_chat(req.clone()) => s,
                _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
            };
            let mut stream = match opened {
                Ok(s) => s,
                // The provider says the prompt is too long: squeeze harder and try again.
                Err(CoreError::Provider(m)) if window::is_overflow_error(&m) && overflow_retries < MAX_OVERFLOW_RETRIES => {
                    overflow_retries += 1;
                    rc.stats.retries += 1;
                    self.fit_context(turn, agent, &mut rc, &mut req, &mut run_start, volatile, summarizer.as_ref(), true).await?;
                    continue;
                }
                Err(e) => return Err(e),
            };
            let mut calls: Vec<ToolCall> = Vec::new();
            let mut raw = None;
            let mut step_text = String::new();
            let mut pending = (String::new(), String::new());
            let mut last_flush = Instant::now();
            let mut last_event = Instant::now();
            let mut stalled = false;
            let mut first_text_in_step = true;
            let mut overflowed = false;

            loop {
                let wait = if pending.0.is_empty() && pending.1.is_empty() { Duration::from_secs(2) } else { FLUSH_EVERY };
                let next = tokio::select! {
                    n = tokio::time::timeout(wait, stream.next()) => n,
                    _ = turn.token.cancelled() => {
                        self.flush(msg, &mut pending);
                        return Err(CoreError::Cancelled);
                    }
                };
                let ev = match next {
                    Err(_) => {
                        self.flush(msg, &mut pending);
                        last_flush = Instant::now();
                        if last_event.elapsed() >= stall {
                            stalled = true;
                            break;
                        }
                        continue;
                    }
                    Ok(None) => break,
                    Ok(Some(ev)) => ev,
                };
                last_event = Instant::now();
                match ev {
                    ProviderEvent::ReasoningDelta(t) => {
                        stats.reasoning_started.get_or_insert_with(Instant::now);
                        if stats.text_started.is_none() {
                            self.activity(&turn.chat.id, &agent.id, AgentActivity::Thinking, None);
                        }
                        msg.reasoning.get_or_insert_with(String::new).push_str(&t);
                        pending.1.push_str(&t);
                    }
                    ProviderEvent::TextDelta(t) => {
                        if stats.text_started.is_none() {
                            stats.text_started = Some(Instant::now());
                            self.activity(&turn.chat.id, &agent.id, AgentActivity::Writing, None);
                        }
                        let mut t = t;
                        if first_text_in_step && !msg.content.is_empty() && !msg.content.ends_with("\n\n") {
                            t = format!("\n\n{}", t.trim_start());
                        }
                        first_text_in_step = false;
                        step_text.push_str(&t);
                        msg.content.push_str(&t);
                        pending.0.push_str(&t);
                    }
                    ProviderEvent::ToolCall(c) => calls.push(c),
                    ProviderEvent::Usage(u) => {
                        stats.tokens_in += u.input_tokens;
                        stats.tokens_out += u.output_tokens;
                        rc.calibrate(u.input_tokens);
                    }
                    ProviderEvent::CacheUsage { read, written } => {
                        rc.stats.cache_read += read;
                        rc.stats.cache_written += written;
                    }
                    ProviderEvent::Done { raw_assistant, .. } => raw = raw_assistant,
                    ProviderEvent::Error(e) => {
                        self.flush(msg, &mut pending);
                        if window::is_overflow_error(&e) && overflow_retries < MAX_OVERFLOW_RETRIES && step_text.is_empty() && calls.is_empty() {
                            overflowed = true;
                            break;
                        }
                        return Err(CoreError::Provider(e));
                    }
                }
                if last_flush.elapsed() >= FLUSH_EVERY {
                    self.flush(msg, &mut pending);
                    last_flush = Instant::now();
                }
            }
            self.flush(msg, &mut pending);

            if overflowed {
                overflow_retries += 1;
                rc.stats.retries += 1;
                self.fit_context(turn, agent, &mut rc, &mut req, &mut run_start, volatile, summarizer.as_ref(), true).await?;
                continue;
            }

            if stalled {
                // Roger checks on a silent agent once; a second stall ends the run.
                self.activity(&turn.chat.id, &agent.id, AgentActivity::Stuck, None);
                if stalled_once {
                    return Err(CoreError::provider(format!("{} stopped responding", agent.name)));
                }
                stalled_once = true;
                self.post(&turn.chat.id, &roger.id, MessageKind::Notice,
                    &format!("{} has been quiet for {}s. I'm checking in on them.", agent.name, stall.as_secs()), json!({ "notice": "stuck" }))?;
                let mut partial = ChatMessage::assistant(step_text.clone());
                partial.raw = None;
                if !step_text.trim().is_empty() {
                    req.messages.push(partial);
                }
                req.messages.push(ChatMessage::user(format!(
                    "[{} (moderator)]: You have been silent for a while. Briefly tell the user what is going on, then continue or wrap up.",
                    roger.name
                )));
                continue;
            }

            if rc.stats.is_interesting() {
                msg.meta["context"] = rc.stats.to_json();
            }
            if calls.is_empty() {
                finished = true;
                break;
            }

            // Tool turn: replay the native assistant turn, then the results.
            self.activity(&turn.chat.id, &agent.id, AgentActivity::UsingTool, Some(tools::summarize(&calls[0].name, &calls[0].arguments)));
            req.messages.push(ChatMessage { role: Role::Assistant, parts: vec![Part::Text(step_text.clone())], tool_calls: calls.clone(), tool_call_id: None, tool_name: None, raw });
            // Web lookups in one step are independent and read-only: run them at once (the results are still
            // logged and handed back in call order below).
            let web_idx: Vec<usize> = (0..calls.len()).filter(|i| tools::is_web_tool(&calls[*i].name)).collect();
            let mut web_results: std::collections::HashMap<usize, Result<String>> = std::collections::HashMap::new();
            if web_idx.len() > 1 {
                self.activity(&turn.chat.id, &agent.id, AgentActivity::UsingTool, Some(format!("Running {} web lookups at once", web_idx.len())));
                let batch: Vec<(&str, &Value)> = web_idx.iter().map(|i| (calls[*i].name.as_str(), &calls[*i].arguments)).collect();
                let results = tokio::select! {
                    r = futures::future::join_all(batch.iter().map(|(n, a)| self.run_web(n, a))) => r,
                    _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
                };
                web_results.extend(web_idx.iter().copied().zip(results));
            }
            for (call_idx, call) in calls.iter().enumerate() {
                let sig = format!("{}:{}", call.name, call.arguments);
                let repeats = match &last_sig { Some((s, n)) if *s == sig => n + 1, _ => 1 };
                last_sig = Some((sig, repeats));
                if repeats >= 3 {
                    self.post(&turn.chat.id, &roger.id, MessageKind::Notice,
                        &format!("{} seems to be stuck repeating “{}”. I stopped them; try rephrasing or giving more detail.", agent.name, tools::summarize(&call.name, &call.arguments)),
                        json!({ "notice": "loop" }))?;
                    return Ok(());
                }
                if call.name == RETRIEVE_TOOL {
                    // Served from memory: no workspace, no permission, nothing to log.
                    self.activity(&turn.chat.id, &agent.id, AgentActivity::UsingTool, Some(tools::summarize(&call.name, &call.arguments)));
                    req.messages.push(ChatMessage::tool_result(call, tools::untrusted("an earlier tool result", &rc.store.retrieve(&call.arguments))));
                    continue;
                }
                let level = match task.level_override {
                    Some(l) => l,
                    None => self.db.grant(&turn.chat.id, &agent.id)?.level,
                };
                let summary = tools::summarize(&call.name, &call.arguments);
                // Text the app itself wrote (a git confirmation request) is an instruction to the agent, not data.
                let mut app_text = false;
                let result = if call.name == "generate_image" {
                    generated_image = true;
                    tokio::select! {
                        r = self.generate_image(agent, msg, call.arguments["prompt"].as_str().unwrap_or_default()) => r,
                        _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
                    }
                } else if call.name == tools::REMEMBER_TOOL {
                    if can_remember { self.remember_tool(agent, &turn.chat, &call.arguments) } else { Err(CoreError::invalid("unknown tool 'remember'")) }
                } else if call.name == tools::DELEGATE_TOOL {
                    delegations += 1;
                    if delegates.is_empty() {
                        Err(CoreError::invalid("unknown tool 'delegate_to_agent'"))
                    } else if delegations > MAX_DELEGATIONS {
                        Err(CoreError::invalid(format!("you already delegated {MAX_DELEGATIONS} times in this run; finish the work yourself")))
                    } else {
                        // The delegate needs a provider slot of its own; hold none while waiting for it.
                        _permit = None;
                        let r = tokio::select! {
                            r = self.delegate(turn, agent, level, &delegates, &call.arguments) => r,
                            _ = turn.token.cancelled() => Err(CoreError::Cancelled),
                        };
                        _permit = Some(tokio::select! {
                            p = self.providers.semaphore(&cfg).acquire_owned() => p.map_err(|_| CoreError::Cancelled)?,
                            _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
                        });
                        match r {
                            Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                            // What a colleague read on the web taints the work they hand back.
                            Ok((text, delegate_tainted)) => {
                                tainted |= delegate_tainted;
                                Ok(text)
                            }
                            Err(e) => Err(e),
                        }
                    }
                } else if call.name == tools::ASK_TOOL {
                    match tools::parse_question(&call.arguments) {
                        Ok(_) if !can_ask => Err(CoreError::invalid("unknown tool 'ask_user'")),
                        Ok((question, options)) => {
                            self.activity(&turn.chat.id, &agent.id, AgentActivity::UsingTool, Some("Waiting for your answer".into()));
                            // A paused agent holds no provider slot.
                            _permit = None;
                            let r = self.ask(&turn.chat.id, &agent.id, &question, options, &turn.token).await;
                            _permit = Some(tokio::select! {
                                p = self.providers.semaphore(&cfg).acquire_owned() => p.map_err(|_| CoreError::Cancelled)?,
                                _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
                            });
                            match r {
                                Err(CoreError::Cancelled) => return Err(CoreError::Cancelled),
                                r => r.map(|a| a.for_agent()),
                            }
                        }
                        Err(e) => Err(e),
                    }
                } else if call.name == "git" {
                    match (&workspace, has_git) {
                        (Some(ws), true) => {
                            let mutating = changes::mutates_workspace("git", &call.arguments);
                            // After reading the web, a git write needs the user's approval even at full permissions.
                            let gate = if mutating && taint_guard && tainted && level == PermissionLevel::Full {
                                let command = crate::gittool::parse_args(&call.arguments).map(|a| crate::gittool::normalize(&a)).unwrap_or_default();
                                self.approve_once(turn, agent, &cfg, &mut _permit, can_ask, &format!("{} (has read web content) wants to run git {}", agent.name, truncate(&command, 300))).await?
                            } else {
                                Ok(())
                            };
                            match gate {
                                Err(e) => Err(e),
                                Ok(()) => {
                                    if mutating {
                                        self.checkpoint_before(&turn.chat.id, ws, stats).await;
                                    }
                                    let confirmed = call.arguments["confirmed"].as_bool() == Some(true)
                                        && crate::gittool::parse_args(&call.arguments).map(|a| self.take_git_confirmation(&turn.chat.id, &crate::gittool::normalize(&a))).unwrap_or(false);
                                    let out = tokio::select! {
                                        r = crate::gittool::execute(std::path::Path::new(ws), level, &call.arguments, confirmed) => r,
                                        _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
                                    };
                                    out.map(|o| {
                                        app_text = o.pending.is_some();
                                        if let Some(p) = o.pending {
                                            match msg.meta["gitPending"].as_array_mut() {
                                                Some(list) if list.iter().any(|c| c.as_str() == Some(&p)) => {}
                                                Some(list) => list.push(json!(p)),
                                                None => msg.meta["gitPending"] = json!([p]),
                                            }
                                        }
                                        o.text
                                    })
                                }
                            }
                        }
                        _ => Err(CoreError::invalid("unknown tool 'git'")),
                    }
                } else if let Some(done) = web_results.remove(&call_idx) {
                    done
                } else if tools::is_web_tool(&call.name) {
                    tokio::select! {
                        r = self.run_web(&call.name, &call.arguments) => r,
                        _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
                    }
                } else if let Some(ws) = &workspace {
                    let policy = self.db.shell_policy(&turn.chat.id)?;
                    // At the Ask level a write or command waits for the user, unless they already allowed it. After reading
                    // the web the same goes for Full, and nothing counts as already allowed.
                    let guarded = taint_guard && tainted;
                    let mut gate: Result<()> = Ok(());
                    let pre_approved = !guarded
                        && match call.name.as_str() {
                            "run_command" => permissions::command_allowed(call.arguments["command"].as_str().unwrap_or(""), &policy.allow_commands),
                            _ => edits_approved,
                        };
                    if permissions::needs_approval(level, &call.name, guarded) && !pre_approved {
                        if !can_ask {
                            gate = Err(CoreError::Permission(format!(
                                "{} needs the user's approval{}, and nobody can answer here (automations, delegated and remote runs cannot ask). Do not retry it.",
                                tools::summarize(&call.name, &call.arguments),
                                if guarded { " because this run has read web content" } else { "" }
                            )));
                        } else {
                            let (question, options, prefix) = tools::approval_question(&agent.name, &call.name, &call.arguments, guarded);
                            gate = match self.ask_releasing(turn, agent, &cfg, &mut _permit, &question, options).await? {
                                QuestionAnswer::Choice(c) if c == tools::APPROVE_ONCE => Ok(()),
                                QuestionAnswer::Choice(c) if c == tools::APPROVE_RUN && !guarded => {
                                    edits_approved = true;
                                    Ok(())
                                }
                                QuestionAnswer::Choice(c) if c.starts_with(tools::APPROVE_ALWAYS) && !guarded && prefix.is_some() => {
                                    let mut p = policy.clone();
                                    p.allow_commands.extend(prefix.filter(|x| !p.allow_commands.contains(x)));
                                    self.db.set_shell_policy(&turn.chat.id, &p)?;
                                    Ok(())
                                }
                                QuestionAnswer::Custom(t) => {
                                    self.note_denial(turn, agent, &summary, Some(&t));
                                    Err(CoreError::Permission(format!("the user did not approve this and said: \"{t}\". Do not run it; adjust your approach.")))
                                }
                                other => {
                                    // Dismissing the question is not saying no.
                                    if !matches!(other, QuestionAnswer::Skipped) {
                                        self.note_denial(turn, agent, &summary, None);
                                    }
                                    Err(CoreError::Permission("the user denied this action. Do not retry it; ask what they would like instead.".into()))
                                }
                            };
                        }
                    }
                    if gate.is_ok() && changes::mutates_workspace(&call.name, &call.arguments) {
                        self.checkpoint_before(&turn.chat.id, ws, stats).await;
                    }
                    match gate {
                        Err(e) => Err(e),
                        Ok(()) => tokio::select! {
                            r = tools::execute(std::path::Path::new(ws), level, policy.network, &call.name, &call.arguments) => r,
                            _ = turn.token.cancelled() => return Err(CoreError::Cancelled),
                        },
                    }
                } else {
                    Err(CoreError::invalid("this chat has no workspace folder"))
                };
                let allowed = !matches!(result, Err(CoreError::Permission(_)));
                let entry = self.db.log_action(&turn.chat.id, &agent.id, &call.name, &summary, allowed, &truncate(&call.arguments.to_string(), 2000))?;
                self.emit(CoreEvent::ActionLogged { entry });
                let (ok, output) = match result {
                    Ok(s) => (true, s),
                    Err(e) => (false, format!("Error: {e}")),
                };
                if let Some(list) = msg.meta["tools"].as_array_mut() {
                    let answer = (ok && call.name == tools::ASK_TOOL).then(|| output.clone());
                    list.push(json!({ "name": call.name, "summary": summary, "ok": ok, "allowed": allowed, "error": (!ok).then(|| truncate(&output, 300)), "answer": answer }));
                }
                self.save_message(msg)?;
                if ok && tools::is_web_tool(&call.name) {
                    tainted = true;
                    msg.meta["readWeb"] = json!(true);
                }
                let output = if ok { rc.shrink_tool_output(&call.name, output) } else { output };
                let output = if ok && !app_text && tools::is_untrusted_tool(&call.name) { tools::untrusted(&summary, &output) } else { output };
                req.messages.push(ChatMessage::tool_result(call, output));
            }
        }

        // Running out of steps used to end the run silently; say so, so a half-finished job isn't mistaken for a finished one.
        if !finished {
            if !msg.content.is_empty() {
                msg.content.push_str("\n\n");
            }
            msg.content.push_str(&format!("_Stopped after {MAX_TOOL_STEPS} tool steps; the work may be unfinished. Ask me to continue._"));
            self.save_message(msg)?;
        }

        // Image agent without tool support: render the <image_prompt> she wrote.
        if agent.specialty == "image" && !generated_image && agent.image_model.is_some()
            && let Some((start, end)) = msg.content.find("<image_prompt>").zip(msg.content.find("</image_prompt>"))
            && end > start
        {
            let prompt = msg.content[start + 14..end].trim().to_string();
            msg.content = format!("{}{}", &msg.content[..start], &msg.content[end + 15..]).trim().to_string();
            self.save_message(msg)?;
            if let Err(e) = self.generate_image(agent, msg, &prompt).await {
                msg.content.push_str(&format!("\n\n_Image generation failed: {e}_"));
            }
        }
        Ok(())
    }

    fn flush(&self, msg: &Message, pending: &mut (String, String)) {
        if pending.0.is_empty() && pending.1.is_empty() {
            return;
        }
        let text = std::mem::take(&mut pending.0);
        let reasoning = std::mem::take(&mut pending.1);
        self.emit(CoreEvent::MessageDelta {
            chat_id: msg.chat_id.clone(),
            message_id: msg.id.clone(),
            text: (!text.is_empty()).then_some(text),
            reasoning: (!reasoning.is_empty()).then_some(reasoning),
        });
    }

    async fn generate_image(&self, agent: &Agent, msg: &mut Message, prompt: &str) -> Result<String> {
        let m = agent.image_model.clone().ok_or_else(|| CoreError::invalid("no image model configured for this agent"))?;
        if prompt.trim().is_empty() {
            return Err(CoreError::invalid("the image prompt is empty"));
        }
        let cfg = self.db.get_provider(&m.provider_id)?;
        let key = self.provider_key(&cfg.id)?;
        let img = image::generate(&cfg, key, &m.model_id, prompt).await?;
        let ext = match img.mime.as_str() {
            "image/jpeg" => "jpg",
            "image/webp" => "webp",
            _ => "png",
        };
        let a = self.stage_attachment(&format!("{}-{}.{ext}", agent.name.to_lowercase(), chrono::Local::now().format("%Y%m%d-%H%M%S")), &img.mime, &img.bytes)?;
        self.db.link_attachment(&a.id, &msg.id, &msg.chat_id)?;
        msg.attachments.push(a);
        msg.meta["imagePrompt"] = json!(prompt);
        self.save_message(msg)?;
        Ok(match img.caption {
            Some(c) => format!("Image generated and shown to the user. The image model said: {c}"),
            None => "Image generated and shown to the user.".into(),
        })
    }
}

/// Fingerprint of a plan's text, taken when the plan is shown and checked when it is approved.
fn plan_hash(content: &str) -> String {
    use sha2::{Digest, Sha256};
    format!("{:x}", Sha256::digest(content.as_bytes()))
}

#[derive(Default)]
struct RunStats {
    reasoning_started: Option<Instant>,
    text_started: Option<Instant>,
    tokens_in: i64,
    tokens_out: i64,
    requests: u32,
    input_chars: usize,
    /// The workspace snapshot taken before the run's first change, once there was one.
    checkpoint: Option<changes::RunCheckpoint>,
    /// Why no snapshot could be taken (git missing, timeout): the run still goes ahead.
    checkpoint_error: Option<String>,
}

impl RunStats {
    /// Thinking time: first reasoning event → first answer text (or → end if no text).
    fn thinking_ms(&self, started: Instant) -> Option<i64> {
        let r = self.reasoning_started?;
        let end = self.text_started.unwrap_or_else(Instant::now);
        let _ = started;
        Some(end.saturating_duration_since(r).as_millis() as i64)
    }

    /// Provider-reported usage, or a ~4 chars/token estimate when the provider reports none.
    fn tokens(&self, msg: &Message) -> (i64, i64) {
        let est_out = ((msg.content.len() + msg.reasoning.as_ref().map_or(0, |r| r.len())) / 4) as i64;
        let tin = if self.tokens_in > 0 { self.tokens_in } else if self.requests > 0 { (self.input_chars / 4) as i64 } else { 0 };
        let tout = if self.tokens_out > 0 { self.tokens_out } else if self.requests > 0 { est_out } else { 0 };
        (tin, tout)
    }
}
