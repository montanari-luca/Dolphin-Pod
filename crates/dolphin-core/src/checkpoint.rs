//! Undo for agent runs: a shadow git repository that records the workspace before and after a run.
//!
//! The repository lives in the app's data folder, never in the workspace, and the workspace's own `.git` is
//! left alone (git never tracks another repository's `.git` folder). `.gitignore` files are honoured, so
//! dependency and build folders stay out. A snapshot is a commit of the whole working tree; a run's changes are the
//! difference between its first and last snapshot, and reverting puts those files back, but only the ones the run
//! touched and nobody changed since.

use std::path::{Component, Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
use tokio::process::Command;
use ts_rs::TS;

use crate::error::{CoreError, Result};
use crate::permissions::resolve_in_workspace;

const TIMEOUT: Duration = Duration::from_secs(120);
const MAX_PATCH: usize = 200_000;

/// Folders that are never snapshotted even when the project forgot to ignore them.
const ALWAYS_EXCLUDED: &str = "node_modules/\ntarget/\n.venv/\nvenv/\n__pycache__/\n.DS_Store\n*.pyc\n";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "snake_case")]
#[ts(export)]
pub enum ChangeKind {
    Added,
    Modified,
    Deleted,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct FileChange {
    pub path: String,
    pub kind: ChangeKind,
    pub added: u32,
    pub removed: u32,
}

/// What reverting a run did.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RevertReport {
    /// Files put back the way they were before the run.
    pub restored: Vec<String>,
    /// Files left alone because they changed again after the run.
    pub skipped: Vec<String>,
}

/// Where a chat's shadow repository lives.
pub fn repo_dir(data_dir: &Path, chat_id: &str) -> PathBuf {
    let safe: String = chat_id.chars().map(|c| if c.is_ascii_alphanumeric() || c == '-' || c == '_' { c } else { '_' }).collect();
    data_dir.join("checkpoints").join(safe)
}

fn git(shadow: &Path, workspace: &Path) -> Command {
    let mut c = Command::new("git");
    c.arg("--no-pager")
        .arg("--literal-pathspecs")
        .arg("--git-dir")
        .arg(shadow)
        .arg("--work-tree")
        .arg(workspace)
        .args([
            "-c", "user.name=Dolphin Pod", "-c", "user.email=checkpoint@dolphin.pod", "-c", "commit.gpgsign=false",
            "-c", "core.autocrlf=false", "-c", "core.quotepath=off", "-c", "core.fsmonitor=false", "-c", "gc.auto=0",
            "-c", "core.hooksPath=/dev/null", "-c", "core.safecrlf=false",
        ])
        .current_dir(workspace);
    for var in ["GIT_DIR", "GIT_WORK_TREE", "GIT_INDEX_FILE", "GIT_EXEC_PATH", "GIT_CONFIG", "GIT_CONFIG_COUNT"] {
        c.env_remove(var);
    }
    // The user's own git settings (hooks, templates, ignore rules) must not change what a snapshot holds.
    c.env("GIT_CONFIG_GLOBAL", "/dev/null").env("GIT_CONFIG_NOSYSTEM", "1").env("GIT_TERMINAL_PROMPT", "0").env("LC_ALL", "C");
    c.kill_on_drop(true).stdin(std::process::Stdio::null());
    c
}

async fn run(mut c: Command) -> Result<Vec<u8>> {
    let out = match tokio::time::timeout(TIMEOUT, c.output()).await {
        Ok(Ok(o)) => o,
        Ok(Err(e)) if e.kind() == std::io::ErrorKind::NotFound => return Err(CoreError::invalid("git is not installed on this device, so runs cannot be undone")),
        Ok(Err(e)) => return Err(e.into()),
        Err(_) => return Err(CoreError::invalid("taking a snapshot timed out")),
    };
    if !out.status.success() {
        return Err(CoreError::invalid(format!("snapshot failed: {}", String::from_utf8_lossy(&out.stderr).trim())));
    }
    Ok(out.stdout)
}

async fn text(c: Command) -> Result<String> {
    Ok(String::from_utf8_lossy(&run(c).await?).trim().to_string())
}

async fn ensure_repo(shadow: &Path, workspace: &Path) -> Result<()> {
    if shadow.join("HEAD").exists() {
        return Ok(());
    }
    std::fs::create_dir_all(shadow)?;
    run({
        let mut c = git(shadow, workspace);
        c.args(["init", "-q"]);
        c
    })
    .await?;
    std::fs::create_dir_all(shadow.join("info"))?;
    std::fs::write(shadow.join("info").join("exclude"), ALWAYS_EXCLUDED)?;
    Ok(())
}

/// Record the workspace as it is now and return the snapshot's id.
pub async fn snapshot(shadow: &Path, workspace: &Path, label: &str) -> Result<String> {
    ensure_repo(shadow, workspace).await?;
    run({
        let mut c = git(shadow, workspace);
        c.args(["add", "-A", "--ignore-errors", "."]);
        c
    })
    .await?;
    run({
        let mut c = git(shadow, workspace);
        c.args(["commit", "-q", "--allow-empty", "--no-verify", "-m", label]);
        c
    })
    .await?;
    text({
        let mut c = git(shadow, workspace);
        c.args(["rev-parse", "HEAD"]);
        c
    })
    .await
}

fn check_id(id: &str) -> Result<()> {
    if id.len() < 7 || !id.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(CoreError::invalid("not a snapshot id"));
    }
    Ok(())
}

/// The files that differ between two snapshots, with line counts.
pub async fn changes(shadow: &Path, workspace: &Path, from: &str, to: &str) -> Result<Vec<FileChange>> {
    check_id(from)?;
    check_id(to)?;
    let status = run({
        let mut c = git(shadow, workspace);
        c.args(["diff", "--no-renames", "--name-status", "-z", from, to]);
        c
    })
    .await?;
    let numstat = run({
        let mut c = git(shadow, workspace);
        c.args(["diff", "--no-renames", "--numstat", "-z", from, to]);
        c
    })
    .await?;
    let mut counts = std::collections::HashMap::new();
    for rec in String::from_utf8_lossy(&numstat).split('\0').filter(|r| !r.is_empty()) {
        let mut it = rec.splitn(3, '\t');
        if let (Some(a), Some(r), Some(path)) = (it.next(), it.next(), it.next()) {
            // Binary files show "-" for both counts.
            counts.insert(path.to_string(), (a.parse().unwrap_or(0u32), r.parse().unwrap_or(0u32)));
        }
    }
    let status = String::from_utf8_lossy(&status).into_owned();
    let mut parts = status.split('\0').filter(|p| !p.is_empty());
    let mut out = Vec::new();
    while let (Some(code), Some(path)) = (parts.next(), parts.next()) {
        let kind = match code {
            "A" => ChangeKind::Added,
            "D" => ChangeKind::Deleted,
            _ => ChangeKind::Modified,
        };
        let (added, removed) = counts.get(path).copied().unwrap_or((0, 0));
        out.push(FileChange { path: path.to_string(), kind, added, removed });
    }
    Ok(out)
}

/// The unified diff between two snapshots, cut off at a size the UI can show.
pub async fn patch(shadow: &Path, workspace: &Path, from: &str, to: &str) -> Result<String> {
    check_id(from)?;
    check_id(to)?;
    let out = text({
        let mut c = git(shadow, workspace);
        c.args(["diff", "--no-renames", "--no-color", "--no-ext-diff", from, to]);
        c
    })
    .await?;
    Ok(if out.len() > MAX_PATCH { format!("{}\n… (diff cut off)", &out[..out.floor_char_boundary(MAX_PATCH)]) } else { out })
}

/// Undo the changes between `before` and `after`. Files that differ from `after` right now were edited by someone
/// else since and are left as they are. The workspace is snapshotted first, so a revert can itself be undone.
pub async fn revert(shadow: &Path, workspace: &Path, before: &str, after: &str) -> Result<RevertReport> {
    check_id(before)?;
    check_id(after)?;
    for id in [before, after] {
        run({
            let mut c = git(shadow, workspace);
            c.args(["cat-file", "-e", &format!("{id}^{{commit}}")]);
            c
        })
        .await
        .map_err(|_| CoreError::invalid("this run's snapshot is not on this device (it was made on another device, or has been removed)"))?;
    }
    let run_files = changes(shadow, workspace, before, after).await?;
    let now = snapshot(shadow, workspace, "before revert").await?;
    let since = changes(shadow, workspace, after, &now).await?;
    let mut report = RevertReport::default();
    for f in run_files {
        if since.iter().any(|s| s.path == f.path) {
            report.skipped.push(f.path);
            continue;
        }
        if Path::new(&f.path).components().any(|c| !matches!(c, Component::Normal(_))) {
            continue;
        }
        let target = resolve_in_workspace(workspace, &f.path)?;
        match f.kind {
            ChangeKind::Added => {
                match tokio::fs::remove_file(&target).await {
                    Ok(()) => {}
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                    Err(e) => return Err(e.into()),
                }
                // Folders the run created and nothing else uses go with it.
                let root = workspace.canonicalize()?;
                let mut dir = target.parent().map(Path::to_path_buf);
                while let Some(d) = dir {
                    if d == root || tokio::fs::remove_dir(&d).await.is_err() {
                        break;
                    }
                    dir = d.parent().map(Path::to_path_buf);
                }
            }
            ChangeKind::Modified | ChangeKind::Deleted => {
                run({
                    let mut c = git(shadow, workspace);
                    c.args(["checkout", before, "--", &f.path]);
                    c
                })
                .await?;
            }
        }
        report.restored.push(f.path);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn have_git() -> bool {
        std::process::Command::new("git").arg("--version").output().is_ok()
    }

    struct Fixture {
        ws: tempfile::TempDir,
        shadow: tempfile::TempDir,
    }

    fn fixture() -> Fixture {
        let ws = tempfile::tempdir().unwrap();
        std::fs::write(ws.path().join("keep.txt"), "keep\n").unwrap();
        std::fs::write(ws.path().join("edit.txt"), "one\ntwo\n").unwrap();
        std::fs::write(ws.path().join("gone.txt"), "bye\n").unwrap();
        Fixture { ws, shadow: tempfile::tempdir().unwrap() }
    }

    fn w(f: &Fixture) -> &Path {
        f.ws.path()
    }

    fn s(f: &Fixture) -> PathBuf {
        f.shadow.path().join("repo")
    }

    /// A run that edits, deletes and creates files (one of them in a new folder).
    fn do_run(f: &Fixture) {
        std::fs::write(w(f).join("edit.txt"), "one\ntwo\nthree\n").unwrap();
        std::fs::remove_file(w(f).join("gone.txt")).unwrap();
        std::fs::create_dir_all(w(f).join("new/deep")).unwrap();
        std::fs::write(w(f).join("new/deep/file.txt"), "fresh\n").unwrap();
    }

    #[tokio::test]
    async fn a_run_is_listed_diffed_and_reverted() {
        if !have_git() {
            return;
        }
        let f = fixture();
        let before = snapshot(&s(&f), w(&f), "before").await.unwrap();
        do_run(&f);
        let after = snapshot(&s(&f), w(&f), "after").await.unwrap();

        let mut files = changes(&s(&f), w(&f), &before, &after).await.unwrap();
        files.sort_by(|a, b| a.path.cmp(&b.path));
        assert_eq!(
            files,
            vec![
                FileChange { path: "edit.txt".into(), kind: ChangeKind::Modified, added: 1, removed: 0 },
                FileChange { path: "gone.txt".into(), kind: ChangeKind::Deleted, added: 0, removed: 1 },
                FileChange { path: "new/deep/file.txt".into(), kind: ChangeKind::Added, added: 1, removed: 0 },
            ]
        );
        let p = patch(&s(&f), w(&f), &before, &after).await.unwrap();
        assert!(p.contains("+three") && p.contains("-bye") && p.contains("+fresh"), "{p}");

        let report = revert(&s(&f), w(&f), &before, &after).await.unwrap();
        assert_eq!(report.restored.len(), 3);
        assert!(report.skipped.is_empty());
        assert_eq!(std::fs::read_to_string(w(&f).join("edit.txt")).unwrap(), "one\ntwo\n");
        assert_eq!(std::fs::read_to_string(w(&f).join("gone.txt")).unwrap(), "bye\n");
        assert!(!w(&f).join("new").exists(), "the folder the run created goes away with its file");
        assert_eq!(std::fs::read_to_string(w(&f).join("keep.txt")).unwrap(), "keep\n");
    }

    #[tokio::test]
    async fn files_edited_after_the_run_are_left_alone() {
        if !have_git() {
            return;
        }
        let f = fixture();
        let before = snapshot(&s(&f), w(&f), "before").await.unwrap();
        do_run(&f);
        let after = snapshot(&s(&f), w(&f), "after").await.unwrap();
        // The user keeps working on one of the files.
        std::fs::write(w(&f).join("edit.txt"), "mine now\n").unwrap();

        let report = revert(&s(&f), w(&f), &before, &after).await.unwrap();
        assert_eq!(report.skipped, ["edit.txt"]);
        assert_eq!(std::fs::read_to_string(w(&f).join("edit.txt")).unwrap(), "mine now\n");
        assert_eq!(std::fs::read_to_string(w(&f).join("gone.txt")).unwrap(), "bye\n");
    }

    #[tokio::test]
    async fn a_revert_can_itself_be_undone_and_the_workspace_git_is_untouched() {
        if !have_git() {
            return;
        }
        let f = fixture();
        std::process::Command::new("git").args(["init", "-q"]).current_dir(w(&f)).status().unwrap();
        let head = |dir: &Path| std::fs::read_to_string(dir.join(".git/HEAD")).unwrap();
        let ws_head = head(w(&f));
        let before = snapshot(&s(&f), w(&f), "before").await.unwrap();
        do_run(&f);
        let after = snapshot(&s(&f), w(&f), "after").await.unwrap();
        revert(&s(&f), w(&f), &before, &after).await.unwrap();
        // The state right before the revert is a snapshot too: reverting "to it" brings the run back.
        let log = text({
            let mut c = git(&s(&f), w(&f));
            c.args(["log", "--format=%H %s"]);
            c
        })
        .await
        .unwrap();
        let pre_revert = log.lines().find(|l| l.ends_with("before revert")).unwrap().split(' ').next().unwrap().to_string();
        let again = changes(&s(&f), w(&f), &pre_revert, &after).await.unwrap();
        assert!(again.is_empty(), "the pre-revert snapshot holds the run's result");
        assert_eq!(head(w(&f)), ws_head);
        // Nothing of the shadow repository leaks into the workspace.
        assert!(std::fs::read_dir(w(&f)).unwrap().flatten().all(|e| e.file_name() != "checkpoints"));
    }

    #[tokio::test]
    async fn ignored_and_excluded_folders_are_not_snapshotted() {
        if !have_git() {
            return;
        }
        let f = fixture();
        std::fs::write(w(&f).join(".gitignore"), "secret.env\n").unwrap();
        std::fs::create_dir_all(w(&f).join("node_modules/x")).unwrap();
        std::fs::write(w(&f).join("node_modules/x/i.js"), "1").unwrap();
        std::fs::write(w(&f).join("secret.env"), "k=1").unwrap();
        let before = snapshot(&s(&f), w(&f), "before").await.unwrap();
        std::fs::write(w(&f).join("node_modules/x/i.js"), "2").unwrap();
        std::fs::write(w(&f).join("secret.env"), "k=2").unwrap();
        let after = snapshot(&s(&f), w(&f), "after").await.unwrap();
        assert!(changes(&s(&f), w(&f), &before, &after).await.unwrap().is_empty());
    }

    #[tokio::test]
    async fn bad_ids_are_refused() {
        let f = fixture();
        assert!(changes(&s(&f), w(&f), "--output=/tmp/x", "abcdef1").await.is_err());
        assert!(patch(&s(&f), w(&f), "abcdef1", "HEAD; rm").await.is_err());
        assert!(revert(&s(&f), w(&f), "nothex!", "abcdef1").await.is_err());
    }
}
