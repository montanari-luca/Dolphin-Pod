//! Content-aware compression of tool output, in the spirit of Headroom: detect what a payload is
//! (JSON, source code, search hits, a listing, a log, prose), keep what matters for that kind (errors,
//! outliers, boundaries, signatures, the tail of a log) and drop the rest. Nothing is lost for good:
//! the full original goes into an [`OriginalStore`] and the model can fetch it back, or search it,
//! with the `context_retrieve` tool (compress → cache → retrieve).
//!
//! Compression is deterministic and local; no model is involved. It only runs on fresh output. Older
//! text is not rewritten unless the context is under real pressure, so the prefix a provider has
//! cached stays byte-identical.

use std::collections::{BTreeSet, VecDeque};
use std::sync::OnceLock;

use regex::Regex;
use serde_json::{Value, json};

use super::tokens;
use crate::util::truncate;

/// Name of the tool that serves originals back to the model.
pub const RETRIEVE_TOOL: &str = "context_retrieve";

// ------------------------------------------------------------------ original store

/// Full originals of compressed outputs, so nothing a model might need is gone.
pub struct OriginalStore {
    items: VecDeque<(String, String)>,
    bytes: usize,
    next: usize,
    max_bytes: usize,
}

impl Default for OriginalStore {
    fn default() -> Self {
        Self::new(48 * 1024 * 1024)
    }
}

impl OriginalStore {
    pub fn new(max_bytes: usize) -> Self {
        Self { items: VecDeque::new(), bytes: 0, next: 1, max_bytes }
    }

    pub fn put(&mut self, text: String) -> String {
        let id = format!("c{}", self.next);
        self.next += 1;
        self.bytes += text.len();
        self.items.push_back((id.clone(), text));
        while self.bytes > self.max_bytes && self.items.len() > 1 {
            if let Some((_, old)) = self.items.pop_front() {
                self.bytes -= old.len();
            }
        }
        id
    }

    pub fn get(&self, id: &str) -> Option<&str> {
        self.items.iter().find(|(i, _)| i == id).map(|(_, t)| t.as_str())
    }

    /// Compress `text` to about `budget_tokens`. Returns the replacement (with a retrieval footer) or
    /// `None` when the text already fits or cannot be made meaningfully smaller.
    pub fn shrink(&mut self, tool: &str, text: &str, budget_tokens: usize) -> Option<String> {
        let original_tokens = tokens::text(text);
        if original_tokens <= budget_tokens {
            return None;
        }
        let (body, kind) = compress(tool, text, budget_tokens)?;
        let new_tokens = tokens::text(&body);
        if new_tokens * 10 >= original_tokens * 9 {
            return None;
        }
        let id = self.put(text.to_string());
        Some(format!("{body}\n{}", footer(kind, original_tokens, new_tokens, &id)))
    }

    /// Re-compress already compressed text further, keeping its retrieval id.
    pub fn shrink_again(&mut self, tool: &str, text: &str, budget_tokens: usize) -> Option<String> {
        if tokens::text(text) <= budget_tokens {
            return None;
        }
        let Some((body, id)) = split_footer(text) else { return self.shrink(tool, text, budget_tokens) };
        let (smaller, kind) = compress(tool, body, budget_tokens)?;
        let (before, after) = (tokens::text(body), tokens::text(&smaller));
        if after * 10 >= before * 9 {
            return None;
        }
        Some(format!("{smaller}\n{}", footer(kind, before, after, &id)))
    }

    /// Serve a `context_retrieve` call.
    pub fn retrieve(&self, args: &Value) -> String {
        let Some(id) = args["id"].as_str() else { return "Error: missing 'id'".into() };
        let Some(text) = self.get(id) else {
            return format!("Error: no stored output '{id}'. It may be from an earlier turn; run the tool again.");
        };
        let lines: Vec<&str> = text.lines().collect();
        let max_lines = args["max_lines"].as_u64().unwrap_or(300).clamp(1, 1000) as usize;
        let out = if let Some(q) = args["query"].as_str().filter(|q| !q.trim().is_empty()) {
            let re = Regex::new(&format!("(?i){q}")).unwrap_or_else(|_| Regex::new(&format!("(?i){}", regex::escape(q))).unwrap());
            let mut keep = BTreeSet::new();
            for (i, l) in lines.iter().enumerate() {
                if re.is_match(l) {
                    keep.extend(i.saturating_sub(2)..(i + 3).min(lines.len()));
                }
            }
            if keep.is_empty() {
                return format!("No line of '{id}' matches \"{q}\" ({} lines in total).", lines.len());
            }
            let mut out = Vec::new();
            let mut prev: Option<usize> = None;
            for i in keep.into_iter().take(max_lines) {
                if prev.is_some_and(|p| i > p + 1) {
                    out.push("…".to_string());
                }
                out.push(format!("{:>5}  {}", i + 1, truncate(lines[i], 400)));
                prev = Some(i);
            }
            out.join("\n")
        } else {
            let start = args["start_line"].as_u64().unwrap_or(1).max(1) as usize;
            let slice: Vec<String> = lines.iter().enumerate().skip(start - 1).take(max_lines).map(|(i, l)| format!("{:>5}  {}", i + 1, truncate(l, 600))).collect();
            if slice.is_empty() {
                return format!("'{id}' has only {} lines.", lines.len());
            }
            let mut s = slice.join("\n");
            let end = start - 1 + slice.len();
            if end < lines.len() {
                s.push_str(&format!("\n… ({} more lines; use start_line={} to continue)", lines.len() - end, end + 1));
            }
            s
        };
        truncate(&out, 60_000)
    }
}

fn footer(kind: &str, before: usize, after: usize, id: &str) -> String {
    format!(
        "[compressed {kind}: ~{before} → ~{after} tokens. Full original: {RETRIEVE_TOOL} id=\"{id}\" (add query to search it, or start_line/max_lines to page through it)]"
    )
}

fn split_footer(text: &str) -> Option<(&str, String)> {
    static RE: OnceLock<Regex> = OnceLock::new();
    let re = RE.get_or_init(|| Regex::new(r#"\n\[compressed [^\n]*context_retrieve id="(c\d+)"[^\n]*\]\s*$"#).unwrap());
    let caps = re.captures(text)?;
    Some((&text[..caps.get(0)?.start()], caps[1].to_string()))
}

pub fn retrieve_spec() -> crate::providers::ToolSpec {
    crate::providers::ToolSpec {
        name: RETRIEVE_TOOL.into(),
        description: "Fetch the full original of a tool result that was compressed (its text ends with a [compressed …] note naming an id). Use `query` (text or regex) to search inside it, or start_line/max_lines to read a range.".into(),
        parameters: json!({ "type": "object", "properties": {
            "id": { "type": "string", "description": "The id from the [compressed …] note, e.g. c3." },
            "query": { "type": "string", "description": "Return only the lines matching this text or regex (with 2 lines of context)." },
            "start_line": { "type": "integer", "description": "1-based first line to return. Default 1." },
            "max_lines": { "type": "integer", "description": "Default 300, max 1000." }
        }, "required": ["id"] }),
    }
}

// ------------------------------------------------------------------ routing

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Json,
    Code,
    Search,
    Listing,
    Prose,
    Log,
}

impl Kind {
    fn label(self) -> &'static str {
        match self {
            Kind::Json => "JSON",
            Kind::Code => "code",
            Kind::Search => "search results",
            Kind::Listing => "listing",
            Kind::Prose => "text",
            Kind::Log => "output",
        }
    }
}

fn re(cell: &'static OnceLock<Regex>, pattern: &str) -> &'static Regex {
    cell.get_or_init(|| Regex::new(pattern).expect("compression regex"))
}

fn numbered_prefix(line: &str) -> Option<usize> {
    let t = line.trim_start();
    let digits = t.chars().take_while(|c| c.is_ascii_digit()).count();
    (digits > 0 && t[digits..].starts_with("  ")).then_some(digits)
}

fn detect(tool: &str, text: &str) -> Kind {
    let t = text.trim_start();
    if (t.starts_with('{') || t.starts_with('[')) && serde_json::from_str::<Value>(text).is_ok() {
        return Kind::Json;
    }
    match tool {
        "read_file" => return Kind::Code,
        "search" => return Kind::Search,
        "list_dir" => return Kind::Listing,
        "fetch_page" | "web_search" => return Kind::Prose,
        "git" => return Kind::Log,
        _ => {}
    }
    let sample: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).take(12).collect();
    if sample.len() >= 6 && sample.iter().filter(|l| numbered_prefix(l).is_some()).count() * 10 >= sample.len() * 8 {
        return Kind::Code;
    }
    Kind::Log
}

/// Returns the compressed body (no retrieval footer) and what it was recognised as.
pub fn compress(tool: &str, text: &str, budget_tokens: usize) -> Option<(String, &'static str)> {
    let budget = tokens::chars_for(budget_tokens).max(400);
    let kind = detect(tool, text);
    let body = match kind {
        Kind::Json => crush_json(text, budget),
        Kind::Code => by_priority(text, budget, 300, &|lines| code_priorities(lines)),
        Kind::Search => by_priority(text, budget, 260, &|lines| search_priorities(lines)),
        Kind::Listing => by_priority(text, budget, 200, &|lines| listing_priorities(lines)),
        Kind::Prose => by_priority(text, budget, 600, &|lines| prose_priorities(lines)),
        Kind::Log => by_priority(&dedupe_lines(text), budget, 400, &|lines| log_priorities(lines)),
    };
    let body = if body.chars().count() > budget + budget / 4 { head_tail(&body, budget) } else { body };
    Some((body, kind.label()))
}

// ------------------------------------------------------------------ priority selection

fn clip(line: &str, max: usize) -> String {
    let line = line.trim_end();
    if line.chars().count() <= max { line.to_string() } else { format!("{}…", line.chars().take(max).collect::<String>()) }
}

/// Keep the highest-priority lines that fit `budget` chars (ties go to earlier lines), in their
/// original order, with a marker where lines were dropped.
fn by_priority(text: &str, budget: usize, max_line: usize, prio: &dyn Fn(&[String]) -> Vec<u8>) -> String {
    let lines: Vec<String> = text.lines().map(|l| clip(l, max_line)).collect();
    if lines.is_empty() {
        return String::new();
    }
    let prios = prio(&lines);
    let room = budget.saturating_sub((budget / 10).max(80));
    let mut order: Vec<usize> = (0..lines.len()).filter(|i| prios[*i] > 0).collect();
    order.sort_by(|a, b| prios[*b].cmp(&prios[*a]).then(a.cmp(b)));
    let mut keep = vec![false; lines.len()];
    let mut used = 0usize;
    for i in order {
        let cost = lines[i].len() + 1;
        if used + cost > room {
            // A line that does not fit must not block shorter ones of the same rank.
            continue;
        }
        used += cost;
        keep[i] = true;
    }
    let mut out = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        if keep[i] {
            out.push(lines[i].clone());
            i += 1;
            continue;
        }
        let start = i;
        while i < lines.len() && !keep[i] {
            i += 1;
        }
        let n = i - start;
        let first = numbered_prefix(&lines[start]).map(|d| lines[start].trim_start()[..d].to_string());
        let last = numbered_prefix(&lines[i - 1]).map(|d| lines[i - 1].trim_start()[..d].to_string());
        out.push(match (first, last) {
            (Some(a), Some(b)) => format!("      … {n} lines omitted (lines {a}–{b})"),
            _ => format!("… {n} lines omitted"),
        });
    }
    out.join("\n")
}

/// Rank `n` positions: the first `head` and last `tail` get `head_p` / `tail_p`.
fn boundaries(prios: &mut [u8], head: usize, head_p: u8, tail: usize, tail_p: u8) {
    let n = prios.len();
    for (i, p) in prios.iter_mut().enumerate() {
        if i < head {
            *p = (*p).max(head_p);
        }
        if i + tail >= n {
            *p = (*p).max(tail_p);
        }
    }
}

fn err_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)\b(errors?|failed|failures?|fatal|panic(ked)?|exceptions?|traceback|denied|time[d ]?out|segfault|assertion|cannot|unable to|not found|refused)\b|✗|✘|\bFAIL\b")
}

fn warn_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"(?i)\b(warn(ing)?s?|deprecated)\b")
}

fn log_priorities(lines: &[String]) -> Vec<u8> {
    let mut p = vec![1u8; lines.len()];
    for (i, l) in lines.iter().enumerate() {
        if err_re().is_match(l) {
            p[i] = 9;
            // Stack frames and continuation lines belong to the error above them.
            for (k, next) in lines.iter().enumerate().skip(i + 1).take(3) {
                if next.starts_with(char::is_whitespace) || next.trim_start().starts_with("at ") {
                    p[k] = p[k].max(6);
                }
            }
        } else if warn_re().is_match(l) {
            p[i] = p[i].max(4);
        }
    }
    boundaries(&mut p, 8, 7, 16, 8);
    p
}

fn signature_re() -> &'static Regex {
    static R: OnceLock<Regex> = OnceLock::new();
    re(&R, r"^\s*(?:(?:pub(?:\([^)]*\))?|export|default|async|public|private|protected|static|abstract|final|unsafe|extern|override|open)\s+)*(?:fn|def|class|struct|enum|impl|trait|interface|type|func|function|mod|module|namespace|package|import|use|from|object|record|@\w+|#\[)\b")
}

fn code_priorities(lines: &[String]) -> Vec<u8> {
    let mut p = vec![1u8; lines.len()];
    for (i, l) in lines.iter().enumerate() {
        let body = match numbered_prefix(l) {
            Some(d) => &l.trim_start()[d..],
            None => l.as_str(),
        };
        if signature_re().is_match(body) {
            p[i] = 8;
        } else if err_re().is_match(body) || body.contains("TODO") || body.contains("FIXME") {
            p[i] = 6;
        } else if body.trim().is_empty() {
            p[i] = 0;
        } else if body.trim_start().starts_with("//") || body.trim_start().starts_with('#') {
            p[i] = 2;
        }
    }
    boundaries(&mut p, 20, 9, 8, 7);
    p
}

fn search_priorities(lines: &[String]) -> Vec<u8> {
    // Lines look like `path:line: text`. Spread the budget across files: the first hits of each file
    // rank above further hits of the same file.
    let mut seen: std::collections::HashMap<&str, u8> = std::collections::HashMap::new();
    lines
        .iter()
        .map(|l| {
            let file = l.split(':').next().unwrap_or("");
            let n = seen.entry(file).or_insert(0);
            *n = n.saturating_add(1);
            match *n {
                1 => 9,
                2 | 3 => 7,
                4..=6 => 4,
                _ => 1,
            }
        })
        .collect()
}

fn listing_priorities(lines: &[String]) -> Vec<u8> {
    lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let depth = l.trim_end_matches('/').matches('/').count();
            match (depth, i) {
                (0, _) => 9,
                (1, 0..=120) => 6,
                (_, 0..=60) => 4,
                _ => 1,
            }
        })
        .collect()
}

fn prose_priorities(lines: &[String]) -> Vec<u8> {
    let mut p: Vec<u8> = lines
        .iter()
        .map(|l| {
            let t = l.trim();
            if t.is_empty() {
                0
            } else if t.starts_with('#') {
                6
            } else if t.contains("http") || t.chars().any(|c| c.is_ascii_digit()) {
                3
            } else {
                1
            }
        })
        .collect();
    boundaries(&mut p, 40, 8, 8, 7);
    p
}

/// Collapse runs of lines that differ only in numbers / ids / timestamps: `line (×N)`.
fn dedupe_lines(text: &str) -> String {
    static NUM: OnceLock<Regex> = OnceLock::new();
    static HEX: OnceLock<Regex> = OnceLock::new();
    let num = re(&NUM, r"\d+");
    let hex = re(&HEX, r"\b[0-9a-fA-F]{8,}\b");
    let skeleton = |l: &str| num.replace_all(&hex.replace_all(l.trim(), "?"), "#").into_owned();
    let mut out: Vec<(String, String, usize)> = Vec::new();
    for l in text.lines() {
        let sk = skeleton(l);
        match out.last_mut() {
            Some((prev, _, n)) if *prev == sk && !sk.is_empty() => *n += 1,
            _ => out.push((sk, l.to_string(), 1)),
        }
    }
    out.into_iter().map(|(_, l, n)| if n > 1 { format!("{l}  (×{n} similar)") } else { l }).collect::<Vec<_>>().join("\n")
}

/// Last resort for blobs without useful line structure.
pub fn head_tail(text: &str, budget: usize) -> String {
    let total = text.chars().count();
    if total <= budget {
        return text.to_string();
    }
    let head = budget * 65 / 100;
    let tail = budget * 25 / 100;
    let h: String = text.chars().take(head).collect();
    let t: String = text.chars().skip(total - tail).collect();
    format!("{h}\n… {} characters omitted …\n{t}", total - head - tail)
}

// ------------------------------------------------------------------ JSON (SmartCrusher-style)

struct Params {
    head: usize,
    tail: usize,
    interesting: usize,
    max_str: usize,
}

const LADDER: [Params; 4] = [
    Params { head: 5, tail: 3, interesting: 6, max_str: 400 },
    Params { head: 3, tail: 2, interesting: 4, max_str: 200 },
    Params { head: 2, tail: 1, interesting: 2, max_str: 100 },
    Params { head: 1, tail: 1, interesting: 1, max_str: 60 },
];

fn crush_json(text: &str, budget: usize) -> String {
    let Ok(v) = serde_json::from_str::<Value>(text) else { return head_tail(text, budget) };
    let mut last = String::new();
    for p in &LADDER {
        last = crush(&v, p).to_string();
        if last.len() <= budget {
            return last;
        }
    }
    head_tail(&last, budget)
}

fn has_error(v: &Value, depth: usize) -> bool {
    match v {
        Value::String(s) => err_re().is_match(&s.chars().take(300).collect::<String>()),
        Value::Array(a) if depth < 4 => a.iter().take(40).any(|x| has_error(x, depth + 1)),
        Value::Object(o) if depth < 4 => o.values().take(40).any(|x| has_error(x, depth + 1)),
        _ => false,
    }
}

/// For arrays of objects: how far each item's numeric fields stray from the rest (in std deviations).
fn outlier_scores(items: &[Value]) -> Vec<f64> {
    let mut scores = vec![0.0; items.len()];
    if items.len() < 8 || !items.iter().all(Value::is_object) {
        return scores;
    }
    let keys: BTreeSet<&String> = items.iter().filter_map(|i| i.as_object()).flat_map(|o| o.keys()).collect();
    for k in keys {
        let xs: Vec<Option<f64>> = items.iter().map(|i| i[k.as_str()].as_f64()).collect();
        let vals: Vec<f64> = xs.iter().flatten().copied().collect();
        if vals.len() < items.len() / 2 {
            continue;
        }
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        let std = (vals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / vals.len() as f64).sqrt();
        if std <= f64::EPSILON {
            continue;
        }
        for (i, x) in xs.iter().enumerate() {
            if let Some(x) = x {
                scores[i] = f64::max(scores[i], ((x - mean) / std).abs());
            }
        }
    }
    scores
}

fn crush(v: &Value, p: &Params) -> Value {
    match v {
        Value::String(s) if s.chars().count() > p.max_str => {
            let head: String = s.chars().take(p.max_str).collect();
            json!(format!("{head}…(+{} chars)", s.chars().count() - p.max_str))
        }
        Value::Array(a) if a.len() > p.head + p.tail + p.interesting => {
            let n = a.len();
            let mut keep: BTreeSet<usize> = (0..p.head).chain(n - p.tail..n).collect();
            let outliers = outlier_scores(a);
            let mut extra: Vec<(usize, f64)> = (p.head..n - p.tail)
                .filter_map(|i| {
                    let score = if has_error(&a[i], 0) { 100.0 } else if outliers[i] >= 3.0 { outliers[i] } else { 0.0 };
                    (score > 0.0).then_some((i, score))
                })
                .collect();
            extra.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
            keep.extend(extra.into_iter().take(p.interesting).map(|(i, _)| i));
            let mut out = Vec::new();
            let mut prev: Option<usize> = None;
            for i in keep {
                if let Some(pv) = prev
                    && i > pv + 1
                {
                    out.push(json!(format!("… {} items omitted", i - pv - 1)));
                }
                out.push(crush(&a[i], p));
                prev = Some(i);
            }
            Value::Array(out)
        }
        Value::Array(a) => Value::Array(a.iter().map(|x| crush(x, p)).collect()),
        Value::Object(o) => Value::Object(o.iter().map(|(k, x)| (k.clone(), crush(x, p))).collect()),
        other => other.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crushes_json_arrays_but_keeps_errors_and_boundaries() {
        let items: Vec<Value> = (0..300)
            .map(|i| json!({ "id": i, "status": if i == 137 { "failed: disk full" } else { "ok" }, "latency": if i == 211 { 9000 } else { 20 + i % 5 }, "note": "x".repeat(50) }))
            .collect();
        let text = serde_json::to_string(&items).unwrap();
        let (out, kind) = compress("run_command", &text, 800).unwrap();
        assert_eq!(kind, "JSON");
        assert!(out.len() < text.len() / 5, "{} vs {}", out.len(), text.len());
        assert!(out.contains("disk full"), "error item kept");
        assert!(out.contains("9000"), "latency outlier kept");
        assert!(out.contains("\"id\":0") && out.contains("\"id\":299"), "first and last kept");
        assert!(out.contains("items omitted"));
        serde_json::from_str::<Value>(&out).expect("still valid JSON");
    }

    #[test]
    fn logs_collapse_repeats_and_keep_failures_and_tail() {
        let mut log = String::new();
        for i in 0..2000 {
            log.push_str(&format!("2025-01-01T10:00:{:02} INFO request {i} served in {}ms\n", i % 60, 10 + i % 7));
        }
        log.push_str("ERROR connection refused: db:5432\n    at connect (db.js:12)\n");
        for i in 0..500 {
            log.push_str(&format!("2025-01-01T10:01:{:02} INFO heartbeat {i}\n", i % 60));
        }
        log.push_str("test result: FAILED. 1 passed; 1 failed\n");
        let (out, _) = compress("run_command", &log, 600).unwrap();
        assert!(out.len() < 4000, "{}", out.len());
        assert!(out.contains("connection refused"));
        assert!(out.contains("db.js:12"), "stack frame stays with its error");
        assert!(out.contains("test result: FAILED"), "tail kept");
        assert!(out.contains("similar"), "repeats collapsed");
    }

    #[test]
    fn code_keeps_signatures_and_line_numbers() {
        let mut src = String::new();
        for i in 1..=600 {
            let line = if i % 50 == 0 { format!("pub fn handler_{i}(x: u32) -> u32 {{") } else { format!("    let v{i} = compute({i}) + other_value_{i};") };
            src.push_str(&format!("{i:>5}  {line}\n"));
        }
        let (out, kind) = compress("read_file", &src, 900).unwrap();
        assert_eq!(kind, "code");
        assert!(out.contains("pub fn handler_300"), "signatures survive");
        assert!(out.contains("lines omitted (lines"), "omissions name their line range");
        assert!(out.lines().count() < 150);
    }

    #[test]
    fn search_hits_are_spread_across_files() {
        let mut hits = String::new();
        for f in 0..30 {
            for l in 0..40 {
                hits.push_str(&format!("src/mod{f}.rs:{l}: let needle = {l};\n"));
            }
        }
        let (out, _) = compress("search", &hits, 700).unwrap();
        let files = (0..30).filter(|f| out.contains(&format!("src/mod{f}.rs"))).count();
        assert!(files >= 20, "most files represented, got {files}");
    }

    #[test]
    fn shrink_stores_the_original_and_retrieval_works() {
        let mut store = OriginalStore::default();
        let words = ["alpha", "beta", "gamma", "delta", "omega", "sigma", "kappa"];
        let text: String = (0..3000).map(|i| format!("row {i} {} value {}\n", words[i % 7], i * 7)).collect();
        let out = store.shrink("run_command", &text, 500).unwrap();
        assert!(out.contains("context_retrieve id=\"c1\""));
        assert!(tokens::text(&out) < 900);
        let got = store.retrieve(&json!({ "id": "c1", "query": "row 2999 " }));
        assert!(got.contains("row 2999 delta value 20993"));
        let page = store.retrieve(&json!({ "id": "c1", "start_line": 10, "max_lines": 3 }));
        assert!(page.contains("row 9 gamma value 63") && page.contains("more lines"));
        assert!(store.retrieve(&json!({ "id": "c9" })).starts_with("Error"));

        // Aging keeps the id and squeezes harder.
        let again = store.shrink_again("run_command", &out, 150).unwrap();
        assert!(again.contains("id=\"c1\"") && tokens::text(&again) < tokens::text(&out));
        assert!(store.shrink("run_command", "small", 500).is_none());
    }

    #[test]
    fn store_evicts_oldest_when_full() {
        let mut s = OriginalStore::new(100);
        let a = s.put("a".repeat(60));
        let b = s.put("b".repeat(60));
        assert!(s.get(&a).is_none() && s.get(&b).is_some());
    }

    #[test]
    fn head_tail_is_char_safe() {
        let s = "é".repeat(5000);
        let out = head_tail(&s, 1000);
        assert!(out.contains("characters omitted") && out.chars().count() < 1100);
    }
}
