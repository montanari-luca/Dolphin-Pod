use rusqlite::{Row, params};

use super::{Db, from_json, to_json};
use crate::error::{CoreError, Result};
use crate::model::{AgentColor, UserRole};
use crate::util::now_ms;

fn row_to_role(r: &Row) -> rusqlite::Result<UserRole> {
    Ok(UserRole {
        id: r.get(0)?,
        name: r.get(1)?,
        icon: r.get(2)?,
        color: from_json(r.get(3)?).unwrap_or(AgentColor::solid("#7c8cff")),
        sort: r.get(4)?,
        updated_at: r.get(5)?,
    })
}

impl Db {
    pub fn list_roles(&self) -> Result<Vec<UserRole>> {
        let conn = self.conn();
        let mut st = conn.prepare(
            "SELECT id, name, icon, color, sort, updated_at FROM user_roles WHERE deleted = 0 ORDER BY sort, updated_at",
        )?;
        let rows = st.query_map([], row_to_role)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn upsert_role(&self, r: &UserRole) -> Result<UserRole> {
        let mut r = r.clone();
        r.updated_at = now_ms();
        self.conn().execute(
            "INSERT INTO user_roles (id, name, icon, color, sort, updated_at, device_id) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)
             ON CONFLICT(id) DO UPDATE SET name = excluded.name, icon = excluded.icon, color = excluded.color,
               sort = excluded.sort, updated_at = excluded.updated_at, device_id = excluded.device_id, deleted = 0",
            params![r.id, r.name, r.icon, to_json(&r.color), r.sort, r.updated_at, self.device_id()],
        )?;
        Ok(r)
    }

    pub fn delete_role(&self, id: &str) -> Result<()> {
        let n = self.conn().execute(
            "UPDATE user_roles SET deleted = 1, updated_at = ?2, device_id = ?3 WHERE id = ?1 AND deleted = 0",
            params![id, now_ms(), self.device_id()],
        )?;
        if n == 0 {
            return Err(CoreError::NotFound(format!("role {id}")));
        }
        Ok(())
    }
}
