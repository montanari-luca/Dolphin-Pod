# Implementation plans (temporary)

> **Delete this whole `implementation/` directory once every phase below is complete.** These are working
> build plans, not product documentation; lasting docs belong in `docs/` and the app READMEs.

Phases 0 (foundation) and 1 (desktop app) are done; Phase 2 is built (see its status notes for what still needs real hardware or accounts); Phase 3 (website) is built with a few carry-overs. What is left:

| File | Phase | Scope |
| --- | --- | --- |
| [Phase02.md](Phase02.md) | 2 | Encrypted cloud sync, SwiftUI iOS app, iPhone remote control (plus Phase 1 carry-overs) |
| [Phase03.md](Phase03.md) | 3 | Website: built; see the carry-overs at the top of the file |

Ground rules that apply to every phase:

- Commit locally at each milestone; never push unless asked.
- Business logic lives in `crates/dolphin-core`; apps stay thin.
- Types shared with TypeScript come from Rust via ts-rs (`pnpm bindings`).
- Credits (MintFlow Technologies, mintflow.dev; lead developer Luca Montanari, instagram.com/montanarii.luca)
  appear in every app's About screen, the website footer and the README.
- Local test models: `qwen3.5:2b` or newer, never qwen3-era models.

When the last phase is finished: move anything still useful into `docs/`, remove the `implementation/`
link from the root README, then delete this directory.
