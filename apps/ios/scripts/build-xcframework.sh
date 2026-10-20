#!/usr/bin/env bash
# Builds crates/dolphin-ffi for iOS, generates the Swift bindings and packs everything into
# apps/ios/Frameworks/DolphinFFI.xcframework (device + Apple-silicon simulator).
#
#   apps/ios/scripts/build-xcframework.sh            # fast "mobile" profile (thin LTO)
#   PROFILE=release apps/ios/scripts/build-xcframework.sh   # full LTO for App Store builds
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../../.." && pwd)"
IOS="$ROOT/apps/ios"
PROFILE="${PROFILE:-mobile}"
OUT="$IOS/Frameworks"
GEN="$IOS/Generated"
NAME="dolphin_ffi"
cd "$ROOT"

echo "==> building $NAME for iOS ($PROFILE)"
for target in aarch64-apple-ios aarch64-apple-ios-sim; do
  IPHONEOS_DEPLOYMENT_TARGET=17.0 cargo build -p dolphin-ffi --profile "$PROFILE" --target "$target"
done

echo "==> generating Swift bindings"
cargo build -p dolphin-ffi          # host library: uniffi-bindgen reads the interface from it
HOST_LIB="$ROOT/target/debug/lib${NAME}.dylib"
rm -rf "$GEN" && mkdir -p "$GEN"
cargo run -q -p dolphin-ffi --features bindgen --bin uniffi-bindgen -- \
  generate --library "$HOST_LIB" --language swift --out-dir "$GEN"

echo "==> packing the XCFramework"
HEADERS="$(mktemp -d)"
cp "$GEN/${NAME}FFI.h" "$HEADERS/"
cp "$GEN/${NAME}FFI.modulemap" "$HEADERS/module.modulemap"
rm -rf "$OUT/DolphinFFI.xcframework" && mkdir -p "$OUT"
xcodebuild -create-xcframework \
  -library "$ROOT/target/aarch64-apple-ios/$PROFILE/lib${NAME}.a" -headers "$HEADERS" \
  -library "$ROOT/target/aarch64-apple-ios-sim/$PROFILE/lib${NAME}.a" -headers "$HEADERS" \
  -output "$OUT/DolphinFFI.xcframework"
# The Swift file goes into the app target; the C header and modulemap live in the framework.
rm -f "$GEN/${NAME}FFI.h" "$GEN/${NAME}FFI.modulemap"
echo "done: $OUT/DolphinFFI.xcframework and $GEN/${NAME}.swift"
