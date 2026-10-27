//! Hybrid AOT prototype.
//!
//! Discovery walks straight-line blocks from an explicit `(PC, ARM/Thumb)`
//! entry, following direct branches and a deliberately local constant
//! propagation for literal/imm BX targets. It stops at unknown/indirect control
//! flow and never scans every ROM word. AOT runs only immutable ROM blocks; all
//! unknown targets (including RAM code, which may self-modify) go through the
//! reference interpreter freshly.

mod artifact;
mod discover;
mod emit;
mod emit_driver;
mod emit_mem;
mod emit_ops;
mod emit_program;
mod harness;
mod report;
mod target_build;

pub use artifact::{AotArtifact, AotBlock, DiscoveryStats, Limits};
pub use discover::discover;
pub use emit::emit_native_module;
pub use emit_program::emit_program_module;
pub use harness::NativeHarness;
pub use report::NativeReport;
pub use target_build::{
    ToolchainFacts, artifact_metadata, build_metadata, host_target_triple, validate_target,
};

#[cfg(test)]
mod tests;
