//! One sync round: capture local edits, push them, pull and merge what other devices wrote.

use std::collections::HashMap;
use std::sync::Arc;

use serde_json::{Value, json};

use super::backend::{RemoteOp, SyncBackend};
use super::kinds::{self, API_KEY_FIELD};
use super::store::{Applied, PlainOp};
use crate::crypto::{DataKey, record_aad};
use crate::db::Db;
use crate::error::{CoreError, Result};
use crate::secrets::{SecretStore, provider_key_name};

const BATCH: usize = 200;
const BLOB_CHUNK: usize = 1024 * 1024;
/// Attachments above this size stay on the device that has them.
pub const MAX_BLOB_BYTES: u64 = 40 * 1024 * 1024;

#[derive(Debug, Default, Clone)]
pub struct CycleReport {
    pub pushed: usize,
    pub pulled: usize,
    pub applied: Vec<Applied>,
    /// Operations that failed authentication (wrong key or tampered with) and were ignored.
    pub rejected: usize,
}

pub struct SyncEngine {
    db: Db,
    secrets: Arc<dyn SecretStore>,
    backend: Arc<dyn SyncBackend>,
    key: DataKey,
    /// Names this remote's pull cursor, so pointing the app at another database starts from zero.
    remote_name: String,
}

impl SyncEngine {
    pub fn new(db: Db, secrets: Arc<dyn SecretStore>, backend: Arc<dyn SyncBackend>, key: DataKey, remote_name: &str) -> Self {
        Self { db, secrets, backend, key, remote_name: remote_name.into() }
    }

    pub fn key(&self) -> &DataKey {
        &self.key
    }

    fn api_key_field(&self, provider_id: &str) -> Vec<(String, Value)> {
        let key = self.secrets.get(&provider_key_name(provider_id)).ok().flatten().filter(|k| !k.is_empty());
        vec![(API_KEY_FIELD.into(), key.map_or(Value::Null, Value::String))]
    }

    /// Queue everything edited locally since the last capture.
    pub fn capture(&self) -> Result<usize> {
        let mut virtuals: HashMap<String, Vec<(String, Value)>> = HashMap::new();
        for id in self.db.sync_dirty_provider_ids()? {
            let fields = self.api_key_field(&id);
            virtuals.insert(id, fields);
        }
        self.db.sync_capture(&virtuals)
    }

    fn encode(&self, op: &PlainOp) -> Result<RemoteOp> {
        let remote_id = self.key.remote_id(&op.kind, &op.id);
        let aad = record_aad(&op.kind, &remote_id, &op.hlc, &op.device_id);
        let plain = serde_json::to_vec(&json!({ "id": op.id, "patch": op.patch }))?;
        Ok(RemoteOp {
            seq: 0,
            kind: op.kind.clone(),
            id: remote_id,
            hlc: op.hlc.clone(),
            device_id: op.device_id.clone(),
            sealed: self.key.seal(&aad, &plain)?,
            deleted: op.patch.get("deleted").and_then(Value::as_i64) == Some(1),
        })
    }

    fn decode(&self, op: &RemoteOp) -> Result<PlainOp> {
        let aad = record_aad(&op.kind, &op.id, &op.hlc, &op.device_id);
        let plain = self.key.open(&aad, &op.sealed)?;
        let v: Value = serde_json::from_slice(&plain)?;
        let id = v["id"].as_str().ok_or_else(|| CoreError::invalid("operation without an id"))?.to_string();
        if self.key.remote_id(&op.kind, &id) != op.id {
            return Err(CoreError::invalid("operation does not belong to its record"));
        }
        let patch = v["patch"].as_object().cloned().ok_or_else(|| CoreError::invalid("operation without a patch"))?;
        Ok(PlainOp { kind: op.kind.clone(), id, hlc: op.hlc.clone(), device_id: op.device_id.clone(), patch })
    }

    pub async fn push(&self) -> Result<usize> {
        let mut pushed = 0;
        loop {
            let rows = self.db.sync_outbox(BATCH)?;
            let Some(last) = rows.last().map(|r| r.seq) else { break };
            let ops = rows.iter().map(|r| self.encode(&r.op)).collect::<Result<Vec<_>>>()?;
            self.backend.push_ops(self.db.device_id(), &ops).await?;
            self.db.sync_outbox_ack(last)?;
            pushed += rows.len();
        }
        Ok(pushed)
    }

    pub async fn pull(&self) -> Result<CycleReport> {
        let mut report = CycleReport::default();
        loop {
            let cursor = self.db.sync_cursor(&self.remote_name)?;
            let ops = self.backend.pull_ops(cursor, BATCH).await?;
            let Some(last) = ops.last().map(|o| o.seq) else { break };
            for op in &ops {
                report.pulled += 1;
                if op.device_id == self.db.device_id() {
                    continue;
                }
                let plain = match self.decode(op) {
                    Ok(p) => p,
                    Err(e) => {
                        tracing::warn!("ignoring a sync operation: {e}");
                        report.rejected += 1;
                        continue;
                    }
                };
                let virtuals = if plain.kind == kinds::PROVIDER { self.api_key_field(&plain.id) } else { vec![] };
                if let Some(applied) = self.db.sync_apply(&plain, &virtuals)? {
                    for (name, value) in &applied.secrets {
                        match value {
                            Some(v) => self.secrets.set(name, v)?,
                            None => self.secrets.delete(name)?,
                        }
                    }
                    report.applied.push(applied);
                }
            }
            self.db.sync_set_cursor(&self.remote_name, last)?;
            if ops.len() < BATCH {
                break;
            }
        }
        Ok(report)
    }

    /// Upload attachment files that haven't been uploaded yet.
    pub async fn upload_blobs(&self) -> Result<usize> {
        let mut n = 0;
        for (id, path) in self.db.sync_blobs_pending()? {
            let Ok(meta) = std::fs::metadata(&path) else { continue };
            if meta.len() > MAX_BLOB_BYTES {
                self.db.sync_blob_done(&id)?; // too big to sync: don't retry
                continue;
            }
            let bytes = std::fs::read(&path)?;
            let total = bytes.chunks(BLOB_CHUNK).count().max(1);
            let mut chunks = Vec::new();
            for (i, part) in bytes.chunks(BLOB_CHUNK).enumerate() {
                chunks.push(self.key.seal(&blob_aad(&id, i, total), part)?);
            }
            if chunks.is_empty() {
                chunks.push(self.key.seal(&blob_aad(&id, 0, 1), &[])?);
            }
            self.backend.put_blob(&id, &chunks).await?;
            self.db.sync_blob_done(&id)?;
            n += 1;
        }
        Ok(n)
    }

    /// Download an attachment's file into `dest`. Returns false if no other device uploaded it.
    pub async fn download_blob(&self, attachment_id: &str, dest: &std::path::Path) -> Result<bool> {
        let Some(chunks) = self.backend.get_blob(attachment_id).await? else { return Ok(false) };
        let mut out = Vec::new();
        for (i, c) in chunks.iter().enumerate() {
            out.extend(self.key.open(&blob_aad(attachment_id, i, chunks.len()), c)?);
        }
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir)?;
        }
        std::fs::write(dest, out)?;
        Ok(true)
    }

    /// One full round. Remote changes are merged after local ones are queued, so a half-typed edit
    /// is never lost to an incoming one.
    pub async fn cycle(&self) -> Result<CycleReport> {
        self.capture()?;
        let _ = self.upload_blobs().await.map_err(|e| tracing::warn!("attachment upload: {e}"));
        let pushed = self.push().await?;
        let mut report = self.pull().await?;
        report.pushed = pushed;
        Ok(report)
    }

    /// First sync on a device that joins an existing pod: take what the pod has, then offer what this
    /// device has that the pod doesn't. (Seeded defaults the pod already holds are not overwritten.)
    pub async fn join(&self) -> Result<CycleReport> {
        let pulled = self.pull().await?;
        self.db.sync_mark_all_dirty()?;
        let mut report = self.cycle().await?;
        report.applied.splice(0..0, pulled.applied);
        report.pulled += pulled.pulled;
        Ok(report)
    }

    /// First sync on the device that creates the pod: everything it has is offered.
    pub async fn create(&self) -> Result<CycleReport> {
        self.db.sync_mark_all_dirty()?;
        self.cycle().await
    }
}

pub fn blob_aad(id: &str, index: usize, total: usize) -> Vec<u8> {
    format!("dolphinpod/blob/v1/{id}/{index}/{total}").into_bytes()
}
