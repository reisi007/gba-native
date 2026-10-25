//! Experimental, documentation-driven hybrid GBA-to-native prototype.
//!
//! This crate implements a deliberately small, explicitly bounded subset of the
//! Game Boy Advance. It is not a playable emulator and does not boot a BIOS or
//! retail cartridges. See `docs/clean-room/gba-contract.md` for the exact subset.

#![forbid(unsafe_code)]

pub mod aot;
pub mod bios;
pub mod bus;
pub mod cart;
pub mod cpu;
pub mod decode;
pub mod demo;
pub mod display;
pub mod error;
pub mod interpret;
pub mod ir;
pub mod render;
pub mod semantics;
pub mod sha256;

pub use error::{Error, Result};
