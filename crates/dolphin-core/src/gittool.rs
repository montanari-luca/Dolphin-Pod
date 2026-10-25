//! Robert's `git` tool: an allowlisted way to run git in the chat's workspace.
//!
//! It runs `git` directly (an argument list, never a shell) instead of through the command sandbox, because the
//! sandbox hides `~/.ssh` and the credential helpers that fetch and push need. What keeps it safe is this module:
//! only known subcommands run, global options and options that execute programs or write elsewhere are refused,
//! git cannot look above the workspace for a repository, and every command is classified:
//!
//! - **Read**: inspects the repository.
//! - **Write**: changes the local repository or working tree in ways that are recoverable; needs full permissions.
//! - **Confirm**: touches a remote or rewrites or discards work; needs full permissions *and* the user's confirmation.
//! - **Blocked**: never runs.

use std::path::Path;
use std::time::Duration;

use serde_json::Value;

use crate::error::{CoreError, Result};
use crate::model::PermissionLevel;
use crate::permissions::resolve_in_workspace;
use crate::util::truncate;

const MAX_OUTPUT: usize = 24_000;
const TIMEOUT: Duration = Duration::from_secs(180);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GitClass {
    Read,
    Write,
    Confirm,
    Blocked(String),
}

/// What running (or declining to run) a git call produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GitOutput {
    pub text: String,
    /// Set when the command needs the user's confirmation first: the normalized command to remember.
    pub pending: Option<String>,
}

pub fn spec() -> crate::providers::ToolSpec {
    crate::providers::ToolSpec {
        name: "git".into(),
        description: "Run a git command in the workspace repository. Pass only the arguments after `git`. Reads and local changes run directly; push, pull and anything that rewrites or discards work first returns a confirmation request.".into(),
        parameters: serde_json::json!({ "type": "object", "properties": {
            "args": { "type": "array", "items": { "type": "string" }, "description": "Arguments after `git`, one per item, e.g. [\"commit\", \"-m\", \"Fix login redirect\"]. No global options (-c, -C, --git-dir ...)." },
            "confirmed": { "type": "boolean", "description": "Set true only when the user has just confirmed this exact command in chat." },
            "cwd": { "type": "string", "description": "Folder relative to the workspace root. Default: root." }
        }, "required": ["args"] }),
    }
}

/// The argument list from the tool call. A single string is split like a shell would (quotes respected).
pub fn parse_args(args: &Value) -> Result<Vec<String>> {
    let list = match &args["args"] {
        Value::Array(items) => items.iter().map(|v| v.as_str().map(str::to_string).ok_or_else(|| CoreError::invalid("every item of 'args' must be a string"))).collect::<Result<Vec<_>>>()?,
        Value::String(s) => split_words(s)?,
        _ => return Err(CoreError::invalid("missing 'args': a list of the arguments after `git`")),
    };
    let list: Vec<String> = if list.first().is_some_and(|a| a == "git") { list[1..].to_vec() } else { list };
    if list.is_empty() {
        return Err(CoreError::invalid("'args' is empty"));
    }
    Ok(list)
}

fn split_words(s: &str) -> Result<Vec<String>> {
    let mut out = Vec::new();
    let mut cur = String::new();
    let mut quote: Option<char> = None;
    let mut started = false;
    for ch in s.chars() {
        match (quote, ch) {
            (Some(q), c) if c == q => quote = None,
            (Some(_), c) => cur.push(c),
            (None, '"' | '\'') => {
                quote = Some(ch);
                started = true;
            }
            (None, c) if c.is_whitespace() => {
                if started || !cur.is_empty() {
                    out.push(std::mem::take(&mut cur));
                    started = false;
                }
            }
            (None, c) => cur.push(c),
        }
    }
    if quote.is_some() {
        return Err(CoreError::invalid("unbalanced quote in 'args'"));
    }
    if started || !cur.is_empty() {
        out.push(cur);
    }
    Ok(out)
}

/// The command as shown to the user and remembered for confirmation.
pub fn normalize(args: &[String]) -> String {
    args.iter().map(|a| if a.contains(char::is_whitespace) || a.is_empty() { format!("{a:?}") } else { a.clone() }).collect::<Vec<_>>().join(" ")
}

fn is_protected(branch: &str) -> bool {
    let b = branch.trim_start_matches('+').rsplit(':').next().unwrap_or(branch);
    let b = b.strip_prefix("refs/heads/").unwrap_or(b);
    matches!(b, "main" | "master")
}

fn blocked(msg: impl Into<String>) -> GitClass {
    GitClass::Blocked(msg.into())
}

/// Options that run programs, write files outside git's own bookkeeping or point git somewhere else.
fn dangerous_option(a: &str) -> bool {
    const BAD: &[&str] = &[
        "--exec", "--upload-pack", "--receive-pack", "--exec-path", "--git-dir", "--work-tree", "--config", "--output", "--open-files-in-pager",
        "--ext-diff", "--textconv", "--template", "--namespace", "--super-prefix", "--separate-git-dir",
    ];
    // `-O` is the orderfile (diff) or pager (grep) option. `-c`/`-C` are fine after the subcommand (`switch -c`),
    // and as global options they never get that far: the subcommand must come first.
    BAD.iter().any(|b| a == *b || a.starts_with(&format!("{b}="))) || a == "-O" || a.starts_with("-O")
}

pub fn classify(args: &[String]) -> GitClass {
    if args.iter().any(|a| a.contains('\0')) {
        return blocked("arguments cannot contain NUL");
    }
    let Some(sub) = args.first().map(String::as_str) else { return blocked("no git subcommand") };
    if sub.starts_with('-') {
        return blocked("global options are not allowed; pass the subcommand first (e.g. [\"status\"])");
    }
    let rest = &args[1..];
    if let Some(bad) = rest.iter().find(|a| dangerous_option(a)) {
        return blocked(format!("the option '{bad}' is not allowed"));
    }
    let flag = |names: &[&str]| rest.iter().any(|a| names.contains(&a.as_str()));
    let positional: Vec<&str> = rest.iter().filter(|a| !a.starts_with('-')).map(String::as_str).collect();
    match sub {
        "status" | "log" | "diff" | "show" | "blame" | "rev-parse" | "rev-list" | "shortlog" | "ls-files" | "ls-tree" | "describe" | "grep"
        | "cat-file" | "name-rev" | "merge-base" | "diff-tree" | "range-diff" | "check-ignore" | "ls-remote" | "version" => GitClass::Read,
        "reflog" => match positional.first().copied() {
            None | Some("show") => GitClass::Read,
            _ => blocked("only `reflog` and `reflog show` are available"),
        },
        "config" => {
            if flag(&["--get", "--get-all", "--list", "-l", "--get-regexp"]) {
                GitClass::Read
            } else {
                blocked("git config can only read values")
            }
        }
        "branch" => {
            if flag(&["-d", "-D", "--delete"]) {
                GitClass::Confirm
            } else if flag(&["-m", "-M", "--move", "-c", "-C", "--copy"]) {
                GitClass::Write
            } else if flag(&["--list", "-l", "--show-current", "-a", "-r", "-v", "-vv", "--contains", "--no-contains", "--merged", "--no-merged"])
                || positional.is_empty()
            {
                GitClass::Read
            } else {
                GitClass::Write
            }
        }
        "tag" => {
            if flag(&["-d", "--delete"]) {
                GitClass::Confirm
            } else if flag(&["-l", "--list"]) || positional.is_empty() {
                GitClass::Read
            } else {
                GitClass::Write
            }
        }
        "remote" => match positional.first().copied() {
            None | Some("show" | "get-url") => GitClass::Read,
            Some("add" | "set-url" | "remove" | "rm" | "rename" | "prune" | "set-head") => GitClass::Confirm,
            _ => blocked("unsupported `git remote` form"),
        },
        "stash" => match positional.first().copied() {
            Some("list" | "show") => GitClass::Read,
            Some("drop" | "clear") => GitClass::Confirm,
            None | Some("push" | "save" | "pop" | "apply") => GitClass::Write,
            _ => blocked("unsupported `git stash` form"),
        },
        "add" | "fetch" | "merge" | "cherry-pick" | "revert" | "mv" | "rm" => GitClass::Write,
        "commit" => {
            if flag(&["--amend"]) {
                GitClass::Confirm
            } else {
                GitClass::Write
            }
        }
        "switch" => {
            if flag(&["-f", "--force", "--discard-changes", "-C", "--force-create"]) {
                GitClass::Confirm
            } else {
                GitClass::Write
            }
        }
        // `checkout <path>` throws away local changes and cannot be told from `checkout <branch>`: confirm unless a branch is created.
        "checkout" => {
            if flag(&["-b", "-B"]) && !flag(&["-f", "--force"]) {
                GitClass::Write
            } else {
                GitClass::Confirm
            }
        }
        "restore" => {
            if flag(&["--staged", "-S"]) && !flag(&["--worktree", "-W"]) {
                GitClass::Write
            } else {
                GitClass::Confirm
            }
        }
        "pull" | "rebase" | "reset" | "clean" => GitClass::Confirm,
        "push" => {
            let force = rest.iter().any(|a| a == "-f" || a == "--force" || a.starts_with("--force-with-lease") || a == "--force-if-includes" || a.starts_with('+'))
                || positional.iter().skip(1).any(|a| a.starts_with('+'));
            if rest.iter().any(|a| a == "--mirror" || a == "--delete" || a == "-d" || a == "--prune") {
                return blocked("push --mirror/--delete/--prune is not available; ask the user to run it");
            }
            if force {
                // A force-push is only considered for an explicitly named branch that is not main or master.
                let named: Vec<&&str> = positional.iter().skip(1).collect();
                if named.is_empty() || named.iter().any(|b| is_protected(b)) {
                    return blocked("force-push needs an explicit branch other than main or master; ask the user to run it");
                }
            }
            GitClass::Confirm
        }
        other => blocked(format!("`git {other}` is not available")),
    }
}

/// Run one git call. `confirmed` must already be validated by the caller (see `Core::take_git_confirmation`).
pub async fn execute(workspace: &Path, level: PermissionLevel, args: &Value, confirmed: bool) -> Result<GitOutput> {
    if args.get("_invalid_json").is_some() {
        return Err(CoreError::invalid("the tool arguments were not valid JSON; send them again"));
    }
    let argv = parse_args(args)?;
    let class = classify(&argv);
    let command = normalize(&argv);
    match &class {
        GitClass::Blocked(why) => return Err(CoreError::Permission(format!("git {command}: {why}"))),
        GitClass::Write | GitClass::Confirm if level < PermissionLevel::Full => {
            return Err(CoreError::Permission(format!(
                "git {command} changes the repository; this agent is read-only in this chat. Ask the user to approve the plan or grant full permissions."
            )));
        }
        GitClass::Confirm if !confirmed => {
            return Ok(GitOutput {
                text: format!(
                    "Not run. `git {command}` needs the user's confirmation. Show the user this exact command, ask them to confirm it, and end your turn. After they say yes, call git again with the same args and confirmed set to true."
                ),
                pending: Some(command),
            });
        }
        _ => {}
    }
    let cwd = resolve_in_workspace(workspace, args["cwd"].as_str().unwrap_or("."))?;
    let root = workspace.canonicalize()?;
    let mut c = tokio::process::Command::new("git");
    c.arg("--no-pager").args(&argv).current_dir(&cwd);
    c.env("GIT_TERMINAL_PROMPT", "0").env("GIT_EDITOR", "true").env("GIT_PAGER", "cat").env("LC_ALL", "C");
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_EXEC_PATH", "GIT_CONFIG", "GIT_CONFIG_GLOBAL", "GIT_CONFIG_SYSTEM", "GIT_CONFIG_COUNT"] {
        c.env_remove(var);
    }
    // Repository discovery stops at the workspace: a workspace that is not a repository must not operate on one above it.
    if let Some(parent) = root.parent() {
        c.env("GIT_CEILING_DIRECTORIES", parent);
    }
    c.kill_on_drop(true).stdin(std::process::Stdio::null());
    let out = match tokio::time::timeout(TIMEOUT, c.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(CoreError::invalid("git is not installed on this device")),
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => return Err(CoreError::invalid(format!("git timed out after {}s", TIMEOUT.as_secs()))),
    };
    let mut text = String::from_utf8_lossy(&out.stdout).into_owned();
    let err = String::from_utf8_lossy(&out.stderr);
    if !err.trim().is_empty() {
        text.push_str("\n[stderr]\n");
        text.push_str(&err);
    }
    Ok(GitOutput { text: truncate(&format!("exit code: {}\n{}", out.status.code().unwrap_or(-1), text.trim_end()), MAX_OUTPUT), pending: None })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn a(s: &str) -> Vec<String> {
        split_words(s).unwrap()
    }

    fn class(s: &str) -> GitClass {
        classify(&a(s))
    }

    fn is_blocked(s: &str) -> bool {
        matches!(class(s), GitClass::Blocked(_))
    }

    #[test]
    fn splits_quoted_strings() {
        assert_eq!(a(r#"commit -m "Fix the thing" --no-verify"#), ["commit", "-m", "Fix the thing", "--no-verify"]);
        assert_eq!(a("commit -m ''"), ["commit", "-m", ""]);
        assert!(split_words("commit -m 'oops").is_err());
        assert_eq!(parse_args(&json!({ "args": "git status -s" })).unwrap(), ["status", "-s"]);
        assert_eq!(parse_args(&json!({ "args": ["git", "log"] })).unwrap(), ["log"]);
        assert!(parse_args(&json!({})).is_err());
    }

    #[test]
    fn classifies_commands() {
        for ok in ["status", "log --oneline -5", "diff HEAD~1", "show abc123", "branch", "branch -a", "branch --contains abc", "tag", "tag -l v*", "remote -v", "stash list", "reflog", "config --get user.name"] {
            assert_eq!(class(ok), GitClass::Read, "{ok}");
        }
        for w in ["add -A", "commit -m x", "fetch origin", "branch feature", "tag v1", "switch main", "switch -c topic", "checkout -b topic", "stash", "stash pop", "merge topic", "restore --staged f.txt", "branch -m new"] {
            assert_eq!(class(w), GitClass::Write, "{w}");
        }
        for c in [
            "push", "push origin main", "push -u origin topic", "pull", "pull --rebase", "rebase main", "reset --hard HEAD~1", "clean -fd", "commit --amend", "branch -D old", "tag -d v1",
            "stash drop", "remote add up u", "checkout main", "checkout -- f.txt", "restore f.txt", "switch -f main", "push --force-with-lease origin topic",
        ] {
            assert_eq!(class(c), GitClass::Confirm, "{c}");
        }
    }

    #[test]
    fn refuses_what_could_run_code_or_escape() {
        for b in [
            "-c core.sshCommand=evil push", "-C /tmp status", "--git-dir=/x status", "status --git-dir=/x", "log --output=/tmp/x", "diff --ext-diff", "fetch --upload-pack=evil",
            "push --force origin main", "push -f", "push origin +main", "push --force origin master", "push --mirror", "push origin --delete topic", "gc", "filter-branch", "config user.name x",
            "config --global alias.x '!sh'", "reflog expire --all", "daemon", "am x.patch", "grep -Oless foo",
        ] {
            assert!(is_blocked(b), "{b} should be blocked, got {:?}", class(b));
        }
        // A force-push to a named, non-protected branch is allowed to ask for confirmation.
        assert_eq!(class("push --force origin topic"), GitClass::Confirm);
    }

    fn git(dir: &Path, args: &[&str]) {
        let ok = std::process::Command::new("git")
            .args(["-c", "user.name=Test", "-c", "user.email=t@example.com", "-c", "commit.gpgsign=false"])
            .args(args)
            .current_dir(dir)
            .status()
            .unwrap()
            .success();
        assert!(ok, "git {args:?}");
    }

    fn have_git() -> bool {
        std::process::Command::new("git").arg("--version").output().is_ok()
    }

    async fn run(ws: &Path, level: PermissionLevel, args: Value, confirmed: bool) -> Result<GitOutput> {
        execute(ws, level, &args, confirmed).await
    }

    #[tokio::test]
    async fn status_log_and_commit_in_a_real_repository() {
        if !have_git() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        git(ws, &["init", "-q", "-b", "main"]);
        std::fs::write(ws.join("a.txt"), "one").unwrap();
        let full = PermissionLevel::Full;
        let ro = PermissionLevel::ReadOnly;

        let st = run(ws, ro, json!({ "args": ["status", "--short"] }), false).await.unwrap();
        assert!(st.text.contains("exit code: 0") && st.text.contains("a.txt"), "{}", st.text);

        // Read-only agents cannot change anything.
        let err = run(ws, ro, json!({ "args": ["add", "-A"] }), false).await.unwrap_err();
        assert!(matches!(err, CoreError::Permission(_)));

        run(ws, full, json!({ "args": ["add", "-A"] }), false).await.unwrap();
        let committed = run(ws, full, json!({ "args": ["-c", "user.name=x", "commit", "-m", "x"] }), false).await.unwrap_err();
        assert!(matches!(committed, CoreError::Permission(_)), "global options are refused");
        // Commit through the tool with identity from the environment-free local config.
        git(ws, &["config", "user.name", "Test"]);
        git(ws, &["config", "user.email", "t@example.com"]);
        git(ws, &["config", "commit.gpgsign", "false"]);
        let out = run(ws, full, json!({ "args": ["commit", "-m", "first commit"] }), false).await.unwrap();
        assert!(out.text.contains("exit code: 0"), "{}", out.text);
        let log = run(ws, ro, json!({ "args": ["log", "--oneline"] }), false).await.unwrap();
        assert!(log.text.contains("first commit"));
    }

    #[tokio::test]
    async fn confirm_class_commands_do_not_run_unconfirmed() {
        if !have_git() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path();
        git(ws, &["init", "-q", "-b", "main"]);
        std::fs::write(ws.join("a.txt"), "one").unwrap();
        let out = run(ws, PermissionLevel::Full, json!({ "args": ["clean", "-fd"] }), false).await.unwrap();
        assert_eq!(out.pending.as_deref(), Some("clean -fd"));
        assert!(out.text.starts_with("Not run."));
        assert!(ws.join("a.txt").exists());
        let out = run(ws, PermissionLevel::Full, json!({ "args": ["clean", "-fd"] }), true).await.unwrap();
        assert!(out.pending.is_none() && out.text.contains("exit code: 0"));
        assert!(!ws.join("a.txt").exists());
        // Read-only agents cannot even ask.
        assert!(run(ws, PermissionLevel::ReadOnly, json!({ "args": ["clean", "-fd"] }), true).await.is_err());
    }

    #[tokio::test]
    async fn git_does_not_climb_above_the_workspace() {
        if !have_git() {
            return;
        }
        let dir = tempfile::tempdir().unwrap();
        git(dir.path(), &["init", "-q", "-b", "main"]);
        let ws = dir.path().join("inner");
        std::fs::create_dir_all(&ws).unwrap();
        let out = run(&ws, PermissionLevel::ReadOnly, json!({ "args": ["status"] }), false).await.unwrap();
        assert!(out.text.contains("not a git repository"), "{}", out.text);
        assert!(run(&ws, PermissionLevel::ReadOnly, json!({ "args": ["status"], "cwd": "../.." }), false).await.is_err());
    }
}
