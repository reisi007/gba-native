//! Command dispatch and user-facing commands.

use std::path::{Path, PathBuf};

use gba_native::aot::{self, NativeHarness};
use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::demo;
use gba_native::display;
use gba_native::error::{Error, Result};
use gba_native::interpret;
use gba_native::render::{self, Backend};

mod aot_build;
mod output;
mod run_output;

use output::{create_fresh_directory, create_unique_workspace, write_safely};

const USAGE: &str = "\
usage:
  gba-native info <rom.gba>
  gba-native demo [--out output/demo.ppm] [--size WxH] [--backend nearest|fsr1|temporal-fsr|ml-fsr|dlss]
  gba-native run <rom.gba> [--budget N] [--out output/<new-directory>] [--synthetic-bios-zero-fill] [--synthetic-bios-rom-writes] [--synthetic-rom-writes-ignored] [--synthetic-backup-sram|--synthetic-backup-sram64k] [--synthetic-bios-decompress] [--synthetic-bios-affine]
  gba-native aot proof [--out output/aot-proof]
  gba-native aot emit <rom.gba> <generated.rs>
  gba-native aot build <rom.gba> --target <triple> [--out output/<new-directory>] [--budget N]

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
            cmd_info(Path::new(&path))
        }
        "demo" => cmd_demo(args),
        "run" => cmd_run(args),
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

fn cmd_info(path: &Path) -> Result<()> {
    let cart = Cartridge::from_path(path)?;
    let header = cart.header();
    println!("file={}", path.display());
    println!("size={}", cart.len());
    println!("sha256={}", cart.sha256_hex());
    println!("title={:?}", header.title);
    println!("game_code={}", String::from_utf8_lossy(&header.game_code));
    println!("maker_code={}", String::from_utf8_lossy(&header.maker_code));
    println!("fixed_byte=0x{:02X}", header.fixed_byte);
    println!("fixed_ok={}", header.fixed_ok);
    println!("main_unit_code=0x{:02X}", header.main_unit_code);
    println!("device_type=0x{:02X}", header.device_type);
    println!("version=0x{:02X}", header.version);
    println!("complement=0x{:02X}", header.complement);
    println!("computed_complement=0x{:02X}", header.computed_complement);
    println!("complement_ok={}", header.complement_ok);
    println!("logo=not-validated (no Nintendo logo bytes embedded)");
    Ok(())
}

fn cmd_demo(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let mut output = PathBuf::from("output/demo.ppm");
    let mut size = (display::WIDTH, display::HEIGHT);
    let mut backend = Backend::Nearest;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" => {
                output = PathBuf::from(
                    args.next()
                        .ok_or_else(|| Error::Args("--out needs a path".to_string()))?,
                );
            }
            "--size" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--size needs WxH".to_string()))?;
                size = parse_size(&value)?;
            }
            "--backend" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--backend needs a name".to_string()))?;
                backend = Backend::parse(&value)?;
            }
            _ => return Err(Error::Args(format!("unknown demo option {argument}"))),
        }
    }

    let cart = demo::demo_cartridge();
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(demo::ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart);
    let used = interpret::run_budget(&mut cpu, &mut bus, demo::DEMO_BUDGET)?;
    let image = display::snapshot(&bus)?;
    let scaled = render::scale(&image, size.0, size.1, backend)?;
    write_safely(&output, &render::to_ppm(&scaled))?;

    println!("fixture=synthetic-direct-entry");
    println!("instructions={used}");
    println!("pc=0x{:08X}", cpu.pc());
    println!("cpsr=0x{:08X}", cpu.cpsr());
    println!("image={}x{}", image.width, image.height);
    println!("image_sha256={}", image.sha256_hex());
    println!("output={}", output.display());
    println!("output_size={}x{}", scaled.width, scaled.height);
    println!("backend={}", backend.name());
    println!("note=experimental synthetic fixture; not a retail-game compatibility result");
    Ok(())
}

fn cmd_run(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let path = args
        .next()
        .ok_or_else(|| Error::Args("run needs a ROM path".to_string()))?;
    let mut budget = 1_000_000u64;
    let mut snapshot_output = None;
    let mut synthetic_bios_zero_fill = false;
    let mut bios_rom_writes = false;
    let mut rom_writes = false;
    let mut backup_sram = false;
    let mut backup_sram64k = false;
    let mut bios_decompress = false;
    let mut bios_affine = false;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--synthetic-bios-zero-fill" => synthetic_bios_zero_fill = true,
            "--synthetic-bios-rom-writes" => bios_rom_writes = true,
            "--synthetic-rom-writes-ignored" => rom_writes = true,
            "--synthetic-backup-sram" => backup_sram = true,
            "--synthetic-backup-sram64k" => backup_sram64k = true,
            "--synthetic-bios-decompress" => bios_decompress = true,
            "--synthetic-bios-affine" => bios_affine = true,
            "--budget" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--budget needs a count".to_string()))?;
                budget = value
                    .parse()
                    .map_err(|_| Error::Args(format!("invalid budget {value}")))?;
            }
            "--out" => {
                snapshot_output =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        Error::Args("--out needs a directory".to_string())
                    })?));
            }
            _ => return Err(Error::Args(format!("unknown run option {argument}"))),
        }
    }
    if backup_sram && backup_sram64k {
        return Err(Error::Args(
            "choose only one of --synthetic-backup-sram and --synthetic-backup-sram64k".to_string(),
        ));
    }
    let cart = Cartridge::from_path(Path::new(&path))?;
    let mut cpu = Cpu::direct_entry();
    cpu.initialize_bios_stacks();
    let mut bus = if synthetic_bios_zero_fill {
        Bus::with_synthetic_bios_zero_fill(cart)
    } else {
        Bus::new(cart)
    };
    if bios_rom_writes {
        bus = bus.with_bios_write_policy(gba_native::bus::BiosWritePolicy::RomWritesDropped);
    }
    if rom_writes {
        bus = bus.with_rom_write_policy(gba_native::bus::RomWritePolicy::Dropped);
    }
    if backup_sram64k {
        bus = bus.with_backup_policy(gba_native::bus::BackupPolicy::Sram64KVolatile);
    } else if backup_sram {
        bus = bus.with_backup_policy(gba_native::bus::BackupPolicy::Sram32KVolatile);
    }
    if bios_decompress {
        bus =
            bus.with_decompression_policy(gba_native::bios::DecompressionPolicy::DocumentedFormats);
    }
    if bios_affine {
        bus = bus.with_affine_policy(gba_native::bios::AffinePolicy::Documented);
    }
    let used = interpret::run_budget(&mut cpu, &mut bus, budget)?;
    let output = if let Some(path) = snapshot_output {
        let image = display::snapshot(&bus)?;
        Some(run_output::write_snapshot(
            &path,
            bus.cart(),
            &cpu,
            &bus,
            budget,
            used,
            &image,
        )?)
    } else {
        None
    };
    println!("status=bounded");
    println!("instructions={used}");
    println!("pc=0x{:08X}", cpu.pc());
    println!("cpsr=0x{:08X}", cpu.cpsr());
    println!("thumb={}", if cpu.thumb() { 1 } else { 0 });
    println!("bios_data_policy={}", bus.bios_data_policy().name());
    println!("bios_write_policy={}", bus.bios_write_policy().name());
    println!("rom_write_policy={}", bus.rom_write_policy().name());
    println!("backup_policy={}", bus.backup_policy().name());
    println!("decompression_policy={}", bus.decompression_policy().name());
    println!("affine_policy={}", bus.affine_policy().name());
    if bios_rom_writes {
        println!(
            "bios_write_assumption=experimental documented mask-ROM no-effect writes; not BIOS data, opcodes or a compatibility claim"
        );
    }
    if backup_sram || backup_sram64k {
        let capacity = if backup_sram64k { "64" } else { "32" };
        println!(
            "backup_assumption=experimental volatile {capacity} KiB SRAM; no flash engine and no persistence"
        );
    }
    if bios_decompress {
        println!(
            "decompression_assumption=experimental documented LZ77/RL/diff HLE from public format descriptions; no BIOS bytes or code; not a general BIOS implementation"
        );
    }
    if bios_affine {
        println!(
            "affine_assumption=experimental documented ObjAffineSet from the public interface and mathematical affine formulas; no BIOS bytes or code"
        );
    }
    if rom_writes {
        println!(
            "rom_write_assumption=experimental ROM-only cartridge no-effect stores; flash command engines are not modelled"
        );
    }
    if synthetic_bios_zero_fill {
        println!(
            "bios_assumption=experimental synthetic zero fill; not authentic BIOS data, opcodes or hardware behavior"
        );
    }
    println!("note=stopped only by the instruction budget; no boot or compatibility claim");
    if let Some(output) = output {
        println!("snapshot_dir={}", output.display());
    }
    Ok(())
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
