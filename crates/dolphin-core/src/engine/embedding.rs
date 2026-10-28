//! Ranking an agent's notes by meaning, with the embedding model the user picked (if any). Anything that goes wrong,
//! from a missing model to a slow server, means "no scores" and the caller ranks by shared words instead: a wrong setting
//! must never break a request.

use std::time::Duration;

use super::*;
use crate::memory::embed::{Scores, cosine, text_hash};

/// A request waits this long for the embedding model; a slower one is skipped for this request.
const EMBED_TIMEOUT: Duration = Duration::from_secs(4);
/// The Test button waits longer: the first call may have to load the model.
const TEST_TIMEOUT: Duration = Duration::from_secs(60);
const QUERY_CHARS: usize = 1500;

impl Core {
    /// How close in meaning each of these notes is to the request, or `None` when that cannot be said (no embedding model
    /// chosen, the provider cannot embed, it failed or was too slow). Notes are embedded once per model and kept; one
    /// batched call embeds the request together with whichever notes are new or reworded.
    pub(crate) async fn semantic_scores(&self, notes: &[&AgentMemory], query: &str) -> Option<Scores> {
        let model = settings::get_model(&self.db, keys::EMBEDDING_MODEL)?;
        let (_, provider) = self.provider(&model.provider_id).ok()?;
        let key = format!("{}:{}", model.provider_id, model.model_id);
        let ids: Vec<String> = notes.iter().map(|m| m.id.clone()).collect();
        let cached = self.db.embeddings_for(&key, &ids).ok()?;
        let fresh_for = |m: &AgentMemory| cached.get(&m.id).filter(|(hash, _)| *hash == text_hash(&m.text)).map(|(_, v)| v);
        let missing: Vec<&&AgentMemory> = notes.iter().filter(|m| fresh_for(m).is_none()).collect();

        // "@Rick why is my sql slow?" means what comes after the mention.
        let mut inputs = vec![truncate(crate::memory::signals::without_leading_mentions(query), QUERY_CHARS)];
        inputs.extend(missing.iter().map(|m| m.text.clone()));
        let vectors = tokio::time::timeout(EMBED_TIMEOUT, provider.embed(&model.model_id, &inputs)).await.ok()?.ok()?;
        if vectors.len() != inputs.len() {
            return None;
        }
        let (query_vec, new_vecs) = vectors.split_first()?;
        for (m, v) in missing.iter().zip(new_vecs) {
            let _ = self.db.put_embedding(&m.id, &key, &text_hash(&m.text), v);
        }
        let new_by_id: std::collections::HashMap<&str, &Vec<f32>> = missing.iter().map(|m| m.id.as_str()).zip(new_vecs).collect();
        Some(
            notes
                .iter()
                .filter_map(|m| {
                    let v = fresh_for(m).or_else(|| new_by_id.get(m.id.as_str()).copied())?;
                    Some((m.id.clone(), cosine(query_vec, v)?))
                })
                .collect(),
        )
    }

    /// Try the chosen embedding model once, for the Settings "Test" button. The size of the vectors it returns.
    pub async fn test_embedding_model(&self) -> Result<usize> {
        let model = settings::get_model(&self.db, keys::EMBEDDING_MODEL).ok_or_else(|| CoreError::invalid("pick an embedding model first"))?;
        let (_, provider) = self.provider(&model.provider_id)?;
        let vectors = tokio::time::timeout(TEST_TIMEOUT, provider.embed(&model.model_id, &["Dolphin Pod embedding test".to_string()]))
            .await
            .map_err(|_| CoreError::provider("the model took too long to answer"))??;
        vectors.first().map(Vec::len).filter(|n| *n > 0).ok_or_else(|| CoreError::provider("the model returned no vector"))
    }
}
