//! Agent tools scoped to the chat's workspace folder. Every call is permission-checked and logged.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::error::{CoreError, Result};
use crate::model::PermissionLevel;
use crate::permissions::{check, resolve_in_workspace};
use crate::providers::ToolSpec;
use crate::util::truncate;
use crate::websearch;

const MAX_OUTPUT: usize = 24_000;

fn spec(name: &str, description: &str, parameters: Value) -> ToolSpec {
    ToolSpec { name: name.into(), description: description.into(), parameters }
}

pub fn image_tool() -> ToolSpec {
    spec(
        "generate_image",
        "Generate an image from a detailed prompt and show it in the chat.",
        json!({ "type": "object", "properties": {
            "prompt": { "type": "string", "description": "Detailed image prompt: subject, composition, style, lighting, palette, mood." }
        }, "required": ["prompt"] }),
    )
}

/// Web tools, available in every chat (no workspace needed). With a native search provider
/// (OpenRouter's server tool) only `fetch_page` is added; its own search runs server-side.
pub fn web_specs(native_search: bool) -> Vec<ToolSpec> {
    let mut v = Vec::new();
    if !native_search {
        v.push(spec(
            "web_search",
            "Search the web for current information. Returns titles, URLs and snippets; use fetch_page to read a result in full.",
            json!({ "type": "object", "properties": {
                "query": { "type": "string", "description": "Search keywords, as you would type them into a search engine." },
                "max_results": { "type": "integer", "description": "Results to return (1-10). Default 6." }
            }, "required": ["query"] }),
        ));
    }
    v.push(spec(
        "fetch_page",
        "Open a public web page (or a plain-text/JSON URL) and read its text content.",
        json!({ "type": "object", "properties": {
            "url": { "type": "string", "description": "Full http(s) URL." },
            "max_chars": { "type": "integer", "description": "Maximum characters to return (1000-30000). Default 12000." }
        }, "required": ["url"] }),
    ));
    v
}

pub fn is_web_tool(name: &str) -> bool {
    matches!(name, "web_search" | "fetch_page")
}

/// Execute a web tool. These are read-only and need no workspace.
pub async fn execute_web(name: &str, args: &Value) -> Result<String> {
    if args.get("_invalid_json").is_some() {
        return Err(CoreError::invalid("the tool arguments were not valid JSON; send them again"));
    }
    match name {
        "web_search" => {
            let q = s(args, "query")?;
            let max = args["max_results"].as_u64().unwrap_or(6).clamp(1, 10) as usize;
            let hits = websearch::search(q, max).await?;
            Ok(truncate(&websearch::format_hits(q, &hits), MAX_OUTPUT))
        }
        "fetch_page" => {
            let max = args["max_chars"].as_u64().unwrap_or(websearch::MAX_PAGE_CHARS as u64).clamp(1000, 30_000) as usize;
            websearch::fetch_page(s(args, "url")?, max).await
        }
        other => Err(CoreError::invalid(format!("unknown tool '{other}'"))),
    }
}

/// Tools offered to an agent. Write tools are only offered with full permissions (and are also
/// enforced at execution time).
pub fn specs(has_workspace: bool, level: PermissionLevel) -> Vec<ToolSpec> {
    if !has_workspace {
        return vec![];
    }
    let mut v = vec![
        spec("list_dir", "List files and folders in the workspace.", json!({ "type": "object", "properties": {
            "path": { "type": "string", "description": "Folder relative to the workspace root. Default: root." },
            "depth": { "type": "integer", "description": "How many levels deep (1-4). Default 2." }
        } })),
        spec("read_file", "Read a text file from the workspace.", json!({ "type": "object", "properties": {
            "path": { "type": "string" },
            "start_line": { "type": "integer", "description": "1-based first line. Default 1." },
            "max_lines": { "type": "integer", "description": "Default 400, max 2000." }
        }, "required": ["path"] })),
        spec("search", "Search file contents in the workspace (respects .gitignore).", json!({ "type": "object", "properties": {
            "query": { "type": "string", "description": "Text or regular expression." },
            "regex": { "type": "boolean", "description": "Treat query as a regex. Default false." },
            "path": { "type": "string", "description": "Folder to search. Default: root." }
        }, "required": ["query"] })),
    ];
    if level == PermissionLevel::Full {
        v.push(spec("write_file", "Create or overwrite a file in the workspace.", json!({ "type": "object", "properties": {
            "path": { "type": "string" }, "content": { "type": "string" }
        }, "required": ["path", "content"] })));
        v.push(spec("edit_file", "Replace an exact snippet in a file. `old` must match exactly once unless replace_all is true.", json!({ "type": "object", "properties": {
            "path": { "type": "string" }, "old": { "type": "string" }, "new": { "type": "string" },
            "replace_all": { "type": "boolean" }
        }, "required": ["path", "old", "new"] })));
        v.push(spec("run_command", "Run a shell command in the workspace and return its output.", json!({ "type": "object", "properties": {
            "command": { "type": "string" },
            "cwd": { "type": "string", "description": "Folder relative to the workspace root." },
            "timeout_seconds": { "type": "integer", "description": "Default 120, max 600." }
        }, "required": ["command"] })));
    }
    v
}

fn s<'a>(args: &'a Value, key: &str) -> Result<&'a str> {
    args[key].as_str().ok_or_else(|| CoreError::invalid(format!("missing '{key}'")))
}

/// One-line human description of a call for the action log and the chat.
pub fn summarize(name: &str, args: &Value) -> String {
    let a = |k: &str| args[k].as_str().unwrap_or("").to_string();
    match name {
        "list_dir" => format!("Listed {}", if a("path").is_empty() { "the workspace".into() } else { a("path") }),
        "read_file" => format!("Read {}", a("path")),
        "search" => format!("Searched for \"{}\"", truncate(&a("query"), 60)),
        "write_file" => format!("Wrote {}", a("path")),
        "edit_file" => format!("Edited {}", a("path")),
        "run_command" => format!("Ran `{}`", truncate(&a("command"), 80)),
        "web_search" => format!("Searched the web for \"{}\"", truncate(&a("query"), 60)),
        "fetch_page" => format!("Opened {}", truncate(&a("url"), 80)),
        "generate_image" => "Generated an image".into(),
        "context_retrieve" => "Re-read a compressed result".into(),
        other => format!("Called {other}"),
    }
}

/// Execute a workspace tool. The permission check happens here, not in the prompt.
pub async fn execute(workspace: &Path, level: PermissionLevel, name: &str, args: &Value) -> Result<String> {
    if args.get("_invalid_json").is_some() {
        return Err(CoreError::invalid("the tool arguments were not valid JSON; send them again"));
    }
    check(level, name)?;
    match name {
        "list_dir" => {
            let dir = resolve_in_workspace(workspace, args["path"].as_str().unwrap_or("."))?;
            let depth = args["depth"].as_u64().unwrap_or(2).clamp(1, 4) as usize;
            let root = workspace.canonicalize()?;
            let mut out = Vec::new();
            for e in ignore::WalkBuilder::new(&dir).max_depth(Some(depth)).hidden(false).filter_entry(|e| e.file_name() != ".git").build().flatten().skip(1) {
                let rel = e.path().strip_prefix(&root).unwrap_or(e.path()).display().to_string();
                let is_dir = e.file_type().is_some_and(|t| t.is_dir());
                out.push(if is_dir { format!("{rel}/") } else { rel });
                if out.len() >= 400 {
                    out.push("… (truncated)".into());
                    break;
                }
            }
            Ok(if out.is_empty() { "(empty)".into() } else { out.join("\n") })
        }
        "read_file" => {
            let p = resolve_in_workspace(workspace, s(args, "path")?)?;
            let bytes = tokio::fs::read(&p).await.map_err(|e| CoreError::invalid(format!("cannot read {}: {e}", s(args, "path").unwrap_or(""))))?;
            if bytes.iter().take(8000).any(|b| *b == 0) {
                return Err(CoreError::invalid("that looks like a binary file"));
            }
            let text = String::from_utf8_lossy(&bytes);
            let start = args["start_line"].as_u64().unwrap_or(1).max(1) as usize;
            let max = args["max_lines"].as_u64().unwrap_or(400).clamp(1, 2000) as usize;
            let lines: Vec<&str> = text.lines().collect();
            let total = lines.len();
            let body: Vec<String> = lines.iter().enumerate().skip(start - 1).take(max).map(|(i, l)| format!("{:>5}  {l}", i + 1)).collect();
            let mut out = body.join("\n");
            if start - 1 + max < total {
                out.push_str(&format!("\n… ({} more lines; use start_line to continue)", total - (start - 1 + max)));
            }
            Ok(truncate(&out, MAX_OUTPUT))
        }
        "search" => {
            let q = s(args, "query")?;
            let dir = resolve_in_workspace(workspace, args["path"].as_str().unwrap_or("."))?;
            let re = if args["regex"].as_bool().unwrap_or(false) {
                regex::Regex::new(q).map_err(|e| CoreError::invalid(format!("bad regex: {e}")))?
            } else {
                regex::RegexBuilder::new(&regex::escape(q)).case_insensitive(true).build().unwrap()
            };
            let root = workspace.canonicalize()?;
            let mut hits = Vec::new();
            'files: for e in ignore::WalkBuilder::new(&dir).build().flatten() {
                if !e.file_type().is_some_and(|t| t.is_file()) || e.metadata().map(|m| m.len() > 1_000_000).unwrap_or(true) {
                    continue;
                }
                let Ok(text) = std::fs::read_to_string(e.path()) else { continue };
                for (i, line) in text.lines().enumerate() {
                    if re.is_match(line) {
                        let rel = e.path().strip_prefix(&root).unwrap_or(e.path()).display();
                        hits.push(format!("{rel}:{}: {}", i + 1, truncate(line.trim(), 200)));
                        if hits.len() >= 100 {
                            hits.push("… (more matches truncated)".into());
                            break 'files;
                        }
                    }
                }
            }
            Ok(if hits.is_empty() { "No matches.".into() } else { hits.join("\n") })
        }
        "write_file" => {
            let p = resolve_in_workspace(workspace, s(args, "path")?)?;
            let content = s(args, "content")?;
            if let Some(parent) = p.parent() {
                tokio::fs::create_dir_all(parent).await?;
            }
            tokio::fs::write(&p, content).await?;
            Ok(format!("Wrote {} bytes to {}", content.len(), s(args, "path")?))
        }
        "edit_file" => {
            let p = resolve_in_workspace(workspace, s(args, "path")?)?;
            let (old, new) = (s(args, "old")?, s(args, "new")?);
            let text = tokio::fs::read_to_string(&p).await?;
            let count = text.matches(old).count();
            let all = args["replace_all"].as_bool().unwrap_or(false);
            if count == 0 {
                return Err(CoreError::invalid("`old` was not found in the file"));
            }
            if count > 1 && !all {
                return Err(CoreError::invalid(format!("`old` matches {count} times; make it unique or set replace_all")));
            }
            let updated = if all { text.replace(old, new) } else { text.replacen(old, new, 1) };
            tokio::fs::write(&p, updated).await?;
            Ok(format!("Replaced {} occurrence(s) in {}", if all { count } else { 1 }, s(args, "path")?))
        }
        "run_command" => {
            let cmd = s(args, "command")?;
            let cwd = resolve_in_workspace(workspace, args["cwd"].as_str().unwrap_or("."))?;
            let secs = args["timeout_seconds"].as_u64().unwrap_or(120).clamp(1, 600);
            let mut c = if cfg!(windows) {
                let mut c = tokio::process::Command::new("cmd");
                c.arg("/C").arg(cmd);
                c
            } else {
                let mut c = tokio::process::Command::new("sh");
                c.arg("-c").arg(cmd);
                c
            };
            c.current_dir(&cwd).kill_on_drop(true).stdin(std::process::Stdio::null());
            let out = match tokio::time::timeout(Duration::from_secs(secs), c.output()).await {
                Ok(r) => r?,
                Err(_) => return Err(CoreError::invalid(format!("command timed out after {secs}s"))),
            };
            let mut text = String::new();
            text.push_str(&String::from_utf8_lossy(&out.stdout));
            let err = String::from_utf8_lossy(&out.stderr);
            if !err.trim().is_empty() {
                text.push_str("\n[stderr]\n");
                text.push_str(&err);
            }
            Ok(truncate(&format!("exit code: {}\n{}", out.status.code().unwrap_or(-1), text.trim_end()), MAX_OUTPUT))
        }
        other => Err(CoreError::invalid(format!("unknown tool '{other}'"))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn read_only_agent_cannot_write_but_can_read() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.txt"), "hello\nworld\n").unwrap();
        let ro = PermissionLevel::ReadOnly;
        let out = execute(dir.path(), ro, "read_file", &json!({"path": "a.txt"})).await.unwrap();
        assert!(out.contains("hello") && out.contains("world"));
        let err = execute(dir.path(), ro, "write_file", &json!({"path": "b.txt", "content": "x"})).await.unwrap_err();
        assert!(matches!(err, CoreError::Permission(_)));
        assert!(!dir.path().join("b.txt").exists());
        assert!(execute(dir.path(), ro, "run_command", &json!({"command": "touch c.txt"})).await.is_err());
        assert!(!dir.path().join("c.txt").exists());
        let found = execute(dir.path(), ro, "search", &json!({"query": "WORLD"})).await.unwrap();
        assert!(found.contains("a.txt:2"));
    }

    #[tokio::test]
    async fn full_agent_writes_edits_and_runs() {
        let dir = tempfile::tempdir().unwrap();
        let full = PermissionLevel::Full;
        execute(dir.path(), full, "write_file", &json!({"path": "src/x.txt", "content": "one two"})).await.unwrap();
        execute(dir.path(), full, "edit_file", &json!({"path": "src/x.txt", "old": "two", "new": "three"})).await.unwrap();
        assert_eq!(std::fs::read_to_string(dir.path().join("src/x.txt")).unwrap(), "one three");
        let out = execute(dir.path(), full, "run_command", &json!({"command": "echo hi"})).await.unwrap();
        assert!(out.contains("exit code: 0") && out.contains("hi"));
        assert!(execute(dir.path(), full, "write_file", &json!({"path": "../evil.txt", "content": "x"})).await.is_err());
    }

    #[test]
    fn web_tools_need_no_workspace_and_native_search_replaces_ours() {
        let own: Vec<String> = web_specs(false).into_iter().map(|t| t.name).collect();
        assert_eq!(own, ["web_search", "fetch_page"]);
        let native: Vec<String> = web_specs(true).into_iter().map(|t| t.name).collect();
        assert_eq!(native, ["fetch_page"]);
        assert!(is_web_tool("web_search") && !is_web_tool("read_file"));
        assert!(check(PermissionLevel::ReadOnly, "web_search").is_ok());
    }

    #[test]
    fn write_tools_only_offered_with_full_permissions() {
        assert!(specs(false, PermissionLevel::Full).is_empty());
        let ro: Vec<String> = specs(true, PermissionLevel::ReadOnly).into_iter().map(|t| t.name).collect();
        assert!(!ro.contains(&"write_file".to_string()));
        let full: Vec<String> = specs(true, PermissionLevel::Full).into_iter().map(|t| t.name).collect();
        assert!(full.contains(&"run_command".to_string()));
    }
}
