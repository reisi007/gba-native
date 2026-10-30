//! Trace-seeded ahead-of-time artifacts.
//!
//! A trace is an offline profile produced by the reference interpreter. It
//! names executed instruction addresses and the live 16/32-bit encoding seen
//! at each address. ROM seeds are decoded from the immutable cartridge image
//! exactly like ordinary discovery. Seeds in EWRAM/IWRAM become *sealed*
//! single-instruction blocks: the generated dispatcher re-reads the live
//! encoding at run time and executes the native block only when it still
//! matches the captured word. Any mismatch uses a fresh interpreter decode.
//!
//! This module never embeds a ROM image or a RAM snapshot. The artifact keeps
//! decoded instructions only, and generated source remains build output under
//! the git-ignored `output/` tree.

use std::collections::BTreeMap;

use super::artifact::{AotArtifact, AotBlock, DiscoveryStats, decode_rom, is_sealable_work_ram};
use gba_native::cart::Cartridge;
use gba_native::decode::{decode_arm, decode_thumb};
use gba_native::error::{Error, Result};
use gba_native::ir::Op;

/// One executed instruction observed by a reference trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceSeed {
    pub addr: u32,
    pub thumb: bool,
    pub raw: u32,
}

/// Parse the experimental trace-seed text format.
///
/// Accepted lines are whitespace-separated `address thumb raw` triples with
/// optional `ROM`/`RAM` prefix and trailing instruction count. `#` starts a
/// comment. Address and raw values are hexadecimal, with or without `0x`.
pub fn parse_trace_seeds(text: &str) -> Result<Vec<TraceSeed>> {
    let mut seeds = BTreeMap::new();
    for (line_number, line) in text.lines().enumerate() {
        let line = line.split('#').next().unwrap_or("").trim();
        if line.is_empty() {
            continue;
        }
        let mut fields: Vec<&str> = line.split_whitespace().collect();
        if matches!(fields.first(), Some(&"ROM") | Some(&"RAM")) {
            fields.remove(0);
        }
        if fields.len() < 3 || fields.len() > 4 {
            return Err(Error::Aot(format!(
                "trace line {} is not `address thumb raw [count]`",
                line_number + 1
            )));
        }
        let addr = parse_hex(fields[0]).map_err(|reason| {
            Error::Aot(format!(
                "trace line {}: bad address {reason}",
                line_number + 1
            ))
        })?;
        let thumb = match fields[1] {
            "0" => false,
            "1" => true,
            other => {
                return Err(Error::Aot(format!(
                    "trace line {}: thumb state {other:?} is not 0 or 1",
                    line_number + 1
                )));
            }
        };
        let raw = parse_hex(fields[2]).map_err(|reason| {
            Error::Aot(format!(
                "trace line {}: bad raw word {reason}",
                line_number + 1
            ))
        })?;
        if fields.len() == 4 {
            fields[3].parse::<u64>().map_err(|_| {
                Error::Aot(format!(
                    "trace line {}: invalid instruction count {:?}",
                    line_number + 1,
                    fields[3]
                ))
            })?;
        }
        if thumb && raw > 0xFFFF {
            return Err(Error::Aot(format!(
                "trace line {}: Thumb raw word 0x{raw:08X} is wider than 16 bits",
                line_number + 1
            )));
        }
        let seed = TraceSeed { addr, thumb, raw };
        seeds.insert((addr, thumb, raw), seed);
    }
    if seeds.is_empty() {
        return Err(Error::Aot("trace contains no executable seeds".to_string()));
    }
    Ok(seeds.into_values().collect())
}

/// Build a one-instruction-per-seed artifact.
///
/// ROM seeds are verified against the cartridge image; a stale trace is an
/// error rather than a silent reinterpretation. Addresses outside ROM and the
/// executable work-RAM windows are counted and skipped, so sentinel PCs such
/// as the modelled IRQ dispatcher continuation never become blocks.
pub fn trace_artifact(
    cart: &Cartridge,
    entry: u32,
    entry_thumb: bool,
    seeds: &[TraceSeed],
) -> Result<AotArtifact> {
    if cart.rom_offset(entry).is_none() {
        return Err(Error::Aot(format!(
            "entry 0x{entry:08X} is outside the immutable ROM image"
        )));
    }
    if (entry_thumb && !entry.is_multiple_of(2)) || (!entry_thumb && !entry.is_multiple_of(4)) {
        return Err(Error::Aot(format!(
            "entry 0x{entry:08X} is not aligned for its instruction state"
        )));
    }
    let mut ordered: Vec<TraceSeed> = seeds.to_vec();
    ordered.sort_by_key(|seed| (seed.addr, seed.thumb, seed.raw));
    ordered.dedup_by_key(|seed| (seed.addr, seed.thumb, seed.raw));
    let mut blocks = Vec::new();
    let mut stats = DiscoveryStats::default();
    for seed in ordered {
        if (seed.thumb && !seed.addr.is_multiple_of(2))
            || (!seed.thumb && !seed.addr.is_multiple_of(4))
        {
            stats.rejected_seeds += 1;
            continue;
        }
        let instruction = if cart.rom_offset(seed.addr).is_some() {
            let instruction = decode_rom(cart, seed.addr, seed.thumb)?;
            if instruction.raw != seed.raw {
                return Err(Error::Aot(format!(
                    "trace word 0x{:08X} at 0x{:08X} does not match the cartridge",
                    seed.raw, seed.addr
                )));
            }
            instruction
        } else {
            if !is_sealable_work_ram(seed.addr, if seed.thumb { 2 } else { 4 }) {
                stats.rejected_seeds += 1;
                continue;
            }
            let instruction = if seed.thumb {
                decode_thumb(seed.addr, seed.raw)?
            } else {
                decode_arm(seed.addr, seed.raw)?
            };
            stats.sealed_blocks += 1;
            instruction
        };
        if matches!(instruction.op, Op::Swi { .. }) {
            stats.swis += 1;
        }
        stats.instructions += 1;
        blocks.push(AotBlock {
            addr: seed.addr,
            thumb: seed.thumb,
            sealed: cart.rom_offset(seed.addr).is_none(),
            instructions: vec![instruction],
        });
    }
    if blocks.is_empty() {
        return Err(Error::Aot(
            "trace produced no executable blocks".to_string(),
        ));
    }
    stats.blocks = blocks.len();
    Ok(AotArtifact {
        rom_sha256: cart.sha256(),
        rom_len: cart.len(),
        entry,
        entry_thumb,
        blocks,
        stats,
    })
}

fn parse_hex(value: &str) -> std::result::Result<u32, String> {
    let value = value.strip_prefix("0x").unwrap_or(value);
    u32::from_str_radix(value, 16).map_err(|_| format!("{value:?} is not hexadecimal"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_optional_prefix_and_count() {
        let seeds =
            parse_trace_seeds("ROM 08000000 0 E12FFF1E 10\n# x\n03001E0C 0 E1A00000\n").unwrap();
        assert_eq!(seeds.len(), 2);
        assert_eq!(seeds[0].addr, 0x0800_0000);
        assert_eq!(seeds[1].raw, 0xE1A0_0000);
    }

    #[test]
    fn accepts_multiple_variants_at_one_address() {
        let seeds = parse_trace_seeds("03001E0C 0 E1A00000\n03001E0C 0 E1A00001\n").unwrap();
        assert_eq!(seeds.len(), 2);
    }

    #[test]
    fn rejects_empty_trace() {
        assert!(parse_trace_seeds("# nothing\n").is_err());
    }
}
