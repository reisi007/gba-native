#!/usr/bin/env bash
#
# Shared helpers for the short/full gates. This file is sourced, never run.
#
# The repository root is resolved from this file, so the scripts work from any
# working directory. Every prerequisite failure is a hard failure with a
# diagnostic; nothing is skipped silently.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "${BASH_SOURCE[0]}")/.." && pwd -P)

# The Ratchet release this repository is gated with. The source is release
# v0.6.3 of all-the-rest/ratchet-cli; the tag is lightweight and points at
# the commit 4cace52ac7731985130292ead6ae81d44b4d72d6.
RATCHET_VERSION="0.6.3"

fail() {
  printf 'error: %s\n' "$*" >&2
  exit 1
}

info() {
  printf '==> %s\n' "$*" >&2
}

require_cargo() {
  command -v cargo >/dev/null 2>&1 ||
    fail "cargo is not on PATH; install Rust 1.98.0 (rustup) and retry"
  command -v rustc >/dev/null 2>&1 ||
    fail "rustc is not on PATH; install Rust 1.98.0 (rustup) and retry"
}

# Resolve the Ratchet binary and validate its version. RATCHET_BIN wins; the
# installed binary is the fallback. A missing or mismatched tool is an error,
# never a skipped gate.
resolve_ratchet() {
  if [ -n "${RATCHET_BIN:-}" ]; then
    [ -x "$RATCHET_BIN" ] ||
      fail "RATCHET_BIN=$RATCHET_BIN is not an executable file (expected ratchet $RATCHET_VERSION)"
    candidate=$RATCHET_BIN
  elif command -v ratchet >/dev/null 2>&1; then
    candidate=$(command -v ratchet)
  else
    fail "ratchet $RATCHET_VERSION is not on PATH and RATCHET_BIN is unset; install the pinned release (all-the-rest/ratchet-cli v0.6.3) or set RATCHET_BIN"
  fi

  actual=$("$candidate" --version 2>/dev/null) ||
    fail "cannot read the version of $candidate; expected ratchet $RATCHET_VERSION"
  [ "$actual" = "ratchet $RATCHET_VERSION" ] ||
    fail "expected ratchet $RATCHET_VERSION, got '$actual' from $candidate"
  printf '%s\n' "$candidate"
}

# A unique directory the script owns inside the ignored output/ directory. It
# is removed on exit; existing files are never touched.
owned_temp() {
  mkdir -p "$ROOT/output"
  mktemp -d "$ROOT/output/quality-$1.XXXXXX"
}

run_ratchet() {
  RATCHET_NO_UPDATE_CHECK=1 "$RATCHET" "$@"
}

# Resolve an installed Android SDK root. Returns 1 when none is found; callers
# turn that into a gate-specific diagnostic.
android_sdk_root() {
  local sdk
  for sdk in "${ANDROID_HOME:-}" "${ANDROID_SDK_ROOT:-}" \
    "$HOME/Library/Android/sdk" "$HOME/Android/Sdk" /usr/local/lib/android/sdk; do
    [ -n "$sdk" ] && [ -d "$sdk" ] && {
      printf '%s\n' "$sdk"
      return 0
    }
  done
  return 1
}

# Resolve an installed Android NDK: an explicit NDK root, or the newest
# <sdk>/ndk/<version>. Returns 1 when none is found.
android_ndk_root() {
  local sdk newest
  for sdk in "${ANDROID_NDK_HOME:-}" "${ANDROID_NDK_ROOT:-}"; do
    [ -n "$sdk" ] && [ -d "$sdk" ] && {
      printf '%s\n' "$sdk"
      return 0
    }
  done
  sdk=$(android_sdk_root) || return 1
  [ -d "$sdk/ndk" ] || return 1
  newest=$(ls -1 "$sdk/ndk" 2>/dev/null |
    sort -t. -k1,1n -k2,2n -k3,3n | tail -1)
  [ -n "$newest" ] || return 1
  printf '%s\n' "$sdk/ndk/$newest"
}
