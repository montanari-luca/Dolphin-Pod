//! Confines `run_command` to the chat's workspace folder.
//!
//! File tools are already confined by `permissions::resolve_in_workspace`; shell commands can do
//! anything the user can, so they run inside an OS sandbox where one exists:
//! - macOS: `sandbox-exec` with a profile that denies every write outside the workspace, temp
//!   folders and the usual package caches, and hides credential folders from reads.
//! - Linux: `bwrap` (bubblewrap) with the root mounted read-only and only the workspace writable.
//! - Windows (and Linux without bubblewrap): no sandbox is available. The command still starts in the
//!   workspace, but nothing stops it from touching other paths, and the tool output says so.

use std::path::{Path, PathBuf};

use tokio::process::Command;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confinement {
    /// Writes outside the workspace are blocked by the OS.
    Sandboxed,
    /// No sandbox on this platform: only the working directory is set.
    Unconfined,
}

pub const UNCONFINED_NOTE: &str =
    "[note: this platform has no command sandbox, so this command was not confined to the workspace]";

/// Build the command for `script` running in `cwd`, confined to `workspace` when the platform allows.
pub fn command(script: &str, workspace: &Path, cwd: &Path) -> (Command, Confinement) {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(script).current_dir(cwd);
        return (c, Confinement::Unconfined);
    }
    #[cfg(target_os = "macos")]
    if Path::new("/usr/bin/sandbox-exec").exists() {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.arg("-p").arg(macos_profile(workspace)).arg("sh").arg("-c").arg(script).current_dir(cwd);
        return (c, Confinement::Sandboxed);
    }
    #[cfg(target_os = "linux")]
    if let Some(bwrap) = find_in_path("bwrap") {
        let mut c = Command::new(bwrap);
        c.args(["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc", "--tmpfs", "/tmp", "--unshare-pid", "--die-with-parent"])
            .arg("--bind")
            .arg(workspace)
            .arg(workspace)
            .arg("--chdir")
            .arg(cwd)
            .args(["sh", "-c", script]);
        for dir in cache_dirs() {
            if dir.exists() {
                c.arg("--bind").arg(&dir).arg(&dir);
            }
        }
        return (c, Confinement::Sandboxed);
    }
    let _ = workspace;
    let mut c = Command::new("sh");
    c.arg("-c").arg(script).current_dir(cwd);
    (c, Confinement::Unconfined)
}

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME").or_else(|| std::env::var_os("USERPROFILE")).map(PathBuf::from)
}

/// Package-manager caches that builds legitimately write to.
fn cache_dirs() -> Vec<PathBuf> {
    let Some(h) = home() else { return vec![] };
    [".cargo", ".npm", ".cache", ".rustup", ".gradle", ".m2", "Library/Caches", ".local/share/pnpm"]
        .iter()
        .map(|d| h.join(d))
        .collect()
}

/// Credential folders that are hidden from commands entirely.
fn secret_dirs() -> Vec<PathBuf> {
    let Some(h) = home() else { return vec![] };
    [".ssh", ".aws", ".gnupg", ".config/gh", ".docker", "Library/Keychains", ".netrc"].iter().map(|d| h.join(d)).collect()
}

#[cfg(target_os = "linux")]
fn find_in_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH")?.to_str()?.split(':').map(|d| Path::new(d).join(name)).find(|p| p.is_file())
}

fn sbpl(p: &Path) -> String {
    let s = p.to_string_lossy();
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\""))
}

/// The macOS sandbox profile. Everything is allowed except writes outside a short allow-list and
/// reads of credential folders.
pub fn macos_profile(workspace: &Path) -> String {
    let mut p = String::from("(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*\n");
    let mut writable = vec![workspace.to_path_buf(), "/private/tmp".into(), "/private/var/folders".into()];
    writable.extend(cache_dirs().into_iter().filter(|d| d.exists()));
    for d in &writable {
        p.push_str(&format!("  (subpath {})\n", sbpl(d)));
    }
    p.push_str("  (literal \"/dev/null\") (literal \"/dev/tty\") (regex #\"^/dev/(fd|ttys?)[0-9]*$\"))\n");
    if !secret_dirs().is_empty() {
        p.push_str("(deny file-read*\n");
        for d in secret_dirs() {
            p.push_str(&format!("  (subpath {})\n", sbpl(&d)));
        }
        p.push_str(")\n");
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn run(script: &str, ws: &Path) -> (String, Confinement) {
        let (mut c, conf) = command(script, ws, ws);
        let out = c.output().await.unwrap();
        (format!("{}{}", String::from_utf8_lossy(&out.stdout), String::from_utf8_lossy(&out.stderr)), conf)
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn blocks_writes_outside_the_workspace_but_not_inside() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws").canonicalize().unwrap_or_else(|_| {
            std::fs::create_dir_all(dir.path().join("ws")).unwrap();
            dir.path().join("ws").canonicalize().unwrap()
        });
        let outside = std::env::current_dir().unwrap().join("sandbox-escape-probe.txt");
        let (out, conf) = run(&format!("echo ok > inside.txt && cat inside.txt; echo bad > '{}'", outside.display()), &ws).await;
        assert_eq!(conf, Confinement::Sandboxed);
        assert!(out.contains("ok"), "{out}");
        assert!(ws.join("inside.txt").exists());
        assert!(!outside.exists(), "write outside the workspace must be denied: {out}");
        assert!(out.to_lowercase().contains("not permitted") || out.to_lowercase().contains("denied"), "{out}");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn temp_folders_and_reads_still_work() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let (out, _) = run("mktemp >/dev/null && ls / >/dev/null && echo fine", &ws).await;
        assert!(out.contains("fine"), "{out}");
    }

    #[test]
    fn profile_quotes_awkward_paths() {
        let p = macos_profile(Path::new("/tmp/we\"ird path"));
        assert!(p.contains("(subpath \"/tmp/we\\\"ird path\")"));
    }
}
