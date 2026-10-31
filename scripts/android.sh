#!/usr/bin/env bash
#
# Android native compilation gate.
#
# Cross-builds the default three-package workspace (`gba-native-core`,
# `gba-native-aot`, root `gba-native`) for the Android NDK ABIs and verifies
# that each artifact is an Android ELF with the expected machine and dynamic
# linker. Android is a compilation target only: there is no Android app, UI,
# audio sink or device-support claim here.
#
# Prerequisites are explicit and never skipped:
#   - an installed Android NDK: ANDROID_NDK_HOME/ANDROID_NDK_ROOT, or the newest
#     `<sdk>/ndk/<version>` under ANDROID_HOME/ANDROID_SDK_ROOT, or the default
#     macOS/Linux SDK locations;
#   - the Rust targets for the selected ABIs
#     (`rustup target add aarch64-linux-android ...`).
#
# Usage: scripts/android.sh [--release] [--api LEVEL] [--abi TRIPLE]...
#
# This script is not part of scripts/short.sh or scripts/full.sh; CI runs it in
# the separate android-native job.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

usage() {
  cat <<'EOF'
usage: scripts/android.sh [--release] [--api LEVEL] [--abi TRIPLE]...

  --release     build the release profile instead of debug
  --api LEVEL   Android API level for the NDK clang wrapper (default 21)
  --abi TRIPLE  build one ABI; repeatable; default is all three supported.

Supported ABIs: aarch64-linux-android armv7-linux-androideabi
                x86_64-linux-android
EOF
}

PROFILE=debug
API="${ANDROID_API_LEVEL:-21}"
TARGETS=()
while [ $# -gt 0 ]; do
  case "$1" in
    --release)
      PROFILE=release
      ;;
    --api)
      [ $# -ge 2 ] || fail "--api needs a numeric API level"
      API=$2
      shift
      ;;
    --abi)
      [ $# -ge 2 ] || fail "--abi needs a Rust Android target triple"
      TARGETS+=("$2")
      shift
      ;;
    --help | -h)
      usage
      exit 0
      ;;
    *)
      fail "unknown argument: $1 (see --help)"
      ;;
  esac
  shift
done
if [ "${#TARGETS[@]}" -eq 0 ]; then
  TARGETS=(aarch64-linux-android armv7-linux-androideabi x86_64-linux-android)
fi
case "$API" in
  '' | *[!0-9]*) fail "--api expects a numeric API level, got '$API'" ;;
esac

case "$(uname -s)" in
  Darwin) NDK_HOST=darwin-x86_64 ;;
  Linux) NDK_HOST=linux-x86_64 ;;
  *) fail "unsupported host for the Android gate: $(uname -s)" ;;
esac

find_ndk_in_sdk() {
  local sdk newest
  for sdk in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" \
    "$HOME/Library/Android/sdk" "$HOME/Android/Sdk"; do
    [ -n "$sdk" ] && [ -d "$sdk/ndk" ] || continue
    newest=$(ls -1 "$sdk/ndk" 2>/dev/null |
      sort -t. -k1,1n -k2,2n -k3,3n | tail -1)
    if [ -n "$newest" ]; then
      printf '%s\n' "$sdk/ndk/$newest"
      return 0
    fi
  done
  return 1
}

if [ -n "${ANDROID_NDK_HOME:-}" ]; then
  [ -d "$ANDROID_NDK_HOME" ] ||
    fail "ANDROID_NDK_HOME=$ANDROID_NDK_HOME is not a directory"
  NDK=$ANDROID_NDK_HOME
elif [ -n "${ANDROID_NDK_ROOT:-}" ]; then
  [ -d "$ANDROID_NDK_ROOT" ] ||
    fail "ANDROID_NDK_ROOT=$ANDROID_NDK_ROOT is not a directory"
  NDK=$ANDROID_NDK_ROOT
else
  NDK=$(find_ndk_in_sdk) ||
    fail "no Android NDK found; set ANDROID_NDK_HOME to an installed NDK or install one with sdkmanager (for example \"sdkmanager 'ndk;29.0.14206865'\")"
fi
NDK_BIN="$NDK/toolchains/llvm/prebuilt/$NDK_HOST/bin"
[ -d "$NDK_BIN" ] ||
  fail "NDK $NDK has no prebuilt toolchain for $NDK_HOST (expected $NDK_BIN)"
info "Android NDK: $NDK"
info "profile: $PROFILE; API level: $API"

clang_prefix() {
  case "$1" in
    aarch64-linux-android) printf 'aarch64-linux-android\n' ;;
    armv7-linux-androideabi) printf 'armv7a-linux-androideabi\n' ;;
    x86_64-linux-android) printf 'x86_64-linux-android\n' ;;
    *) return 1 ;;
  esac
}

expected_machine() {
  case "$1" in
    aarch64-linux-android) printf 'AArch64\n' ;;
    armv7-linux-androideabi) printf 'ARM\n' ;;
    x86_64-linux-android) printf 'X86-64\n' ;;
    *) return 1 ;;
  esac
}

for target in "${TARGETS[@]}"; do
  clang_prefix "$target" >/dev/null ||
    fail "unsupported Android ABI target: $target"
done

command -v rustup >/dev/null 2>&1 ||
  fail "rustup is not on PATH; install Rust with rustup so the Android targets can be checked"
installed=$(rustup target list --installed 2>/dev/null || true)
missing=()
for target in "${TARGETS[@]}"; do
  grep -qx "$target" <<<"$installed" || missing+=("$target")
done
if [ "${#missing[@]}" -ne 0 ]; then
  fail "Rust Android targets are not installed: ${missing[*]}; run: rustup target add ${missing[*]}"
fi

READELF="$NDK_BIN/llvm-readelf"
[ -x "$READELF" ] || fail "NDK llvm-readelf is missing: $READELF"

mkdir -p "$ROOT/output"
for target in "${TARGETS[@]}"; do
  prefix=$(clang_prefix "$target") ||
    fail "unsupported Android ABI target: $target"
  linker="$NDK_BIN/${prefix}${API}-clang"
  [ -x "$linker" ] ||
    fail "the NDK has no clang wrapper for API $API: $linker (choose another --api level)"

  info "building $target"
  build=(build --offline --locked --quiet --target "$target")
  [ "$PROFILE" = release ] && build+=(--release)
  env "CARGO_TARGET_$(printf '%s' "$target" | tr '[:lower:]-' '[:upper:]_')_LINKER=$linker" \
    cargo "${build[@]}"

  binary="target/$target/$PROFILE/gba-native"
  [ -f "$binary" ] || fail "expected binary was not produced: $binary"
  for library in libgba_native_core.rlib libgba_native_aot.rlib; do
    [ -f "target/$target/$PROFILE/$library" ] ||
      fail "expected library was not produced: target/$target/$PROFILE/$library"
  done

  machine=$("$READELF" -h "$binary" |
    awk -F: '/Machine:/ { gsub(/^[ \t]+/, "", $2); print $2 }')
  expected=$(expected_machine "$target")
  case "$machine" in
    *"$expected"*) ;;
    *) fail "$binary reports ELF machine '$machine', expected $expected" ;;
  esac
  interpreter=$("$READELF" -l "$binary" |
    sed -n 's/.*Requesting program interpreter: \([^]]*\)\].*/\1/p' | head -1)
  case "$interpreter" in
    /system/bin/linker*) ;;
    *) fail "$binary has interpreter '$interpreter', expected /system/bin/linker*" ;;
  esac

  printf 'abi=%s\n' "$target"
  printf 'profile=%s\n' "$PROFILE"
  printf 'api=%s\n' "$API"
  printf 'linker=%s\n' "$linker"
  printf 'artifact=%s\n' "$binary"
  printf 'elf_machine=%s\n' "$machine"
  printf 'elf_interpreter=%s\n' "$interpreter"
done

printf 'android build ok: %d ABI(s)\n' "${#TARGETS[@]}"
