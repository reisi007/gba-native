#!/usr/bin/env bash
# Regression: the bootstrap self-test and web-gate first step work before
# output/ or target/ exist in a clean checkout.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
TEST_ROOT=$(mktemp -d "${TMPDIR:-/tmp}/gba-web-clean-checkout.XXXXXX")
trap 'rm -rf "$TEST_ROOT"' EXIT

CHECKOUT="$TEST_ROOT/checkout"
BIN="$TEST_ROOT/bin"
mkdir -p "$CHECKOUT/scripts" "$BIN"
cp "$ROOT/scripts/bootstrap-web-tools.sh" "$ROOT/scripts/lib.sh" \
  "$ROOT/scripts/web.sh" "$CHECKOUT/scripts/"

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

cat >"$BIN/cargo" <<'STUB'
#!/bin/sh
printf 'web gate reached bootstrap provisioning\n' >&2
exit 97
STUB
cat >"$BIN/rustc" <<'STUB'
#!/bin/sh
exit 0
STUB
cat >"$BIN/rustup" <<'STUB'
#!/bin/sh
printf 'wasm32-unknown-unknown\n'
STUB
chmod +x "$BIN/cargo" "$BIN/rustc" "$BIN/rustup"

[ ! -e "$CHECKOUT/output" ] && [ ! -e "$CHECKOUT/target" ] || {
  printf 'fixture is not clean before web-gate entry\n' >&2
  exit 1
}
if PATH="$BIN:/usr/bin:/bin" "$CHECKOUT/scripts/web.sh" --build-only \
  >"$TEST_ROOT/web-gate.log" 2>&1; then
  printf 'web gate unexpectedly passed its stubbed provisioning step\n' >&2
  exit 1
else
  status=$?
fi
[ "$status" -eq 97 ] || {
  cat "$TEST_ROOT/web-gate.log" >&2
  printf 'web gate failed before the expected stub (status %s)\n' "$status" >&2
  exit 1
}
grep -Fq 'bootstrap Node integrity self-tests passed' "$TEST_ROOT/web-gate.log"
grep -Fq 'web gate reached bootstrap provisioning' "$TEST_ROOT/web-gate.log"
SELF_TEST_LINE=$(grep -nF 'bootstrap Node integrity self-tests passed' \
  "$TEST_ROOT/web-gate.log" | cut -d: -f1)
PROVISION_LINE=$(grep -nF 'web gate reached bootstrap provisioning' \
  "$TEST_ROOT/web-gate.log" | cut -d: -f1)
[ "$SELF_TEST_LINE" -lt "$PROVISION_LINE" ] || {
  cat "$TEST_ROOT/web-gate.log" >&2
  printf 'web gate reached provisioning before its bootstrap self-test\n' >&2
  exit 1
}
if grep -Fq 'No such file or directory' "$TEST_ROOT/web-gate.log"; then
  cat "$TEST_ROOT/web-gate.log" >&2
  printf 'web gate encountered a missing-path failure in the clean checkout\n' >&2
  exit 1
fi

cat "$TEST_ROOT/self-test.log"
cat "$TEST_ROOT/web-gate.log"
printf 'clean-checkout bootstrap and web-gate ordering regression passed\n'
