# Phase 8: embedding-based retrieval for memory

## Design
- `Provider::embed(model, inputs) -> Vec<Vec<f32>>`, default "not supported". Implemented for Ollama (`/api/embed`) and the
  OpenAI-compatible adapter (`/embeddings`: LM Studio, OpenRouter, custom endpoints, ...). Others fall back.
- Global optional setting `memory.embedding_model` (a provider + model). **Unset by default**: nothing changes and no model is
  chosen or downloaded for the user.
- Only matters once an agent has many notes (the existing "send everything while small" rule still comes first). Then the
  always-on notes (style / avoid / pinned) are still sent, and the others are ranked by cosine similarity to the user's request
  (top 6 above a floor) instead of by shared keywords.
- Vectors are cached on this device (`memory_embedding`, keyed by note + model, with a hash of the text so an edit re-embeds).
  One batched call per request embeds the query and any notes not cached yet.
- Any failure, timeout (4 s) or odd answer falls back to keyword selection, silently: a bad setting must not break a request.
- Settings -> General: pick the model (all models are listed, embedding models are not "reasoning" models) and a **Test**
  button that says whether it works (vector size) or why not.

## Touch list
- `providers/{mod,ollama,openai_compat}.rs`, new `memory/embed.rs`, `memory/mod.rs` (ranked select), `db/embeddings.rs`,
  `db/migration_0011_*.sql`, `settings.rs`, `engine.rs` + new `engine/embedding.rs`, `core.rs` (test), `scheduler.rs` (prune)
- Tauri `test_embedding_model`, `api.ts` / `tauri.ts` / `mock.ts`, `GeneralPages.tsx`
- tests: parsing, math, `tests/memory.rs` (semantic beats keywords, cache, fallback), frontend mock
