# Rune app agent rules

## Embedded bound image (D54)

`GBA_NATIVE_EMBED_ROM=1` is a local-only single-file build switch. When set
alongside `GBA_NATIVE_ROM_PATH`, `scripts/rune-bound.sh` pre-emits the
translated program with the host CLI and the bound image is compiled into the
executable, so the app starts without a sibling `.gba`. The app crate itself
must not depend on `gba-native-aot`; the Rune dependency-boundary gate checks
that.

- Off by default; ordinary builds keep the external runtime image.
- The build script refuses `CI` and `wasm32` targets.
- The output is retail-derived and machine-local: it stays under the
  git-ignored `target/`. Never commit, publish, attach it to a CI job or share
  it, and never add ROM bytes to source, tests, documentation or fixtures.
- SHA-256, length, title and game-code binding still run before a session.
- Embedding changes no ISA, PPU, timing, save or compatibility claim.
