//! Bounded discovery measurements on independently generated synthetic code.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::interpret;
use gba_native_aot::aot::{self, NativeHarness};

const ENTRY_OFFSET: usize = 0xC0;
const ROM_BASE: u32 = 0x0800_0000;
static NEXT_TEMP: AtomicU64 = AtomicU64::new(0);

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        let id = NEXT_TEMP.fetch_add(1, Ordering::SeqCst);
        let path = std::env::temp_dir().join(format!("gba-aot-scale-{}-{id}", std::process::id()));
        std::fs::create_dir(&path).expect("create owned temporary directory");
        Self(path)
    }
    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn fixture(words: &[u32]) -> Cartridge {
    let mut bytes = vec![0u8; ENTRY_OFFSET + words.len() * 4];
    bytes[0xA0..0xAC].copy_from_slice(b"SCALE TEST\0\0");
    bytes[0xAC..0xB0].copy_from_slice(b"SCL1");
    bytes[0xB0..0xB2].copy_from_slice(b"01");
    bytes[0xB2] = 0x96;
    let sum = bytes[0xA0..=0xBC]
        .iter()
        .fold(0u8, |a, b| a.wrapping_add(*b));
    bytes[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    for (index, word) in words.iter().enumerate() {
        let offset = ENTRY_OFFSET + index * 4;
        bytes[offset..offset + 4].copy_from_slice(&word.to_le_bytes());
    }
    let cart = Cartridge::from_bytes(bytes).expect("well-sized generated cartridge");
    assert!(cart.header().fixed_ok && cart.header().complement_ok);
    cart
}

fn branch_to_next(address: u32) -> u32 {
    let displacement = (address + 4) as i64 - (address + 8) as i64;
    assert_eq!(displacement % 4, 0);
    let imm24 = (displacement / 4) as i32;
    assert!((-0x80_0000..=0x7f_ffff).contains(&imm24));
    let encoding = 0xEA00_0000 | (imm24 as u32 & 0x00FF_FFFF);
    assert_eq!(((encoding >> 25) & 7), 5, "ARM B opcode");
    assert_eq!(
        address
            .wrapping_add(8)
            .wrapping_add(((encoding << 8) as i32 >> 6) as u32),
        address + 4
    );
    encoding
}

fn initial_state(cart: &Cartridge) -> (Cpu, Bus) {
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    cpu.set_pc(ROM_BASE + ENTRY_OFFSET as u32);
    cpu.set_thumb(false);
    (cpu, Bus::new(cart.clone()))
}

#[test]
fn branch_chain_obeys_global_block_limit_and_runtime_budget() {
    const WORDS: usize = 2048;
    let first = ROM_BASE + ENTRY_OFFSET as u32;
    let words: Vec<_> = (0..WORDS)
        .map(|i| branch_to_next(first + (i * 4) as u32))
        .collect();
    let cart = fixture(&words);
    assert_eq!(cart.read_u32(first), Some(0xEAFF_FFFF));
    let artifact = aot::discover(&cart, first, false, aot::Limits::default()).expect("discover");
    assert_eq!(
        (artifact.stats.blocks, artifact.stats.instructions),
        (512, 512)
    );
    assert!(artifact.stats.limit_reached);
    assert_eq!(artifact.stats.instructions * 100 / WORDS, 25);
    artifact
        .verify(&cart)
        .expect("all discovered blocks are valid immutable-ROM code");
    assert!(
        artifact
            .blocks
            .iter()
            .all(|b| !b.sealed && cart.rom_offset(b.addr).is_some())
    );

    let source = aot::emit_native_module(&cart, &artifact, 520).expect("emit");
    let temp = TempDir::new();
    let rom = temp.path().join("chain.gba");
    std::fs::write(&rom, cart.bytes()).expect("write generated ROM");
    let harness = NativeHarness::build(&source, &temp.path().join("harness")).expect("build");
    let report = harness.run(&rom).expect("bounded run");
    let (mut cpu, mut bus) = initial_state(&cart);
    let used = interpret::run_budget(&mut cpu, &mut bus, 520).expect("reference run");
    assert_eq!(used, 520);
    assert_eq!(report.budget_used, 520);
    assert_eq!(report.budget_remaining, 0);
    assert_eq!(report.native_instructions, 512);
    assert_eq!(report.fallbacks, 8);
    assert_eq!(report.irq_returns, 0);
    assert_eq!(report.pc, first + 520 * 4);
    assert!(
        report
            .matches_reference(&cpu, &bus, used)
            .expect("compare full state")
    );
}

#[test]
fn contiguous_stream_obeys_instruction_and_block_caps_without_scanning_data() {
    const WORDS: usize = 16_384;
    let first = ROM_BASE + ENTRY_OFFSET as u32;
    let cart = fixture(&vec![0xE1A0_0000; WORDS]); // MOV r0,r0
    // Extra unrelated bytes precede entry; discovery is explicitly seeded only at entry.
    assert_eq!(cart.len(), ENTRY_OFFSET + WORDS * 4);
    let artifact = aot::discover(&cart, first, false, aot::Limits::default()).expect("discover");
    assert_eq!(
        (artifact.stats.blocks, artifact.stats.instructions),
        (128, 8192)
    );
    assert_eq!(artifact.stats.block_limit_splits, 127);
    assert!(artifact.stats.limit_reached);
    assert_eq!(artifact.stats.instructions * 100 / WORDS, 50);
    assert_eq!(artifact.entry, first);
    assert_eq!(artifact.blocks[0].addr, first);
    assert!(artifact.blocks.iter().all(|b| b.addr >= first && !b.sealed));
    assert!(artifact.blocks.iter().all(|b| b.instructions.len() == 64));
    artifact
        .verify(&cart)
        .expect("verify every emitted instruction against ROM");
}
