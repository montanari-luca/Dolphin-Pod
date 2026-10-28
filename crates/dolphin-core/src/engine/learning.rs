//! Learning from the user: recording the signals (a revert, a refusal, a thumbs down, a correction, "remember that...")
//! and running the learner that turns them into notes in the agent's memory. See `memory/` for the rules.
//!
//! Recording is a cheap database write at the moment it happens. The model call is made later, in the
//! background, one learner at a time, so it never delays an answer.

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::*;
use crate::memory::learner::{self, MAX_SIGNALS, Op, PromptOptions};
use crate::memory::signals::{Feedback, Signal, looks_like_correction, remember_request};
use crate::memory::{self, clean_text, looks_secret};
use crate::providers::complete;
use crate::util::new_id;

/// A learner call that takes longer than this is given up on; its signals are dropped.
const LEARNER_TIMEOUT: Duration = Duration::from_secs(90);
/// Signals older than this are forgotten whether or not they were used.
const KEEP_SIGNALS_DAYS: i64 = 14;
const EXCERPT_CHARS: usize = 700;

/// A chat is looked back over once its newest message is this old.
const IDLE_AFTER_MS: i64 = 10 * 60_000;
/// Fewer new messages than this are not worth a model call.
const REFLECT_MIN_MESSAGES: usize = 4;
const REFLECT_MAX_MESSAGES: usize = 24;
const REFLECT_MAX_AGENTS: usize = 3;
const REFLECT_MESSAGE_CHARS: usize = 400;
/// Device-local settings (`device.` never syncs): when reflecting was first seen on, and how far each chat got.
const REFLECT_SINCE: &str = "device.reflect_since";

impl Core {
    /// Log a signal for the learner. Automatic learning must be on for the agent, except for "remember", which is
    /// the user's own instruction. Whether it was logged.
    pub(crate) fn record_feedback(&self, agent_id: &str, chat: &Chat, message_id: Option<&str>, signal: Signal, mut detail: Value) -> bool {
        let Ok(agent) = self.db.get_agent(agent_id) else { return false };
        if !agent.learning && signal != Signal::Remember {
            return false;
        }
        if let Some(project) = memory::project_of(chat.workspace_path.as_deref()) {
            detail["project"] = json!(project);
        }
        let f = Feedback { id: new_id(), agent_id: agent.id, chat_id: chat.id.clone(), message_id: message_id.map(str::to_string), signal, detail, created_at: now_ms() };
        self.db.insert_feedback(&f).is_ok()
    }

    /// The user's request an answer was given to, and the answer, for the learner to read.
    fn exchange_around(&self, chat_id: &str, message_id: &str) -> (String, String) {
        let Ok(msgs) = self.db.list_messages(chat_id, 60) else { return Default::default() };
        let Some(pos) = msgs.iter().position(|m| m.id == message_id) else { return Default::default() };
        let reply = truncate(msgs[pos].content.trim(), EXCERPT_CHARS);
        let request = msgs[..pos].iter().rev().find(|m| m.agent_id.is_none() && m.kind == MessageKind::Text).map(|m| truncate(m.content.trim(), EXCERPT_CHARS)).unwrap_or_default();
        (request, reply)
    }

    /// A short message right after an agent's answer that reads like "no, not like that" is a signal for that agent.
    pub(crate) fn note_correction(&self, chat: &Chat, user_msg: &Message) {
        if remember_request(&user_msg.content).is_some() || !looks_like_correction(&user_msg.content) {
            return;
        }
        let Ok(msgs) = self.db.list_messages(&chat.id, 40) else { return };
        let Some(pos) = msgs.iter().position(|m| m.id == user_msg.id) else { return };
        let lincoln = self.agent_by_slug("lincoln").map(|a| a.id).unwrap_or_default();
        // The answer this replies to: the closest earlier message that is not a routing ack or a notice.
        let mut answer = None;
        for m in msgs[..pos].iter().rev() {
            if matches!(m.kind, MessageKind::Ack | MessageKind::Notice) {
                continue;
            }
            answer = Some(m);
            break;
        }
        let Some(answer) = answer else { return };
        let Some(agent_id) = answer.agent_id.as_deref() else { return };
        let says_something = matches!(answer.kind, MessageKind::Text | MessageKind::Plan) && answer.status == MessageStatus::Done && !answer.content.trim().is_empty();
        let is_a_question = answer.meta.get("clarification").is_some() || answer.meta.get("leanPrompt").is_some();
        if agent_id == lincoln || !says_something || is_a_question {
            return;
        }
        let (request, reply) = self.exchange_around(&chat.id, &answer.id);
        self.record_feedback(agent_id, chat, Some(&answer.id), Signal::Correction, json!({ "user": truncate(user_msg.content.trim(), EXCERPT_CHARS), "reply": reply, "request": request }));
    }

    /// "Remember that..." goes to the agents that answered it: whoever the user was talking to learns it.
    pub(crate) fn note_remember(&self, chat: &Chat, user_msg: &Message) {
        let Some(text) = remember_request(&user_msg.content) else { return };
        let Ok(msgs) = self.db.list_messages(&chat.id, 40) else { return };
        let Some(pos) = msgs.iter().position(|m| m.id == user_msg.id) else { return };
        let mut answered: Vec<&str> = Vec::new();
        for m in &msgs[pos + 1..] {
            if let Some(id) = m.agent_id.as_deref()
                && matches!(m.kind, MessageKind::Text | MessageKind::Plan)
                && m.status == MessageStatus::Done
                && !answered.contains(&id)
            {
                answered.push(id);
            }
        }
        for id in answered {
            self.record_feedback(id, chat, Some(&user_msg.id), Signal::Remember, json!({ "text": text }));
        }
    }

    /// The user declined an action an agent asked to take (not a rule refusing it).
    pub(super) fn note_denial(&self, turn: &Turn, agent: &Agent, summary: &str, said: Option<&str>) {
        self.record_feedback(
            &agent.id,
            &turn.chat,
            None,
            Signal::Deny,
            json!({ "action": truncate(summary, 300), "said": said.unwrap_or_default(), "request": truncate(turn.user_msg.content.trim(), EXCERPT_CHARS) }),
        );
    }

    /// The user reverted what a run changed.
    pub(crate) fn note_revert(&self, chat: &Chat, run: &Message, restored: usize) {
        let Some(agent_id) = run.agent_id.as_deref() else { return };
        let (request, reply) = self.exchange_around(&chat.id, &run.id);
        self.record_feedback(agent_id, chat, Some(&run.id), Signal::Revert, json!({ "files": restored, "request": request, "reply": reply }));
    }

    /// A thumbs up or down on an agent's message; `None` takes the rating back. Rating the same way twice counts once.
    pub fn rate_message(self: &Arc<Self>, message_id: &str, rating: Option<Rating>) -> Result<()> {
        let mut msg = self.db.get_message(message_id)?;
        let Some(agent_id) = msg.agent_id.clone() else { return Err(CoreError::invalid("only an agent's message can be rated")) };
        let chat = self.db.get_chat(&msg.chat_id)?;
        let before = msg.meta.get("rating").and_then(Value::as_str).map(str::to_string);
        let after = rating.map(|r| if r == Rating::Up { "up" } else { "down" });
        match after {
            Some(r) => msg.meta["rating"] = json!(r),
            None => {
                if let Some(o) = msg.meta.as_object_mut() {
                    o.remove("rating");
                }
            }
        }
        self.save_message(&msg)?;
        if let Some(r) = after
            && before.as_deref() != Some(r)
        {
            let (request, reply) = self.exchange_around(&chat.id, &msg.id);
            let signal = if r == "up" { Signal::ThumbUp } else { Signal::ThumbDown };
            if self.record_feedback(&agent_id, &chat, Some(&msg.id), signal, json!({ "request": request, "reply": reply })) {
                self.schedule_learning();
            }
        }
        Ok(())
    }

    /// Look back over chats that have gone quiet, for preferences the user showed without ever correcting anything.
    /// Off unless the user turned on "Learn from finished conversations". Only conversations after it was turned on
    /// count. Queues one `Reflection` signal per agent that took part and starts the learner; how many were queued.
    pub fn reflect_idle(self: &Arc<Self>, now: i64) -> usize {
        let since = self.db.setting_raw(REFLECT_SINCE).ok().flatten().and_then(|v| v.as_i64());
        if !settings::get_bool(&self.db, keys::LEARNING_REFLECT) {
            // Turning it off forgets when it was on, so turning it on again does not reach back over the time in between.
            if since.is_some() {
                let _ = self.db.set_setting_raw(REFLECT_SINCE, &Value::Null);
            }
            return 0;
        }
        let Some(since) = since else {
            let _ = self.db.set_setting_raw(REFLECT_SINCE, &json!(now));
            return 0;
        };
        let Ok(chats) = self.db.list_chats() else { return 0 };
        let queued: usize = chats.iter().map(|c| self.reflect_on_chat(c, since, now)).sum();
        if queued > 0 {
            self.schedule_learning();
        }
        queued
    }

    fn reflect_on_chat(&self, chat: &Chat, since: i64, now: i64) -> usize {
        let marker = format!("device.reflected.{}", chat.id);
        let upto = self.db.setting_raw(&marker).ok().flatten().and_then(|v| v.as_i64()).unwrap_or(0).max(since);
        let Ok(msgs) = self.db.list_messages(&chat.id, 200) else { return 0 };
        if msgs.iter().any(|m| matches!(m.status, MessageStatus::Streaming | MessageStatus::Pending)) {
            return 0;
        }
        let fresh: Vec<&Message> = msgs.iter().filter(|m| m.created_at > upto).collect();
        let Some(last) = fresh.last() else { return 0 };
        if now - last.updated_at.max(last.created_at) < IDLE_AFTER_MS || fresh.len() < REFLECT_MIN_MESSAGES {
            return 0;
        }
        // What was said: the user's and the agents' own words, not routing acks, notices or images.
        let said: Vec<&&Message> = fresh
            .iter()
            .filter(|m| matches!(m.kind, MessageKind::Text | MessageKind::Plan) && m.status == MessageStatus::Done && !m.content.trim().is_empty())
            .collect();
        let _ = self.db.set_setting_raw(&marker, &json!(last.created_at));
        let lincoln = self.agent_by_slug("lincoln").map(|a| a.id).unwrap_or_default();
        let said: Vec<&&Message> = said.into_iter().rev().take(REFLECT_MAX_MESSAGES).rev().collect();
        if !said.iter().any(|m| m.agent_id.is_none()) || !said.iter().any(|m| m.agent_id.is_some()) {
            return 0;
        }
        let names: std::collections::HashMap<String, String> = self.db.list_agents().unwrap_or_default().into_iter().map(|a| (a.id, a.name)).collect();
        let transcript = said
            .iter()
            .map(|m| {
                let who = m.agent_id.as_ref().and_then(|id| names.get(id)).map(String::as_str).unwrap_or("User");
                format!("{who}: {}", truncate(m.content.trim().replace('\n', " ").as_str(), REFLECT_MESSAGE_CHARS))
            })
            .collect::<Vec<_>>()
            .join("\n");
        // The agents that said the most look back; the rest of a crowded chat stay out of it.
        let mut counts: Vec<(String, usize)> = Vec::new();
        for id in said.iter().filter_map(|m| m.agent_id.as_ref()).filter(|id| **id != lincoln) {
            match counts.iter_mut().find(|(a, _)| a == id) {
                Some((_, n)) => *n += 1,
                None => counts.push((id.clone(), 1)),
            }
        }
        counts.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.cmp(&b.0)));
        counts
            .into_iter()
            .take(REFLECT_MAX_AGENTS)
            .filter(|(id, _)| self.record_feedback(id, chat, None, Signal::Reflection, json!({ "transcript": transcript })))
            .count()
    }

    /// Start the learner in the background unless one is already running (it then goes round once more).
    pub(crate) fn schedule_learning(self: &Arc<Self>) {
        if self.learning_busy.swap(true, Ordering::SeqCst) {
            self.learning_again.store(true, Ordering::SeqCst);
            return;
        }
        let core = self.clone();
        tokio::spawn(async move {
            loop {
                core.learn_pending().await;
                if !core.learning_again.swap(false, Ordering::SeqCst) {
                    break;
                }
            }
            core.learning_busy.store(false, Ordering::SeqCst);
            // A signal that arrived between the last look and releasing the flag still needs its turn.
            if core.db.agents_with_pending_feedback().map(|a| !a.is_empty()).unwrap_or(false) {
                core.schedule_learning();
            }
        });
    }

    async fn learn_pending(self: &Arc<Self>) {
        let _ = self.db.prune_feedback(KEEP_SIGNALS_DAYS);
        let Ok(agents) = self.db.agents_with_pending_feedback() else { return };
        for agent_id in agents {
            let (Ok(agent), Ok(pending)) = (self.db.get_agent(&agent_id), self.db.pending_feedback(&agent_id)) else { continue };
            let ids: Vec<String> = pending.iter().map(|f| f.id.clone()).collect();
            let (asked, noticed): (Vec<Feedback>, Vec<Feedback>) = pending.into_iter().partition(|f| f.signal == Signal::Remember);
            // What the user asked for first, then what was merely noticed. Only the latest few signals are looked at.
            for (batch, explicit) in [(asked, true), (noticed, false)] {
                let skip = batch.len().saturating_sub(MAX_SIGNALS);
                let recent: Vec<Feedback> = batch.into_iter().skip(skip).collect();
                if !recent.is_empty() && (explicit || agent.learning) {
                    self.learn_from(&agent, &recent, explicit).await;
                }
            }
            let _ = self.db.mark_feedback_processed(&ids);
        }
    }

    /// One learner call for one agent. A failure of any kind drops the signals; an explicit request is then kept as typed.
    async fn learn_from(self: &Arc<Self>, agent: &Agent, signals: &[Feedback], explicit: bool) {
        let Ok(roger) = self.agent_by_slug("roger") else { return };
        let Ok(all) = self.db.list_memories(&agent.id) else { return };
        let project = signals.iter().rev().find_map(|f| f.detail.get("project").and_then(Value::as_str)).map(str::to_string);
        let shown = learner::notes_shown(&all);

        let mut ops = Vec::new();
        if let Some((provider, mut req)) = self.utility_handle(&roger) {
            let options = PromptOptions { explicit, reflection: signals.iter().any(|f| f.signal == Signal::Reflection), persona: agent.suggest_persona && !explicit };
            req.system = learner::system_prompt(options);
            req.messages = vec![ChatMessage::user(learner::user_prompt(agent, &shown, project.as_deref(), signals))];
            req.json_mode = true;
            req.tools.clear();
            let started = Instant::now();
            if let Ok(Ok((text, used))) = tokio::time::timeout(LEARNER_TIMEOUT, complete(provider.as_ref(), req)).await {
                let _ = self.record_internal_usage(&roger.id, &used, started);
                ops = learner::parse_ops(&text);
            }
        }
        if ops.is_empty() && explicit {
            // No usable answer: keep what the user asked for, as they said it.
            ops = signals
                .iter()
                .filter_map(|f| f.detail.get("text").and_then(Value::as_str))
                .map(|t| Op::Add { kind: MemoryKind::Fact, text: t.to_string(), project_only: false })
                .collect();
        }
        // A persona suggestion is not a note: it waits for the user. Only offered to the model when the agent may suggest.
        let (persona_ops, ops): (Vec<Op>, Vec<Op>) = ops.into_iter().partition(|o| matches!(o, Op::Persona { .. }));
        if agent.suggest_persona
            && !explicit
            && let Some(Op::Persona { text, reason }) = persona_ops.into_iter().next()
        {
            self.propose_persona(agent, &text, &reason);
        }
        if ops.is_empty() {
            return;
        }
        let Ok(outcome) = learner::apply(&self.db, &agent.id, project.as_deref(), explicit, &shown, ops) else { return };
        for m in outcome.added {
            self.emit(CoreEvent::MemoryLearned { memory: m, previous_text: None });
        }
        for (m, before) in outcome.updated {
            self.emit(CoreEvent::MemoryLearned { memory: m, previous_text: Some(before) });
        }
        if outcome.changed {
            self.emit(CoreEvent::MemoriesChanged { agent_id: agent.id.clone() });
        }
    }

    /// Keep a persona the learner suggested, for the user to accept or dismiss. Nothing here changes the persona.
    pub(crate) fn propose_persona(&self, agent: &Agent, text: &str, reason: &str) -> Option<PersonaProposal> {
        let text = settings::clean_persona(text)?;
        let same_as_now = agent.persona.as_deref().is_some_and(|p| p.trim().eq_ignore_ascii_case(&text));
        if text.chars().count() > settings::PERSONA_MAX_CHARS || looks_secret(&text) || same_as_now || self.db.persona_was_rejected(&agent.id, &text).unwrap_or(true) {
            return None;
        }
        let reason = clean_text(reason).map(|r| truncate(&r, 200)).unwrap_or_default();
        let proposal = PersonaProposal { id: new_id(), agent_id: agent.id.clone(), text, reason, created_at: now_ms() };
        self.db.insert_persona_proposal(&proposal).ok()?;
        self.emit(CoreEvent::PersonaProposalChanged { agent_id: agent.id.clone(), proposal: Some(proposal.clone()) });
        Some(proposal)
    }

    /// The `remember` tool: the agent saves one lasting note. What the tool answers is what the model reads.
    pub(crate) fn remember_tool(&self, agent: &Agent, chat: &Chat, args: &Value) -> Result<String> {
        if !agent.learning {
            return Err(CoreError::invalid("learning is turned off for you; do not use this tool"));
        }
        let text = args.get("text").and_then(Value::as_str).and_then(clean_text).ok_or_else(|| CoreError::invalid("'text' is empty"))?;
        if text.chars().count() > memory::MAX_TEXT_CHARS {
            return Err(CoreError::invalid(format!("too long: write the note as one short sentence under {} characters", memory::MAX_TEXT_CHARS)));
        }
        if looks_secret(&text) {
            return Err(CoreError::invalid("that looks like a secret (a password, key or token). Never save those; nothing was saved"));
        }
        let kind = args.get("kind").and_then(Value::as_str).and_then(|k| serde_json::from_value(Value::String(k.trim().to_lowercase())).ok()).unwrap_or(MemoryKind::Fact);
        let project_only = args.get("project_only").and_then(Value::as_bool) == Some(true);
        let project = memory::project_of(chat.workspace_path.as_deref());
        let outcome = learner::apply(&self.db, &agent.id, project.as_deref(), false, &[], vec![Op::Add { kind, text, project_only }])?;
        if outcome.changed {
            self.emit(CoreEvent::MemoriesChanged { agent_id: agent.id.clone() });
        }
        match outcome.added.into_iter().next() {
            Some(m) => {
                self.emit(CoreEvent::MemoryLearned { memory: m, previous_text: None });
                Ok("Saved. You will see it in future conversations with this user.".into())
            }
            None => Ok("Not saved: it is already noted, or your notes are full of entries the user wrote.".into()),
        }
    }
}
