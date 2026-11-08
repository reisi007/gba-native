#!/usr/bin/env bash
# Canonical Rune browser gate. Provision pinned tools/Chromium separately with
# scripts/web-browsers-install.sh; --build-only is explicitly a partial gate.
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

BUILD_ONLY=0
MODE=local
while [ "$#" -gt 0 ]; do
  case "$1" in
    --build-only) BUILD_ONLY=1 ;;
    --ci) MODE=ci ;;
    *) fail "scripts/web.sh: unknown argument $1 (expected --build-only or --ci)" ;;
  esac
  shift
done

cd "$ROOT"
require_cargo

info "canonical Rune browser build and adapter tests"
bash "$ROOT/scripts/rune-browser.sh"

if [ "$BUILD_ONLY" = 1 ]; then
  info "Rune browser build-only gate passed (Playwright runtime checks skipped; partial evidence only)"
  exit 0
fi

if [ "$MODE" = ci ]; then
  info "Rune browser interaction and capture checks (headless CI mode)"
  CI=true RUNE_E2E_HEADLESS=1 bash "$ROOT/scripts/rune-web-screenshots.sh"
else
  info "Rune browser interaction and capture checks"
  bash "$ROOT/scripts/rune-web-screenshots.sh"
fi

info "canonical Rune browser gate passed (WASM, adapter tests, Chromium interaction checks)"
