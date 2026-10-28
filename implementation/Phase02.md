# Phase 2: per-agent memory store, injection and Memory list

Part of the per-agent personality / memory / adaptation work (plan: `~/.claude/plans/each-agent-should-have-giggly-pretzel.md`).
Phase 1 (persona) is done. Phase 3 adds the signals and the learner that write into this store.

## Scope
- `agent_memory` table (migration 0007), synced. Entries: kind `style|fact|workflow|avoid`, text, optional project tag,
  source `auto|explicit|edited`, pinned, hits.
- Project tag = the workspace **folder name** (e.g. `dolphin-pod`), not the path: `chats.workspace_path` is device-local and
  never syncs, so a path would mean nothing on another device.
- `hits` and `updated_at` are device-local (not synced). `updated_at` stays local because a synced last-touched timestamp is its own
  last-writer-wins field and two devices writing within the same millisecond left them 1 ms apart; `agents` doesn't sync it either.
  Bumping `hits` does not mark the row dirty: the update trigger only fires for synced columns.
- Injection: a short "what you know about this user" block as a volatile message after the history (never in the stable
  system prompt, so prompt caches keep hitting). Candidates = this agent's entries with no project tag or the chat's project.
  Always included: pinned, `style`, `avoid`. `fact`/`workflow`: all if small, otherwise keyword-relevant (reuses `preferences.rs`).
  No utility-model call: selection costs nothing.
- Core CRUD (list / add / update / delete / forget all) + Tauri commands + `api.ts` / `tauri.ts` / `mock.ts`.
- `CoreEvent::MemoriesChanged` so the list refreshes after a remote sync; store keeps a per-agent version counter.
- Memory list in Settings -> Agents: add, edit text, kind, scope (everywhere / one of your workspace folders), pin, delete, forget all.
- Hard cap 40 entries per agent for manual adds (Phase 3's learner merges/expires to stay under it).

## Not in this phase
- Per-agent "learning on/off" switch: it has nothing to switch until Phase 3 (signals + learner), so it lands there.
- Memories are not shared between agents; the global `preferences` blob is untouched.

## Touch list
- `db/migration_0007_agent_memory.sql`, `db/mod.rs`, new `db/memory.rs`
- `model.rs` (MemoryKind, MemorySource, AgentMemory), `events.rs` (MemoriesChanged), `sync/kinds.rs`, `sync/manager.rs` (emit)
- new `memory/mod.rs` (limits, project tag, selection, block formatting), `preferences.rs` (`pick` helper), `lib.rs`
- `core.rs` (CRUD), `engine.rs` (inject)
- `apps/desktop/src-tauri/src/lib.rs`, `ipc/{api,tauri,mock}.ts`, `store/app.ts`, new `features/settings/AgentMemory.tsx`, `AgentsPage.tsx`
- tests: `memory/` unit tests, `tests/memory.rs` (injection placement, scoping, isolation between agents), sync round trip, store/mock tests
- docs: `docs/agents.md` short section (full `docs/memory.md` lands in Phase 4)

## Done when
- `cargo test -p dolphin-core`, `pnpm typecheck`, desktop vitest pass; clippy shows nothing new beyond the 13 errors already on master.
- Memory list checked in the browser. Committed locally, not pushed.
