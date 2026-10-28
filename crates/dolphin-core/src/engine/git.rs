//! Backend side of Robert's confirmations: a push, pull or other confirm-class git command only runs when
//! the user answered "yes" after Robert asked. This is checked here, never trusted to the model's say-so.

use super::*;

impl Core {
    /// Whether `command` was asked about (and left pending) by an earlier, finished message of this chat and the
    /// user's latest message since is a clear yes. A confirmation is spent when it is taken: one yes allows one run.
    pub(super) fn take_git_confirmation(&self, chat_id: &str, command: &str) -> bool {
        let Ok(msgs) = self.db.list_messages(chat_id, 80) else { return false };
        let pending_at = msgs.iter().rposition(|m| {
            m.agent_id.is_some()
                && m.status != MessageStatus::Streaming
                && m.meta["gitPending"].as_array().is_some_and(|l| l.iter().any(|c| c.as_str() == Some(command)))
        });
        let Some(i) = pending_at else { return false };
        // The newest thing the user wrote must be the yes: an unrelated message, or a "yes" followed by "wait", is not one.
        let Some(reply) = msgs[i + 1..].iter().rfind(|m| m.agent_id.is_none()) else { return false };
        if !is_affirmative(&reply.content) {
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

/// Whether a reply is a plain yes. Deliberately strict: anything that also says no, wait or stop is not one, and
/// long replies (which are probably something else) are not either.
pub(super) fn is_affirmative(text: &str) -> bool {
    let cleaned: String = text.to_lowercase().chars().map(|c| if c.is_alphanumeric() || c == '\'' || c.is_whitespace() { c } else { ' ' }).collect();
    let words: Vec<&str> = cleaned.split_whitespace().collect();
    if words.is_empty() || words.len() > 8 {
        return false;
    }
    const NO: &[&str] = &["no", "nope", "not", "dont", "don't", "do n't", "stop", "wait", "cancel", "abort", "never", "hold", "instead", "but", "except", "without", "nah"];
    if words.iter().any(|w| NO.contains(w)) {
        return false;
    }
    const YES: &[&str] = &["yes", "y", "yep", "yeah", "yup", "ok", "okay", "sure", "confirm", "confirmed", "approve", "approved", "proceed", "continue", "affirmative", "ja", "oui", "si", "sí"];
    let joined = words.join(" ");
    words.iter().any(|w| YES.contains(w)) || ["do it", "go ahead", "please do", "go for it"].iter().any(|p| joined.contains(p))
}

#[cfg(test)]
mod tests {
    use super::is_affirmative;

    #[test]
    fn only_clear_yeses_confirm() {
        for yes in ["yes", "Yes!", "yes, push it", "ok", "Okay, go ahead", "sure", "confirm", "do it", "yes please", "y", "Yep."] {
            assert!(is_affirmative(yes), "{yes}");
        }
        for not in ["", "no", "yes but not main", "yes wait", "don't", "what does that do?", "hmm", "push to a different branch instead", "can you explain what clean does before I say yes to anything at all today", "stop"] {
            assert!(!is_affirmative(not), "{not}");
        }
    }
}
