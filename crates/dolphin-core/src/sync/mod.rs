//! Encrypted multi-device sync. The local SQLite database stays the source of truth; a remote database
//! only relays sealed field patches (see `docs/sync.md`).

pub mod backend;
pub mod engine;
pub mod hlc;
pub mod kinds;
pub mod store;

pub use backend::{MemoryBackend, SyncBackend};
pub use engine::{CycleReport, SyncEngine};

#[cfg(test)]
mod tests;
