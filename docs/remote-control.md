# Remote control (iPhone to desktop)

Start work on your computer from your iPhone: the request runs on the desktop, where your agents, files and local
models are, and the answer streams back to your phone. It needs [sync](sync.md) on both devices.

## Set it up

1. On the desktop: **Settings → Remote control**, turn on *Accept remote requests*, then switch on your iPhone in
   *Devices that may control this computer*. Remote control is off by default, and each device is allowed one by one.
2. On the iPhone: next to the message box, pick the desktop instead of *This iPhone*, then send.

## Two paths, one request

The iPhone tries the fast path first and falls back by itself:

- **Nearby (Wi-Fi).** The desktop advertises itself on the local network and the phone connects straight to it over an
  encrypted channel keyed from your pod key. The run streams back live. iOS asks once for *Local Network* access.
- **Cloud.** If the desktop is not on the same network, the request is sealed into your sync database. The desktop
  picks it up within a few seconds and the answer comes back as ordinary synced messages, a few times per second
  while it is running.

The composer shows which path carried the message.

## Safety

- Every request is **signed by the phone's own key**. When you allow a device, the desktop pins that device's public
  key. Someone who has the database login and your passphrase still cannot send requests the desktop would obey.
- Requests **expire after five minutes**, carry a unique id and run **at most once**, so a replayed or retried request
  does nothing.
- Agents follow the **same permission rules as local use**: read-only until you approve a plan.
- Every remote request appears in the chat as a message marked *via iPhone* and in the action log, and the desktop
  shows a notification.
- Turn the switch off, or remove a device, and requests stop immediately.
