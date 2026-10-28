use std::collections::HashMap;

use rusqlite::{params, params_from_iter};

use super::Db;
use crate::error::Result;
use crate::memory::embed::{from_bytes, to_bytes};

/// Cached vectors of notes. Local to this device: they depend on the embedding model and are cheap to rebuild.
impl Db {
    /// The cached vectors for these notes under one model: note id to (hash of the text it was made from, vector).
    pub fn embeddings_for(&self, model: &str, memory_ids: &[String]) -> Result<HashMap<String, (String, Vec<f32>)>> {
        if memory_ids.is_empty() {
            return Ok(HashMap::new());
        }
        let marks = vec!["?"; memory_ids.len()].join(", ");
        let conn = self.conn();
        let mut st = conn.prepare(&format!("SELECT memory_id, text_hash, vector FROM memory_embedding WHERE model = ? AND memory_id IN ({marks})"))?;
        let mut args: Vec<&str> = vec![model];
        args.extend(memory_ids.iter().map(String::as_str));
        let rows = st.query_map(params_from_iter(args), |r| Ok((r.get::<_, String>(0)?, r.get::<_, String>(1)?, r.get::<_, Vec<u8>>(2)?)))?;
        let mut out = HashMap::new();
        for row in rows {
            let (id, hash, blob) = row?;
            if let Some(v) = from_bytes(&blob) {
                out.insert(id, (hash, v));
            }
        }
        Ok(out)
    }

    pub fn put_embedding(&self, memory_id: &str, model: &str, text_hash: &str, vector: &[f32]) -> Result<()> {
        self.conn().execute(
            "INSERT INTO memory_embedding (memory_id, model, text_hash, vector) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(memory_id, model) DO UPDATE SET text_hash = excluded.text_hash, vector = excluded.vector",
            params![memory_id, model, text_hash, to_bytes(vector)],
        )?;
        Ok(())
    }

    /// Drop vectors of notes that no longer exist. How many went.
    pub fn prune_embeddings(&self) -> Result<usize> {
        Ok(self.conn().execute("DELETE FROM memory_embedding WHERE memory_id NOT IN (SELECT id FROM agent_memory WHERE deleted = 0)", [])?)
    }
}
