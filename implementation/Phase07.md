# Phase 7: sharing a note with other agents (opt-in)

## Design
- A note has a `share`: `none` (default), `all`, or `agents { ids }`. Stored on the note (synced field `share`, JSON text).
  The owner keeps it and edits it; other agents are *sent* it, they do not own a copy.
- Injection: an agent's candidates are its own notes plus notes other agents shared with it. Same project scoping, the same
  always-on / relevance rule, and a shared note whose text equals one of the agent's own is skipped.
- A shared note is the user's decision, so it is **locked for the learner** like a pinned or typed note (never rewritten or
  expired by the owner's learner). Notes shared in are never in the receiving agent's learner prompt and do not count toward its
  40-entry cap.
- Changing or deleting a shared note, or forgetting an agent's memory, tells every agent it reaches (`MemoriesChanged`).
- UI: per-note "Share" control in the Memory list (private / all agents / chosen agents, with a checklist dialog) and a
  read-only "From other agents" list in each agent's Memory panel.
- Global `preferences` is unchanged (still sent to every agent).

## Touch list
- `db/migration_0010_*.sql` (column + recreated sync trigger), `db/memory.rs`, `model.rs` (MemoryShare), `sync/kinds.rs`
- `memory/mod.rs`, `memory/learner.rs` (lock), `engine.rs` (inject), `core.rs` (update, list shared, events)
- Tauri, `api.ts`, `tauri.ts`, `mock.ts`, `AgentMemory.tsx`, bindings
- tests: `memory/` unit, `tests/memory.rs`, `tests/learning.rs` (learner lock), sync, mock
