//! Experimental hybrid AOT tooling for the `gba-native-core` runtime.
//!
//! Discovery, artifact binding, Rust emission, report parsing, the native
//! harness build/run path and the bounded wasm32 module build path live here.
//! This package deliberately depends on the std-only core under the
//! `gba_native` namespace (the generated source and the historical public paths
//! use that spelling) and never on the root CLI compatibility facade.
//!
//! AOT is restricted to immutable ROM bytes. RAM and self-modifying code always
//! go through the reference interpreter with a fresh fetch/decode.

#![forbid(unsafe_code)]

pub mod aot;
