#!/usr/bin/env bash
#
# Optional frontend gate: the isolated synthetic GPU presentation package.
#
# This gate is deliberately separate from scripts/short.sh and scripts/full.sh:
# the default core workspace stays std-only, offline-testable and independent of
# Winit/Pixels/WGPU. Ratchet is not required here.
#
#   1. hard limit: every maintained Rust file has at most 500 physical lines,
#      including apps/desktop/src (the root counter owns this rule)
#   2. rustfmt and Clippy (warnings denied) for apps/desktop
#   3. the frontend unit tests and a locked build
#   4. a headless --help invocation; this gate never creates a GPU window
#
# Every Cargo invocation uses the root-level ignored target/frontend directory.
# A plain `cargo` run inside apps/desktop would create apps/desktop/target, whose
# generated *.rs files would legitimately enter the maintained-source scan.
# The path is relative to the repository root after `cd`, which also works in
# Git Bash on Windows; an inherited CARGO_TARGET_DIR is always overridden.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo

[ "$#" -eq 0 ] || fail "scripts/frontend.sh takes no arguments (got: $*)"
[ -f apps/desktop/Cargo.toml ] ||
  fail "apps/desktop/Cargo.toml is missing; the optional frontend package is not present"
[ -f Cargo.toml ] ||
  fail "Cargo.toml is missing; run this script from the repository checkout"

export CARGO_TARGET_DIR="target/frontend"

info "hard limit: 500 physical lines per maintained Rust file (including apps/desktop)"
cargo run --quiet --offline --manifest-path Cargo.toml --bin quality-counter -- oversized --root . --max 500 --format items --hard

info "frontend cargo fmt --check"
cargo fmt --manifest-path apps/desktop/Cargo.toml --check

info "frontend cargo clippy --all-targets -- -D warnings"
cargo clippy --locked --manifest-path apps/desktop/Cargo.toml --all-targets -- -D warnings

info "frontend cargo test (unit tests; no GPU window)"
cargo test --locked --manifest-path apps/desktop/Cargo.toml

info "frontend cargo build (locked)"
cargo build --locked --manifest-path apps/desktop/Cargo.toml

info "frontend dependency tree (core runtime only; no AOT/root facade)"
TREE=$(cargo tree --locked --offline --manifest-path apps/desktop/Cargo.toml)
printf '%s\n' "$TREE" | grep 'gba-native-core v' >/dev/null ||
  fail "frontend dependency tree is missing the gba-native-core runtime"
if printf '%s\n' "$TREE" | grep -E '(^|[[:space:]])gba-native-aot v' >/dev/null; then
  fail "frontend must not depend on host-side gba-native-aot tooling"
fi
if printf '%s\n' "$TREE" | grep -E '(^|[[:space:]])gba-native v' >/dev/null; then
  fail "frontend must not depend on the root gba-native compatibility facade"
fi

info "frontend headless CLI help (no window or GPU claim)"
cargo run --quiet --locked --manifest-path apps/desktop/Cargo.toml -- --help

info "optional frontend gate passed (build/tests only; live GPU smoke is manual)"
