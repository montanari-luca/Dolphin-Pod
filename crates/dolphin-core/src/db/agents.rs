use rusqlite::{OptionalExtension, Row, params};

use super::{Db, enum_str, from_json, parse_enum, to_json};
use crate::error::{CoreError, Result};
use crate::model::{Agent, AgentColor, AgentRole};
use crate::util::now_ms;

const COLS: &str = "id, slug, name, role, specialty, job, color, model, image_model, ios_model, sort, updated_at, persona, learning";

fn row_to_agent(r: &Row) -> rusqlite::Result<Agent> {
    Ok(Agent {
        id: r.get(0)?,
        slug: r.get(1)?,
        name: r.get(2)?,
        role: parse_enum(&r.get::<_, String>(3)?).unwrap_or(AgentRole::Specialist),
        specialty: r.get(4)?,
        job: r.get(5)?,
        color: from_json(r.get(6)?).unwrap_or(AgentColor::solid("#7c8cff")),
        model: from_json(r.get(7)?),
        image_model: from_json(r.get(8)?),
        ios_model: from_json(r.get(9)?),
        sort: r.get(10)?,
        updated_at: r.get(11)?,
        persona: r.get(12)?,
        learning: r.get::<_, i64>(13)? != 0,
    })
}

impl Db {
    pub fn list_agents(&self) -> Result<Vec<Agent>> {
        let conn = self.conn();
        let mut st = conn.prepare(&format!("SELECT {COLS} FROM agents WHERE deleted = 0 ORDER BY sort, name"))?;
        let rows = st.query_map([], row_to_agent)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_agent(&self, id: &str) -> Result<Agent> {
        self.conn()
            .query_row(&format!("SELECT {COLS} FROM agents WHERE id = ?1 AND deleted = 0"), [id], row_to_agent)
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("agent {id}")))
    }

    pub fn agent_by_slug(&self, slug: &str) -> Result<Agent> {
        self.conn()
            .query_row(&format!("SELECT {COLS} FROM agents WHERE slug = ?1 AND deleted = 0"), [slug], row_to_agent)
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("agent {slug}")))
    }

    /// Whether a row with this id or slug exists at all, deleted or not (slugs stay unique across tombstones).
    pub fn agent_row_exists(&self, id: &str, slug: &str) -> Result<bool> {
        let n: i64 = self.conn().query_row("SELECT COUNT(*) FROM agents WHERE id = ?1 OR slug = ?2", params![id, slug], |r| r.get(0))?;
        Ok(n > 0)
    }

    pub fn upsert_agent(&self, a: &Agent) -> Result<Agent> {
        let mut a = a.clone();
        a.updated_at = now_ms();
        self.conn().execute(
            "INSERT INTO agents (id, slug, name, role, specialty, job, color, model, image_model, ios_model, sort, updated_at, device_id, persona, learning)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, job = excluded.job, color = excluded.color,
               model = excluded.model, image_model = excluded.image_model, ios_model = excluded.ios_model,
               sort = excluded.sort, updated_at = excluded.updated_at, device_id = excluded.device_id,
               persona = excluded.persona, learning = excluded.learning",
            params![
                a.id,
                a.slug,
                a.name,
                enum_str(&a.role),
                a.specialty,
                a.job,
                to_json(&a.color),
                a.model.as_ref().map(to_json),
                a.image_model.as_ref().map(to_json),
                a.ios_model.as_ref().map(to_json),
                a.sort,
                a.updated_at,
                self.device_id(),
                a.persona,
                a.learning as i64
            ],
        )?;
        Ok(a)
    }
}
