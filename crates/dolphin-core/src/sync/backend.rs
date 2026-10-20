//! The remote side of sync, abstracted. The remote is a dumb relay: it stores encrypted operations in
//! arrival order and hands them back. Everything meaningful (merging, ordering) happens on the devices.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use async_trait::async_trait;

use crate::crypto::{Account, Sealed};
use crate::error::{CoreError, Result};
use crate::util::now_ms;

/// One encrypted field patch as stored remotely. `id` is the keyed hash of the record id.
#[derive(Debug, Clone, PartialEq)]
pub struct RemoteOp {
    /// Position in the remote's log; assigned by the remote.
    pub seq: i64,
    pub kind: String,
    pub id: String,
    pub hlc: String,
    pub device_id: String,
    pub sealed: Sealed,
    pub deleted: bool,
}

/// A device in the pod. Only the id, heartbeat and revoked flag are readable by the database host; the
/// name, platform and capabilities are sealed.
#[derive(Debug, Clone, PartialEq)]
pub struct DeviceRow {
    pub id: String,
    pub last_seen_ms: i64,
    pub revoked: bool,
    pub sealed: Sealed,
}

/// A remote-control request from one device to another, sealed so the host sees only who and when.
#[derive(Debug, Clone, PartialEq)]
pub struct CommandRow {
    pub id: String,
    pub seq: i64,
    pub from_device_id: String,
    pub target_device_id: String,
    pub created_at: i64,
    pub expires_at: i64,
    pub sealed: Sealed,
    /// "new" → "taken" → "done" | "rejected" | "expired"
    pub status: String,
}

#[async_trait]
pub trait SyncBackend: Send + Sync {
    /// Create tables if they don't exist.
    async fn init(&self) -> Result<()>;

    async fn get_account(&self) -> Result<Option<Account>>;
    /// Store the account if none exists yet. False means another device won the race.
    async fn create_account(&self, account: &Account) -> Result<bool>;
    /// Replace the account only if its generation is still `expected_generation`.
    async fn replace_account(&self, account: &Account, expected_generation: u32) -> Result<bool>;

    /// Append operations. Re-sending one that is already stored is harmless. Refused for a revoked device.
    async fn push_ops(&self, author: &str, ops: &[RemoteOp]) -> Result<()>;
    /// Operations with `seq > after`, oldest first.
    async fn pull_ops(&self, after: i64, limit: usize) -> Result<Vec<RemoteOp>>;

    async fn upsert_device(&self, device: &DeviceRow) -> Result<()>;
    async fn list_devices(&self) -> Result<Vec<DeviceRow>>;
    async fn revoke_device(&self, id: &str) -> Result<()>;

    async fn put_command(&self, command: &CommandRow) -> Result<()>;
    /// Commands addressed to `target` that are still "new", oldest first.
    async fn pull_commands(&self, target: &str, limit: usize) -> Result<Vec<CommandRow>>;
    async fn get_command(&self, id: &str) -> Result<Option<CommandRow>>;
    async fn set_command_status(&self, id: &str, status: &str) -> Result<()>;

    async fn put_blob(&self, id: &str, chunks: &[Sealed]) -> Result<()>;
    async fn get_blob(&self, id: &str) -> Result<Option<Vec<Sealed>>>;
    async fn has_blob(&self, id: &str) -> Result<bool>;
}

// ---------------------------------------------------------------------------------------------- memory

#[derive(Default)]
struct MemoryState {
    account: Option<Account>,
    ops: Vec<RemoteOp>,
    devices: HashMap<String, DeviceRow>,
    commands: Vec<CommandRow>,
    blobs: HashMap<String, Vec<Sealed>>,
    offline: bool,
}

/// An in-process relay for tests and for running without a database. Clone it to share one remote
/// between several simulated devices.
#[derive(Clone, Default)]
pub struct MemoryBackend(Arc<Mutex<MemoryState>>);

impl MemoryBackend {
    pub fn new() -> Self {
        Self::default()
    }

    /// Simulate losing the network: every call fails until switched back.
    pub fn set_offline(&self, offline: bool) {
        self.0.lock().unwrap().offline = offline;
    }

    /// Everything the relay stores, for tests that check no plaintext is there.
    pub fn dump_ops(&self) -> Vec<RemoteOp> {
        self.0.lock().unwrap().ops.clone()
    }

    fn state(&self) -> Result<std::sync::MutexGuard<'_, MemoryState>> {
        let s = self.0.lock().unwrap();
        if s.offline {
            return Err(CoreError::provider("the sync server is unreachable"));
        }
        Ok(s)
    }
}

#[async_trait]
impl SyncBackend for MemoryBackend {
    async fn init(&self) -> Result<()> {
        self.state().map(|_| ())
    }

    async fn get_account(&self) -> Result<Option<Account>> {
        Ok(self.state()?.account.clone())
    }

    async fn create_account(&self, account: &Account) -> Result<bool> {
        let mut s = self.state()?;
        if s.account.is_some() {
            return Ok(false);
        }
        s.account = Some(account.clone());
        Ok(true)
    }

    async fn replace_account(&self, account: &Account, expected_generation: u32) -> Result<bool> {
        let mut s = self.state()?;
        match &s.account {
            Some(a) if a.generation == expected_generation => {
                s.account = Some(account.clone());
                Ok(true)
            }
            _ => Ok(false),
        }
    }

    async fn push_ops(&self, author: &str, ops: &[RemoteOp]) -> Result<()> {
        let mut s = self.state()?;
        if s.devices.get(author).is_some_and(|d| d.revoked) {
            return Err(CoreError::Permission("this device was removed from the pod".into()));
        }
        for op in ops {
            let dup = s.ops.iter().any(|o| o.kind == op.kind && o.id == op.id && o.hlc == op.hlc && o.device_id == op.device_id);
            if !dup {
                let seq = s.ops.len() as i64 + 1;
                s.ops.push(RemoteOp { seq, ..op.clone() });
            }
        }
        Ok(())
    }

    async fn pull_ops(&self, after: i64, limit: usize) -> Result<Vec<RemoteOp>> {
        Ok(self.state()?.ops.iter().filter(|o| o.seq > after).take(limit).cloned().collect())
    }

    async fn upsert_device(&self, device: &DeviceRow) -> Result<()> {
        let mut s = self.state()?;
        let revoked = s.devices.get(&device.id).is_some_and(|d| d.revoked);
        s.devices.insert(device.id.clone(), DeviceRow { revoked, ..device.clone() });
        Ok(())
    }

    async fn list_devices(&self) -> Result<Vec<DeviceRow>> {
        let mut v: Vec<_> = self.state()?.devices.values().cloned().collect();
        v.sort_by(|a, b| a.id.cmp(&b.id));
        Ok(v)
    }

    async fn revoke_device(&self, id: &str) -> Result<()> {
        let mut s = self.state()?;
        if let Some(d) = s.devices.get_mut(id) {
            d.revoked = true;
        }
        Ok(())
    }

    async fn put_command(&self, command: &CommandRow) -> Result<()> {
        let mut s = self.state()?;
        if s.commands.iter().any(|c| c.id == command.id) {
            return Ok(());
        }
        let seq = s.commands.len() as i64 + 1;
        s.commands.push(CommandRow { seq, ..command.clone() });
        Ok(())
    }

    async fn pull_commands(&self, target: &str, limit: usize) -> Result<Vec<CommandRow>> {
        let now = now_ms();
        Ok(self
            .state()?
            .commands
            .iter()
            .filter(|c| c.target_device_id == target && c.status == "new" && c.expires_at > now)
            .take(limit)
            .cloned()
            .collect())
    }

    async fn get_command(&self, id: &str) -> Result<Option<CommandRow>> {
        Ok(self.state()?.commands.iter().find(|c| c.id == id).cloned())
    }

    async fn set_command_status(&self, id: &str, status: &str) -> Result<()> {
        if let Some(c) = self.state()?.commands.iter_mut().find(|c| c.id == id) {
            c.status = status.into();
        }
        Ok(())
    }

    async fn put_blob(&self, id: &str, chunks: &[Sealed]) -> Result<()> {
        self.state()?.blobs.insert(id.into(), chunks.to_vec());
        Ok(())
    }

    async fn get_blob(&self, id: &str) -> Result<Option<Vec<Sealed>>> {
        Ok(self.state()?.blobs.get(id).cloned())
    }

    async fn has_blob(&self, id: &str) -> Result<bool> {
        Ok(self.state()?.blobs.contains_key(id))
    }
}
