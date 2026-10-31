//! Target-profile build facts and deterministic artifact metadata.
//!
//! Metadata is written as ordered `key=value` lines. It deliberately contains
//! no absolute paths, timestamps or ROM bytes; identity comes from the bounded
//! ROM digest/length and from SHA-256 hashes of the generated source and the
//! produced executable.

use std::fmt::Write as _;
use std::path::Path;
use std::process::Command;

use super::artifact::AotArtifact;
use gba_native::error::Result;
use gba_native::sha256;

/// Validate a Cargo target triple without claiming that the local toolchain can
/// build it. The build itself is the authority on target support. Cargo also
/// accepts JSON target-spec files, but the public contract is one target triple,
/// so a spec filename is rejected instead of becoming a build input.
pub fn validate_target(target: &str) -> std::result::Result<(), &'static str> {
    if target.is_empty() {
        return Err("target triple is empty");
    }
    if target.trim() != target {
        return Err("target triple has surrounding whitespace");
    }
    if target.starts_with('-') {
        return Err("target triple starts with '-'");
    }
    if !target
        .bytes()
        .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'+'))
    {
        return Err("target triple contains unsupported characters");
    }
    if target.to_ascii_lowercase().ends_with(".json") {
        return Err("target triple must not be a Cargo JSON target-spec filename");
    }
    if target.split('-').count() < 2 || target.split('-').any(str::is_empty) {
        return Err("target triple needs at least two non-empty '-' components");
    }
    Ok(())
}

/// Bounded executable-name suffix policy for a requested target. Only Windows
/// triples are distinguished; every other target keeps the host-independent
/// name used by the existing harness API.
pub fn executable_suffix(target: &str) -> &'static str {
    if target.split('-').any(|part| part == "windows") {
        ".exe"
    } else {
        ""
    }
}

/// Actual Cargo/Rust compiler facts where the local toolchain exposes them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ToolchainFacts {
    pub cargo: String,
    pub rustc: String,
    pub host: String,
}

impl ToolchainFacts {
    pub fn detect() -> Self {
        let cargo = version_command("CARGO", "cargo", &["--version"])
            .and_then(|line| line.split_whitespace().nth(1).map(str::to_string))
            .unwrap_or_else(|| "unknown".to_string());
        let (rustc, host) = rustc_facts().unwrap_or_else(|| {
            (
                "unknown".to_string(),
                host_target_triple().unwrap_or_else(|| "unknown".to_string()),
            )
        });
        Self { cargo, rustc, host }
    }
}

/// Host triple of the installed Rust toolchain, derived from `rustc -vV` with a
/// `cargo -vV` fallback. Tests use it so local and CI runs build for their own
/// host instead of an unavailable cross target.
pub fn host_target_triple() -> Option<String> {
    rustc_facts().map(|(_, host)| host).or_else(cargo_host)
}

fn cargo_host() -> Option<String> {
    let output = Command::new(tool("CARGO", "cargo"))
        .arg("-vV")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    parse_field(&String::from_utf8_lossy(&output.stdout), "host:")
}

fn rustc_facts() -> Option<(String, String)> {
    let output = Command::new(tool("RUSTC", "rustc"))
        .arg("-vV")
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let release = parse_field(&text, "release:")?;
    let host = parse_field(&text, "host:")?;
    Some((release, host))
}

fn version_command(variable: &str, fallback: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(tool(variable, fallback))
        .args(args)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .next()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(str::to_string)
}

fn tool(variable: &str, fallback: &str) -> std::ffi::OsString {
    std::env::var_os(variable).unwrap_or_else(|| fallback.into())
}

fn parse_field(text: &str, prefix: &str) -> Option<String> {
    text.lines()
        .find_map(|line| line.strip_prefix(prefix))
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(str::to_string)
}

/// Render the target-bound artifact metadata. The executable is read only to
/// hash it; ROM bytes are never read again or embedded here.
pub fn artifact_metadata(
    artifact: &AotArtifact,
    target: &str,
    budget: u64,
    source: &str,
    executable: &Path,
) -> Result<String> {
    let executable_bytes = std::fs::read(executable)?;
    let mut out = String::new();
    writeln!(out, "schema=1").unwrap();
    writeln!(out, "target={target}").unwrap();
    writeln!(out, "rom_sha256={}", artifact.rom_sha256_hex()).unwrap();
    writeln!(out, "rom_bytes={}", artifact.rom_len).unwrap();
    writeln!(out, "entry=0x{:08X}", artifact.entry).unwrap();
    writeln!(
        out,
        "entry_state={}",
        if artifact.entry_thumb { "thumb" } else { "arm" }
    )
    .unwrap();
    writeln!(out, "budget={budget}").unwrap();
    let stats = &artifact.stats;
    writeln!(out, "blocks={}", stats.blocks).unwrap();
    writeln!(out, "translated_instructions={}", stats.instructions).unwrap();
    writeln!(out, "resolved_bx={}", stats.resolved_bx).unwrap();
    writeln!(out, "dynamic_bx={}", stats.dynamic_bx).unwrap();
    writeln!(out, "bl_calls={}", stats.bl_calls).unwrap();
    writeln!(out, "bl_returns={}", stats.bl_returns).unwrap();
    writeln!(out, "dynamic_bl={}", stats.dynamic_bl).unwrap();
    writeln!(out, "pc_writes={}", stats.pc_writes).unwrap();
    writeln!(out, "swis={}", stats.swis).unwrap();
    writeln!(out, "sealed_blocks={}", stats.sealed_blocks).unwrap();
    writeln!(out, "rejected_seeds={}", stats.rejected_seeds).unwrap();
    writeln!(out, "decode_stops={}", stats.decode_stops).unwrap();
    writeln!(out, "external_targets={}", stats.external_targets).unwrap();
    writeln!(out, "block_limit_splits={}", stats.block_limit_splits).unwrap();
    writeln!(out, "limit_reached={}", stats.limit_reached).unwrap();
    writeln!(
        out,
        "source_sha256={}",
        sha256::hex(&sha256::digest(source.as_bytes()))
    )
    .unwrap();
    writeln!(
        out,
        "executable_sha256={}",
        sha256::hex(&sha256::digest(&executable_bytes))
    )
    .unwrap();
    writeln!(out, "embedded_rom_bytes=0").unwrap();
    writeln!(out, "requires_external_rom=true").unwrap();
    Ok(out)
}

/// Render toolchain metadata for the same build. No path or timestamp is
/// recorded, so repeated builds with the same facts render byte-identically.
pub fn build_metadata(target: &str, facts: &ToolchainFacts) -> String {
    let mut out = String::new();
    writeln!(out, "schema=1").unwrap();
    writeln!(out, "target={target}").unwrap();
    writeln!(out, "profile=release").unwrap();
    writeln!(out, "build_offline=true").unwrap();
    writeln!(out, "cargo_version={}", facts.cargo).unwrap();
    writeln!(out, "rustc_version={}", facts.rustc).unwrap();
    writeln!(out, "rustc_host={}", facts.host).unwrap();
    out
}
