# Phase 2: Sync, iOS app and remote control

> Temporary plan. Delete the `implementation/` directory once all phases are complete.

**Goal:** a chat started on the desktop continues on the iPhone, and a message on the iPhone can trigger a
desktop action both when the devices are on the same network and when they are far apart.

**Exit criteria**
- Two desktops and one iPhone sharing a passphrase converge on the same chats, agents, settings, providers
  (keys included), preferences, automations and usage totals, with no plaintext reaching the database.
- Chat cleanup on one device clears the same messages everywhere.
- From the iPhone: a request routed to a desktop agent runs there, nearby (LAN) and remote (cloud), with the
  result streamed back.

---

## 0. Carry-overs from Phase 1

Do these first; they are small and unblock testing.

- [ ] Replace `UPDATE_MANIFEST_URL` in `crates/dolphin-core/src/update.rs` with the real raw GitHub URL (waiting on
      the owner) and publish a `latest.json` (`latest`, `macos`, `windows`, `linux`, `ios`).
- [ ] Confine `run_command` to the workspace (macOS `sandbox-exec`, Linux `bwrap`, documented fallback on Windows).
      File tools are already confined by `permissions::resolve_in_workspace`. A proposal card for this exists.
- [ ] Verify by eye: the native window (vibrancy, overlay title bar, traffic-light spacing) and the icon in Finder,
      the Dock and the Windows taskbar.
- [ ] Run the live harness against one cloud provider (OpenRouter free model) as well as Ollama:
      `cargo run -p dolphin-core --example live_ollama -- qwen3.5:2b`, plus a cloud variant.
- [ ] Automations: honor `automations.runner_device_id` so only the designated device fires each automation
      (the column already exists; the scheduler ignores it today).

## 1. Sync engine (`crates/dolphin-core/src/sync/`)

Local-first: SQLite on each device stays the source of truth; the cloud database is a relay.

**Already in place:** syncable tables carry `updated_at`, `device_id` and `deleted` tombstones; usage is stored per
device (`usage` primary key `agent_id, device_id`) and summed on read; cleanup writes tombstones.

**To build**
- [ ] Schema v2 migration (additive): a `sync_outbox` table (record type, id, field patch, hybrid logical clock),
      a `sync_state` table (last pulled cursor per remote), and per-field clocks for LWW records.
- [ ] Hybrid logical clocks (HLC) instead of wall-clock `updated_at` for ordering, so skewed device clocks can't
      win incorrectly.
- [ ] Merge rules: last-writer-wins per field (agents, chats, settings, providers, automations, preferences);
      append-only for messages (content immutable once `done`; status and meta merge by field); tombstones always
      win over older writes; usage rows are never merged, only summed.
- [ ] Per-platform agent overrides: `agents.ios_model` (exists) syncs as its own field and never clobbers `model`.
- [ ] Transport: `SyncBackend` trait with a Postgres implementation (`tokio-postgres` + rustls). Remote schema is
      one `records` table (`id`, `kind`, `hlc`, `device_id`, `ciphertext`, `nonce`, `deleted`) plus `devices`
      and `commands` (section 4).
- [ ] Providers: **NeonDB by default**; any Postgres-compatible URL can be entered in Settings → Sync on each
      device. v0.1 supports Postgres only. The URL is stored in the keychain, and changing it is a sensitive setting.
- [ ] Scheduling: push on local change (debounced), pull on launch, on focus and every 30 s. Use `LISTEN/NOTIFY`
      for near-real-time where the provider allows it.
- [ ] Attachments: encrypted blobs in a `blobs` table (chunked) or provider object storage; lazy download on view.
- [ ] Tests: property tests for merge convergence (random op orders on 3 replicas converge), tombstone vs late
      edit, usage summation, clock skew.

## 2. End-to-end encryption (`crates/dolphin-core/src/crypto.rs`)

- [ ] Key derivation from the user passphrase with Argon2id (memory-hard parameters tuned to about 250 ms on
      the phone); per-account random salt stored remotely in plaintext.
- [ ] Record encryption with XChaCha20-Poly1305 (random 24-byte nonce). The associated data binds `kind`, `id` and
      `hlc` so ciphertexts can't be swapped between records.
- [ ] Everything is encrypted, not only API keys: chats, messages, settings, agents and preferences. The
      database sees only ids, kinds, clocks and sizes.
- [ ] The passphrase-derived key is cached in the OS keychain on each device (`SecretStore`, already abstracted).
- [ ] Recovery key: a random 256-bit key shown once as words or a QR code, which wraps the data key, because a
      forgotten passphrase can't be reset. Passphrase change re-wraps the data key without re-encrypting records.
- [ ] Settings → Sync UI: enable, enter passphrase, show recovery key, choose provider and URL, device list with
      revoke, last-sync status.

## 3. iOS app (`apps/ios/`, SwiftUI)

- [ ] UniFFI bindings for `dolphin-core`. Expose `Core` as a UniFFI object and core events as a callback
      interface, and build an XCFramework (`aarch64-apple-ios`, `aarch64-apple-ios-sim` targets are installed).
      Add a `crates/dolphin-ffi` wrapper crate so `dolphin-core` stays free of FFI attributes.
- [ ] Xcode project consuming the XCFramework; the AppIcon set (light, dark, tinted) is already generated in
      `apps/ios/DolphinPod/Assets.xcassets/AppIcon.appiconset`.
- [ ] Scope: chat-only by default. The chat list, chat view (Markdown, streamed thinking, agent colors and
      gradients, @mention autocomplete), attachments from Photos and Files, and a settings sheet for sync, the
      passphrase and per-agent iOS model overrides.
- [ ] Design: the same frosted look with SwiftUI materials (`.ultraThinMaterial`), dark and light. The agent card
      (job, all-time tokens, thinking and working time) opens on long-press or tap, since phones have no hover.
- [ ] Local model capability checks: synced local models (Ollama or LM Studio on a PC) are unreachable or too big
      on the phone, so they show as unavailable and the agent falls back to its iOS override. On-device inference
      uses MLX (or llama.cpp) with a curated list of small reasoning models; cloud and custom endpoints always work.
- [ ] Keychain storage for keys and the passphrase cache. Background refresh triggers sync.
- [ ] Updates: read the `ios` link from the same manifest (TestFlight or App Store URL).
- [ ] About screen with MintFlow and Luca Montanari credits.
- [ ] Verify early: direct Postgres from iOS (TLS, background limits). If it's a problem, use Neon's HTTP/WebSocket
      SQL interface or a minimal relay.

## 4. Remote control (iPhone → desktop)

- [ ] Opt-in on the desktop (Settings → Remote control), per-device authorization, and the same permission levels
      as local use. Every remote action appears in the chat and the action log, marked as remote.
- [ ] Presence: desktops publish a heartbeat record (`devices`: name, last seen, capabilities); the phone lists
      the running desktops and picks a target (default: most recently active).
- [ ] Fast path (nearby): the desktop advertises `_dolphinpod._tcp` via Bonjour; the phone connects over a
      WebSocket. Mutual authentication uses keys derived from the shared sync key (Noise or TLS with a PSK). This
      requires the iOS Local Network permission.
- [ ] Cloud path (distant): commands are written as encrypted `commands` records; the desktop picks them up, runs
      them through the normal pipeline and writes results back as message records.
- [ ] Selection logic: try the fast path with a short timeout, fall back to the cloud automatically, and show which
      path is in use.
- [ ] Tests: command authorization, replay protection (nonces and expiry), fallback switching.

## Open questions

1. Hosting the Neon project: the owner's account or each user's own (the default implies each user brings a URL)?
2. On-device iOS models: which curated list ships first (size and quality trade-off)?
3. App Store vs TestFlight-only for the first iOS release.
