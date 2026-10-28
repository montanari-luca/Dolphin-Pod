# Phase 6: persona proposals, never silent drift

## Design
- Per-agent switch `agents.suggest_persona` (default off, synced, settable by Roger as `agent.suggest_persona`). It only has an
  effect while the agent's learning is on, since proposals come out of the learner.
- With it on, the learner's prompt shows the agent's current persona and offers one extra op, `persona`. The op never changes
  the persona: it creates a **proposal** (`persona_proposal` table, device-local) with a short reason. At most one pending
  proposal per agent (a newer one replaces it); text identical to the current persona or to a proposal the user already
  rejected is dropped; same length cap (1,500) and secret filter as a persona typed by hand.
- The user decides: **Accept** sets the persona (the agent syncs as usual) and **Dismiss** rejects it. Nothing else can write a persona.
- `CoreEvent::PersonaProposalChanged { agent_id, proposal }`: `Some` = a new proposal (toast with "Review"), `None` = resolved.
- UI: switch under the learning switch in the Memory panel (goes through the editor's draft and Save); a card above the persona
  field shows the pending proposal, its reason and the current persona, with Accept / Dismiss.

## Touch list
- `db/migration_0009_*.sql`, `db/persona.rs`, `db/agents.rs`, `model.rs` (Agent.suggest_persona, PersonaProposal), `sync/kinds.rs`, `seed.rs`
- `memory/learner.rs` (Op::Persona, prompt options), `engine/learning.rs`, `core.rs` (accept / reject), `events.rs`, `settings.rs`
- Tauri + `api.ts` / `tauri.ts` / `mock.ts`, store, `AgentMemory.tsx`, `AgentsPage.tsx`, bindings
- tests: learner unit tests, `tests/learning.rs`, sync, store
