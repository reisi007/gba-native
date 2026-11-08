# AOT artifact integrity and reproducibility

This note describes the trust and reproducibility properties of the current
`gba-native aot build` outputs. It is not a software-supply-chain attestation,
security certification, or compatibility claim.

## Trust boundary

An AOT artifact is executable content. The native branch builds generated Rust
source into a normal host executable; this project does not sandbox it. The WASM
branch runs in the host's WebAssembly runtime and this module exposes no
filesystem, process, or stdout API; it relies on the isolation and policies of
that runtime. Neither branch's hash checks authenticate code or toolchain
inputs. Treat `generated.rs`, the generated manifest, compiler inputs, and
produced binaries as code, and use them only when the repository checkout and
build environment are trusted.

The build's trust roots include the selected repository source tree (the CLI,
AOT tooling, and core runtime), the generated source and manifest, Cargo and
rustc, the target linker/toolchain, the host OS, and the environment that
selects those tools. The metadata does not identify or authenticate all of
these inputs. `build_offline=true` records the Cargo mode; it does not prove
that cached dependencies or tools are trustworthy.

The generated native harness uses an offline Cargo build and a path dependency
on the sibling `gba-native-core` package, not on the AOT crate or root facade.
The root lockfile contains only the workspace's internal path packages. The
WASM branch additionally uses the pinned `wasm-bindgen` 0.2.129 package and the
repository-owned CLI at that version. Version pins and offline operation narrow
the inputs but are not signatures, source verification, or proof that a tool is
benign.

The caller's ROM remains an external, read-only input and is not copied into
generated source or the output workspace. Generated code binds the expected ROM
by SHA-256 and length and checks those values before guest execution. This
rejects an accidental or deliberate *different ROM*; it does not authenticate
the generated program, prove its behavior, or make the executable safe to run.

## What the metadata hashes mean

`artifact.meta` records the ROM digest and length, target and entry settings,
budget and discovery facts, and a SHA-256 for generated source. The native
branch calls that field `source_sha256` and also records
`executable_sha256`; the WASM branch calls it `program_source_sha256` and
records `module_sha256` and `js_glue_sha256`. `build.meta` records the schema,
target, release profile, offline flag, Cargo and rustc version strings, and
rustc host. WASM build metadata also records the wasm-bindgen version; the
artifact metadata repeats that version.
The metadata is ordered, contains no absolute paths, timestamps, or ROM bytes,
and is written alongside the files it describes.

These hashes are content identifiers and can detect accidental changes when
compared with a trusted expected value. They are not signatures: the metadata is
not signed, and an actor able to replace both an artifact and its metadata can
replace the hashes too. The metadata is not an authenticated provenance chain.
It does not record a verified source commit/tree, compiler or linker binary
hashes, all build flags and environment, or hashes binding a complete dependency
inventory. A version string alone does not prove which executable provided that
version.

For an independent integrity check, compute hashes from the produced files and
compare them with the corresponding metadata fields. For a native build compare
`generated.rs` and the executable with `source_sha256` and
`executable_sha256`; for a WASM build compare the generated Rust program source,
`pkg/*_bg.wasm`, and generated JavaScript glue with `program_source_sha256`,
`module_sha256`, and `js_glue_sha256`. Keep any trusted expected hashes or
signatures separate from the workspace if they are intended to authenticate a
particular release. This
project currently produces no signed manifest or signature-verification tool.

## Reproducibility claims

The metadata renderer is deterministic for the same recorded inputs. A CLI test
builds the synthetic fixture twice in fresh workspaces and checks that
`build.meta` and the stable artifact metadata are byte-identical; the native
executable hash is excluded from that metadata comparison because it identifies
the output binary itself. That test does not assert executable-hash equality or
compare complete build outputs. WASM metadata renderer unit tests compare
rendered metadata for fixed supplied inputs; they do not perform two WASM builds
or compare package outputs.

This is **not** a claim that independently produced native executables or WASM
packages are bit-for-bit reproducible. The repository does not currently run
two independent clean builds and compare their complete output hashes, nor does
the metadata capture every input needed to reproduce a build. Toolchain,
linker, host, configuration, and dependency changes may alter output. The
recorded artifact hashes identify one produced output; they do not establish
reproducibility or a performance, compatibility, or safety property.

To make a stronger reproducibility claim, a future process would need to bind
the exact source tree, all dependency sources and build tools, target and build
configuration, and relevant environment, then perform independent clean builds
and compare the complete outputs. That process and any signing or attestation
remain out of scope for the current prototype.
