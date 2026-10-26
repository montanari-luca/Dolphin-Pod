//! Confines `run_command` to the chat's workspace folder.
//!
//! File tools are already confined by `permissions::resolve_in_workspace`; shell commands can do
//! anything the user can, so they run inside an OS sandbox where one exists:
//! - macOS: `sandbox-exec` with a profile that denies every write outside the workspace, temp
//!   folders and the usual package caches, and hides credential folders from reads.
//! - Linux: `bwrap` (bubblewrap) with the root mounted read-only and only the workspace writable.
//! Commands have no network access unless the chat opted in: on macOS outbound connections, DNS and unix sockets are
//! denied (loopback still works for local servers and tests), on Linux the command gets its own empty network namespace.
//! - Windows (and Linux without bubblewrap): no sandbox is available. The command still starts in the
//!   workspace, but nothing stops it from touching other paths or the network, and the tool output says so.

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

pub const NO_NETWORK_NOTE: &str =
    "[note: commands in this chat have no network access, so downloads and installs fail. The user can allow it in the chat settings.]";

/// Build the command for `script` running in `cwd`, confined to `workspace` when the platform allows.
pub fn command(script: &str, workspace: &Path, cwd: &Path, network: bool) -> (Command, Confinement) {
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
        return (c, Confinement::Sandboxed);
    }
    let _ = (workspace, network);
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
    if !network {
        p.push_str("(deny network-outbound)\n(allow network-outbound (remote ip \"localhost:*\"))\n");
    }
    p
}

#[cfg(test)]
mod tests {
    use super::*;

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
