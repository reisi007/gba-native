//! Experimental, documentation-driven hybrid GBA-to-native prototype.
//!
//! This root package is the CLI plus a compatibility facade. The std-only
//! runtime and typed IR live in the `gba-native-core` package, and host-side AOT
//! tooling lives in `gba-native-aot`. Re-exporting both preserves the historical
//! `gba_native::...` import paths used by the CLI, tests and external callers.
//! It is not a playable emulator and does not boot a BIOS or retail cartridges.
//! See `docs/clean-room/gba-contract.md` for the exact subset.

#![forbid(unsafe_code)]

pub use gba_native_aot::aot;
pub use gba_native_core::{Error, Result};
pub use gba_native_core::{
    bios, bus, cart, cpu, decode, demo, display, error, interpret, ir, memory, render, semantics,
    sha256,
};
