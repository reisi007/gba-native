#!/usr/bin/env bash
#
# Full local/merge gate: the short gate plus every repository gate.
#
#   1. scripts/short.sh (hard line limit, Ratchet fast, fmt, Clippy, fast tests)
#   2. all debug and release tests, including the native AOT integration test
#   3. the demo CLI
#   4. a real native AOT proof against the synthetic fixture
#   5. the Ratchet slow tier: the native/reference consistency proof
#
# Generated output stays in a unique directory this script owns and removes.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo
RATCHET=$(resolve_ratchet)

info "short gate"
"$ROOT/scripts/short.sh" "$@"

info "cargo test (debug, includes the native AOT integration test)"
cargo test --offline

info "cargo test (release, includes the native AOT integration test)"
cargo test --release --offline

mkdir -p "$ROOT/output"
WORK=$(mktemp -d "$ROOT/output/quality-full.XXXXXX")
cleanup() {
  rm -rf "$WORK"
}
trap cleanup EXIT

info "demo CLI"
cargo run --quiet --offline --bin gba-native -- demo --size 480x320 --backend nearest --out "$WORK/demo.ppm"

info "native AOT proof"
cargo run --quiet --offline --bin gba-native -- aot proof --out "$WORK/aot-proof"

info "Ratchet slow tier"
run_ratchet check --tier slow "$@"

info "full gate passed"
