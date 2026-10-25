use gba_native::demo;

use crate::aot::{Limits, discover, emit_native_module};

#[test]
fn discovery_is_bounded_and_follows_static_bx() {
    let cart = demo::demo_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    assert_eq!(artifact.stats.blocks, 4);
    assert_eq!(artifact.stats.instructions, 17);
    assert_eq!(artifact.stats.resolved_bx, 1);
    assert_eq!(artifact.stats.dynamic_bx, 1);
    assert!(artifact.stats.instructions < cart.len() / 4);
    artifact.verify(&cart).expect("artifact matches");
}

#[test]
fn fallback_fixture_leaves_dynamic_target_undiscovered() {
    let cart = demo::fallback_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    assert_eq!(artifact.stats.blocks, 5);
    assert_eq!(artifact.stats.instructions, 12);
    assert!(artifact.stats.dynamic_bx >= 1);
    assert!(
        artifact
            .blocks
            .iter()
            .all(|block| block.addr != demo::FALLBACK_TARGET)
    );
}

#[test]
fn discovery_rejects_non_rom_entry_and_honors_limits() {
    let cart = demo::demo_cartridge();
    assert!(discover(&cart, gba_native::bus::EWRAM_BASE, false, Limits::default()).is_err());
    let limited = discover(
        &cart,
        demo::ENTRY,
        false,
        Limits {
            max_blocks: 1,
            ..Limits::default()
        },
    )
    .expect("limited discovery");
    assert!(limited.stats.limit_reached);
    assert_eq!(limited.stats.blocks, 1);
}

#[test]
fn per_block_cap_splits_blocks_instead_of_aborting_discovery() {
    let cart = demo::demo_cartridge();
    let limited = discover(
        &cart,
        demo::ENTRY,
        false,
        Limits {
            max_block_instructions: 1,
            ..Limits::default()
        },
    )
    .expect("split discovery");
    assert!(limited.blocks.len() > 1);
    assert!(limited.stats.instructions > 1);
    assert!(limited.stats.block_limit_splits > 0);
    assert!(!limited.stats.limit_reached);
    assert!(
        limited
            .blocks
            .iter()
            .all(|block| block.instructions.len() == 1)
    );
}

#[test]
fn zero_per_block_cap_is_an_explicit_error() {
    let cart = demo::demo_cartridge();
    let error = discover(
        &cart,
        demo::ENTRY,
        false,
        Limits {
            max_block_instructions: 0,
            ..Limits::default()
        },
    )
    .expect_err("zero cap cannot make progress");
    assert!(format!("{error}").contains("max_block_instructions"));
}

#[test]
fn global_instruction_budget_is_exact_and_terminal() {
    let cart = demo::demo_cartridge();
    let limited = discover(
        &cart,
        demo::ENTRY,
        false,
        Limits {
            max_instructions: 5,
            ..Limits::default()
        },
    )
    .expect("instruction-limited discovery");
    assert_eq!(limited.stats.instructions, 5);
    assert_eq!(
        limited
            .blocks
            .iter()
            .map(|block| block.instructions.len())
            .sum::<usize>(),
        5
    );
    assert!(limited.stats.limit_reached);
}

#[test]
fn verify_and_emission_reject_empty_artifact_blocks() {
    let cart = demo::demo_cartridge();
    let mut artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    artifact.blocks[0].instructions.clear();
    let error = artifact.verify(&cart).expect_err("empty block rejected");
    assert!(format!("{error}").contains("no instructions"));
    assert!(emit_native_module(&cart, &artifact, 10).is_err());
}

#[test]
fn emitter_rejects_wrong_cartridge() {
    let cart = demo::demo_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    let other = demo::fallback_cartridge();
    assert!(emit_native_module(&other, &artifact, 10).is_err());
}

#[test]
fn emitted_source_contains_no_rom_copy() {
    let cart = demo::demo_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    let source = emit_native_module(&cart, &artifact, 10).expect("emit");
    assert!(!source.contains("ROM_BYTES"));
    assert!(!source.contains("GBA-NATIVE"));
    assert!(source.contains("--rom"));
    assert!(source.contains("ROM_LEN"));
}
