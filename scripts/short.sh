#!/usr/bin/env bash
#
# Fast local gate: seconds, no network after the prerequisites are installed.
#
#   1. hard limit: every maintained Rust file has at most 500 physical lines
#   2. Ratchet fast tier (the same counter above plus the source-population floor)
#   3. rustfmt, Clippy with warnings denied, and the fast unit tests
#
# The hard limit is checked directly, not only through the Ratchet baseline, so
# editing or raising the baseline cannot dilute it.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo
RATCHET=$(resolve_ratchet)

info "hard limit: 500 physical lines per maintained Rust file"
cargo run --quiet --offline --bin quality-counter -- oversized --root "$ROOT" --max 500 --format items --hard

info "Ratchet fast tier"
run_ratchet check --tier fast "$@"

info "cargo fmt --check"
cargo fmt --check

info "cargo clippy --all-targets -- -D warnings"
cargo clippy --all-targets --offline -- -D warnings

info "cargo test (unit and binary tests, debug)"
cargo test --offline --quiet --lib --bins

info "short gate passed"
