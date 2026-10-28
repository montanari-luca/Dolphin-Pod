# Phase 5: idle-time reflection

Follow-up to the four-phase per-agent personality / memory / adaptation work. Catches what the live signals missed:
preferences a user showed repeatedly without ever correcting anything.

## Design
- Reuses the learner pipeline: reflecting on a chat queues a `Signal::Reflection` (detail: a transcript excerpt) for each agent
  that took part; the existing learner turns it into notes under the same rules (only its own notes are touched, secrets and
  duplicates skipped, the 40 cap, Undo toasts).
- **Opt-in, off by default** (`learning.reflect`, Settings -> General). It sends more of the conversation to the utility model
  than signal-triggered learning does (a transcript, not three excerpts), which is a privacy step the user should take knowingly.
- Triggered by the 30 s scheduler tick: a chat is reflected on when its newest finished message is 10 minutes old, nothing in it
  is still running, and at least 4 messages (with a user message and an agent answer) came since the last reflection.
  At most the 3 most active agents, last 24 messages, 400 characters each.
- Only conversations after the switch was turned on are looked at: `device.reflect_since` is set the first time the tick sees it
  on, and each chat's progress is `device.reflected.<chat id>`. Both are device-local settings (`device.` is never synced).
- Learning off for an agent skips it; an agent that was reflected on is not reflected on again for the same messages.

## Touch list
- `settings.rs` (key), `memory/signals.rs` (Reflection), `memory/learner.rs` (describe, prompt), `engine/learning.rs`
  (`reflect_idle`), `scheduler.rs` (tick), `tests/learning.rs`, `GeneralPages.tsx` (row), `docs/memory.md`
