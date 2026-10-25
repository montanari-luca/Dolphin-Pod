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

## Generate the IPA

An `.ipa` needs a signed archive, so set `DEVELOPMENT_TEAM` in `project.yml` (or pick your team in Xcode) first. Build
the framework in release mode, regenerate the project, then archive:

```bash
PROFILE=release apps/ios/scripts/build-xcframework.sh
```

```bash
cd apps/ios && xcodegen generate
```

```bash
xcodebuild -project DolphinPod.xcodeproj -scheme DolphinPod -configuration Release -destination 'generic/platform=iOS' -archivePath build/DolphinPod.xcarchive -allowProvisioningUpdates archive
```

Create `build/ExportOptions.plist` to tell Xcode how to export. Set `method` to `development` or `ad-hoc` for installing on
registered devices, or `app-store-connect` for TestFlight and the App Store, and replace `YOURTEAMID` with your Apple
team ID. To find it, open Xcode → Settings → Accounts and select your team, or open
[developer.apple.com/account](https://developer.apple.com/account) → Membership details. It is 10 characters, for
example `JUQ9UN9NM7`. (The code in parentheses in a signing identity name is not the team ID.) A free Personal Team can
only use `development`, and the app expires after 7 days:

```bash
cat > build/ExportOptions.plist <<'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>method</key><string>ad-hoc</string>
  <key>teamID</key><string>YOURTEAMID</string>
  <key>signingStyle</key><string>automatic</string>
</dict>
</plist>
EOF
```

```bash
xcodebuild -exportArchive -archivePath build/DolphinPod.xcarchive -exportPath build/ipa -exportOptionsPlist build/ExportOptions.plist -allowProvisioningUpdates
```

The result is `build/ipa/Dolphin Pod.ipa`. To publish a new build, raise `CURRENT_PROJECT_VERSION` (and
`MARKETING_VERSION` for a new release) in `project.yml` before archiving, and point the `ios` link of the release
manifest at the uploaded file. With `app-store-connect` the export produces an IPA you upload with Transporter or
`xcrun altool --upload-app`. You can also use Xcode: *Product → Archive*, then *Distribute App* in the Organizer.

## Notes

- Keys (provider keys, the sync key, the signing key) live in the iOS keychain through the same `SecretStore` the
  desktop uses.
- Background refresh (`dev.mintflow.dolphinpod.refresh`) asks the core to sync so the app opens up to date.
- Updates: *About → Check for updates* reads the `ios` link of the shared release manifest.
