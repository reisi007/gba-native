//! Bounded reference run with optional input, audio and save persistence.

use std::path::{Path, PathBuf};
use std::time::Instant;

use gba_native::audio::AudioSettings;
use gba_native::bus::Bus;
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::display;
use gba_native::error::{Error, Result};
use gba_native::input::{InputScript, run_scripted, run_scripted_audio};
use gba_native::interpret;
use gba_native::perf::PerfStats;

use super::output::write_safely;
use super::play;
use super::run_output;

pub(super) fn run(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
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
    let mut backup_flash = false;
    let mut bios_decompress = false;
    let mut bios_affine = false;
    let mut input_script: Option<InputScript> = None;
    let mut audio_out: Option<PathBuf> = None;
    let mut audio_volume = AudioSettings::default().master_volume;
    let mut audio_muted = false;
    let mut audio_play = false;
    let mut save_dir: Option<PathBuf> = None;
    let mut save_slot: Option<String> = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--input" => {
                let path = PathBuf::from(
                    args.next()
                        .ok_or_else(|| Error::Args("--input needs a script path".to_string()))?,
                );
                let text = std::fs::read_to_string(&path).map_err(|error| {
                    Error::Args(format!(
                        "cannot read input script {}: {error}",
                        path.display()
                    ))
                })?;
                input_script = Some(
                    InputScript::parse(&text)
                        .map_err(|error| Error::Args(format!("invalid input script: {error}")))?,
                );
            }
            "--audio-out" => {
                audio_out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    Error::Args("--audio-out needs a WAV path".to_string())
                })?));
            }
            "--volume" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--volume needs 0..=100".to_string()))?;
                audio_volume = value
                    .parse()
                    .map_err(|_| Error::Args(format!("invalid volume {value}")))?;
                if audio_volume > 100 {
                    return Err(Error::Args("--volume must be 0..=100".to_string()));
                }
            }
            "--mute" => audio_muted = true,
            "--play" => audio_play = true,
            "--save-dir" => {
                save_dir = Some(PathBuf::from(args.next().ok_or_else(|| {
                    Error::Args("--save-dir needs a directory".to_string())
                })?));
            }
            "--save-slot" => {
                save_slot = Some(
                    args.next()
                        .ok_or_else(|| Error::Args("--save-slot needs a name".to_string()))?,
                );
            }
            "--synthetic-bios-zero-fill" => synthetic_bios_zero_fill = true,
            "--synthetic-bios-rom-writes" => bios_rom_writes = true,
            "--synthetic-rom-writes-ignored" => rom_writes = true,
            "--synthetic-backup-sram" => backup_sram = true,
            "--synthetic-backup-sram64k" => backup_sram64k = true,
            "--synthetic-backup-flash128k" => backup_flash = true,
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
    if audio_play && audio_out.is_none() {
        return Err(Error::Args(
            "--play needs --audio-out <wav>; nothing was played".to_string(),
        ));
    }
    if u8::from(backup_sram) + u8::from(backup_sram64k) + u8::from(backup_flash) > 1 {
        return Err(Error::Args(
            "choose only one backup device: --synthetic-backup-sram, --synthetic-backup-sram64k or --synthetic-backup-flash128k".to_string(),
        ));
    }
    let cart = Cartridge::from_path(Path::new(&path))?;
    let rom_digest = cart.sha256_hex();
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
    if backup_flash {
        bus = bus.with_backup_policy(gba_native::bus::BackupPolicy::Flash128KVolatile);
    } else if backup_sram64k {
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
    let save_context = if let Some(dir) = &save_dir {
        let library = gba_native::save::SaveLibrary::new(dir);
        let slot = match save_slot.clone() {
            Some(slot) => slot,
            None => library
                .default_slot(&rom_digest)
                .map_err(|error| Error::Args(error.to_string()))?
                .unwrap_or_else(|| "default".to_string()),
        };
        if library
            .exists(&rom_digest, &slot)
            .map_err(|error| Error::Args(error.to_string()))?
        {
            let bytes = library
                .load(&rom_digest, &slot)
                .map_err(|error| Error::Args(error.to_string()))?;
            bus.load_backup(&bytes)?;
        }
        Some((library, rom_digest.clone(), slot))
    } else {
        None
    };
    let started = Instant::now();
    let mut audio_engine = audio_out.as_ref().map(|_| {
        gba_native::audio::AudioEngine::new(AudioSettings::new(audio_volume, audio_muted))
    });
    let empty_script = InputScript::default();
    let script = input_script.as_ref().unwrap_or(&empty_script);
    let used = match audio_engine.as_mut() {
        Some(engine) => run_scripted_audio(&mut cpu, &mut bus, budget, script, engine)?,
        None => match &input_script {
            Some(script) => run_scripted(&mut cpu, &mut bus, budget, script)?,
            None => interpret::run_budget(&mut cpu, &mut bus, budget)?,
        },
    };
    let stats = PerfStats::new(used, 0, 0, started.elapsed());
    let save_written = if let Some((library, digest, slot)) = &save_context {
        let image = bus.backup_image();
        library
            .store(digest, slot, image)
            .map_err(|error| Error::Args(error.to_string()))?;
        Some((
            slot.clone(),
            image.len(),
            image.iter().filter(|byte| **byte != 0xFF).count(),
            gba_native::sha256::hex(&gba_native::sha256::digest(image)),
            library
                .default_slot(digest)
                .map_err(|error| Error::Args(error.to_string()))?
                .unwrap_or_default(),
        ))
    } else {
        None
    };
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
    let audio_written = if let (Some(path), Some(engine)) = (&audio_out, &audio_engine) {
        let wav = engine.wav();
        write_safely(path, &wav)?;
        let played = if audio_play {
            Some(play::play(path)?)
        } else {
            None
        };
        Some((
            path.clone(),
            engine.samples().len(),
            gba_native::sha256::hex(&gba_native::sha256::digest(&wav)),
            engine.samples().iter().any(|sample| *sample != 0),
            engine.audio_stats(),
            played,
        ))
    } else {
        None
    };
    println!("status=bounded");
    println!("instructions={used}");
    for line in stats.report_lines() {
        println!("{line}");
    }
    if let Some((slot, bytes, non_erased, digest, default)) = save_written {
        println!("save_slot={slot}");
        println!("save_bytes={bytes}");
        println!("save_non_erased_bytes={non_erased}");
        println!("save_sha256={digest}");
        println!("save_default={default}");
    }
    if let Some((path, samples, digest, audible, audio_stats, played)) = audio_written {
        println!("audio_out={}", path.display());
        println!("audio_sample_rate={}", gba_native::audio::SAMPLE_RATE);
        println!("audio_samples={samples}");
        println!("audio_master_volume={audio_volume}");
        println!("audio_muted={audio_muted}");
        println!("audio_audible={audible}");
        println!(
            "audio_direct_sound_samples={}",
            audio_stats.direct_sound_samples
        );
        println!(
            "audio_direct_sound_underruns={}",
            audio_stats.direct_sound_underruns
        );
        println!(
            "audio_direct_sound_dma_refills={}",
            audio_stats.direct_sound_dma_refills
        );
        println!("audio_wav_sha256={digest}");
        if let Some(player) = played {
            println!("audio_player={}", player.display());
            println!("audio_played=true");
        }
    }
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
    if backup_flash {
        println!(
            "backup_assumption=experimental volatile 128 KiB flash with documented autoselect/bank/program/erase subset; no persistence"
        );
    }
    if bios_decompress {
        println!(
            "decompression_assumption=experimental documented LZ77/RL/diff HLE from public format descriptions; no BIOS bytes or code; not a general BIOS implementation"
        );
    }
    if bios_affine {
        println!(
            "affine_assumption=experimental documented BgAffineSet/ObjAffineSet from the public interface and mathematical affine formulas; no BIOS bytes or code"
        );
    }
    if rom_writes {
        println!(
            "rom_write_assumption=experimental ROM-only cartridge no-effect stores; the backup window is governed by its own device policy"
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
