//! What a run changed in the workspace, and undoing it. A snapshot is taken just before the first action of a run
//! that can change files; when the run ends a second one is taken and the difference is attached to the message.

use std::path::PathBuf;

use super::*;
use crate::checkpoint::{self, RevertReport};

/// The snapshot a run started from.
pub(super) struct RunCheckpoint {
    workspace: PathBuf,
    before: String,
}

/// Whether a tool call can change files in the workspace.
pub(super) fn mutates_workspace(name: &str, args: &Value) -> bool {
    match name {
        "write_file" | "edit_file" | "run_command" => true,
        "git" => crate::gittool::parse_args(args).is_ok_and(|a| !matches!(crate::gittool::classify(&a), crate::gittool::GitClass::Read | crate::gittool::GitClass::Blocked(_))),
        _ => false,
    }
}

impl Core {
    fn shadow_repo(&self, chat_id: &str) -> PathBuf {
        checkpoint::repo_dir(self.data_dir(), chat_id)
    }

    /// Snapshot the workspace before the run's first change. Failing to snapshot never stops the work: the run goes
    /// ahead and the message says undo is unavailable.
    pub(super) async fn checkpoint_before(&self, chat_id: &str, workspace: &str, stats: &mut RunStats) {
        if stats.checkpoint.is_some() || stats.checkpoint_error.is_some() {
            return;
        }
        let ws = PathBuf::from(workspace);
        match checkpoint::snapshot(&self.shadow_repo(chat_id), &ws, "before run").await {
            Ok(before) => stats.checkpoint = Some(RunCheckpoint { workspace: ws, before }),
            Err(e) => stats.checkpoint_error = Some(e.to_string()),
        }
    }

    /// Record what the run changed on its message (called once, when the run ends, whatever way it ended).
    pub(super) async fn finish_checkpoint(&self, chat_id: &str, stats: &RunStats, msg: &mut Message) {
        if let Some(why) = &stats.checkpoint_error {
            msg.meta["changesUnavailable"] = json!(truncate(why, 200));
        }
        let Some(cp) = &stats.checkpoint else { return };
        let shadow = self.shadow_repo(chat_id);
        let result = async {
            let after = checkpoint::snapshot(&shadow, &cp.workspace, "after run").await?;
            let files = checkpoint::changes(&shadow, &cp.workspace, &cp.before, &after).await?;
            Ok::<_, CoreError>((after, files))
        }
        .await;
        match result {
            Ok((_, files)) if files.is_empty() => {}
            Ok((after, files)) => {
                msg.meta["changes"] = json!({ "before": cp.before, "after": after, "files": files, "reverted": false });
            }
            Err(e) => msg.meta["changesUnavailable"] = json!(truncate(&e.to_string(), 200)),
        }
    }

    fn changes_of(&self, message_id: &str) -> Result<(Message, Chat, PathBuf, String, String)> {
        let msg = self.db.get_message(message_id)?;
        let before = msg.meta["changes"]["before"].as_str().map(String::from);
        let after = msg.meta["changes"]["after"].as_str().map(String::from);
        let (Some(before), Some(after)) = (before, after) else {
            return Err(CoreError::invalid("this message did not change any files"));
        };
        let chat = self.db.get_chat(&msg.chat_id)?;
        let ws = chat
            .workspace_path
            .clone()
            .filter(|p| std::path::Path::new(p).is_dir())
            .ok_or_else(|| CoreError::invalid("this chat has no workspace folder any more"))?;
        Ok((msg, chat, PathBuf::from(ws), before, after))
    }

    /// The unified diff of everything the run changed.
    pub async fn run_diff(&self, message_id: &str) -> Result<String> {
        let (_, chat, ws, before, after) = self.changes_of(message_id)?;
        checkpoint::patch(&self.shadow_repo(&chat.id), &ws, &before, &after).await
    }

    /// Put the files a run changed back the way they were. Files edited again since are left alone and reported.
    pub async fn revert_run(self: &Arc<Self>, message_id: &str) -> Result<RevertReport> {
        let (mut msg, chat, ws, before, after) = self.changes_of(message_id)?;
        if msg.meta["changes"]["reverted"].as_bool() == Some(true) {
            return Err(CoreError::invalid("these changes were already reverted"));
        }
        let report = checkpoint::revert(&self.shadow_repo(&chat.id), &ws, &before, &after).await?;
        msg.meta["changes"]["reverted"] = json!(true);
        msg.meta["changes"]["revert"] = json!(report);
        self.save_message(&msg)?;
        let who = msg.agent_id.as_deref().and_then(|id| self.db.get_agent(id).ok()).map(|a| a.name).unwrap_or_else(|| "the agent".into());
        let left = if report.skipped.is_empty() { String::new() } else { format!(" {} left alone because they changed again afterwards.", report.skipped.len()) };
        let lincoln = self.agent_by_slug("lincoln")?;
        self.post(&chat.id, &lincoln.id, MessageKind::Notice, &format!("↩️ Reverted {} file(s) changed by {who}.{left}", report.restored.len()), json!({ "notice": "revert" }))?;
        self.note_revert(&chat, &msg, report.restored.len());
        self.schedule_learning();
        Ok(report)
    }
}
