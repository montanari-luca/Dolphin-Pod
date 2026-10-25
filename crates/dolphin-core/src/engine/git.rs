//! Backend side of Robert's confirmations: a push, pull or other confirm-class git command only runs when
//! the user answered after Robert asked. This is checked here, never trusted to the model's say-so.

use super::*;

impl Core {
    /// Whether `command` was asked about (and left pending) by an earlier, finished message of this chat and the
    /// user has written since. A confirmation is spent when it is taken: one yes allows one run.
    pub(super) fn take_git_confirmation(&self, chat_id: &str, command: &str) -> bool {
        let Ok(msgs) = self.db.list_messages(chat_id, 80) else { return false };
        let pending_at = msgs.iter().rposition(|m| {
            m.agent_id.is_some()
                && m.status != MessageStatus::Streaming
                && m.meta["gitPending"].as_array().is_some_and(|l| l.iter().any(|c| c.as_str() == Some(command)))
        });
        let Some(i) = pending_at else { return false };
        if !msgs[i + 1..].iter().any(|m| m.agent_id.is_none()) {
            return false;
        }
        let mut m = msgs[i].clone();
        if let Some(list) = m.meta["gitPending"].as_array_mut()
            && let Some(pos) = list.iter().position(|c| c.as_str() == Some(command))
        {
            list.remove(pos);
        }
        self.save_message(&m).is_ok()
    }
}
