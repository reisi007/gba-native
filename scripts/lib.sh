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
# v0.6.2 of all-the-rest/ratchet-cli; its annotated tag object is
# 1feb7b5fae653272a922ed9f0479660ecde22ed2 and the peeled commit is
# 99c25b1e18331a1473d43091d37e23f23bc1c30d.
RATCHET_VERSION="0.6.2"

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
    fail "ratchet $RATCHET_VERSION is not on PATH and RATCHET_BIN is unset; install the pinned release (all-the-rest/ratchet-cli v0.6.2) or set RATCHET_BIN"
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
