#!/usr/bin/env bash
# The pinned-tool bootstrap integrity self-test must work before target/ exists.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
TEST_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/gba-browser-clean-checkout.XXXXXX")
trap 'rm -rf "$TEST_ROOT"' EXIT

CHECKOUT="$TEST_ROOT/checkout"
mkdir -p "$CHECKOUT/scripts"
cp "$ROOT/scripts/bootstrap-web-tools.sh" "$ROOT/scripts/lib.sh" "$CHECKOUT/scripts/"

[ ! -e "$CHECKOUT/output" ] && [ ! -e "$CHECKOUT/target" ] || {
  printf 'fixture unexpectedly contains output/ or target/ before testing\n' >&2
  exit 1
}

if ! "$CHECKOUT/scripts/bootstrap-web-tools.sh" --self-test \
  >"$TEST_ROOT/self-test.log" 2>&1; then
  cat "$TEST_ROOT/self-test.log" >&2
  exit 1
fi
grep -Fq 'bootstrap Node integrity self-tests passed' "$TEST_ROOT/self-test.log"
[ ! -e "$CHECKOUT/output" ] && [ ! -e "$CHECKOUT/target" ] || {
  printf 'self-test created repository output/ or target/ state\n' >&2
  exit 1
}

cat "$TEST_ROOT/self-test.log"
printf 'clean-checkout browser-tool bootstrap self-test passed\n'
