# Sync and encryption

Sync keeps chats, agents, settings, provider keys, your profile, automations and usage identical on all your
devices. It is **off by default** and needs a Postgres database you control; [NeonDB](https://neon.tech) has a
free tier, and any Postgres-compatible URL works.

## Turn it on

1. **Settings → Sync**, paste the database URL (use the direct, non-pooled connection) and choose a passphrase of
   at least 8 characters.
2. The first device creates your pod and shows a **recovery key** (24 words) exactly once. Save it somewhere safe.
3. On every other device (desktop or iPhone) enter the same URL and passphrase to join.

The URL is kept in the system keychain, and changing it later asks for confirmation.

## What is private

Everything is encrypted on your device before it is uploaded. The database only ever sees ciphertext, random
record ids (keyed hashes, never the real ids), record kinds, clocks and sizes. It cannot read chat names, message
text, settings, agent definitions or API keys.

- Your passphrase goes through **Argon2id** (64 MiB, 3 passes) to unwrap a random 256-bit **data key**.
- Records are sealed with **XChaCha20-Poly1305**. The associated data binds each ciphertext to its kind, id, clock
  and author, so a ciphertext cannot be moved to another record.
- The data key is cached in the OS keychain of each device so you only type the passphrase once.
- Changing the passphrase re-wraps the data key; nothing is re-encrypted. The recovery key is a second wrap of the
  same data key, which is why a forgotten passphrase can be recovered but never reset by anyone else.

**If you lose both the passphrase and the recovery key, the data cannot be recovered.**

## How devices agree

Each device keeps a normal local database and works offline. Edits become small field patches, stamped with a
hybrid logical clock (so a phone with a wrong clock cannot overwrite newer work), pushed after a short delay and
pulled on launch, on focus and every 30 seconds.

- **Last writer wins per field.** Two devices editing different fields of the same agent keep both edits. The
  iPhone model override is its own field and never overwrites the desktop model.
- **Messages never change once finished.** A message still being written stays on its device until it is done.
- **Deletes win over older edits.** Cleaning a chat on one device removes the same messages everywhere, and the text
  is erased, not just hidden.
- **Usage is summed**, never overwritten: each device counts its own tokens and times, and every device shows the total.
- Device-local things stay local: a chat's workspace folder, permission grants, the action log and which files an
  attachment is stored in.
- Attachments upload as encrypted chunks and download the first time you open them on another device.

## Devices

**Settings → Sync → Devices** lists every device with when it was last seen. *Remove* stops a device from syncing the
next time it connects. Removing is cooperative: to lock out a lost device for good, also change the database password
and your passphrase.

## Good to know

- Sync polls instead of holding a connection open, so a NeonDB project can still scale to zero when you are idle.
- Local models (Ollama, LM Studio, localhost endpoints) sync as configuration but are unreachable from a phone. They
  show as unavailable there and the agent uses its iPhone model instead.
- Only one device runs each automation: the one it was created on (change this on the automation).
