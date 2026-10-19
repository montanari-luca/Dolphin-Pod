//! API keys live in the OS keychain (macOS Keychain, Windows Credential Manager, Secret Service on Linux).
//! They never touch the SQLite database. Phase 2 sync stores them as passphrase-encrypted records.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use crate::error::{CoreError, Result};

pub const SERVICE: &str = "dev.mintflow.dolphinpod";

pub trait SecretStore: Send + Sync {
    fn get(&self, key: &str) -> Result<Option<String>>;
    fn set(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// OS keychain with an in-memory cache, so the keychain is read at most once per key per launch.
pub struct KeyringStore {
    cache: Mutex<HashMap<String, Option<String>>>,
}

impl KeyringStore {
    pub fn new() -> Self {
        Self { cache: Mutex::new(HashMap::new()) }
    }

    fn entry(key: &str) -> Result<keyring::Entry> {
        keyring::Entry::new(SERVICE, key).map_err(|e| CoreError::Secret(e.to_string()))
    }
}

impl Default for KeyringStore {
    fn default() -> Self {
        Self::new()
    }
}

impl SecretStore for KeyringStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        if let Some(v) = self.cache.lock().unwrap().get(key) {
            return Ok(v.clone());
        }
        let v = match Self::entry(key)?.get_password() {
            Ok(v) => Some(v),
            Err(keyring::Error::NoEntry) => None,
            Err(e) => return Err(CoreError::Secret(e.to_string())),
        };
        self.cache.lock().unwrap().insert(key.into(), v.clone());
        Ok(v)
    }

    fn set(&self, key: &str, value: &str) -> Result<()> {
        Self::entry(key)?.set_password(value).map_err(|e| CoreError::Secret(e.to_string()))?;
        self.cache.lock().unwrap().insert(key.into(), Some(value.into()));
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        match Self::entry(key)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => {}
            Err(e) => return Err(CoreError::Secret(e.to_string())),
        }
        self.cache.lock().unwrap().insert(key.into(), None);
        Ok(())
    }
}

/// In-memory store for tests and the browser mock.
#[derive(Default, Clone)]
pub struct MemoryStore(Arc<Mutex<HashMap<String, String>>>);

impl SecretStore for MemoryStore {
    fn get(&self, key: &str) -> Result<Option<String>> {
        Ok(self.0.lock().unwrap().get(key).cloned())
    }
    fn set(&self, key: &str, value: &str) -> Result<()> {
        self.0.lock().unwrap().insert(key.into(), value.into());
        Ok(())
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.0.lock().unwrap().remove(key);
        Ok(())
    }
}

pub fn provider_key_name(provider_id: &str) -> String {
    format!("provider:{provider_id}")
}
