//! Smart context handling. Three layers keep a conversation inside a model's window and cheap:
//!
//! 1. **Compression** ([`compress`]): fresh tool output is squeezed by content type (JSON, code, logs,
//!    search hits…) and the original stays retrievable.
//! 2. **Compaction** ([`compact`], [`fit`]): when a request closes in on the model's context window,
//!    old tool results are squeezed harder and older messages become a summary. Nothing is dropped
//!    outright unless there is no other way to fit.
//! 3. **Caching**: prompts are laid out so the stable part comes first and providers can reuse it
//!    (explicit breakpoints for Anthropic, cache keys for OpenAI, a fixed `num_ctx` for Ollama).
//!
//! The functions here are pure with respect to the app (no database, no events); the engine decides
//! what to persist and show.

pub mod compact;
pub mod compress;
pub mod tokens;
pub mod window;

use std::time::Instant;

use serde_json::{Value, json};

use crate::db::Db;
use crate::providers::{ChatMessage, ChatRequest, Part, Role, Usage};
use crate::settings::{self, keys};

pub use compact::{PROGRESS_MARK, SUMMARY_MARK, Summarizer};
pub use compress::{OriginalStore, RETRIEVE_TOOL};
pub use window::Budget;

/// What a tool result is squeezed to once it ages out of the newest round, under pressure.
const AGED_BUDGET_TOKENS: usize = 400;

#[derive(Debug, Clone, Copy)]
pub struct ContextSettings {
    pub compress_tools: bool,
    pub auto_compact: bool,
    pub compact_at: u32,
    pub local_window: u32,
    pub history_messages: usize,
}

impl ContextSettings {
    pub fn load(db: &Db) -> Self {
        Self {
            compress_tools: settings::get_bool(db, keys::CONTEXT_COMPRESS),
            auto_compact: settings::get_bool(db, keys::CONTEXT_AUTO_COMPACT),
            compact_at: settings::get_int(db, keys::CONTEXT_COMPACT_AT).clamp(30, 98) as u32,
            local_window: settings::get_int(db, keys::CONTEXT_LOCAL_WINDOW).max(window::MIN_WINDOW as i64) as u32,
            history_messages: settings::get_int(db, keys::HISTORY_MESSAGES).max(4) as usize,
        }
    }
}

/// What happened to the context during one agent run (stored on the message for the UI).
#[derive(Debug, Clone, Default)]
pub struct ContextStats {
    pub window: u32,
    pub peak_tokens: usize,
    /// Tokens removed by compressing tool output.
    pub saved_tokens: i64,
    pub compactions: u32,
    pub compacted_messages: usize,
    pub cache_read: i64,
    pub cache_written: i64,
    pub retries: u32,
}

impl ContextStats {
    pub fn to_json(&self) -> Value {
        json!({
            "window": self.window,
            "peakTokens": self.peak_tokens,
            "savedTokens": self.saved_tokens,
            "compactions": self.compactions,
            "compactedMessages": self.compacted_messages,
            "cacheRead": self.cache_read,
            "cacheWritten": self.cache_written,
        })
    }

    pub fn is_interesting(&self) -> bool {
        self.saved_tokens > 0 || self.compactions > 0 || self.cache_read > 0 || self.cache_written > 0
    }
}

/// Per-run state: the budget, the originals of compressed output and the running stats.
pub struct RunContext {
    pub budget: Budget,
    pub settings: ContextSettings,
    pub store: OriginalStore,
    pub stats: ContextStats,
    /// Providers count differently from the estimator; this scales estimates up (never down) to match.
    calibration: f64,
    last_estimate: usize,
}

impl RunContext {
    pub fn new(budget: Budget, settings: ContextSettings) -> Self {
        Self {
            budget,
            settings,
            store: OriginalStore::default(),
            stats: ContextStats { window: budget.window, ..Default::default() },
            calibration: 1.0,
            last_estimate: 0,
        }
    }

    /// Budget for one tool result: roughly a twentieth of the usable window, between 1.5k and 6k tokens.
    pub fn tool_output_budget(&self) -> usize {
        (self.budget.limit / 20).clamp(1500, 6000)
    }

    /// Compress fresh tool output if it is bigger than its budget.
    pub fn shrink_tool_output(&mut self, tool: &str, output: String) -> String {
        if !self.settings.compress_tools || tool == RETRIEVE_TOOL {
            return output;
        }
        let budget = self.tool_output_budget();
        match self.store.shrink(tool, &output, budget) {
            Some(small) => {
                self.stats.saved_tokens += tokens::text(&output).saturating_sub(tokens::text(&small)) as i64;
                small
            }
            None => output,
        }
    }

    /// Estimated input tokens of a request, calibrated.
    pub fn estimate(&mut self, req: &ChatRequest) -> usize {
        let raw = tokens::text(&req.system) + tokens::specs(&req.tools) + tokens::messages(&req.messages);
        self.last_estimate = raw;
        (raw as f64 * self.calibration) as usize
    }

    /// Feed back the input-token count a provider reported for the last request.
    pub fn calibrate(&mut self, reported_input_tokens: i64) {
        if reported_input_tokens > 0 && self.last_estimate > 0 {
            self.calibration = (reported_input_tokens as f64 / self.last_estimate as f64).clamp(1.0, 2.5);
        }
    }
}

// ------------------------------------------------------------------ fitting a request

#[derive(Debug, Clone)]
pub struct Compaction {
    /// "history" (older chat messages) or "progress" (earlier steps of this task).
    pub scope: &'static str,
    pub messages: usize,
    pub tokens_before: usize,
    pub tokens_after: usize,
    pub by_model: bool,
}

#[derive(Default)]
pub struct FitOutcome {
    pub usage: Usage,
    pub compactions: Vec<Compaction>,
    pub model_calls_ms: i64,
}

/// Bring `req` under the context trigger. `run_start` is the index where this run's own tool-loop
/// messages begin (everything before it is chat history plus the task); it is kept up to date.
/// `force` is used after a provider rejected the prompt as too long: aim for half the window.
pub async fn fit(
    rc: &mut RunContext,
    req: &mut ChatRequest,
    run_start: &mut usize,
    sum: Option<&Summarizer>,
    force: bool,
    progress: &(dyn Fn(&str) + Sync),
) -> FitOutcome {
    let mut out = FitOutcome::default();
    let mut est = rc.estimate(req);
    rc.stats.peak_tokens = rc.stats.peak_tokens.max(est);
    let trigger = if force { rc.budget.limit / 2 } else { rc.budget.trigger };
    let ceiling = if force { rc.budget.limit * 6 / 10 } else { rc.budget.limit };
    if est <= trigger {
        return out;
    }

    // 1. Squeeze older tool results harder. Their originals stay retrievable.
    if rc.settings.compress_tools {
        let protect = last_round_start(&req.messages);
        let saved = age_tool_results(&mut req.messages, protect, &mut rc.store);
        rc.stats.saved_tokens += saved;
        est = rc.estimate(req);
        if est <= trigger {
            return out;
        }
    }

    // 2. Summarize.
    if rc.settings.auto_compact || force {
        let max_summary = (rc.budget.target / 4).clamp(300, 2500);

        // 2a. Older chat history (everything before the task, except the last couple of messages).
        let pin = pin_start(&req.messages, *run_start);
        if pin >= 4 {
            let cut = pin - 2;
            progress("Compacting the conversation");
            let before = est;
            let started = Instant::now();
            let (prev, entries) = split_summary(&req.messages[..cut]);
            let s = compact::summarize(sum, prev.as_deref(), &entries, max_summary).await;
            out.model_calls_ms += started.elapsed().as_millis() as i64;
            add_usage(&mut out.usage, &s.usage);
            replace_range(&mut req.messages, 0..cut, compact::summary_message(&s.text));
            *run_start = run_start.saturating_sub(cut - 1);
            est = rc.estimate(req);
            note(rc, &mut out, "history", cut, before, est, s.by_model);
        }

        // 2b. Earlier steps of this task (everything between the task and the latest tool round).
        if est > trigger {
            let last_round = last_round_start(&req.messages);
            let pin = pin_start(&req.messages, *run_start);
            let from = (*run_start).max(pin);
            if last_round > from + 1 {
                progress("Compacting earlier steps");
                let before = est;
                let started = Instant::now();
                let (prev, entries) = split_summary(&req.messages[from..last_round]);
                let s = compact::summarize(sum, prev.as_deref(), &entries, max_summary).await;
                out.model_calls_ms += started.elapsed().as_millis() as i64;
                add_usage(&mut out.usage, &s.usage);
                let n = last_round - from;
                replace_range(&mut req.messages, from..last_round, compact::progress_message(&s.text));
                est = rc.estimate(req);
                note(rc, &mut out, "progress", n, before, est, s.by_model);
            }
        }
    }

    // 3. Last resort: truncate and drop until it fits.
    if est > ceiling {
        hard_fit(rc, req, run_start, ceiling);
    }
    out
}

fn note(rc: &mut RunContext, out: &mut FitOutcome, scope: &'static str, messages: usize, before: usize, after: usize, by_model: bool) {
    rc.stats.compactions += 1;
    rc.stats.compacted_messages += messages;
    out.compactions.push(Compaction { scope, messages, tokens_before: before, tokens_after: after, by_model });
}

fn add_usage(into: &mut Usage, u: &Usage) {
    into.input_tokens += u.input_tokens;
    into.output_tokens += u.output_tokens;
}

/// Index of the first message of the task: the trailing run (at most 3) of user messages before
/// `run_start` (the user's message, Lincoln's brief, preferences).
pub fn pin_start(msgs: &[ChatMessage], run_start: usize) -> usize {
    let run_start = run_start.min(msgs.len());
    let contiguous = msgs[..run_start].iter().rev().take_while(|m| m.role == Role::User).count();
    run_start - contiguous.min(3)
}

/// Index of the assistant message that started the latest tool round (or `len` when there is none).
pub fn last_round_start(msgs: &[ChatMessage]) -> usize {
    msgs.iter().rposition(|m| m.role == Role::Assistant && !m.tool_calls.is_empty()).unwrap_or(msgs.len())
}

/// Split a range into (previous summary, transcript entries) if it starts with a summary message.
fn split_summary(msgs: &[ChatMessage]) -> (Option<String>, Vec<String>) {
    match msgs.first().and_then(|m| compact::summary_body(&m.text_content()).map(str::to_string)) {
        Some(body) => (Some(body), compact::render_entries(&msgs[1..])),
        None => (None, compact::render_entries(msgs)),
    }
}

fn replace_range(msgs: &mut Vec<ChatMessage>, range: std::ops::Range<usize>, text: String) {
    msgs.splice(range, [ChatMessage::user(text)]);
}

/// Squeeze every tool result before `protect` down to a small budget.
fn age_tool_results(msgs: &mut [ChatMessage], protect: usize, store: &mut OriginalStore) -> i64 {
    let mut saved = 0i64;
    for m in msgs.iter_mut().take(protect) {
        if m.role != Role::Tool {
            continue;
        }
        let text = m.text_content();
        let tool = m.tool_name.clone().unwrap_or_default();
        if let Some(small) = store.shrink_again(&tool, &text, AGED_BUDGET_TOKENS) {
            saved += tokens::text(&text).saturating_sub(tokens::text(&small)) as i64;
            m.parts = vec![Part::Text(small)];
        }
    }
    saved
}

fn clip_message(m: &mut ChatMessage, max_tokens: usize) {
    let mut left = tokens::chars_for(max_tokens);
    for p in &mut m.parts {
        if let Part::Text(t) = p {
            if t.chars().count() > left {
                *t = compress::head_tail(t, left.max(200));
            }
            left = left.saturating_sub(t.chars().count());
        }
    }
}

/// Guarantee a fit by brute force. Keeps the newest messages and the summary, in that order of value.
fn hard_fit(rc: &mut RunContext, req: &mut ChatRequest, run_start: &mut usize, ceiling: usize) {
    let mut est = rc.estimate(req);
    // Old tool results to a stub.
    let protect = last_round_start(&req.messages);
    for m in req.messages.iter_mut().take(protect).filter(|m| m.role == Role::Tool) {
        clip_message(m, 100);
    }
    est = est.min(rc.estimate(req));
    // Drop history from the front (never the task itself or the latest round).
    while est > ceiling {
        let pin = pin_start(&req.messages, *run_start);
        if pin == 0 {
            break;
        }
        req.messages.remove(0);
        *run_start = run_start.saturating_sub(1);
        est = rc.estimate(req);
    }
    // Shrink the biggest messages other than the newest two.
    while est > ceiling {
        let n = req.messages.len();
        let Some((i, size)) = req.messages.iter().enumerate().take(n.saturating_sub(2)).map(|(i, m)| (i, tokens::message(m))).max_by_key(|(_, s)| *s) else { break };
        if size < 300 {
            break;
        }
        clip_message(&mut req.messages[i], size / 3);
        est = rc.estimate(req);
    }
    // The newest messages themselves.
    while est > ceiling {
        let n = req.messages.len();
        let Some(i) = (0..n).max_by_key(|i| tokens::message(&req.messages[*i])) else { break };
        let size = tokens::message(&req.messages[i]);
        if size < 300 {
            break;
        }
        clip_message(&mut req.messages[i], size / 2);
        est = rc.estimate(req);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::ToolCall;

    fn rc(window: u32) -> RunContext {
        RunContext::new(
            Budget::new(window, 85),
            ContextSettings { compress_tools: true, auto_compact: true, compact_at: 85, local_window: 32_768, history_messages: 40 },
        )
    }

    fn call(id: &str) -> ToolCall {
        ToolCall { id: id.into(), name: "run_command".into(), arguments: json!({ "command": "make" }) }
    }

    /// system + 30 history messages + task + 6 tool rounds with big outputs.
    fn big_request() -> (ChatRequest, usize) {
        let mut messages = Vec::new();
        for i in 0..30 {
            messages.push(if i % 2 == 0 { ChatMessage::user(format!("question {i} {}", "context ".repeat(150))) } else { ChatMessage::assistant(format!("answer {i} {}", "detail ".repeat(150))) });
        }
        messages.push(ChatMessage::user("please fix the build"));
        let run_start = messages.len();
        for r in 0..6 {
            let c = call(&format!("t{r}"));
            let mut a = ChatMessage::assistant(format!("running step {r}"));
            a.tool_calls = vec![c.clone()];
            messages.push(a);
            let out: String = (0..400).map(|i| format!("step {r} line {i}: compiling module_{i} ok\n")).collect();
            messages.push(ChatMessage::tool_result(&c, out));
        }
        (ChatRequest { system: "sys".into(), messages, ..Default::default() }, run_start)
    }

    #[test]
    fn fresh_tool_output_is_compressed_and_retrievable() {
        let mut rc = rc(32_000);
        let out: String = (0..5000).map(|i| format!("log line {i}: something happened here\n")).collect();
        let small = rc.shrink_tool_output("run_command", out.clone());
        assert!(tokens::text(&small) < tokens::text(&out) / 4);
        assert!(rc.stats.saved_tokens > 1000);
        let got = rc.store.retrieve(&json!({ "id": "c1", "query": "line 4999:" }));
        assert!(got.contains("log line 4999"));
        assert_eq!(rc.shrink_tool_output("run_command", "short".into()), "short");
        let mut off = rc;
        off.settings.compress_tools = false;
        assert_eq!(off.shrink_tool_output("run_command", out.clone()), out);
    }

    #[tokio::test]
    async fn a_request_near_the_window_is_compacted_without_a_model() {
        let (mut req, mut run_start) = big_request();
        let mut rc = rc(16_384);
        let before = rc.estimate(&req);
        assert!(before > rc.budget.trigger, "{before} vs {}", rc.budget.trigger);
        let out = fit(&mut rc, &mut req, &mut run_start, None, false, &|_| {}).await;
        let after = rc.estimate(&req);
        assert!(after <= rc.budget.trigger, "{after} > {}", rc.budget.trigger);
        assert!(!out.compactions.is_empty() || rc.stats.saved_tokens > 0);
        // The task and the latest tool round survive untouched.
        assert!(req.messages.iter().any(|m| m.text_content() == "please fix the build"));
        let last = req.messages.last().unwrap();
        assert_eq!(last.role, Role::Tool);
        assert!(last.text_content().contains("step 5 line 399"));
        // Tool calls and results still pair up.
        let mut open: Vec<String> = Vec::new();
        for m in &req.messages {
            for c in &m.tool_calls {
                open.push(c.id.clone());
            }
            if m.role == Role::Tool {
                assert!(open.contains(m.tool_call_id.as_ref().unwrap()), "orphaned tool result");
            }
        }
    }

    #[tokio::test]
    async fn old_tool_results_are_squeezed_before_anything_is_summarized() {
        let mut rc = rc(32_768);
        let (mut req, mut run_start) = big_request();
        req.messages.drain(..30);
        run_start -= 30;
        rc.budget.trigger = tokens::messages(&req.messages) - 500;
        let out = fit(&mut rc, &mut req, &mut run_start, None, false, &|_| {}).await;
        assert!(out.compactions.is_empty(), "squeezing alone was enough");
        assert!(rc.stats.saved_tokens > 0);
        assert!(req.messages[2].text_content().contains("context_retrieve id="));
    }

    #[tokio::test]
    async fn forced_fit_always_succeeds() {
        let (mut req, mut run_start) = big_request();
        let huge = ChatMessage::user("x".repeat(400_000));
        req.messages.insert(run_start, huge);
        run_start += 1;
        let mut rc = rc(8192);
        fit(&mut rc, &mut req, &mut run_start, None, true, &|_| {}).await;
        assert!(rc.estimate(&req) <= rc.budget.limit, "{} > {}", rc.estimate(&req), rc.budget.limit);
    }

    #[test]
    fn calibration_only_scales_up() {
        let mut rc = rc(32_000);
        let req = ChatRequest { system: "word ".repeat(1000), ..Default::default() };
        let raw = rc.estimate(&req);
        rc.calibrate((raw / 2) as i64);
        assert_eq!(rc.estimate(&req), raw);
        rc.calibrate((raw * 2) as i64);
        assert!(rc.estimate(&req) >= raw * 19 / 10);
    }
}
