//! Feedback signals: the things a user does that say how an agent should behave next time, and the cheap
//! detectors that spot two of them in plain chat. A signal only queues work for the learner; whether it
//! teaches the agent anything lasting is for the learner to decide.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::model::Id;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Signal {
    /// The user undid the changes a run made.
    Revert,
    /// The user declined an action the agent asked to take.
    Deny,
    ThumbUp,
    ThumbDown,
    /// A message right after an agent's answer that reads like "no, not like that".
    Correction,
    /// The user asked the agent to remember something.
    Remember,
    /// A finished conversation, looked back over for preferences the user showed without correcting anything.
    Reflection,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Feedback {
    pub id: Id,
    pub agent_id: Id,
    pub chat_id: Id,
    pub message_id: Option<Id>,
    pub signal: Signal,
    /// What happened, as the learner needs it: the user's words, an excerpt of the reply, the action, the project.
    pub detail: Value,
    pub created_at: i64,
}

/// Longest message still treated as a correction; anything longer is a new request.
const MAX_CORRECTION_CHARS: usize = 400;

/// First words that, on their own, read as a correction. "no" needs a second look (see below).
const CORRECTION_OPENERS: &[&str] = &[
    "nope", "wrong", "stop", "never", "don't", "dont", "doesn't", "didn't", "shouldn't", "instead", "actually", "incorrect", "undo", "revert",
];

/// Phrases that read as a correction wherever they sit in a short message.
const CORRECTION_PHRASES: &[&str] = &[
    "i said ", "i told you", "not what i asked", "not what i wanted", "not what i meant", "didn't ask for", "did not ask for", "i asked for",
    "that's wrong", "that is wrong", "that's not", "that is not", "not like that", "don't add", "don't use", "don't write", "don't change",
    "do not add", "do not use", "do not change", "stop adding", "stop using", "stop writing", "instead of", "rather than", "next time",
    "too long", "too verbose", "too short", "too wordy", "why did you", "you should have", "you shouldn't have",
];

/// Words after "no" that make it a polite answer, not a correction ("no problem", "no thanks").
const NO_IS_FINE_BEFORE: &[&str] = &["problem", "problems", "worries", "thanks", "thank", "idea", "need", "rush", "hurry", "questions", "more"];

/// The message without leading @mentions and punctuation, so "@Rick no, use tabs" is judged on "no, use tabs".
fn without_leading_mentions(text: &str) -> &str {
    let mut rest = text.trim();
    while let Some(after) = rest.strip_prefix('@') {
        rest = after.trim_start_matches(|c: char| c.is_alphanumeric() || c == '_' || c == '-').trim_start_matches([',', ':', ' ']);
    }
    rest.trim_start_matches(|c: char| !c.is_alphanumeric())
}

fn words(text: &str) -> impl Iterator<Item = String> + '_ {
    text.split_whitespace().map(|w| w.trim_matches(|c: char| !c.is_alphanumeric() && c != '\'').to_lowercase())
}

/// Whether a user message, sent right after an agent answered, reads like a correction. Deliberately loose: a false
/// positive costs one learner call that finds nothing lasting; a miss costs a lesson.
pub fn looks_like_correction(text: &str) -> bool {
    let body = without_leading_mentions(text);
    if body.is_empty() || body.chars().count() > MAX_CORRECTION_CHARS {
        return false;
    }
    let lower = body.to_lowercase();
    let mut w = words(body);
    let first = w.next().unwrap_or_default();
    if first == "no" {
        return !w.next().is_some_and(|second| NO_IS_FINE_BEFORE.contains(&second.as_str()));
    }
    CORRECTION_OPENERS.contains(&first.as_str()) || CORRECTION_PHRASES.iter().any(|p| lower.contains(p))
}

/// Openers that ask an agent to keep something in mind. The rest of the message is what to keep.
const REMEMBER_OPENERS: &[&str] = &[
    "please remember that ", "please remember: ", "please remember ", "remember that ", "remember this: ", "remember: ", "remember, ", "remember ",
    "don't forget that ", "do not forget that ", "don't forget: ", "keep in mind that ", "keep in mind: ", "keep in mind ", "note that ",
];

/// Phrases whose whole sentence is the rule ("from now on answer in French"), so nothing is cut off.
const STANDING_OPENERS: &[&str] = &["from now on", "going forward", "in future,", "in the future,"];

/// The thing a user asked an agent to remember ("remember that I use tabs" gives "I use tabs"), or `None` when the
/// message is not such a request.
pub fn remember_request(text: &str) -> Option<String> {
    let body = without_leading_mentions(text);
    let lower = body.to_ascii_lowercase();
    let rest = if let Some(opener) = REMEMBER_OPENERS.iter().find(|o| lower.starts_with(**o)) {
        &body[opener.len()..]
    } else if STANDING_OPENERS.iter().any(|o| lower.starts_with(o)) {
        body
    } else {
        return None;
    };
    let rest = rest.trim();
    // "Remember me?" and "remember when we..." are questions about the past, not things to keep.
    (rest.chars().count() >= 4 && !rest.ends_with('?')).then(|| rest.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corrections_are_spotted_loosely_and_polite_nos_are_not() {
        for yes in [
            "No, use tabs",
            "no. that's not what I asked",
            "Don't add comments to the code",
            "Stop using emojis",
            "I said short answers",
            "@Rick no, instead use a match",
            "That's wrong",
            "Too long, keep it to three sentences",
            "Actually, make it a table",
            "next time run the tests first",
        ] {
            assert!(looks_like_correction(yes), "{yes}");
        }
        for no in [
            "No problem, thanks!",
            "no worries",
            "Thanks, that worked",
            "How do I reverse a string in Rust?",
            "Great, now do the same for the other file",
            "",
            "  ",
        ] {
            assert!(!looks_like_correction(no), "{no}");
        }
        assert!(!looks_like_correction(&format!("No, {}", "x".repeat(500))), "a long message is a new request");
    }

    #[test]
    fn remember_requests_give_back_what_to_keep() {
        assert_eq!(remember_request("Remember that I use tabs").as_deref(), Some("I use tabs"));
        assert_eq!(remember_request("@Rick please remember: no emojis, ever").as_deref(), Some("no emojis, ever"));
        assert_eq!(remember_request("Don't forget that the database is PostgreSQL.").as_deref(), Some("the database is PostgreSQL."));
        assert_eq!(remember_request("keep in mind I am a beginner").as_deref(), Some("I am a beginner"));
        assert_eq!(remember_request("From now on answer in French").as_deref(), Some("From now on answer in French"), "the whole rule stays");
    }

    #[test]
    fn questions_and_ordinary_messages_are_not_remember_requests() {
        for no in ["Do you remember what we did yesterday?", "Remember me?", "remember", "Remember when we fixed that?", "Write a function", "I remember that bug"] {
            assert_eq!(remember_request(no), None, "{no}");
        }
    }

    #[test]
    fn signals_round_trip_by_name() {
        assert_eq!(serde_json::to_string(&Signal::ThumbDown).unwrap(), "\"thumb_down\"");
        assert_eq!(serde_json::from_str::<Signal>("\"remember\"").unwrap(), Signal::Remember);
    }
}
