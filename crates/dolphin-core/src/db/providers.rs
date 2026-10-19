use rusqlite::{OptionalExtension, Row, params};

use super::{Db, from_json};
use crate::error::{CoreError, Result};
use crate::model::{ProviderConfig, ProviderKind};
use crate::util::now_ms;

const COLS: &str = "id, kind, name, base_url, extra, enabled, updated_at";

fn row_to_provider(r: &Row) -> rusqlite::Result<ProviderConfig> {
    let kind: String = r.get(1)?;
    Ok(ProviderConfig {
        id: r.get(0)?,
        kind: ProviderKind::parse(&kind).unwrap_or(ProviderKind::CustomOpenai),
        name: r.get(2)?,
        base_url: r.get(3)?,
        extra: from_json(r.get(4)?).unwrap_or(serde_json::json!({})),
        enabled: r.get::<_, i64>(5)? != 0,
        has_key: false,
        updated_at: r.get(6)?,
    })
}

impl Db {
    pub fn list_providers(&self) -> Result<Vec<ProviderConfig>> {
        let conn = self.conn();
        let mut st =
            conn.prepare(&format!("SELECT {COLS} FROM provider_configs WHERE deleted = 0 ORDER BY updated_at"))?;
        let rows = st.query_map([], row_to_provider)?.collect::<rusqlite::Result<Vec<_>>>()?;
        Ok(rows)
    }

    pub fn get_provider(&self, id: &str) -> Result<ProviderConfig> {
        self.conn()
            .query_row(
                &format!("SELECT {COLS} FROM provider_configs WHERE id = ?1 AND deleted = 0"),
                [id],
                row_to_provider,
            )
            .optional()?
            .ok_or_else(|| CoreError::NotFound(format!("provider {id}")))
    }

    pub fn upsert_provider(&self, p: &ProviderConfig) -> Result<ProviderConfig> {
        let mut p = p.clone();
        p.updated_at = now_ms();
        self.conn().execute(
            "INSERT INTO provider_configs (id, kind, name, base_url, extra, enabled, updated_at, device_id)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
             ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name, base_url = excluded.base_url,
               extra = excluded.extra, enabled = excluded.enabled, updated_at = excluded.updated_at, device_id = excluded.device_id, deleted = 0",
            params![
                p.id,
                p.kind.as_str(),
                p.name,
                p.base_url,
                p.extra.to_string(),
                p.enabled as i64,
                p.updated_at,
                self.device_id()
            ],
        )?;
        Ok(p)
    }

    pub fn delete_provider(&self, id: &str) -> Result<()> {
        self.conn().execute(
            "UPDATE provider_configs SET deleted = 1, updated_at = ?2, device_id = ?3 WHERE id = ?1",
            params![id, now_ms(), self.device_id()],
        )?;
        Ok(())
    }
}
