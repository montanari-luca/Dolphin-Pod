//! End-to-end encryption for sync. Nothing the remote database stores is readable without the user's
//! passphrase (or recovery key).
//!
//! ```text
//! passphrase --Argon2id(salt, params)--> KEK_pass ─┐
//! recovery key (256-bit, 24 words) --HKDF--> KEK_rec ─┼─ wrap ─> data key (random 256-bit)
//!                                                     │
//! data key --HKDF--> records key   (XChaCha20-Poly1305 over every record, AD = kind|id|hlc|device)
//!                --HKDF--> ids key  (HMAC-SHA256: the remote only ever sees keyed hashes of record ids)
//!                --HKDF--> link key (pre-shared key for the nearby-network channel)
//! ```
//!
//! Changing the passphrase re-wraps the data key with a new salt; no record is re-encrypted. A forgotten
//! passphrase can't be reset: the recovery key is the only other way in.

use argon2::{Algorithm, Argon2, Params, Version};
use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use chacha20poly1305::aead::{Aead, KeyInit, Payload};
use chacha20poly1305::{XChaCha20Poly1305, XNonce};
use hkdf::Hkdf;
use hmac::{Hmac, Mac};
use rand::RngCore;
use serde::{Deserialize, Serialize};
use sha2::Sha256;
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::error::{CoreError, Result};

pub const KEY_LEN: usize = 32;
pub const NONCE_LEN: usize = 24;
pub const SALT_LEN: usize = 16;
const ACCOUNT_VERSION: u32 = 1;

fn random<const N: usize>() -> [u8; N] {
    let mut b = [0u8; N];
    rand::rng().fill_bytes(&mut b);
    b
}

fn crypto_err(what: &str) -> CoreError {
    CoreError::Invalid(what.into())
}

/// Argon2id cost, stored with the account so every device derives the same key.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct KdfParams {
    pub m_kib: u32,
    pub t: u32,
    pub p: u32,
}

impl KdfParams {
    /// 64 MiB, 3 passes, 1 lane: roughly a quarter of a second on a recent phone.
    pub const DEFAULT: KdfParams = KdfParams { m_kib: 64 * 1024, t: 3, p: 1 };
    /// Cheap parameters for tests only.
    pub const FAST_FOR_TESTS: KdfParams = KdfParams { m_kib: 64, t: 1, p: 1 };

    fn argon(&self) -> Result<Argon2<'static>> {
        let params = Params::new(self.m_kib, self.t, self.p, Some(KEY_LEN)).map_err(|e| crypto_err(&e.to_string()))?;
        Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
    }
}

/// The key that encrypts everything. Zeroed on drop.
#[derive(Clone, ZeroizeOnDrop)]
pub struct DataKey([u8; KEY_LEN]);

impl DataKey {
    pub fn generate() -> Self {
        Self(random())
    }

    pub fn from_bytes(b: &[u8]) -> Result<Self> {
        let arr: [u8; KEY_LEN] = b.try_into().map_err(|_| crypto_err("a data key is 32 bytes"))?;
        Ok(Self(arr))
    }

    /// For the OS keychain cache.
    pub fn to_base64(&self) -> String {
        URL_SAFE_NO_PAD.encode(self.0)
    }

    pub fn from_base64(s: &str) -> Result<Self> {
        Self::from_bytes(&URL_SAFE_NO_PAD.decode(s.trim()).map_err(|_| crypto_err("bad key encoding"))?)
    }

    fn subkey(&self, info: &str) -> [u8; KEY_LEN] {
        let mut out = [0u8; KEY_LEN];
        Hkdf::<Sha256>::new(None, &self.0).expand(info.as_bytes(), &mut out).expect("32 bytes is a valid HKDF length");
        out
    }

    /// Pre-shared key for the nearby-network channel between two of the user's devices.
    pub fn link_key(&self) -> [u8; KEY_LEN] {
        self.subkey("dolphinpod/link/v1")
    }

    /// The remote-side name of a record: a keyed hash, so the database never sees real ids
    /// (setting keys, chat ids) but identical records still group together.
    pub fn remote_id(&self, kind: &str, id: &str) -> String {
        let mut mac = <Hmac<Sha256> as Mac>::new_from_slice(&self.subkey("dolphinpod/ids/v1")).expect("any key length");
        mac.update(kind.as_bytes());
        mac.update(&[0]);
        mac.update(id.as_bytes());
        URL_SAFE_NO_PAD.encode(mac.finalize().into_bytes())
    }

    /// Encrypt `plaintext`, binding it to its place (`aad`) so a ciphertext can't be moved to another record.
    pub fn seal(&self, aad: &[u8], plaintext: &[u8]) -> Result<Sealed> {
        let key = self.subkey("dolphinpod/records/v1");
        let cipher = XChaCha20Poly1305::new((&key).into());
        let nonce: [u8; NONCE_LEN] = random();
        let ciphertext = cipher
            .encrypt(XNonce::from_slice(&nonce), Payload { msg: plaintext, aad })
            .map_err(|_| crypto_err("encryption failed"))?;
        Ok(Sealed { nonce: nonce.to_vec(), ciphertext })
    }

    pub fn open(&self, aad: &[u8], sealed: &Sealed) -> Result<Vec<u8>> {
        if sealed.nonce.len() != NONCE_LEN {
            return Err(crypto_err("bad nonce"));
        }
        let key = self.subkey("dolphinpod/records/v1");
        let cipher = XChaCha20Poly1305::new((&key).into());
        cipher
            .decrypt(XNonce::from_slice(&sealed.nonce), Payload { msg: &sealed.ciphertext, aad })
            .map_err(|_| crypto_err("could not decrypt: wrong key, or the record was tampered with"))
    }
}

impl std::fmt::Debug for DataKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("DataKey(..)")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sealed {
    pub nonce: Vec<u8>,
    pub ciphertext: Vec<u8>,
}

/// Associated data for a record: kind, remote id, clock and author, length-prefixed so fields can't blur.
pub fn record_aad(kind: &str, remote_id: &str, hlc: &str, device_id: &str) -> Vec<u8> {
    let mut aad = b"dolphinpod/record/v1".to_vec();
    for part in [kind, remote_id, hlc, device_id] {
        aad.extend_from_slice(&(part.len() as u32).to_be_bytes());
        aad.extend_from_slice(part.as_bytes());
    }
    aad
}

/// What the remote stores about the user's keys. Safe to keep in plaintext: both wrapped copies are
/// useless without the passphrase or the recovery key.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Account {
    pub version: u32,
    pub salt: Vec<u8>,
    pub kdf: KdfParams,
    /// Data key sealed under the passphrase-derived key (nonce ‖ ciphertext).
    pub wrapped_by_passphrase: Vec<u8>,
    /// Data key sealed under the recovery key (nonce ‖ ciphertext).
    pub wrapped_by_recovery: Vec<u8>,
    /// Bumped on every passphrase change; lets devices notice a re-wrap.
    pub generation: u32,
}

fn passphrase_kek(passphrase: &str, salt: &[u8], kdf: &KdfParams) -> Result<[u8; KEY_LEN]> {
    let mut out = [0u8; KEY_LEN];
    kdf.argon()?.hash_password_into(passphrase.as_bytes(), salt, &mut out).map_err(|e| crypto_err(&e.to_string()))?;
    Ok(out)
}

fn recovery_kek(recovery: &[u8; KEY_LEN]) -> [u8; KEY_LEN] {
    let mut out = [0u8; KEY_LEN];
    Hkdf::<Sha256>::new(None, recovery).expand(b"dolphinpod/recovery-wrap/v1", &mut out).expect("32 bytes");
    out
}

fn wrap(kek: &[u8; KEY_LEN], dk: &DataKey, label: &str) -> Result<Vec<u8>> {
    let cipher = XChaCha20Poly1305::new(kek.into());
    let nonce: [u8; NONCE_LEN] = random();
    let ct = cipher
        .encrypt(XNonce::from_slice(&nonce), Payload { msg: &dk.0, aad: label.as_bytes() })
        .map_err(|_| crypto_err("could not wrap the key"))?;
    let mut out = nonce.to_vec();
    out.extend(ct);
    Ok(out)
}

fn unwrap_key(kek: &[u8; KEY_LEN], wrapped: &[u8], label: &str) -> Result<DataKey> {
    if wrapped.len() <= NONCE_LEN {
        return Err(crypto_err("the stored key is damaged"));
    }
    let (nonce, ct) = wrapped.split_at(NONCE_LEN);
    let mut bytes = XChaCha20Poly1305::new(kek.into())
        .decrypt(XNonce::from_slice(nonce), Payload { msg: ct, aad: label.as_bytes() })
        .map_err(|_| crypto_err("wrong passphrase or recovery key"))?;
    let key = DataKey::from_bytes(&bytes);
    bytes.zeroize();
    key
}

const PASS_LABEL: &str = "dolphinpod/wrap/passphrase/v1";
const RECOVERY_LABEL: &str = "dolphinpod/wrap/recovery/v1";

/// The 256-bit recovery key, shown to the user once as 24 words.
#[derive(Clone, ZeroizeOnDrop)]
pub struct RecoveryKey([u8; KEY_LEN]);

impl RecoveryKey {
    pub fn words(&self) -> String {
        bip39::Mnemonic::from_entropy(&self.0).expect("32 bytes is valid entropy").to_string()
    }

    pub fn parse(words: &str) -> Result<Self> {
        let normalized = words.split_whitespace().collect::<Vec<_>>().join(" ").to_lowercase();
        let m = bip39::Mnemonic::parse_normalized(&normalized)
            .map_err(|_| crypto_err("that is not a valid recovery key (24 words)"))?;
        let entropy = m.to_entropy();
        let arr: [u8; KEY_LEN] = entropy.as_slice().try_into().map_err(|_| crypto_err("a recovery key has 24 words"))?;
        Ok(Self(arr))
    }
}

/// First device: make the data key, wrap it under the passphrase and a fresh recovery key.
pub fn create_account(passphrase: &str, kdf: KdfParams) -> Result<(Account, DataKey, RecoveryKey)> {
    let dk = DataKey::generate();
    let recovery = RecoveryKey(random());
    let salt: [u8; SALT_LEN] = random();
    let account = Account {
        version: ACCOUNT_VERSION,
        salt: salt.to_vec(),
        kdf,
        wrapped_by_passphrase: wrap(&passphrase_kek(passphrase, &salt, &kdf)?, &dk, PASS_LABEL)?,
        wrapped_by_recovery: wrap(&recovery_kek(&recovery.0), &dk, RECOVERY_LABEL)?,
        generation: 0,
    };
    Ok((account, dk, recovery))
}

pub fn unlock_with_passphrase(account: &Account, passphrase: &str) -> Result<DataKey> {
    unwrap_key(&passphrase_kek(passphrase, &account.salt, &account.kdf)?, &account.wrapped_by_passphrase, PASS_LABEL)
}

pub fn unlock_with_recovery(account: &Account, recovery: &RecoveryKey) -> Result<DataKey> {
    unwrap_key(&recovery_kek(&recovery.0), &account.wrapped_by_recovery, RECOVERY_LABEL)
}

/// New passphrase, new salt; the data key (and so every stored record) stays as it is.
pub fn change_passphrase(account: &Account, dk: &DataKey, new_passphrase: &str) -> Result<Account> {
    let salt: [u8; SALT_LEN] = random();
    Ok(Account {
        salt: salt.to_vec(),
        wrapped_by_passphrase: wrap(&passphrase_kek(new_passphrase, &salt, &account.kdf)?, dk, PASS_LABEL)?,
        generation: account.generation + 1,
        ..account.clone()
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    const FAST: KdfParams = KdfParams::FAST_FOR_TESTS;

    #[test]
    fn passphrase_and_recovery_key_both_unlock_the_same_data_key() {
        let (account, dk, recovery) = create_account("correct horse battery", FAST).unwrap();
        let a = unlock_with_passphrase(&account, "correct horse battery").unwrap();
        let b = unlock_with_recovery(&account, &RecoveryKey::parse(&recovery.words()).unwrap()).unwrap();
        assert_eq!(a.to_base64(), dk.to_base64());
        assert_eq!(b.to_base64(), dk.to_base64());
        assert!(unlock_with_passphrase(&account, "wrong").is_err());
        assert_eq!(recovery.words().split(' ').count(), 24);
    }

    #[test]
    fn changing_the_passphrase_rewraps_without_changing_the_data_key() {
        let (account, dk, recovery) = create_account("old passphrase", FAST).unwrap();
        let next = change_passphrase(&account, &dk, "new passphrase").unwrap();
        assert_ne!(next.salt, account.salt);
        assert_eq!(next.generation, 1);
        assert!(unlock_with_passphrase(&next, "old passphrase").is_err());
        assert_eq!(unlock_with_passphrase(&next, "new passphrase").unwrap().to_base64(), dk.to_base64());
        assert_eq!(unlock_with_recovery(&next, &recovery).unwrap().to_base64(), dk.to_base64());
        // Records sealed before the change still open afterwards.
        let aad = record_aad("chat", "r", "h", "d");
        let sealed = dk.seal(&aad, b"hello").unwrap();
        let reopened = unlock_with_passphrase(&next, "new passphrase").unwrap();
        assert_eq!(reopened.open(&aad, &sealed).unwrap(), b"hello");
    }

    #[test]
    fn ciphertext_is_bound_to_its_record() {
        let dk = DataKey::generate();
        let aad = record_aad("message", "rid-1", "0001", "dev-a");
        let sealed = dk.seal(&aad, b"secret text").unwrap();
        assert_ne!(sealed.ciphertext, b"secret text");
        assert_eq!(dk.open(&aad, &sealed).unwrap(), b"secret text");
        for other in [
            record_aad("chat", "rid-1", "0001", "dev-a"),
            record_aad("message", "rid-2", "0001", "dev-a"),
            record_aad("message", "rid-1", "0002", "dev-a"),
            record_aad("message", "rid-1", "0001", "dev-b"),
        ] {
            assert!(dk.open(&other, &sealed).is_err(), "swapping a ciphertext between records must fail");
        }
        let mut tampered = sealed.clone();
        tampered.ciphertext[0] ^= 1;
        assert!(dk.open(&aad, &tampered).is_err());
        assert!(DataKey::generate().open(&aad, &sealed).is_err());
    }

    #[test]
    fn nonces_are_fresh_and_remote_ids_hide_real_ids() {
        let dk = DataKey::generate();
        let aad = record_aad("a", "b", "c", "d");
        assert_ne!(dk.seal(&aad, b"x").unwrap().nonce, dk.seal(&aad, b"x").unwrap().nonce);
        let id = dk.remote_id("setting", "profile.display_name");
        assert!(!id.contains("display"));
        assert_eq!(id, dk.remote_id("setting", "profile.display_name"), "deterministic, so a record's ops group together");
        assert_ne!(id, dk.remote_id("chat", "profile.display_name"));
        assert_ne!(id, DataKey::generate().remote_id("setting", "profile.display_name"));
    }

    #[test]
    fn keychain_round_trip_and_bad_recovery_words() {
        let dk = DataKey::generate();
        assert_eq!(DataKey::from_base64(&dk.to_base64()).unwrap().to_base64(), dk.to_base64());
        assert!(DataKey::from_base64("nope").is_err());
        assert!(RecoveryKey::parse("not twenty four words").is_err());
    }

    #[test]
    fn default_parameters_take_a_noticeable_but_bearable_time() {
        let start = std::time::Instant::now();
        create_account("benchmark passphrase", KdfParams::DEFAULT).unwrap();
        let ms = start.elapsed().as_millis();
        assert!(ms < 5_000, "key derivation took {ms} ms");
    }
}
