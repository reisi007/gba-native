//! Experimental, documentation-driven hybrid GBA-to-native runtime core.
//!
//! This package is the std-only, third-party-dependency-free runtime: cartridge
//! header, explicit bus, CPU state, typed IR, independent ARM/Thumb decode,
//! reference interpreter, documented BIOS HLE primitives and bitmap snapshot.
//! It is not a playable emulator and does not boot a BIOS. With explicit
//! synthetic device/HLE opt-ins it can reach an observed retail title screen,
//! without a general retail-game or gameplay compatibility claim.
//! See `docs/clean-room/gba-contract.md` for the exact subset.
//!
//! The root `gba-native` package re-exports these modules for the legacy
//! `gba_native::...` import paths. AOT tooling lives in `gba-native-aot`, which
//! depends on this package under the historical `gba_native` alias.

#![forbid(unsafe_code)]

pub mod audio;
pub mod bios;
pub mod bus;
pub mod cart;
pub mod cpu;
pub mod decode;
pub mod demo;
pub mod display;
pub mod error;
pub mod input;
pub mod interpret;
pub mod ir;
pub mod memory;
pub mod perf;
pub mod render;
pub mod save;
pub mod semantics;
pub mod sha256;

pub use error::{Error, Result};
