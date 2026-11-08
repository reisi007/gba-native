#!/usr/bin/env bash
# Canonical native Rune frontend gate (headless; no live GPU/window claim).
set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
if [ "$#" -ne 0 ]; then
  printf 'usage: %s\n' "$0" >&2
  exit 2
fi
exec bash "$ROOT/scripts/rune.sh"
