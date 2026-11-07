#!/usr/bin/env bash
# Build both iOS architectures, then package and launch the simulator app.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo
[ "$(uname -s)" = Darwin ] || fail "the iOS package/launch gate requires macOS and Xcode"
command -v xcrun >/dev/null 2>&1 || fail "xcrun is required for the iOS simulator gate"
command -v codesign >/dev/null 2>&1 || fail "codesign is required for the simulator app bundle"
xcode-select -p >/dev/null 2>&1 || fail "select an Xcode installation before running the iOS gate"

export CARGO_TARGET_DIR="target/rune"
SIMULATOR_SDK=$(xcrun --sdk iphonesimulator --show-sdk-path)
DEVICE_SDK=$(xcrun --sdk iphoneos --show-sdk-path)
mkdir -p "$ROOT/target" "$ROOT/output"
WORK=$(mktemp -d "$ROOT/target/rune-ios-simulator-XXXXXXXX")
OUT=$(mktemp -d "$ROOT/output/rune-ios-review-$(date +%Y%m%d-%H%M%S)-XXXXXXXX")
UDID=""
BOOTED_BY_SCRIPT=0

cleanup() {
  if [ "$BOOTED_BY_SCRIPT" = 1 ] && [ -n "$UDID" ]; then
    xcrun simctl shutdown "$UDID" >/dev/null 2>&1 || true
  fi
  rm -rf "$WORK"
}
trap cleanup EXIT INT TERM

info "link iOS device executable (not signed for a physical device)"
SDKROOT="$DEVICE_SDK" cargo build --locked --manifest-path apps/rune/Cargo.toml \
  --bin gba-native-rune --target aarch64-apple-ios

info "link iOS simulator executable"
SDKROOT="$SIMULATOR_SDK" cargo build --locked --manifest-path apps/rune/Cargo.toml \
  --bin gba-native-rune --target aarch64-apple-ios-sim

APP="$WORK/GbaNativeRune.app"
mkdir -p "$APP"
cp target/rune/aarch64-apple-ios-sim/debug/gba-native-rune "$APP/gba-native-rune"
cp apps/rune/ios/Info.plist "$APP/Info.plist"
plutil -lint "$APP/Info.plist"
codesign --force --deep --sign - "$APP"
codesign --verify --deep --strict "$APP"

UDID=$(xcrun simctl list devices available --json | python3 -c '
import json, sys
devices = json.load(sys.stdin)["devices"]
for runtime, entries in devices.items():
    if "iOS" in runtime:
        for device in entries:
            if device.get("isAvailable", True) and device["name"].startswith("iPhone"):
                print(device["udid"])
                raise SystemExit(0)
raise SystemExit("no available iOS simulator device")
')
STATE=$(xcrun simctl list devices --json | python3 -c '
import json, sys
udid = sys.argv[1]
for entries in json.load(sys.stdin)["devices"].values():
    for device in entries:
        if device["udid"] == udid:
            print(device["state"])
            raise SystemExit(0)
raise SystemExit("simulator device disappeared")
' "$UDID")
if [ "$STATE" != Booted ]; then
  xcrun simctl boot "$UDID"
  BOOTED_BY_SCRIPT=1
fi
xcrun simctl bootstatus "$UDID" -b
xcrun simctl install "$UDID" "$APP"
xcrun simctl launch "$UDID" org.gbanative.rune
sleep 5
xcrun simctl io "$UDID" screenshot "$OUT/launch.png"
[ -s "$OUT/launch.png" ] || fail "simulator launch screenshot is missing or empty"

info "iOS simulator app launched; screenshot: ${OUT#"$ROOT"/}"
info "this smoke gate does not verify physical-device signing, audible output, document picking or screen-reader parity"
