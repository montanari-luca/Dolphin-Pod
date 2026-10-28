# Phase 3: signals, the background learner, the remember tool and the learning switch

Part of the per-agent personality / memory / adaptation work (plan: `~/.claude/plans/each-agent-should-have-giggly-pretzel.md`).
Phases 1 (persona) and 2 (memory store, injection, Memory list) are done. This phase fills the store automatically.

## Scope
- `agent_feedback` (migration 0008): device-local log of signals `revert|deny|thumb_up|thumb_down|correction|remember` with a
  `processed` flag. Not synced.
- `agents.learning` (default on): per-agent switch. Synced with the agent, settable by Roger (`agent.learning`).
  Off = no automatic learning (signals are not even recorded, no `remember` tool). Explicit "remember that..." and the manual
  Memory list still work: those are the user's own words.
- Signals recorded where they happen:
  - revert: `revert_run`; denial: only the two branches where the *user* declined an approval (a read-only refusal is a rule,
    not feedback); thumbs: new `rate_message` command (UI lands in Phase 4);
  - correction: in `pipeline()`, a short user message right after an agent's answer that looks like a correction
    ("no", "don't", "instead", "I said"...). A cheap heuristic that only flags; the learner decides if it is a lasting preference;
  - remember: "remember that..." / "from now on..." in a user message, credited to the agent(s) that answered it.
- Learner (`memory/learner.rs`, `engine/learning.rs`): one background task at a time, only when signals are queued, on the
  utility model (falls back to Roger's). Input: the agent's notes + the recent signals. Output: JSON `add/update/merge/delete`
  ops, at most 3. Bad or empty JSON, or no model: the signals are dropped and marked processed (an explicit "remember"
  without a usable model is stored as typed).
  - It may only change entries it wrote itself (`source = auto`, not pinned): never the user's own, edited or pinned ones.
  - Secret filter on everything it writes; 300-char limit; duplicates skipped; at the 40-entry cap it expires the
    least-used auto entry, or drops the add when nothing is expirable.
  - Never touches the persona.
- `remember` tool (`tools.rs`): offered when the model supports tools, learning is on, and it is not an automation or delegated run.
- `CoreEvent::MemoryLearned { memory, previous_text }` for every add/update (Phase 4 turns it into a toast with Undo).
- Idle-reflection hook: the learner takes "signals" in, so a later transcript-reflection pass can feed the same code (Phase 5).

## Not in this phase
- Thumbs and learned-toast UI, `docs/memory.md` (Phase 4). `ask_user` answers as weak signals: left out; they are stated
  choices for one task, which the learner prompt would mostly discard anyway.

## Touch list
- `db/migration_0008_*.sql`, `db/mod.rs`, new `db/feedback.rs`; `model.rs` (Agent.learning, Rating); `sync/kinds.rs`; `seed.rs`
- new `memory/signals.rs`, `memory/learner.rs`; `memory/mod.rs` (secret filter)
- new `engine/learning.rs`; `engine.rs` (pipeline hooks, tool offer + dispatch, denial hook); `engine/changes.rs` (revert hook)
- `tools.rs`, `router/prompts.rs`, `events.rs`, `settings.rs`, `core.rs`
- Tauri command + `api.ts`/`tauri.ts`/`mock.ts` for `rate_message`; learning switch in the Memory panel; bindings
- tests: unit (heuristics, parsing, apply rules), `tests/learning.rs` (end to end with a scripted utility model), sync, frontend

## Done when
- `cargo test -p dolphin-core`, typecheck and vitest pass; clippy adds nothing beyond the 13 errors already on master.
- Committed locally, not pushed.
