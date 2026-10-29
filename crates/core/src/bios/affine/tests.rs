//! Synthetic vectors for the opt-in ObjAffineSet HLE.

use super::*;
use crate::bus::OAM_BASE;
use crate::cart::Cartridge;

const ROM_BASE: u32 = 0x0800_0000;
const SOURCE_OFFSET: usize = 0x100;
const DESTINATION: u32 = 0x0200_0000;

fn fixture(entries: &[(i16, i16, u16)], policy: AffinePolicy) -> Bus {
    let mut rom = vec![0u8; 0x400];
    for (index, (scale_x, scale_y, angle)) in entries.iter().enumerate() {
        let base = SOURCE_OFFSET + index * 6;
        rom[base..base + 2].copy_from_slice(&scale_x.to_le_bytes());
        rom[base + 2..base + 4].copy_from_slice(&scale_y.to_le_bytes());
        rom[base + 4..base + 6].copy_from_slice(&angle.to_le_bytes());
    }
    Bus::new(Cartridge::from_bytes(rom).expect("synthetic cart")).with_affine_policy(policy)
}

fn configure(cpu: &mut Cpu, count: u32, offset: u32) {
    cpu.set_reg(0, ROM_BASE + SOURCE_OFFSET as u32);
    cpu.set_reg(1, DESTINATION);
    cpu.set_reg(2, count);
    cpu.set_reg(3, offset);
}

fn read_parameters(bus: &Bus, address: u32) -> [u16; 4] {
    [
        bus.read16(address).expect("PA"),
        bus.read16(address + 2).expect("PB"),
        bus.read16(address + 4).expect("PC"),
        bus.read16(address + 6).expect("PD"),
    ]
}

#[test]
fn identity_scale_and_zero_angle_write_the_documented_order() {
    let mut bus = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 2);
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus).expect("affine");
    assert_eq!(
        read_parameters(&bus, DESTINATION),
        [0x0100, 0x0000, 0x0000, 0x0100]
    );
}

#[test]
fn quarter_turn_maps_to_the_documented_parameter_signs() {
    let mut bus = fixture(&[(0x0100, 0x0200, 0x4000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 2);
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus).expect("affine");
    assert_eq!(
        read_parameters(&bus, DESTINATION),
        [0x0000, 0xFF00, 0x0200, 0x0000]
    );
}

#[test]
fn half_turn_negates_both_diagonal_parameters() {
    let mut bus = fixture(&[(0x0100, 0x0100, 0x8000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 2);
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus).expect("affine");
    assert_eq!(
        read_parameters(&bus, DESTINATION),
        [0xFF00, 0x0000, 0x0000, 0xFF00]
    );
}

#[test]
fn only_the_upper_angle_byte_is_used() {
    let mut low = fixture(&[(0x0100, 0x0100, 0x40FF)], AffinePolicy::Documented);
    let mut plain = fixture(&[(0x0100, 0x0100, 0x4000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 2);
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut low).expect("affine");
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut plain).expect("affine");
    assert_eq!(
        read_parameters(&low, DESTINATION),
        read_parameters(&plain, DESTINATION)
    );
}

#[test]
fn negative_scale_flips_the_off_diagonal_sign() {
    let mut bus = fixture(&[(-0x0100, 0x0100, 0x4000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 2);
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus).expect("affine");
    assert_eq!(
        read_parameters(&bus, DESTINATION),
        [0x0000, 0x0100, 0x0100, 0x0000]
    );
}

#[test]
fn offset_eight_places_one_parameter_per_oam_slot() {
    let entries = [(0x0100, 0x0100, 0x0000), (0x0200, 0x0200, 0x0000)];
    let mut bus = fixture(&entries, AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 2, 8);
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus).expect("affine");
    assert_eq!(
        [
            bus.read16(DESTINATION).expect("entry0 PA"),
            bus.read16(DESTINATION + 8).expect("entry0 PB"),
            bus.read16(DESTINATION + 16).expect("entry0 PC"),
            bus.read16(DESTINATION + 24).expect("entry0 PD"),
        ],
        [0x0100, 0x0000, 0x0000, 0x0100]
    );
    assert_eq!(
        [
            bus.read16(DESTINATION + 32).expect("entry1 PA"),
            bus.read16(DESTINATION + 40).expect("entry1 PB"),
            bus.read16(DESTINATION + 48).expect("entry1 PC"),
            bus.read16(DESTINATION + 56).expect("entry1 PD"),
        ],
        [0x0200, 0x0000, 0x0000, 0x0200]
    );
    assert_eq!(bus.read16(DESTINATION + 2), Ok(0), "gap stays untouched");
}

#[test]
fn offset_two_stores_the_four_parameters_contiguously() {
    let entries = [(0x0100, 0x0100, 0x0000), (0x0200, 0x0200, 0x0000)];
    let mut bus = fixture(&entries, AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 2, 2);
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus).expect("affine");
    assert_eq!(
        read_parameters(&bus, DESTINATION),
        [0x0100, 0x0000, 0x0000, 0x0100]
    );
    assert_eq!(
        read_parameters(&bus, DESTINATION + 8),
        [0x0200, 0x0000, 0x0000, 0x0200]
    );
}

#[test]
fn zero_count_writes_nothing() {
    let mut bus = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Documented);
    bus.write16(DESTINATION, 0xBEEF).expect("seed");
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 0, 2);
    call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus).expect("affine");
    assert_eq!(bus.read16(DESTINATION), Ok(0xBEEF));
}

#[test]
fn odd_offset_and_misaligned_addresses_are_explicit() {
    let mut bus = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 3);
    assert!(matches!(
        call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedParameter { .. }))
    ));
    // The offset is validated even when no calculation would be performed.
    configure(&mut cpu, 0, 3);
    assert!(matches!(
        call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedParameter { .. }))
    ));

    configure(&mut cpu, 1, 2);
    cpu.set_reg(1, DESTINATION + 1);
    assert!(matches!(
        call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned {
            service: SERVICE_OBJ_AFFINE_SET,
            align: 2,
            ..
        }))
    ));

    cpu.set_reg(1, DESTINATION);
    cpu.set_reg(0, ROM_BASE + SOURCE_OFFSET as u32 + 1);
    assert!(matches!(
        call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned { .. }))
    ));
}

#[test]
fn oversized_destination_span_is_an_explicit_range_overflow() {
    let mut bus = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 2, 0x8000_0000);
    assert!(matches!(
        call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::RangeOverflow { .. }))
    ));
}

#[test]
fn rom_destination_and_overrunning_source_are_explicit() {
    let mut bus = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 2);
    cpu.set_reg(1, ROM_BASE);
    assert!(matches!(
        call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedDestination { .. }))
    ));

    cpu.set_reg(1, DESTINATION);
    cpu.set_reg(0, ROM_BASE + 0x3FC);
    cpu.set_reg(2, 2);
    assert!(matches!(
        call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::RangeOutsideRegion { .. }))
    ));
}

#[test]
fn destination_region_cannot_be_crossed() {
    let mut bus = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 2);
    cpu.set_reg(1, OAM_BASE + 0x400 - 4);
    assert!(matches!(
        call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::RangeOutsideRegion { .. }))
    ));
}

#[test]
fn strict_policy_keeps_objaffineset_unsupported_and_mutates_nothing() {
    let mut bus = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Strict);
    bus.write16(DESTINATION, 0xBEEF).expect("seed");
    let mut cpu = Cpu::direct_entry();
    configure(&mut cpu, 1, 2);
    let before = (cpu.state_digest(), bus.state_digest());
    assert_eq!(
        crate::bios::call(SERVICE_OBJ_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedService(
            SERVICE_OBJ_AFFINE_SET
        )))
    );
    assert_eq!((cpu.state_digest(), bus.state_digest()), before);
    assert_eq!(bus.read16(DESTINATION), Ok(0xBEEF));
}

#[test]
fn sine_table_has_the_mathematical_cardinal_values_and_symmetry() {
    assert_eq!(table::SINE_Q12[0], 0);
    assert_eq!(table::SINE_Q12[64], 4096);
    assert_eq!(table::SINE_Q12[128], 0);
    assert_eq!(table::SINE_Q12[192], -4096);
    for index in 0..128 {
        assert_eq!(table::SINE_Q12[index + 128], -table::SINE_Q12[index]);
        if index <= 64 {
            assert_eq!(table::SINE_Q12[128 - index], table::SINE_Q12[index]);
        }
    }
}

#[test]
fn affine_policy_is_excluded_from_state_digest() {
    let strict = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Strict);
    let documented = fixture(&[(0x0100, 0x0100, 0x0000)], AffinePolicy::Documented);
    assert_eq!(strict.state_digest(), documented.state_digest());
}

fn bg_fixture(entries: &[(i32, i32, i16, i16, i16, i16, u16)], policy: AffinePolicy) -> Bus {
    let mut rom = vec![0u8; 0x400];
    for (index, (origin_x, origin_y, display_x, display_y, scale_x, scale_y, angle)) in
        entries.iter().enumerate()
    {
        let base = SOURCE_OFFSET + index * 20;
        rom[base..base + 4].copy_from_slice(&origin_x.to_le_bytes());
        rom[base + 4..base + 8].copy_from_slice(&origin_y.to_le_bytes());
        rom[base + 8..base + 10].copy_from_slice(&display_x.to_le_bytes());
        rom[base + 10..base + 12].copy_from_slice(&display_y.to_le_bytes());
        rom[base + 12..base + 14].copy_from_slice(&scale_x.to_le_bytes());
        rom[base + 14..base + 16].copy_from_slice(&scale_y.to_le_bytes());
        rom[base + 16..base + 18].copy_from_slice(&angle.to_le_bytes());
    }
    Bus::new(Cartridge::from_bytes(rom).expect("synthetic cart")).with_affine_policy(policy)
}

fn configure_bg(cpu: &mut Cpu, count: u32) {
    cpu.set_reg(0, ROM_BASE + SOURCE_OFFSET as u32);
    cpu.set_reg(1, DESTINATION);
    cpu.set_reg(2, count);
}

fn read_bg_parameters(bus: &Bus, address: u32) -> ([u16; 4], [u32; 2]) {
    (
        [
            bus.read16(address).expect("PA"),
            bus.read16(address + 2).expect("PB"),
            bus.read16(address + 4).expect("PC"),
            bus.read16(address + 6).expect("PD"),
        ],
        [
            bus.read32(address + 8).expect("start X"),
            bus.read32(address + 12).expect("start Y"),
        ],
    )
}

#[test]
fn bg_affine_zero_scale_is_the_observed_hide_parameters() {
    let mut bus = bg_fixture(
        &[(0x0000_8000, 0x0000_8000, 120, 80, 0, 0, 0)],
        AffinePolicy::Documented,
    );
    let mut cpu = Cpu::direct_entry();
    configure_bg(&mut cpu, 1);
    call(SERVICE_BG_AFFINE_SET, &mut cpu, &mut bus).expect("bg affine");
    assert_eq!(
        read_bg_parameters(&bus, DESTINATION),
        ([0, 0, 0, 0], [0x0000_8000, 0x0000_8000])
    );
}

#[test]
fn bg_affine_identity_centres_the_display_field() {
    let mut bus = bg_fixture(
        &[(0x0000_8000, 0x0000_8000, 120, 80, 0x0100, 0x0100, 0)],
        AffinePolicy::Documented,
    );
    let mut cpu = Cpu::direct_entry();
    configure_bg(&mut cpu, 1);
    call(SERVICE_BG_AFFINE_SET, &mut cpu, &mut bus).expect("bg affine");
    assert_eq!(
        read_bg_parameters(&bus, DESTINATION),
        ([0x0100, 0, 0, 0x0100], [0x0000_0800, 0x0000_3000])
    );
}

#[test]
fn bg_affine_quarter_turn_uses_the_documented_signs() {
    let mut bus = bg_fixture(
        &[(0x0000_8000, 0x0000_8000, 120, 80, 0x0100, 0x0100, 0x4000)],
        AffinePolicy::Documented,
    );
    let mut cpu = Cpu::direct_entry();
    configure_bg(&mut cpu, 1);
    call(SERVICE_BG_AFFINE_SET, &mut cpu, &mut bus).expect("bg affine");
    assert_eq!(
        read_bg_parameters(&bus, DESTINATION),
        ([0x0000, 0xFF00, 0x0100, 0x0000], [0x0000_D000, 0x0000_0800])
    );
}

#[test]
fn bg_affine_writes_multiple_sixteen_byte_entries() {
    let entries = [
        (0x0000_8000, 0x0000_8000, 120, 80, 0x0100, 0x0100, 0),
        (0x0000_4000, 0x0000_4000, 60, 40, 0x0080, 0x0080, 0x8000),
    ];
    let mut bus = bg_fixture(&entries, AffinePolicy::Documented);
    let mut cpu = Cpu::direct_entry();
    configure_bg(&mut cpu, 2);
    call(SERVICE_BG_AFFINE_SET, &mut cpu, &mut bus).expect("bg affine");
    assert_eq!(
        read_bg_parameters(&bus, DESTINATION),
        ([0x0100, 0, 0, 0x0100], [0x0000_0800, 0x0000_3000])
    );
    assert_eq!(
        read_bg_parameters(&bus, DESTINATION + 16),
        ([0xFF80, 0, 0, 0xFF80], [0x0000_5E00, 0x0000_5400])
    );
}

#[test]
fn bg_affine_strict_policy_and_bad_ranges_fail_closed() {
    let mut bus = bg_fixture(
        &[(0x0000_8000, 0x0000_8000, 120, 80, 0x0100, 0x0100, 0)],
        AffinePolicy::Strict,
    );
    let mut cpu = Cpu::direct_entry();
    configure_bg(&mut cpu, 1);
    assert_eq!(
        crate::bios::call(SERVICE_BG_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::UnsupportedService(
            SERVICE_BG_AFFINE_SET
        )))
    );

    let mut bus = bg_fixture(
        &[(0x0000_8000, 0x0000_8000, 120, 80, 0x0100, 0x0100, 0)],
        AffinePolicy::Documented,
    );
    configure_bg(&mut cpu, 1);
    cpu.set_reg(1, DESTINATION + 2);
    assert!(matches!(
        call(SERVICE_BG_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned {
            service: SERVICE_BG_AFFINE_SET,
            align: 4,
            ..
        }))
    ));

    cpu.set_reg(1, DESTINATION);
    cpu.set_reg(0, ROM_BASE + SOURCE_OFFSET as u32 + 1);
    assert!(matches!(
        call(SERVICE_BG_AFFINE_SET, &mut cpu, &mut bus),
        Err(Error::Bios(BiosError::Misaligned { .. }))
    ));
}
