use rusqlite::{OptionalExtension, Row, params};

use super::Db;
use crate::error::{CoreError, Result};
use crate::model::PersonaProposal;

fn row_to_proposal(r: &Row) -> rusqlite::Result<PersonaProposal> {
    Ok(PersonaProposal { id: r.get(0)?, agent_id: r.get(1)?, text: r.get(2)?, reason: r.get(3)?, created_at: r.get(4)? })
}

const COLS: &str = "id, agent_id, text, reason, created_at";

/// Persona changes an agent has suggested for itself. Local to this device; the persona they would change syncs.
impl Db {
    /// The proposal waiting for the user, if any.
    pub fn pending_persona_proposal(&self, agent_id: &str) -> Result<Option<PersonaProposal>> {
        Ok(self
            .conn()
            .query_row(&format!("SELECT {COLS} FROM persona_proposal WHERE agent_id = ?1 AND status = 'pending' ORDER BY created_at DESC LIMIT 1"), [agent_id], row_to_proposal)
            .optional()?)
    }

    pub fn get_persona_proposal(&self, id: &str) -> Result<PersonaProposal> {
        self.conn()
            .query_row(&format!("SELECT {COLS} FROM persona_proposal WHERE id = ?1 AND status = 'pending'"), [id], row_to_proposal)
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("persona suggestion {id}")))
    }

    /// Whether the user already turned this exact text down for this agent.
    pub fn persona_was_rejected(&self, agent_id: &str, text: &str) -> Result<bool> {
        let n: i64 = self.conn().query_row(
            "SELECT COUNT(*) FROM persona_proposal WHERE agent_id = ?1 AND status = 'rejected' AND lower(text) = lower(?2)",
            params![agent_id, text],
            |r| r.get(0),
        )?;
        Ok(n > 0)
    }

    /// Make this the pending proposal; an older pending one is dropped, since the newer reflects more of what the user did.
    pub fn insert_persona_proposal(&self, p: &PersonaProposal) -> Result<()> {
        let conn = self.conn();
        conn.execute("DELETE FROM persona_proposal WHERE agent_id = ?1 AND status = 'pending'", [&p.agent_id])?;
        conn.execute(
            "INSERT INTO persona_proposal (id, agent_id, text, reason, status, created_at) VALUES (?1, ?2, ?3, ?4, 'pending', ?5)",
            params![p.id, p.agent_id, p.text, p.reason, p.created_at],
        )?;
        Ok(())
    }

    pub fn resolve_persona_proposal(&self, id: &str, accepted: bool) -> Result<()> {
        self.conn().execute("UPDATE persona_proposal SET status = ?2 WHERE id = ?1", params![id, if accepted { "accepted" } else { "rejected" }])?;
        Ok(())
    }
}
