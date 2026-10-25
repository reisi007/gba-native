#!/usr/bin/env bash
#
# Provision the pinned browser-build tools for apps/web.
#
# Everything is installed into the repository-ignored target/ tree; nothing is
# installed globally and no user configuration is changed:
#
#   target/tools/bin/                wasm-bindgen CLI 0.2.129 (cargo install --root)
#   target/tools/node-pinned/        official Node 24.22.1, provenance-verified
#   target/web-tools/npm/            npm-installed Tailwind 4, DaisyUI 5, Playwright
#   target/web-tools/cargo-install/  cargo install's own target directory
#
# The binary pins are exact. A missing tool is a hard failure with the command
# needed to fix it, never a silent skip.

set -euo pipefail

ROOT=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd -P)
# shellcheck source=scripts/lib.sh
. "$ROOT/scripts/lib.sh"

WASM_BINDGEN_VERSION="0.2.129"
NODE_VERSION="24.22.1"
TOOLS_DIR="$ROOT/target/tools"
NPM_DIR="$ROOT/target/web-tools/npm"
DOWNLOADS_DIR="$ROOT/target/web-tools/downloads"
NODE_LINK="$TOOLS_DIR/node-pinned"

node_archive_pin() {
  case "$1/$2" in
    darwin/arm64) printf '%s\n' '89c682593d76cd44233fa53b1563264e7de9cbb189cad169135c5b4fadfbc0a7' ;;
    darwin/x64) printf '%s\n' 'd132557e7ee2b85af08b0c8c34338e65c6430f2238d01f727728ef42dc83b035' ;;
    linux/arm64) printf '%s\n' '33c37be234ba8bee041961047b19160f4d7d0f164e8e246f78bdd10feff148b3' ;;
    linux/x64) printf '%s\n' 'eb6728e77c576a35162c122c45ccadf224d89ff00abc8dae46cc2897c6002d7c' ;;
    *) return 1 ;;
  esac
}

sha256_file() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | awk '{ print $1 }'
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 "$1" | awk '{ print $1 }'
  else
    return 1
  fi
}

verify_node_archive() {
  [ -f "$1" ] || return 1
  [ "$(sha256_file "$1")" = "$2" ]
}

node_install_valid() {
  local directory="$1" platform="$2" archive_sum="$3" manifest binary_sum
  manifest="$directory/.gba-node-provenance"
  [ -x "$directory/bin/node" ] || return 1
  [ -f "$directory/.gba-node-managed" ] || return 1
  [ -f "$manifest" ] || return 1
  grep -Fqx "version=$NODE_VERSION" "$manifest" || return 1
  grep -Fqx "platform=$platform" "$manifest" || return 1
  grep -Fqx "archive_sha256=$archive_sum" "$manifest" || return 1
  [ "$(grep -c '^binary_sha256=' "$manifest" || true)" = 1 ] || return 1
  binary_sum=$(sed -n 's/^binary_sha256=//p' "$manifest")
  [ "$(sha256_file "$directory/bin/node")" = "$binary_sum" ] || return 1
  [ "$("$directory/bin/node" --version 2>/dev/null || true)" = "v$NODE_VERSION" ]
}

bootstrap_self_test() (
  local test_root test_install archive_sum binary_sum platform sample
  platform="darwin-arm64"
  archive_sum=$(node_archive_pin darwin arm64) || fail "missing test Node archive pin"
  test_root=$(mktemp -d "${TMPDIR:-/tmp}/gba-web-bootstrap-test.XXXXXX")
  trap 'rm -rf "$test_root"' EXIT
  test_install="$test_root/node"
  mkdir -p "$test_install/bin"
  : >"$test_install/.gba-node-managed"
  printf '#!/bin/sh\nprintf "v%s\\n"\n' "$NODE_VERSION" >"$test_install/bin/node"
  chmod +x "$test_install/bin/node"
  binary_sum=$(sha256_file "$test_install/bin/node")
  {
    printf 'version=%s\n' "$NODE_VERSION"
    printf 'platform=%s\n' "$platform"
    printf 'archive_sha256=%s\n' "$archive_sum"
    printf 'binary_sha256=%s\n' "$binary_sum"
  } >"$test_install/.gba-node-provenance"
  node_install_valid "$test_install" "$platform" "$archive_sum" ||
    fail "verified Node install fixture was rejected"
  if node_install_valid "$test_install" "$platform" '0000000000000000000000000000000000000000000000000000000000000000'; then
    fail "Node install accepted an unexpected archive digest"
  fi
  mv "$test_install/.gba-node-provenance" "$test_install/provenance.saved"
  if node_install_valid "$test_install" "$platform" "$archive_sum"; then
    fail "partial Node install without its manifest was trusted"
  fi
  mv "$test_install/provenance.saved" "$test_install/.gba-node-provenance"
  printf '# changed\n' >>"$test_install/bin/node"
  if node_install_valid "$test_install" "$platform" "$archive_sum"; then
    fail "Node install accepted a binary whose digest changed"
  fi
  if node_archive_pin plan9 sparc >/dev/null 2>&1; then
    fail "unsupported Node platform was accepted"
  fi
  sample="$test_root/archive"
  printf 'archive fixture\n' >"$sample"
  if verify_node_archive "$sample" '0000000000000000000000000000000000000000000000000000000000000000'; then
    fail "Node archive checksum mismatch was accepted"
  fi
  info "bootstrap Node integrity self-tests passed"
)

if [ "${1:-}" = "--self-test" ]; then
  bootstrap_self_test
  exit 0
fi
[ "$#" = 0 ] || fail "usage: scripts/bootstrap-web-tools.sh [--self-test]"

cd "$ROOT"
require_cargo

info "wasm-bindgen CLI $WASM_BINDGEN_VERSION into target/tools"
if [ -x "$TOOLS_DIR/bin/wasm-bindgen" ]; then
  ACTUAL=$("$TOOLS_DIR/bin/wasm-bindgen" --version 2>/dev/null || true)
  if [ "$ACTUAL" = "wasm-bindgen $WASM_BINDGEN_VERSION" ]; then
    info "wasm-bindgen already pinned: $ACTUAL"
  else
    info "replacing target-local wasm-bindgen ($ACTUAL)"
    mkdir -p "$ROOT/target/web-tools/cargo-install"
    CARGO_TARGET_DIR="$ROOT/target/web-tools/cargo-install" \
      cargo install wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked \
        --root "$TOOLS_DIR"
  fi
else
  mkdir -p "$ROOT/target/web-tools/cargo-install"
  CARGO_TARGET_DIR="$ROOT/target/web-tools/cargo-install" \
    cargo install wasm-bindgen-cli --version "$WASM_BINDGEN_VERSION" --locked \
      --root "$TOOLS_DIR"
fi
ACTUAL=$("$TOOLS_DIR/bin/wasm-bindgen" --version 2>/dev/null || true)
[ "$ACTUAL" = "wasm-bindgen $WASM_BINDGEN_VERSION" ] ||
  fail "expected wasm-bindgen $WASM_BINDGEN_VERSION in target/tools, got '$ACTUAL'"

info "Node $NODE_VERSION for Tailwind and the Playwright runner (pinned archive + binary provenance)"
case "$(uname -s)" in
  Darwin) NODE_OS="darwin" ;;
  Linux) NODE_OS="linux" ;;
  *) fail "unsupported host OS for the pinned Node runtime: $(uname -s)" ;;
esac
case "$(uname -m)" in
  arm64 | aarch64) NODE_ARCH="arm64" ;;
  x86_64 | amd64) NODE_ARCH="x64" ;;
  *) fail "unsupported host architecture for the pinned Node runtime: $(uname -m)" ;;
esac
NODE_DIR="$TOOLS_DIR/node-v$NODE_VERSION-$NODE_OS-$NODE_ARCH"
NODE_ARCHIVE="node-v$NODE_VERSION-$NODE_OS-$NODE_ARCH.tar.gz"
NODE_PLATFORM="$NODE_OS-$NODE_ARCH"
NODE_ARCHIVE_SHA=$(node_archive_pin "$NODE_OS" "$NODE_ARCH") ||
  fail "unsupported Node platform $NODE_OS/$NODE_ARCH; add an official archive digest pin before enabling it"
NODE_DIR=""
NODE_BIN="$NODE_LINK/bin/node"
mkdir -p "$DOWNLOADS_DIR"
if [ -L "$NODE_LINK" ]; then
  NODE_LINK_TARGET=$(readlink "$NODE_LINK") || fail "cannot inspect repository-owned Node link $NODE_LINK"
  case "$NODE_LINK_TARGET" in
    /*)
      case "$NODE_LINK_TARGET" in
        */../* | */..) fail "$NODE_LINK has a parent traversal; refusing to follow it" ;;
      esac
      case "$NODE_LINK_TARGET/" in
        "$TOOLS_DIR"/*) NODE_DIR="$NODE_LINK_TARGET" ;;
        *) fail "$NODE_LINK points outside target/tools; refusing to replace it" ;;
      esac
      ;;
    ../* | */../* | */..) fail "$NODE_LINK has an unsafe target; refusing to follow it" ;;
    *) NODE_DIR="$TOOLS_DIR/$NODE_LINK_TARGET" ;;
  esac
  if [ -d "$NODE_DIR" ]; then
    NODE_DIR=$(CDPATH= cd -- "$NODE_DIR" && pwd -P)
    case "$NODE_DIR/" in
      "$TOOLS_DIR"/*) ;;
      *) fail "$NODE_LINK resolves outside target/tools; refusing to trust it" ;;
    esac
  fi
elif [ -e "$NODE_LINK" ]; then
  fail "$NODE_LINK exists but is not a symlink; refusing to overwrite it"
fi

if [ -n "$NODE_DIR" ] && node_install_valid "$NODE_DIR" "$NODE_PLATFORM" "$NODE_ARCHIVE_SHA"; then
  info "verified installed Node binary and archive provenance"
else
  info "Node install is absent or lacks matching archive/binary provenance; installing from the literal SHA-256 pin"
  if [ ! -f "$DOWNLOADS_DIR/$NODE_ARCHIVE" ]; then
    DOWNLOAD_TMP=$(mktemp "$DOWNLOADS_DIR/node-download.XXXXXX")
    curl --fail --silent --show-error --location \
      "https://nodejs.org/dist/v$NODE_VERSION/$NODE_ARCHIVE" \
      --output "$DOWNLOAD_TMP" || fail "could not download the pinned Node archive"
    verify_node_archive "$DOWNLOAD_TMP" "$NODE_ARCHIVE_SHA" ||
      fail "Node archive checksum mismatch for downloaded $NODE_ARCHIVE"
    mv "$DOWNLOAD_TMP" "$DOWNLOADS_DIR/$NODE_ARCHIVE"
  fi
  verify_node_archive "$DOWNLOADS_DIR/$NODE_ARCHIVE" "$NODE_ARCHIVE_SHA" ||
    fail "Node archive cache checksum mismatch for $NODE_ARCHIVE; refusing to trust or overwrite the cached file"

  NODE_STAGE=$(mktemp -d "$TOOLS_DIR/.node-stage.XXXXXX")
  : >"$NODE_STAGE/.gba-node-stage-owned"
  tar -xzf "$DOWNLOADS_DIR/$NODE_ARCHIVE" -C "$NODE_STAGE" || {
    rm -rf "$NODE_STAGE"
    fail "could not extract the verified Node archive"
  }
  NODE_ROOT="$NODE_STAGE/node-v$NODE_VERSION-$NODE_OS-$NODE_ARCH"
  [ -x "$NODE_ROOT/bin/node" ] || {
    rm -rf "$NODE_STAGE"
    fail "verified Node archive did not contain the expected executable"
  }
  [ "$("$NODE_ROOT/bin/node" --version 2>/dev/null || true)" = "v$NODE_VERSION" ] || {
    rm -rf "$NODE_STAGE"
    fail "verified Node archive contained an unexpected Node version"
  }
  NODE_BINARY_SHA=$(sha256_file "$NODE_ROOT/bin/node")
  : >"$NODE_ROOT/.gba-node-managed"
  {
    printf 'version=%s\n' "$NODE_VERSION"
    printf 'platform=%s\n' "$NODE_PLATFORM"
    printf 'archive_sha256=%s\n' "$NODE_ARCHIVE_SHA"
    printf 'binary_sha256=%s\n' "$NODE_BINARY_SHA"
  } >"$NODE_ROOT/.gba-node-provenance"
  NODE_DIR="$NODE_ROOT"
  NODE_LINK_RELATIVE=${NODE_DIR#"$TOOLS_DIR"/}
  ln -sfn "$NODE_LINK_RELATIVE" "$NODE_LINK"
fi
NODE_BIN="$NODE_LINK/bin/node"
[ "$("$NODE_BIN" --version)" = "v$NODE_VERSION" ] ||
  fail "expected Node v$NODE_VERSION at $NODE_BIN"

[ -f apps/web/package.json ] || fail "apps/web/package.json is missing"
[ -f apps/web/package-lock.json ] || fail "apps/web/package-lock.json is missing"

info "npm tooling from apps/web/package.json + package-lock.json into target/web-tools/npm"
mkdir -p "$NPM_DIR"
cp apps/web/package.json apps/web/package-lock.json "$NPM_DIR"/
(
  cd "$NPM_DIR"
  PATH="$NODE_LINK/bin:$PATH" npm ci --ignore-scripts --no-audit --no-fund
)

info "installed tool versions"
printf 'node %s\n' "$("$NODE_BIN" --version)"
printf '%s\n' "$ACTUAL"
VERSIONS=$(cd "$NPM_DIR" && "$NODE_BIN" -e '
const fs = require("fs");
for (const name of ["daisyui", "@tailwindcss/cli", "@playwright/test"]) {
  const version = JSON.parse(fs.readFileSync(`node_modules/${name}/package.json`, "utf8")).version;
  console.log(`${name} ${version}`);
}
')
printf '%s\n' "$VERSIONS"
printf '%s\n' "$VERSIONS" | grep -qx 'daisyui 5.7.50' || fail "unexpected daisyUI version"
printf '%s\n' "$VERSIONS" | grep -qx '@tailwindcss/cli 4.3.3' || fail "unexpected Tailwind CLI version"
printf '%s\n' "$VERSIONS" | grep -qx '@playwright/test 1.64.1' || fail "unexpected Playwright version"

info "browser tooling ready (pinned, repository-owned caches)"
