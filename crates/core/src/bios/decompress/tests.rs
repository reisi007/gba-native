//! Synthetic vectors for the opt-in decompression HLE.

use super::*;
use crate::bios::BiosError;
use crate::cart::Cartridge;
use crate::error::Error;

const ROM_BASE: u32 = 0x0800_0000;
const STREAM_OFFSET: usize = 0x100;

fn header(format_type: u8, unit_nibble: u8, size: u32) -> u32 {
    (size << 8) | (u32::from(format_type) << 4) | u32::from(unit_nibble)
}

fn bus_with(stream: &[u8], offset: usize, policy: DecompressionPolicy) -> Bus {
    let mut rom = vec![0u8; 0x400];
    rom[offset..offset + stream.len()].copy_from_slice(stream);
    Bus::new(Cartridge::from_bytes(rom).expect("synthetic cart")).with_decompression_policy(policy)
}

fn decompress(policy: DecompressionPolicy, stream: &[u8], destination: u32) -> Result<Bus> {
    let mut bus = bus_with(stream, STREAM_OFFSET, policy);
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, destination);
    call(SERVICE_LZ77_8, &mut cpu, &mut bus)?;
    Ok(bus)
}

fn read_bytes(bus: &Bus, base: u32, len: usize) -> Vec<u8> {
    (0..len as u32)
        .map(|offset| bus.read8(base + offset).expect("read output"))
        .collect()
}

fn lz77_stream() -> Vec<u8> {
    let mut stream = header(1, 0, 6).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0b0010_0000, b'A', b'B', 0x10, 0x01]);
    stream
}

#[test]
fn lz77_byte_variant_expands_literals_and_overlapping_back_references() {
    let bus = decompress(
        DecompressionPolicy::DocumentedFormats,
        &lz77_stream(),
        0x0200_0000,
    )
    .expect("decompress");
    assert_eq!(read_bytes(&bus, 0x0200_0000, 6), b"ABABAB");
}

#[test]
fn lz77_halfword_variant_stores_pair_units_and_preserves_its_neighbour() {
    let mut stream = header(1, 0, 6).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0b0010_0000, b'A', b'B', 0x10, 0x01]);
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    bus.write16(0x0600_0006, 0x1234).expect("seed");
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0600_0000);
    call(SERVICE_LZ77_16, &mut cpu, &mut bus).expect("decompress");
    assert_eq!(bus.read16(0x0600_0000), Ok(u16::from_le_bytes(*b"AB")));
    assert_eq!(bus.read16(0x0600_0002), Ok(u16::from_le_bytes(*b"AB")));
    assert_eq!(bus.read16(0x0600_0004), Ok(u16::from_le_bytes(*b"AB")));
    assert_eq!(bus.read16(0x0600_0006), Ok(0x1234));
}

#[test]
fn lz77_halfword_variant_rejects_documented_disp_zero() {
    let mut stream = header(1, 0, 4).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0b1000_0000, 0x00, 0x00]);
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0600_0000);
    assert_eq!(
        call(SERVICE_LZ77_16, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::DecompressZeroDisplacement {
            service: SERVICE_LZ77_16
        }))
    );
}

#[test]
fn lz77_rejects_a_back_reference_before_the_output_start() {
    let mut stream = header(1, 0, 3).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0b1000_0000, 0x00, 0x00]);
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0200_0000);
    assert_eq!(
        call(SERVICE_LZ77_8, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::DecompressBackReference {
            service: SERVICE_LZ77_8,
            output: 0,
            displacement: 0,
        }))
    );
}

#[test]
fn header_validation_rejects_type_and_reserved_nibble_mismatches() {
    let wrong_type = header(2, 0, 4).to_le_bytes().to_vec();
    let mut bus = bus_with(
        &wrong_type,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0200_0000);
    assert!(matches!(
        call(SERVICE_LZ77_8, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::DecompressHeader {
            service: SERVICE_LZ77_8,
            expected_type: 1,
            ..
        }))
    ));

    let reserved = header(1, 3, 4).to_le_bytes().to_vec();
    let mut bus = bus_with(
        &reserved,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    assert!(matches!(
        call(SERVICE_LZ77_8, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::DecompressHeader { .. }))
    ));
}

#[test]
fn byte_variant_rejects_video_ram_destinations() {
    let stream = lz77_stream();
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0600_0000);
    assert_eq!(
        call(SERVICE_LZ77_8, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedDestination {
            service: SERVICE_LZ77_8,
            addr: 0x0600_0000,
        }))
    );
}

#[test]
fn halfword_variant_requires_alignment_and_an_even_size() {
    let mut odd_size = header(1, 0, 5).to_le_bytes().to_vec();
    odd_size.extend_from_slice(&[0; 8]);
    let mut bus = bus_with(
        &odd_size,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0600_0002);
    assert_eq!(
        call(SERVICE_LZ77_16, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::DecompressUnitSize {
            service: SERVICE_LZ77_16,
            size: 5
        }))
    );

    let stream = lz77_stream();
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    cpu.set_reg(1, 0x0600_0001);
    assert!(matches!(
        call(SERVICE_LZ77_16, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned {
            service: SERVICE_LZ77_16,
            align: 2,
            ..
        }))
    ));
}

#[test]
fn run_length_expands_uncompressed_and_repeated_blocks() {
    let mut stream = header(3, 0, 6).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0x02, b'X', b'Y', b'Z', 0x80, b'Q']);
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0200_0000);
    call(SERVICE_RL_8, &mut cpu, &mut bus).expect("decompress");
    assert_eq!(read_bytes(&bus, 0x0200_0000, 6), b"XYZQQQ");
}

#[test]
fn diff8_filter_accumulates_with_byte_wrapping() {
    let mut stream = header(8, 1, 4).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0x10, 0x01, 0xFF, 0x02]);
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0200_0000);
    call(SERVICE_DIFF8_8, &mut cpu, &mut bus).expect("decompress");
    assert_eq!(read_bytes(&bus, 0x0200_0000, 4), [0x10, 0x11, 0x10, 0x12]);
}

#[test]
fn diff16_filter_accumulates_little_endian_halfwords() {
    let mut stream = header(8, 2, 6).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0x00, 0x10, 0x01, 0x00, 0xFF, 0xFF]);
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0600_0000);
    call(SERVICE_DIFF16_16, &mut cpu, &mut bus).expect("decompress");
    assert_eq!(bus.read16(0x0600_0000), Ok(0x1000));
    assert_eq!(bus.read16(0x0600_0002), Ok(0x1001));
    assert_eq!(bus.read16(0x0600_0004), Ok(0x1000));
}

#[test]
fn malformed_stream_leaves_the_destination_untouched() {
    let stream = header(1, 0, 4).to_le_bytes().to_vec();
    let offset = 0x400 - stream.len();
    let mut bus = bus_with(&stream, offset, DecompressionPolicy::DocumentedFormats);
    bus.write32(0x0200_0000, 0xA5A5_A5A5).expect("seed");
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + offset as u32);
    cpu.set_reg(1, 0x0200_0000);
    assert!(matches!(
        call(SERVICE_LZ77_8, &mut cpu, &mut bus),
        Err(Error::RomOverread { .. })
    ));
    assert_eq!(bus.read32(0x0200_0000), Ok(0xA5A5_A5A5));
}

#[test]
fn zero_size_stream_is_a_successful_no_op() {
    let stream = header(1, 0, 0).to_le_bytes().to_vec();
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    bus.write16(0x0600_0000, 0xBEEF).expect("seed");
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0600_0000);
    call(SERVICE_LZ77_16, &mut cpu, &mut bus).expect("empty stream");
    assert_eq!(bus.read16(0x0600_0000), Ok(0xBEEF));
}

#[test]
fn run_length_halfword_variant_routes_to_vram() {
    let mut stream = header(3, 0, 6).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0x02, b'X', b'Y', b'Z', 0x80, b'Q']);
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0600_0000);
    call(SERVICE_RL_16, &mut cpu, &mut bus).expect("decompress");
    assert_eq!(bus.read16(0x0600_0000), Ok(u16::from_le_bytes(*b"XY")));
    assert_eq!(bus.read16(0x0600_0002), Ok(u16::from_le_bytes(*b"ZQ")));
    assert_eq!(bus.read16(0x0600_0004), Ok(u16::from_le_bytes(*b"QQ")));
}

#[test]
fn diff8_halfword_variant_routes_to_vram() {
    let mut stream = header(8, 1, 4).to_le_bytes().to_vec();
    stream.extend_from_slice(&[0x10, 0x01, 0xFF, 0x02]);
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0600_0000);
    call(SERVICE_DIFF8_16, &mut cpu, &mut bus).expect("decompress");
    assert_eq!(
        bus.read16(0x0600_0000),
        Ok(u16::from_le_bytes([0x10, 0x11]))
    );
    assert_eq!(
        bus.read16(0x0600_0002),
        Ok(u16::from_le_bytes([0x10, 0x12]))
    );
}

#[test]
fn source_must_be_four_byte_aligned() {
    let stream = lz77_stream();
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32 + 2);
    cpu.set_reg(1, 0x0200_0000);
    assert!(matches!(
        call(SERVICE_LZ77_8, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned {
            service: SERVICE_LZ77_8,
            align: 4,
            ..
        }))
    ));
}

#[test]
fn decompression_policy_is_excluded_from_state_digest() {
    let stream = lz77_stream();
    let strict = bus_with(&stream, STREAM_OFFSET, DecompressionPolicy::Strict);
    let documented = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    assert_eq!(strict.state_digest(), documented.state_digest());
}

#[test]
fn zero_size_stream_still_requires_a_writable_destination() {
    let stream = header(1, 0, 0).to_le_bytes().to_vec();
    let mut bus = bus_with(
        &stream,
        STREAM_OFFSET,
        DecompressionPolicy::DocumentedFormats,
    );
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, ROM_BASE);
    assert_eq!(
        call(SERVICE_LZ77_8, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedDestination {
            service: SERVICE_LZ77_8,
            addr: ROM_BASE,
        }))
    );
}

#[test]
fn strict_policy_keeps_every_decompression_service_unsupported() {
    let stream = lz77_stream();
    let mut bus = bus_with(&stream, STREAM_OFFSET, DecompressionPolicy::Strict);
    let mut cpu = Cpu::direct_entry();
    cpu.set_reg(0, ROM_BASE + STREAM_OFFSET as u32);
    cpu.set_reg(1, 0x0200_0000);
    for service in [
        SERVICE_LZ77_8,
        SERVICE_LZ77_16,
        SERVICE_RL_8,
        SERVICE_RL_16,
        SERVICE_DIFF8_8,
        SERVICE_DIFF8_16,
        SERVICE_DIFF16_16,
    ] {
        assert_eq!(
            crate::bios::call(service, &mut cpu, &mut bus),
            Err(Error::Bios(BiosError::UnsupportedService(service)))
        );
    }
}
