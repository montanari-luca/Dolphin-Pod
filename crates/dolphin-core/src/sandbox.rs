//! Confines `run_command` to the chat's workspace folder.
//!
//! File tools are already confined by `permissions::resolve_in_workspace`; shell commands can do
//! anything the user can, so they run inside an OS sandbox where one exists:
//! - macOS: `sandbox-exec` with a profile that denies every write outside the workspace, temp
//!   folders and the usual package caches, and hides credential folders from reads.
//! - Linux: `bwrap` (bubblewrap) with the root mounted read-only and only the workspace writable.
//! Commands have no network access unless the chat opted in: on macOS outbound connections, DNS and unix sockets are
//! denied (loopback still works for local servers and tests), on Linux the command gets its own empty network namespace.
//! The workspace's own `.git/hooks` and `.git/config` are read-only to commands: git runs outside the sandbox with the
//! user's credentials, and a hook or config entry planted from inside would run there. Environment variables that look
//! like secrets are not passed on.
//! - Windows (and Linux without bubblewrap): no sandbox is available. The command still starts in the
//!   workspace, but nothing stops it from touching other paths or the network, and the tool output says so.

use std::path::{Path, PathBuf};
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;

use crate::error::{CoreError, Result};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Confinement {
    /// Writes outside the workspace are blocked by the OS.
    Sandboxed,
    /// No sandbox on this platform: only the working directory is set.
    Unconfined,
}

pub const UNCONFINED_NOTE: &str =
    "[note: this platform has no command sandbox, so this command was not confined to the workspace]";

pub const NO_NETWORK_NOTE: &str =
    "[note: commands in this chat have no network access, so downloads and installs fail. The user can allow it in the chat settings.]";

/// Build the command for `script` running in `cwd`, confined to `workspace` when the platform allows.
pub fn command(script: &str, workspace: &Path, cwd: &Path, network: bool) -> (Command, Confinement) {
    let (mut c, confinement) = build(script, workspace, cwd, network);
    scrub_env(&mut c);
    (c, confinement)
}

/// Whether an environment variable name suggests it holds a secret (API keys, tokens, passwords, agent sockets).
pub fn looks_secret(name: &str) -> bool {
    let n = name.to_uppercase();
    ["KEY", "TOKEN", "SECRET", "PASSWORD", "PASSWD", "CREDENTIAL", "AUTH", "COOKIE"].iter().any(|w| n.contains(w))
        || ["AWS_", "GITHUB_", "GITLAB_", "AZURE_"].iter().any(|p| n.starts_with(p))
}

fn scrub_env(c: &mut Command) {
    for (k, _) in std::env::vars_os() {
        if looks_secret(&k.to_string_lossy()) {
            c.env_remove(&k);
        }
    }
}

fn build(script: &str, workspace: &Path, cwd: &Path, network: bool) -> (Command, Confinement) {
    if cfg!(windows) {
        let mut c = Command::new("cmd");
        c.arg("/C").arg(script).current_dir(cwd);
        return (c, Confinement::Unconfined);
    }
    #[cfg(target_os = "macos")]
    if Path::new("/usr/bin/sandbox-exec").exists() {
        let mut c = Command::new("/usr/bin/sandbox-exec");
        c.arg("-p").arg(macos_profile(workspace, network)).arg("sh").arg("-c").arg(script).current_dir(cwd);
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
        if !network {
            c.arg("--unshare-net");
        }
        for dir in cache_dirs() {
            if dir.exists() {
                c.arg("--bind").arg(&dir).arg(&dir);
            }
        }
        // Credential folders are replaced by empty ones, and the repository's hooks and config cannot be changed.
        for dir in secret_dirs() {
            if dir.is_dir() {
                c.arg("--tmpfs").arg(&dir);
            }
        }
        for path in protected_git_paths(workspace) {
            if path.exists() {
                c.arg("--ro-bind").arg(&path).arg(&path);
            }
        }
        return (c, Confinement::Sandboxed);
    }
    let _ = (workspace, network);
    let mut c = Command::new("sh");
    c.arg("-c").arg(script).current_dir(cwd);
    (c, Confinement::Unconfined)
}

/// What a finished command left behind.
pub struct Finished {
    pub code: Option<i32>,
    pub success: bool,
    pub stdout: Vec<u8>,
    pub stderr: Vec<u8>,
    /// More output was produced than kept.
    pub truncated: bool,
}

/// How much of each output stream is kept; the rest is read and thrown away so the command never blocks on a full pipe.
const MAX_CAPTURE: usize = 1_000_000;

async fn capture(mut r: impl AsyncRead + Unpin) -> (Vec<u8>, bool) {
    let (mut kept, mut cut) = (Vec::new(), false);
    let mut chunk = [0u8; 16 * 1024];
    loop {
        match r.read(&mut chunk).await {
            Ok(0) | Err(_) => return (kept, cut),
            Ok(n) => {
                let room = MAX_CAPTURE.saturating_sub(kept.len());
                kept.extend_from_slice(&chunk[..n.min(room)]);
                cut |= n > room;
            }
        }
    }
}

/// Kills a command's whole process group (it and everything it started) unless disarmed: when the command times out,
/// or when the run is stopped and the future is dropped.
struct GroupKill(Option<u32>);

impl GroupKill {
    fn fire(&mut self) {
        #[cfg(unix)]
        if let Some(pid) = self.0.take() {
            // SAFETY: killpg only signals the process group we created for this command.
            unsafe { libc::killpg(pid as i32, libc::SIGKILL) };
        }
    }
}

impl Drop for GroupKill {
    fn drop(&mut self) {
        self.fire();
    }
}

/// Run `c` to completion, at most `timeout`. Its output is capped, and on timeout (or if this future is dropped)
/// the command and everything it started are killed, not just the shell.
pub async fn run(mut c: Command, timeout: Duration) -> Result<Finished> {
    c.stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped()).kill_on_drop(true);
    #[cfg(unix)]
    c.process_group(0);
    let mut child = c.spawn()?;
    let mut group = GroupKill(child.id());
    let (out, err) = (child.stdout.take().expect("piped"), child.stderr.take().expect("piped"));
    let work = async { tokio::join!(child.wait(), capture(out), capture(err)) };
    match tokio::time::timeout(timeout, work).await {
        Ok((status, (stdout, cut_out), (stderr, cut_err))) => {
            let status = status?;
            // The command finished by itself; anything it left running in the background is left alone.
            group.0 = None;
            Ok(Finished { code: status.code(), success: status.success(), stdout, stderr, truncated: cut_out || cut_err })
        }
        Err(_) => {
            group.fire();
            Err(CoreError::invalid(format!("command timed out after {}s and was stopped", timeout.as_secs())))
        }
    }
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

/// Files inside the workspace's repository that decide what git runs: commands may not change them.
fn protected_git_paths(workspace: &Path) -> Vec<PathBuf> {
    vec![workspace.join(".git/hooks"), workspace.join(".git/config")]
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
/// reads of credential folders, and, unless `network` is on, outbound network except to this machine.
pub fn macos_profile(workspace: &Path, network: bool) -> String {
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
    p.push_str("(deny file-write*\n");
    for path in protected_git_paths(workspace) {
        p.push_str(&format!("  (subpath {})\n", sbpl(&path)));
    }
    p.push_str(")\n");
    if !network {
        p.push_str("(deny network-outbound)\n(allow network-outbound (remote ip \"localhost:*\"))\n");
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

    use super::run as run_with;

    async fn run(script: &str, ws: &Path) -> (String, Confinement) {
        let (mut c, conf) = command(script, ws, ws, false);
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

    /// A throwaway HTTP server on loopback; returns its port.
    fn loopback_server() -> u16 {
        use std::io::{Read, Write};
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten().take(4) {
                let mut stream = stream;
                let mut buf = [0u8; 1024];
                let _ = stream.read(&mut buf);
                let _ = stream.write_all(b"HTTP/1.0 200 OK\r\nContent-Length: 2\r\n\r\nhi");
            }
        });
        port
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn commands_have_no_network_unless_allowed_but_loopback_works() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let port = loopback_server();
        let (out, _) = run(&format!("curl -s -m 5 http://127.0.0.1:{port}/"), &ws).await;
        assert_eq!(out.trim(), "hi", "loopback must keep working: {out}");
        let (out, _) = run("/bin/bash -c 'exec 3<>/dev/tcp/1.1.1.1/80' 2>&1; echo done", &ws).await;
        assert!(out.contains("Operation not permitted"), "outbound connections must be denied: {out}");
        let (out, _) = run("/usr/bin/dscacheutil -q host -a name example.com", &ws).await;
        assert!(out.trim().is_empty(), "DNS lookups must be denied (they leak data): {out}");
    }

    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn the_repositorys_hooks_and_config_are_read_only_to_commands() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        std::fs::create_dir_all(ws.join(".git/hooks")).unwrap();
        std::fs::write(ws.join(".git/config"), "[core]\n").unwrap();
        let (out, _) = run("echo evil > .git/hooks/pre-commit; echo evil >> .git/config; echo ref > .git/HEAD; echo done", &ws).await;
        assert!(out.contains("done"), "{out}");
        assert!(!ws.join(".git/hooks/pre-commit").exists(), "a hook must not be plantable: {out}");
        assert_eq!(std::fs::read_to_string(ws.join(".git/config")).unwrap(), "[core]\n", "{out}");
        assert_eq!(std::fs::read_to_string(ws.join(".git/HEAD")).unwrap().trim(), "ref", "the rest of .git stays writable so git commands work");
    }

    #[test]
    fn secret_looking_variables_are_not_passed_on() {
        for name in ["OPENAI_API_KEY", "GITHUB_TOKEN", "AWS_ACCESS_KEY_ID", "DB_PASSWORD", "SSH_AUTH_SOCK", "npm_config_authtoken", "MY_SECRET"] {
            assert!(looks_secret(name), "{name}");
        }
        for name in ["PATH", "HOME", "LANG", "TERM", "SHELL", "USER", "TMPDIR", "CARGO_HOME"] {
            assert!(!looks_secret(name), "{name}");
        }
        let dir = tempfile::tempdir().unwrap();
        let (c, _) = command("true", dir.path(), dir.path(), false);
        let removed: Vec<_> = c.as_std().get_envs().filter(|(_, v)| v.is_none()).map(|(k, _)| k.to_string_lossy().into_owned()).collect();
        for (k, _) in std::env::vars_os() {
            let k = k.to_string_lossy().into_owned();
            assert_eq!(removed.contains(&k), looks_secret(&k), "{k}");
        }
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn a_timeout_stops_everything_the_command_started() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let (c, _) = command("sleep 30 & echo $! > pid; sleep 30", &ws, &ws, false);
        let err = run_with(c, Duration::from_secs(1)).await.err().expect("times out");
        assert!(err.to_string().contains("timed out"), "{err}");
        let pid: i32 = std::fs::read_to_string(ws.join("pid")).unwrap().trim().parse().unwrap();
        tokio::time::sleep(Duration::from_millis(300)).await;
        // SAFETY: signal 0 only checks that the process exists.
        let alive = unsafe { libc::kill(pid, 0) } == 0;
        assert!(!alive, "the background process {pid} must be killed with the command");
    }

    #[tokio::test]
    async fn runaway_output_is_capped_not_buffered() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().canonicalize().unwrap();
        let (c, _) = command("head -c 3000000 /dev/zero | tr '\\0' x", &ws, &ws, false);
        let out = run_with(c, Duration::from_secs(30)).await.unwrap();
        assert_eq!(out.stdout.len(), MAX_CAPTURE);
        assert!(out.truncated && out.success);
    }

    #[test]
    fn the_profile_only_denies_the_network_when_asked_to() {
        let off = macos_profile(Path::new("/tmp/ws"), false);
        let on = macos_profile(Path::new("/tmp/ws"), true);
        assert!(off.contains("(deny network-outbound)"));
        assert!(!on.contains("network"));
    }

    #[test]
    fn profile_quotes_awkward_paths() {
        let p = macos_profile(Path::new("/tmp/we\"ird path"), false);
        assert!(p.contains("(subpath \"/tmp/we\\\"ird path\")"));
    }
}
