//! Command dispatch and user-facing commands.

use std::path::{Path, PathBuf};

use gba_native::aot::{self, NativeHarness};
use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::demo;
use gba_native::error::{Error, Result};
use gba_native::interpret;

mod aot_build;
mod audio_cmd;
mod demo_cmd;
mod fixture_cmd;
mod info_cmd;
mod output;
mod run_cmd;
mod run_output;
mod save_cmd;

use output::{create_fresh_directory, create_unique_workspace, write_safely};

const USAGE: &str = "\
usage:
  gba-native info <rom.gba>
  gba-native demo [--out output/demo.ppm] [--size WxH] [--backend nearest|fsr1|temporal-fsr|ml-fsr|dlss]
  gba-native fixture input-tester --out output/<input-tester.gba>
  gba-native save list|import|export|use|default|delete <rom.gba> [slot] [--dir output/saves]
  gba-native audio --out output/<tone.wav> [--frequency 440] [--seconds 2] [--volume 80] [--mute]
  gba-native run <rom.gba> [--budget N] [--out output/<new-directory>] [--input <script>] [--audio-out <wav>] [--volume 0..100] [--mute] [--synthetic-bios-zero-fill] [--synthetic-bios-rom-writes] [--synthetic-rom-writes-ignored] [--synthetic-backup-sram|--synthetic-backup-sram64k|--synthetic-backup-flash128k] [--synthetic-bios-decompress] [--synthetic-bios-affine]
  gba-native aot proof [--out output/aot-proof]
  gba-native aot emit <rom.gba> <generated.rs>
  gba-native aot build <rom.gba> --target <triple> [--out output/<new-directory>] [--budget N] [--trace <trace.txt>] [--synthetic-bios-zero-fill] [--synthetic-bios-rom-writes] [--synthetic-rom-writes-ignored] [--synthetic-backup-sram|--synthetic-backup-sram64k|--synthetic-backup-flash128k] [--synthetic-bios-decompress] [--synthetic-bios-affine]

The synthetic demo is the only supported end-to-end fixture. Progress is
measured in instructions, not frames; retail games are not supported.
--synthetic-bios-zero-fill is an explicit experimental opt-in that returns
zero for BIOS data reads; it is not authentic BIOS data or hardware behavior.
Generated files are restricted to <cwd>/output/; an explicit --out
directory must not already exist. aot build compiles a headless native
artifact, or a ROM-bound WASM execution module for wasm32-unknown-unknown;
neither is a playable game or browser app.";

pub fn run() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let command = args.next().ok_or_else(|| Error::Args(USAGE.to_string()))?;
    match command.as_str() {
        "info" => {
            let path = args
                .next()
                .ok_or_else(|| Error::Args("info needs a ROM path".to_string()))?;
            info_cmd::run(Path::new(&path))
        }
        "audio" => audio_cmd::run(args),
        "demo" => demo_cmd::run(args),
        "fixture" => fixture_cmd::run(args),
        "save" => save_cmd::run(args),
        "run" => run_cmd::run(args),
        "aot" => {
            let subcommand = args
                .next()
                .ok_or_else(|| Error::Args("aot needs proof, emit or build".to_string()))?;
            match subcommand.as_str() {
                "proof" => cmd_aot_proof(args),
                "emit" => cmd_aot_emit(args),
                "build" => aot_build::run(args),
                _ => Err(Error::Args(USAGE.to_string())),
            }
        }
        "-h" | "--help" | "help" => {
            println!("{USAGE}");
            Ok(())
        }
        _ => Err(Error::Args(USAGE.to_string())),
    }
}

fn cmd_aot_proof(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let mut explicit_work: Option<PathBuf> = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" => {
                explicit_work =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        Error::Args("--out needs a directory".to_string())
                    })?));
            }
            _ => return Err(Error::Args(format!("unknown aot proof option {argument}"))),
        }
    }
    // Every run owns a fresh workspace. The default allocates a new unique
    // directory, so repeated proofs never erase or reuse earlier output. An
    // explicit --out must be a new path and is never treated as a reusable
    // crate directory.
    let work = match explicit_work {
        Some(path) => create_fresh_directory(&path)?,
        None => create_unique_workspace("aot-proof")?,
    };

    let cart = demo::demo_cartridge();
    let mut reference_cpu = Cpu::direct_entry();
    reference_cpu.initialize_bios_stacks();
    reference_cpu.set_pc(demo::ENTRY);
    reference_cpu.set_thumb(false);
    let mut reference_bus = Bus::new(cart.clone());
    let used = interpret::run_budget(&mut reference_cpu, &mut reference_bus, demo::DEMO_BUDGET)?;

    let artifact = aot::discover(&cart, demo::ENTRY, false, aot::Limits::default())?;
    let source = aot::emit_native_module(&cart, &artifact, demo::DEMO_BUDGET)?;
    // The generated harness never embeds ROM bytes. Proof runs against an
    // independently generated synthetic fixture written to the owned work dir.
    let fixture = work.join("fixture.gba");
    write_safely(&fixture, cart.bytes())?;
    // The harness crate is another fresh child: no caller-controlled file or
    // link can pre-exist inside it.
    let harness = NativeHarness::build(&source, &work.join("harness"))?;
    let report = harness.run(&fixture)?;
    let matches = report.matches_reference(&reference_cpu, &reference_bus, used)?;

    println!("fixture=synthetic-direct-entry");
    println!("rom_sha256={}", artifact.rom_sha256_hex());
    println!("blocks={}", artifact.stats.blocks);
    println!("translated_instructions={}", artifact.stats.instructions);
    println!(
        "rom_bytes={} instructions_per_rom_byte={:.4}",
        cart.len(),
        artifact.stats.instructions as f64 / cart.len() as f64
    );
    println!("resolved_bx={}", artifact.stats.resolved_bx);
    println!("dynamic_bx={}", artifact.stats.dynamic_bx);
    println!("bl_calls={}", artifact.stats.bl_calls);
    println!("bl_returns={}", artifact.stats.bl_returns);
    println!("dynamic_bl={}", artifact.stats.dynamic_bl);
    println!("pc_writes={}", artifact.stats.pc_writes);
    println!("swis={}", artifact.stats.swis);
    println!("decode_stops={}", artifact.stats.decode_stops);
    println!("external_targets={}", artifact.stats.external_targets);
    println!("budget_used={}", report.budget_used);
    println!("fallbacks={}", report.fallbacks);
    println!("image_sha256={}", report.image_sha256);
    println!("bus_sha256={}", report.bus_sha256);
    println!(
        "native_vs_reference={}",
        if matches { "match" } else { "MISMATCH" }
    );
    println!("generated_dir={}", work.display());
    println!("fixture_path={}", fixture.display());
    if !matches {
        return Err(Error::Aot("native and reference states differ".to_string()));
    }
    println!("proof=ok");
    println!("note=synthetic fixture only; no retail-game or general static-recovery claim");
    Ok(())
}

fn cmd_aot_emit(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let rom = args
        .next()
        .ok_or_else(|| Error::Args("aot emit needs a ROM path".to_string()))?;
    let out = args
        .next()
        .ok_or_else(|| Error::Args("aot emit needs an output path".to_string()))?;
    if args.next().is_some() {
        return Err(Error::Args("aot emit takes exactly two paths".to_string()));
    }
    let cart = Cartridge::from_path(Path::new(&rom))?;
    let artifact = aot::discover(&cart, 0x0800_0000, false, aot::Limits::default())?;
    let source = aot::emit_native_module(&cart, &artifact, 1_000_000)?;
    write_safely(Path::new(&out), source.as_bytes())?;
    println!("rom_sha256={}", artifact.rom_sha256_hex());
    println!("blocks={}", artifact.stats.blocks);
    println!("translated_instructions={}", artifact.stats.instructions);
    println!("resolved_bx={}", artifact.stats.resolved_bx);
    println!("dynamic_bx={}", artifact.stats.dynamic_bx);
    println!("bl_calls={}", artifact.stats.bl_calls);
    println!("bl_returns={}", artifact.stats.bl_returns);
    println!("dynamic_bl={}", artifact.stats.dynamic_bl);
    println!("pc_writes={}", artifact.stats.pc_writes);
    println!("swis={}", artifact.stats.swis);
    println!("decode_stops={}", artifact.stats.decode_stops);
    println!("output={out}");
    println!("embedded_rom_bytes=0");
    println!(
        "note=generated source binds SHA-256+length and requires --rom PATH; it is not executed by this command"
    );
    Ok(())
}

fn parse_size(value: &str) -> Result<(u32, u32)> {
    let (width, height) = value
        .split_once('x')
        .or_else(|| value.split_once('X'))
        .ok_or_else(|| Error::Args(format!("invalid size {value:?}, expected WxH")))?;
    let width = width
        .parse()
        .map_err(|_| Error::Args(format!("invalid width in {value:?}")))?;
    let height = height
        .parse()
        .map_err(|_| Error::Args(format!("invalid height in {value:?}")))?;
    Ok((width, height))
}
