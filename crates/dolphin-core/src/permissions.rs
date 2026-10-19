//! Permission enforcement for agent tools. Enforced here in the backend, never only by prompt text.

use std::path::{Component, Path, PathBuf};

use crate::error::{CoreError, Result};
use crate::model::PermissionLevel;

pub fn required_level(tool: &str) -> PermissionLevel {
    match tool {
        "write_file" | "edit_file" | "run_command" => PermissionLevel::Full,
        _ => PermissionLevel::ReadOnly,
    }
}

pub fn check(granted: PermissionLevel, tool: &str) -> Result<()> {
    if granted < required_level(tool) {
        return Err(CoreError::Permission(format!(
            "{tool} needs full agentic permissions; this agent is read-only in this chat. Ask the user to approve the plan or grant full permissions."
        )));
    }
    Ok(())
}

/// Resolve `requested` inside `workspace`, rejecting anything that escapes it (`..`, absolute paths
/// elsewhere, or symlinks pointing outside).
pub fn resolve_in_workspace(workspace: &Path, requested: &str) -> Result<PathBuf> {
    let root = workspace.canonicalize().map_err(|e| CoreError::invalid(format!("workspace unavailable: {e}")))?;
    let req = requested.trim();
    let req = if req.is_empty() { "." } else { req };
    let joined = if Path::new(req).is_absolute() { PathBuf::from(req) } else { root.join(req) };

    // Lexically normalize first so `..` cannot walk out before the file exists.
    let mut norm = PathBuf::new();
    for c in joined.components() {
        match c {
            Component::ParentDir => {
                if !norm.pop() {
                    return Err(escape(requested));
                }
            }
            Component::CurDir => {}
            other => norm.push(other),
        }
    }
    // Then resolve symlinks on the deepest existing ancestor.
    let mut existing = norm.clone();
    let mut tail = Vec::new();
    while !existing.exists() {
        match (existing.file_name().map(|s| s.to_os_string()), existing.parent()) {
            (Some(name), Some(parent)) => {
                tail.push(name);
                existing = parent.to_path_buf();
            }
            _ => return Err(escape(requested)),
        }
    }
    let mut resolved = existing.canonicalize()?;
    for name in tail.into_iter().rev() {
        resolved.push(name);
    }
    if !resolved.starts_with(&root) {
        return Err(escape(requested));
    }
    Ok(resolved)
}

fn escape(p: &str) -> CoreError {
    CoreError::Permission(format!("'{p}' is outside the chat's workspace folder"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_only_blocks_writes_and_commands() {
        assert!(check(PermissionLevel::ReadOnly, "read_file").is_ok());
        assert!(check(PermissionLevel::ReadOnly, "search").is_ok());
        assert!(check(PermissionLevel::ReadOnly, "write_file").is_err());
        assert!(check(PermissionLevel::ReadOnly, "run_command").is_err());
        assert!(check(PermissionLevel::Full, "run_command").is_ok());
    }

    #[test]
    fn paths_cannot_escape_workspace() {
        let dir = tempfile::tempdir().unwrap();
        let ws = dir.path().join("ws");
        std::fs::create_dir_all(ws.join("src")).unwrap();
        std::fs::write(dir.path().join("secret.txt"), "x").unwrap();
        assert!(resolve_in_workspace(&ws, "src/main.rs").unwrap().ends_with("ws/src/main.rs"));
        assert!(resolve_in_workspace(&ws, "new/dir/file.txt").is_ok());
        assert!(resolve_in_workspace(&ws, "../secret.txt").is_err());
        assert!(resolve_in_workspace(&ws, "src/../../secret.txt").is_err());
        assert!(resolve_in_workspace(&ws, dir.path().join("secret.txt").to_str().unwrap()).is_err());
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(dir.path(), ws.join("link")).unwrap();
            assert!(resolve_in_workspace(&ws, "link/secret.txt").is_err());
        }
    }
}
