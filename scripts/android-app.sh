#!/usr/bin/env bash
#
# Android APK build gate.
#
# Builds the `apps/android` NativeActivity library for the three ABIs, packages
# a signed debug APK with the SDK build-tools and verifies the package identity,
# the launchable activity, the native-code list and the embedded libraries.
#
# The APK contains no Java code, no ROM, no BIOS and no save data: the activity
# is the framework `android.app.NativeActivity`, which loads the Rust library
# built here. Unit tests for the host-testable model run separately
# (`cargo test --manifest-path apps/android/Cargo.toml`), both in ci.yml and
# locally.
#
# Prerequisites are explicit and never skipped:
#   - an installed Android SDK with platform + build-tools (aapt2, zipalign,
#     apksigner) and an NDK;
#   - a JDK (`keytool`, and the `java` runtime apksigner needs);
#   - the Rust Android targets for the three ABIs.
#
# Usage: scripts/android-app.sh [--debug]

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

PROFILE=release
BUILD_ARGS=(--release)
case "${1:-}" in
  --debug)
    PROFILE=debug
    BUILD_ARGS=()
    ;;
  --help | -h)
    printf 'usage: scripts/android-app.sh [--debug]\n'
    exit 0
    ;;
  '') ;;
  *) fail "unknown argument: $1 (see --help)" ;;
esac

SDK=$(android_sdk_root) ||
  fail "no Android SDK found; set ANDROID_HOME to an installed SDK (platform and build-tools are required)"
NDK=$(android_ndk_root) ||
  fail "no Android NDK found; set ANDROID_NDK_HOME to an installed NDK or install one with sdkmanager"

case "$(uname -s)" in
  Darwin) NDK_HOST=darwin-x86_64 ;;
  Linux) NDK_HOST=linux-x86_64 ;;
  *) fail "unsupported host for the Android app gate: $(uname -s)" ;;
esac
NDK_BIN="$NDK/toolchains/llvm/prebuilt/$NDK_HOST/bin"
[ -d "$NDK_BIN" ] || fail "NDK $NDK has no prebuilt toolchain for $NDK_HOST"

BT_VERSION=$(ls -1 "$SDK/build-tools" 2>/dev/null |
  sort -t. -k1,1n -k2,2n -k3,3n | tail -1)
[ -n "$BT_VERSION" ] ||
  fail "no Android build-tools under $SDK/build-tools; install them with sdkmanager (for example \"sdkmanager 'build-tools;36.1.0'\")"
BT="$SDK/build-tools/$BT_VERSION"
for tool in aapt2 zipalign apksigner; do
  [ -x "$BT/$tool" ] || fail "the Android build-tools at $BT are missing $tool"
done

PLATFORM_VERSION=$(ls -1 "$SDK/platforms" 2>/dev/null |
  sed -n 's/^android-\([0-9][0-9]*\)$/\1/p' |
  sort -n | tail -1)
[ -n "$PLATFORM_VERSION" ] ||
  fail "no Android platform under $SDK/platforms; install one with sdkmanager (for example \"sdkmanager 'platforms;android-36'\")"
ANDROID_JAR="$SDK/platforms/android-$PLATFORM_VERSION/android.jar"
[ -f "$ANDROID_JAR" ] || fail "missing $ANDROID_JAR"

KEYTOOL=""
for candidate in "${JAVA_HOME:-}/bin/keytool" "$(command -v keytool 2>/dev/null || true)"; do
  if [ -n "$candidate" ] && [ -x "$candidate" ]; then
    KEYTOOL=$candidate
    break
  fi
done
[ -n "$KEYTOOL" ] ||
  fail "keytool is not available; install a JDK (for example Temurin 17) or set JAVA_HOME"
command -v java >/dev/null 2>&1 ||
  fail "the java runtime is not on PATH; install a JDK so apksigner can run"

command -v zip >/dev/null 2>&1 || fail "zip is not on PATH; it is needed to add the native libraries to the APK"
command -v unzip >/dev/null 2>&1 || fail "unzip is not on PATH; it is needed to verify the APK contents"

info "Android SDK: $SDK"
info "Android NDK: $NDK"
info "build-tools: $BT_VERSION; platform: $PLATFORM_VERSION; profile: $PROFILE"

mkdir -p "$ROOT/output/android-app"
WORK=$(mktemp -d "$ROOT/output/android-app.XXXXXX")
export CARGO_TARGET_DIR="$ROOT/target/android"
mkdir -p "$WORK/lib"

ABIS=(arm64-v8a armeabi-v7a x86_64)
for abi in "${ABIS[@]}"; do
  case "$abi" in
    arm64-v8a) target=aarch64-linux-android ;;
    armeabi-v7a) target=armv7-linux-androideabi ;;
    x86_64) target=x86_64-linux-android ;;
  esac
  prefix=$target
  [ "$target" = armv7-linux-androideabi ] && prefix=armv7a-linux-androideabi
  linker="$NDK_BIN/${prefix}21-clang"
  [ -x "$linker" ] || fail "the NDK has no clang wrapper: $linker"
  info "building $abi ($target)"
  env "CARGO_TARGET_$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')_LINKER=$linker" \
    cargo build --offline --locked --quiet --manifest-path apps/android/Cargo.toml \
    --target "$target" "${BUILD_ARGS[@]}"
  library="target/android/$target/$PROFILE/libgba_native_android.so"
  [ -f "$library" ] || fail "expected Android library was not produced: $library"
  mkdir -p "$WORK/lib/$abi"
  cp "$library" "$WORK/lib/$abi/libgba_native_android.so"
done

info "linking the base APK"
"$BT/aapt2" link \
  --manifest apps/android/AndroidManifest.xml \
  -I "$ANDROID_JAR" \
  --min-sdk-version 21 \
  --target-sdk-version "$PLATFORM_VERSION" \
  --version-code 1 \
  --version-name 0.1.0 \
  -o "$WORK/base.apk"
cp "$WORK/base.apk" "$WORK/app.apk"
(
  cd "$WORK"
  zip -q -r app.apk lib
)

"$BT/zipalign" -f -p 4 "$WORK/app.apk" "$WORK/aligned.apk"
KEYSTORE="$WORK/debug.keystore"
"$KEYTOOL" -genkeypair \
  -keystore "$KEYSTORE" \
  -storepass android \
  -keypass android \
  -alias androiddebugkey \
  -dname "CN=Android Debug,O=Android,C=US" \
  -keyalg RSA \
  -keysize 2048 \
  -validity 10000 >/dev/null 2>&1 ||
  fail "keytool could not create the debug keystore"

APK="$ROOT/output/android-app/gba-native-android.apk"
"$BT/apksigner" sign \
  --ks "$KEYSTORE" \
  --ks-pass pass:android \
  --key-pass pass:android \
  --out "$APK" \
  "$WORK/aligned.apk"
[ -f "$APK" ] || fail "apksigner did not produce $APK"

info "verifying the APK"
"$BT/apksigner" verify --print-certs "$APK" >"$WORK/verify.txt" ||
  fail "apksigner could not verify $APK"
"$BT/aapt2" dump badging "$APK" >"$WORK/badging.txt" ||
  fail "aapt2 could not read the package identity of $APK"
grep -q "package: name='dev.gbanative.synthetic'" "$WORK/badging.txt" ||
  fail "the APK package name is not dev.gbanative.synthetic"
grep -q "launchable-activity: name='android.app.NativeActivity'" "$WORK/badging.txt" ||
  fail "the APK has no launchable android.app.NativeActivity"
grep -q "native-code: 'arm64-v8a' 'armeabi-v7a' 'x86_64'" "$WORK/badging.txt" ||
  fail "the APK native-code list is not the three expected ABIs"
unzip -l "$APK" >"$WORK/contents.txt"
for abi in "${ABIS[@]}"; do
  grep -q "lib/$abi/libgba_native_android.so" "$WORK/contents.txt" ||
    fail "the APK is missing lib/$abi/libgba_native_android.so"
done
if grep -qE "\.(gba|sav|bin)$|assets/" "$WORK/contents.txt"; then
  fail "the APK must not contain a ROM, BIOS or save payload"
fi

printf 'apk=%s\n' "$APK"
printf 'package=dev.gbanative.synthetic\n'
printf 'activity=android.app.NativeActivity\n'
printf 'profile=%s\n' "$PROFILE"
printf 'native-code=%s\n' "${ABIS[*]}"
printf 'signature=verified\n'
grep -m1 "Signer #1 certificate SHA-256 digest:" "$WORK/verify.txt" |
  sed 's/^ *//' || true
printf 'android app build ok\n'
