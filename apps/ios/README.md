# Dolphin Pod for iOS

SwiftUI client for the same pod as the desktop app. All logic lives in `crates/dolphin-core`; the app consumes it
through UniFFI (`crates/dolphin-ffi`) as an XCFramework. Structured data crosses the boundary as JSON that the
Swift `Codable` models in `DolphinPod/Core/Models.swift` decode, and core events arrive through a callback
interface.

What it does: chat list and chat view (Markdown, streamed thinking, agent colors and gradients, `@mention`
autocomplete), photos and files, agent cards on tap or long-press, encrypted sync (Settings → Sync), per-agent iPhone
model overrides, providers and keys, and remote control of a desktop (pick it next to the message box; nearby over
Wi-Fi via Bonjour, otherwise through the cloud). The About screen carries the MintFlow Technologies and Luca
Montanari credits.

## Build

Requirements: Xcode 16+, Rust with the iOS targets, [XcodeGen](https://github.com/yonaskolb/XcodeGen).

```bash
rustup target add aarch64-apple-ios aarch64-apple-ios-sim
```

```bash
apps/ios/scripts/build-xcframework.sh
```

```bash
cd apps/ios && xcodegen generate && open DolphinPod.xcodeproj
```

The script builds the static libraries for device and Apple-silicon simulator, generates the Swift bindings into
`Generated/` and packs `Frameworks/DolphinFFI.xcframework` (all git-ignored). `PROFILE=release` uses full LTO for
App Store builds. To run in the simulator from the command line:

```bash
xcodebuild -project apps/ios/DolphinPod.xcodeproj -scheme DolphinPod -destination 'platform=iOS Simulator,name=iPhone 17' build
```

Set your team in Xcode (Signing & Capabilities) to run on a device. Push notifications and in-app purchases are not
used. The app asks for Local Network access once, only to find your desktop for remote control.

## Notes

- Keys (provider keys, the sync key, the signing key) live in the iOS keychain through the same `SecretStore` the
  desktop uses.
- Background refresh (`dev.mintflow.dolphinpod.refresh`) asks the core to sync so the app opens up to date.
- Updates: *About → Check for updates* reads the `ios` link of the shared release manifest.
