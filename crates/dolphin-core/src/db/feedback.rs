use rusqlite::{Row, params, params_from_iter};

use super::{Db, enum_str, parse_enum};
use crate::error::Result;
use crate::memory::signals::{Feedback, Signal};
use crate::util::now_ms;

const COLS: &str = "id, agent_id, chat_id, message_id, signal, detail, created_at";

fn row_to_feedback(r: &Row) -> rusqlite::Result<Feedback> {
    Ok(Feedback {
        id: r.get(0)?,
        agent_id: r.get(1)?,
        chat_id: r.get(2)?,
        message_id: r.get(3)?,
        signal: parse_enum(&r.get::<_, String>(4)?).unwrap_or(Signal::Correction),
        detail: serde_json::from_str(&r.get::<_, String>(5)?).unwrap_or_default(),
        created_at: r.get(6)?,
    })
}

/// The log of signals the learner works through. Local to this device: never synced.
impl Db {
    pub fn insert_feedback(&self, f: &Feedback) -> Result<()> {
        self.conn().execute(
            "INSERT INTO agent_feedback (id, agent_id, chat_id, message_id, signal, detail, processed, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7)",
            params![f.id, f.agent_id, f.chat_id, f.message_id, enum_str(&f.signal), f.detail.to_string(), f.created_at],
        )?;
        Ok(())
    }

    /// Agents with signals worth a learner call. A thumbs up alone is not one: it waits and rides along with the next
    /// real signal as context.
    pub fn agents_with_pending_feedback(&self) -> Result<Vec<String>> {
        let conn = self.conn();
        let mut st = conn.prepare("SELECT DISTINCT agent_id FROM agent_feedback WHERE processed = 0 AND signal != 'thumb_up' ORDER BY agent_id")?;
        Ok(st.query_map([], |r| r.get(0))?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    /// One agent's unprocessed signals, oldest first.
    pub fn pending_feedback(&self, agent_id: &str) -> Result<Vec<Feedback>> {
        let conn = self.conn();
        let mut st = conn.prepare(&format!("SELECT {COLS} FROM agent_feedback WHERE agent_id = ?1 AND processed = 0 ORDER BY created_at, id"))?;
        Ok(st.query_map([agent_id], row_to_feedback)?.collect::<rusqlite::Result<Vec<_>>>()?)
    }

    pub fn mark_feedback_processed(&self, ids: &[String]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        let marks = vec!["?"; ids.len()].join(", ");
        self.conn().execute(&format!("UPDATE agent_feedback SET processed = 1 WHERE id IN ({marks})"), params_from_iter(ids.iter()))?;
        Ok(())
    }

    /// Whether the same signal for the same message is already logged (a second thumbs click must not count twice).
    pub fn feedback_exists(&self, message_id: &str, signal: Signal) -> Result<bool> {
        let n: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM agent_feedback WHERE message_id = ?1 AND signal = ?2",
            params![message_id, enum_str(&signal)],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Forget signals that are old, handled or not: a lesson from weeks ago is stale.
    pub fn prune_feedback(&self, older_than_days: i64) -> Result<usize> {
        let cutoff = now_ms() - older_than_days * 24 * 3600 * 1000;
        Ok(self.conn().execute("DELETE FROM agent_feedback WHERE created_at < ?1", [cutoff])?)
    }
}
