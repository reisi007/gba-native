use gba_native::demo;

use crate::aot::{Limits, discover, emit_native_module, emit_program_module};

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

#[test]
fn program_module_has_no_host_boundary_or_rom_binding() {
    let cart = demo::demo_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    let source = emit_program_module(&cart, &artifact).expect("emit program");
    for marker in [
        "fn main(",
        "std::",
        "println!",
        "eprintln!",
        "--rom",
        "ROM_SHA256",
        "ROM_LEN",
        "ROM_BYTES",
        "verify_binding",
        "Cartridge",
        "sha256",
        "GBA-NATIVE",
    ] {
        assert!(
            !source.contains(marker),
            "program section must not contain {marker:?}"
        );
    }
    assert!(source.contains(
        "pub fn run_program(cpu: &mut Cpu, bus: &mut Bus, budget: u64, input: &gba_native::input::InputScript, audio: &mut Option<gba_native::audio::AudioEngine>)"
    ));
    assert!(source.contains("pub struct ProgramReport"));
    assert!(source.contains("pub budget_remaining: u64,"));
    assert!(source.contains("pub fallbacks: u64,"));
}

#[test]
fn program_module_dispatches_translated_blocks_and_falls_back() {
    let cart = demo::demo_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    let source = emit_program_module(&cart, &artifact).expect("emit program");
    assert!(source.contains("fn block_08000128_thumb"));
    assert!(source.contains("(0x08000128, true)"));
    assert!(source.contains("(0x08000100, false)"));
    assert!(source.contains("interpret::step(cpu, bus)?;"));
    assert!(source.contains("remaining -= 1;"));
    assert!(source.contains("fallbacks += 1;"));
    assert!(source.contains("while remaining > 0"));
    assert!(source.contains("if next == Next::Halt"));
}

#[test]
fn emitted_blocks_advance_audio_at_every_instruction_boundary() {
    let cart = demo::demo_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    let source = emit_program_module(&cart, &artifact).expect("emit program");
    let ticks = source
        .matches("engine.advance_instruction_bus(bus)?;")
        .count();
    assert!(
        ticks >= artifact.stats.instructions as usize,
        "every translated instruction must advance the audio clock"
    );
    assert!(
        !source.contains("engine.advance_bus(bus,"),
        "the dispatcher must not advance audio once per block with a block-end snapshot"
    );
}

#[test]
fn native_module_composes_program_with_binding_driver() {
    let cart = demo::demo_cartridge();
    let artifact = discover(&cart, demo::ENTRY, false, Limits::default()).expect("discover");
    let program = emit_program_module(&cart, &artifact).expect("emit program");
    let source = emit_native_module(&cart, &artifact, 10).expect("emit native");
    assert!(
        source.starts_with(&program),
        "native source must reuse the program section"
    );
    assert!(
        source.contains("run_program(&mut cpu, &mut bus, BUDGET, &script, &mut audio_engine)?;")
    );
    assert!(source.contains("fn main()"));
    assert!(source.contains("verify_binding(&cart)?;"));
    assert!(source.contains("--rom"));
    assert!(source.contains("--play"));
    assert!(source.contains("fn play_audio(path: &str)"));
    assert!(source.contains("audio_played=true"));
    assert!(source.contains("ROM_LEN"));
    assert!(!source.contains("ROM_BYTES"));
}
