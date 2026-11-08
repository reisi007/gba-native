#!/usr/bin/env bash
# Gate helper: stage pinned Ratchet release assets through GitHub's release API. The
# pinned upstream action consumes this file:// base and validates the checksum.
set -euo pipefail

: "${RUNNER_TEMP:?RUNNER_TEMP is required}"
: "${GITHUB_ENV:?GITHUB_ENV is required}"

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
cd "$ROOT"

ASSETS="$RUNNER_TEMP/ratchet-release/v0.6.3"
mkdir -p "$ASSETS"
RELEASE_JSON="$RUNNER_TEMP/ratchet-v0.6.3-release.json"

curl --fail --silent --show-error --location \
  --retry 5 --retry-delay 2 --retry-max-time 60 \
  -H 'Accept: application/vnd.github+json' \
  -H 'X-GitHub-Api-Version: 2022-11-28' \
  'https://api.github.com/repos/all-the-rest/ratchet-cli/releases/tags/v0.6.3' \
  --output "$RELEASE_JSON"

python3 - "$RELEASE_JSON" "$ASSETS" <<'PY'
import json
import subprocess
import sys
from pathlib import Path

release_path, assets_path = map(Path, sys.argv[1:])
release = json.loads(release_path.read_text(encoding="utf-8"))
if release.get("tag_name") != "v0.6.3":
    raise SystemExit(f"expected Ratchet release v0.6.3, got {release.get('tag_name')!r}")

required = {"checksums.txt", "ratchet-0.6.3-linux-x86_64"}
assets = {asset["name"]: asset for asset in release["assets"]}
missing = required - assets.keys()
if missing:
    raise SystemExit(f"Ratchet v0.6.3 release is missing assets: {sorted(missing)}")

api_prefix = "https://api.github.com/repos/all-the-rest/ratchet-cli/releases/assets/"
for name in sorted(required):
    asset = assets[name]
    url = asset["url"]
    if not url.startswith(api_prefix):
        raise SystemExit(f"unexpected GitHub release asset URL for {name}: {url}")
    subprocess.run([
        "curl", "--fail", "--silent", "--show-error", "--location",
        "--retry", "5", "--retry-delay", "2", "--retry-max-time", "60",
        "-H", "Accept: application/octet-stream",
        "-H", "X-GitHub-Api-Version: 2022-11-28",
        "--output", str(assets_path / name), url,
    ], check=True)
    if (assets_path / name).stat().st_size == 0:
        raise SystemExit(f"Ratchet release asset {name} is empty")
PY

python3 - "$ASSETS" >>"$GITHUB_ENV" <<'PY'
from pathlib import Path
import sys

print(f"RATCHET_ASSET_BASE={Path(sys.argv[1]).parent.as_uri()}")
PY
