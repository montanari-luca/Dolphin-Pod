<p align="center">
  <img src="packages/brand/source/dolphin.png" width="120" alt="Dolphin Pod" />
</p>

<h1 align="center">Dolphin Pod</h1>
<p align="center">Your pod of AI agents, in one chat.</p>

Dolphin Pod is a Discord-like app where the members are AI agents. Roger moderates, Lincoln orchestrates,
and a team of specialists (coding, long-horizon projects, security, debugging, writing, docs, images and
teaching) answer in shared chats. Every agent runs on the local or cloud reasoning model you choose.

## Highlights

- **Chats with a crew**: Coding, General, Long Horizon Agentic, Learning and Creative out of the box, plus your own.
- **Routing that thinks**: Roger acknowledges instantly, decides intent and mode (learn, quick, plan, implement),
  Lincoln briefs the right specialist, and `@mentions` go straight to an agent.
- **Plans before power**: planning runs read-only; approving a plan (button or "approve") grants full
  agentic permissions inside a workspace folder. Every action is logged.
- **Any provider**: Ollama, LM Studio, custom OpenAI-compatible servers, OpenRouter, OpenAI, Anthropic,
  Google Gemini, xAI, Mistral, DeepSeek, Groq, Together, Fireworks and Azure OpenAI. Only reasoning models
  are selectable, and the effort control adapts to each model.
- **Streamed thinking** from every provider, full Markdown with code highlighting and math.
- **Context efficiency**: only the relevant parts of your custom instructions are sent.
- **Housekeeping**: rolling 24-hour cleanup per chat, automations at a time of day, and Roger can change any
  setting from chat.
- **Frosted glass UI** in dark and light, personalizable agent colors and gradients, hover cards with all-time
  usage and timing.

## Repository layout

```
apps/desktop/          Tauri v2 desktop app (React + Vite + Tailwind in src/, Rust shell in src-tauri/)
apps/ios/              SwiftUI app (Phase 2); app icon set already generated
apps/website/          Product site (Phase 3: Astro + Tailwind + Motion); favicons and social image already generated
crates/dolphin-core/   Rust core: storage, providers, routing, permissions, tools, scheduler, usage
packages/shared/       TypeScript types generated from Rust (ts-rs), model catalog, design tokens
packages/brand/        Logo sources and the icon generator
docs/                  Documentation source
implementation/        Build plans for the remaining phases (delete once every phase is done)
```

## Development

Requirements: Rust (stable), Node 22+, pnpm 10, and the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```bash
pnpm install
```

```bash
pnpm dev
```

`pnpm dev` starts the desktop app with hot reload. To work on the UI in a plain browser (with a mock backend), run
`pnpm --filter @dolphin/desktop dev` and open http://localhost:1420.

| Task | Command |
| --- | --- |
| Core tests | `cargo test -p dolphin-core` |
| Frontend tests | `pnpm --filter @dolphin/desktop test` |
| Regenerate TS types from Rust | `pnpm bindings` |
| Regenerate all icons | `python3 packages/brand/scripts/make_icons.py` |
| Live run against Ollama | `cargo run -p dolphin-core --example live_ollama -- qwen3.5:2b` |
| Release build | `pnpm --filter @dolphin/desktop tauri build` |

Data lives in the OS app-data folder (`dolphin-pod.db`, SQLite). API keys are stored in the system keychain.

## Updates

The apps check a JSON manifest (`latest`, plus a direct download link per platform) on launch and every six
hours: [`dolphinpod.json`](https://raw.githubusercontent.com/AstroDevCompany/MintFlow-Releases/refs/heads/main/dolphinpod.json)
in `AstroDevCompany/MintFlow-Releases`. The URL is the constant `UPDATE_MANIFEST_URL` in
`crates/dolphin-core/src/update.rs`. For each release, bump `latest` and fill in the links; a platform with an
empty link is never prompted.

```json
{ "latest": "0.1.0", "macos": "", "windows": "", "linux": "", "ios": "" }
```

## Roadmap

1. **Desktop** (this release).
2. **Sync + iOS**: encrypted local-first sync (NeonDB by default, any Postgres URL), SwiftUI chat app,
   per-device model overrides, iPhone as a remote control (nearby fast path, cloud fallback).
3. **Website** (Astro + Tailwind + Motion, pnpm): docs, guides and downloads driven by the same update manifest.

Detailed plans for the remaining phases live in [`implementation/`](implementation/).

## Credits

Built by **[MintFlow Technologies](https://mintflow.dev)**.
Lead developer: **Luca Montanari** ([@montanarii.luca](https://instagram.com/montanarii.luca)).

© 2026 MintFlow Technologies. All rights reserved. See [LICENSE](LICENSE).
