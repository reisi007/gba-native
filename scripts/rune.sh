#!/usr/bin/env bash
# Headless host gate for the isolated shared Rune UI package.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

cd "$ROOT"
require_cargo
[ -f apps/rune/Cargo.toml ] || fail "apps/rune/Cargo.toml is missing"
export CARGO_TARGET_DIR="target/rune"

info "hard limit: 500 physical lines per maintained Rust file"
cargo run --quiet --offline --manifest-path Cargo.toml --bin quality-counter -- \
  oversized --root "$ROOT" --max 500 --format items --hard

info "Rune app cargo fmt --check"
cargo fmt --manifest-path apps/rune/Cargo.toml --all --check

info "Rune app Clippy (all targets, warnings denied)"
cargo clippy --locked --manifest-path apps/rune/Cargo.toml --all-targets -- -D warnings

info "Rune app unit tests"
cargo test --locked --manifest-path apps/rune/Cargo.toml

info "Rune isolated iOS adapter bounded-staging tests"
cargo test --locked --manifest-path apps/rune/ios-platform/Cargo.toml

info "Rune app native host build"
cargo build --locked --manifest-path apps/rune/Cargo.toml

info "Rune static HTML/layout smoke (no GPU required)"
cargo run --quiet --locked --manifest-path apps/rune/Cargo.toml -- \
  --size 1280x900 --html target/rune/ui.html --html-widths 390,844,1280
for label in Demo Settings Saves 'Toggle fullscreen' Details; do
  grep -Fq "aria-label=\"$label\"" target/rune/ui.html ||
    fail "Rune static HTML is missing toolbar label '$label'"
done

info "Rune dependency boundary"
TREE=$(cargo tree --locked --manifest-path apps/rune/Cargo.toml)
grep -Fq 'gba-native-core v' <<<"$TREE" ||
  fail "Rune app is missing the direct gba-native-core runtime"
if grep -Eq '(^|[[:space:]])gba-native-aot v' <<<"$TREE"; then
  fail "Rune app must not depend on host-side gba-native-aot tooling"
fi
if grep -Eq '(^|[[:space:]])gba-native v' <<<"$TREE"; then
  fail "Rune app must not depend on the root gba-native compatibility facade"
fi

info "Rune host gate passed (no live window, GPU or device claim)"
