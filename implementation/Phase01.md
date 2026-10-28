# Phase 1: editable per-agent persona

Part of the per-agent personality / memory / adaptation work (plan: `~/.claude/plans/each-agent-should-have-giggly-pretzel.md`).

## Scope
- `agents.persona TEXT NULL` (migration 0006), synced with the agent row.
- The persona is a layer on top of the built-in specialty text, never a replacement: `persona(specialty)` carries working
  instructions (git confirmations, researcher method, image tool), so it stays. A set persona adds a "Personality and voice"
  paragraph to the stable part of `agent_system`, before the date.
- Editable in Settings -> Agents and through the settings registry (`agent.persona`) so Roger can change it from chat.

## Touch list
- `db/migration_0006_persona.sql`, `db/mod.rs` (MIGRATIONS), `db/agents.rs` (COLS, row_to_agent, upsert)
- `model.rs` (Agent.persona), `sync/kinds.rs` (agent fields), `seed.rs`, `router/mention.rs` tests (struct literals)
- `settings.rs` (key, def, handler), `core.rs::update_agent`
- `router/prompts.rs` (agent_system)
- `apps/desktop`: AgentsPage persona editor, mock data; `pnpm bindings` for the TS type
- `tests/`: persona in system prompt, null persona leaves the prompt unchanged, settings round trip, sync round trip

## Done when
- `cargo test -p dolphin-core`, clippy `-D warnings`, `pnpm typecheck` and desktop vitest pass.
- Committed locally (not pushed).
