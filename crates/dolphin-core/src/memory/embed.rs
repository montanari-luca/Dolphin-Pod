//! The maths and bookkeeping for ranking notes by meaning: similarity, a stable hash of a note's text (so an edit is
//! noticed and a cached vector is not trusted for different words) and the byte form vectors are stored in.
//! Getting the vectors is the provider's job (`Provider::embed`); the cache and the call live in `engine/embedding.rs`.

use std::collections::HashMap;

/// A note must be at least this similar to the request to be sent on relevance. Unrelated texts usually score well
/// below this with a decent embedding model; the cap on how many are picked does the rest.
pub const MIN_SIMILARITY: f32 = 0.35;

/// Note id to its similarity with the request.
pub type Scores = HashMap<String, f32>;

/// Cosine similarity, `None` when the vectors cannot be compared (different sizes, or one has no direction).
pub fn cosine(a: &[f32], b: &[f32]) -> Option<f32> {
    if a.len() != b.len() || a.is_empty() {
        return None;
    }
    let (mut dot, mut na, mut nb) = (0f32, 0f32, 0f32);
    for (x, y) in a.iter().zip(b) {
        dot += x * y;
        na += x * x;
        nb += y * y;
    }
    (na > 0.0 && nb > 0.0).then(|| dot / (na.sqrt() * nb.sqrt()))
}

/// FNV-1a, so the same text always has the same hash, whatever the Rust version.
pub fn text_hash(text: &str) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for b in text.trim().to_lowercase().bytes() {
        h ^= b as u64;
        h = h.wrapping_mul(0x100000001b3);
    }
    format!("{h:016x}")
}

pub fn to_bytes(v: &[f32]) -> Vec<u8> {
    v.iter().flat_map(|f| f.to_le_bytes()).collect()
}

pub fn from_bytes(b: &[u8]) -> Option<Vec<f32>> {
    let (words, rest) = b.as_chunks::<4>();
    (!words.is_empty() && rest.is_empty()).then(|| words.iter().map(|w| f32::from_le_bytes(*w)).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn similarity_is_direction_not_length() {
        assert_eq!(cosine(&[1.0, 0.0], &[3.0, 0.0]), Some(1.0));
        assert_eq!(cosine(&[1.0, 0.0], &[0.0, 2.0]), Some(0.0));
        assert!((cosine(&[1.0, 1.0], &[1.0, 0.0]).unwrap() - 0.70710677).abs() < 1e-6);
        assert_eq!(cosine(&[1.0], &[1.0, 2.0]), None, "different sizes cannot be compared");
        assert_eq!(cosine(&[0.0, 0.0], &[1.0, 1.0]), None);
        assert_eq!(cosine(&[], &[]), None);
    }

    #[test]
    fn the_hash_follows_the_words_not_the_case_or_the_edges() {
        assert_eq!(text_hash("The database is PostgreSQL"), text_hash("  the database is postgresql "));
        assert_ne!(text_hash("Use tabs"), text_hash("Use spaces"));
        assert_eq!(text_hash("x").len(), 16);
    }

    #[test]
    fn vectors_survive_being_stored() {
        let v = vec![0.5, -1.25, 3.0e-3, 42.0];
        assert_eq!(from_bytes(&to_bytes(&v)), Some(v));
        assert_eq!(from_bytes(&[1, 2, 3]), None);
        assert_eq!(from_bytes(&[]), None);
    }
}
