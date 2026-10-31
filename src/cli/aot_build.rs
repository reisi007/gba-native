//! Experimental headless AOT artifact commands.
//!
//! Native targets compile a ROM-bound release executable. The dedicated
//! `wasm32-unknown-unknown` branch composes the target-neutral program section
//! with a wasm-bindgen `run_rom(Uint8Array)` driver, builds a fresh cdylib
//! offline and post-processes it with the repository-owned wasm-bindgen CLI.
//! Neither branch executes guest code, copies ROM bytes or claims playability,
//! a GUI, save support, a browser app or an HTML/JS frontend.

use std::path::{Path, PathBuf};

use gba_native::aot::{self, HarnessPolicy, NativeHarness};
use gba_native::bus::BackupPolicy;
use gba_native::cart::{Cartridge, ROM_BASE};
use gba_native::error::{Error, Result};

use super::output::{create_fresh_directory, create_unique_workspace, write_safely};

const DEFAULT_BUDGET: u64 = 1_000_000;

/// Parsed `aot build` request. The ROM stays external and read-only.
struct BuildRequest {
    rom: String,
    target: String,
    out: Option<PathBuf>,
    budget: u64,
    trace: Option<PathBuf>,
    policy: HarnessPolicy,
}

pub(super) fn run(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let rom = args
        .next()
        .ok_or_else(|| Error::Args("aot build needs a ROM path".to_string()))?;
    let mut target: Option<String> = None;
    let mut out: Option<PathBuf> = None;
    let mut budget = DEFAULT_BUDGET;
    let mut trace = None;
    let mut policy = HarnessPolicy::default();
    let mut backup_flags = 0u8;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--target" => {
                target = Some(
                    args.next()
                        .ok_or_else(|| Error::Args("--target needs a triple".to_string()))?,
                );
            }
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    Error::Args("--out needs a directory".to_string())
                })?));
            }
            "--budget" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--budget needs a count".to_string()))?;
                budget = value
                    .parse()
                    .map_err(|_| Error::Args(format!("invalid budget {value}")))?;
            }
            "--trace" => {
                trace =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        Error::Args("--trace needs a path".to_string())
                    })?));
            }
            "--synthetic-bios-zero-fill" => policy.bios_zero_fill = true,
            "--synthetic-bios-rom-writes" => policy.bios_rom_writes = true,
            "--synthetic-rom-writes-ignored" => policy.rom_writes_dropped = true,
            "--synthetic-backup-sram" => {
                policy.backup = BackupPolicy::Sram32KVolatile;
                backup_flags += 1;
            }
            "--synthetic-backup-sram64k" => {
                policy.backup = BackupPolicy::Sram64KVolatile;
                backup_flags += 1;
            }
            "--synthetic-backup-flash128k" => {
                policy.backup = BackupPolicy::Flash128KVolatile;
                backup_flags += 1;
            }
            "--synthetic-bios-decompress" => policy.decompression = true,
            "--synthetic-bios-affine" => policy.affine = true,
            _ => return Err(Error::Args(format!("unknown aot build option {argument}"))),
        }
    }
    if backup_flags > 1 {
        return Err(Error::Args(
            "choose only one backup device: --synthetic-backup-sram, --synthetic-backup-sram64k or --synthetic-backup-flash128k".to_string(),
        ));
    }
    let target = target.ok_or_else(|| {
        Error::Args(
            "aot build requires --target <triple> (for example aarch64-apple-darwin or wasm32-unknown-unknown)"
                .to_string(),
        )
    })?;
    // Validate the target before loading the ROM or creating any output, so an
    // argument failure leaves neither a workspace nor an outside directory.
    aot::validate_target(&target)
        .map_err(|reason| Error::Args(format!("invalid --target {target:?}: {reason}")))?;
    aot::validate_build_target(&target)?;

    let request = BuildRequest {
        rom,
        target,
        out,
        budget,
        trace,
        policy,
    };
    if request.target == aot::WASM_TARGET {
        build_wasm_module(request)
    } else {
        build_native_artifact(request)
    }
}

/// Native branch: unchanged headless release executable for one Cargo triple.
fn build_native_artifact(request: BuildRequest) -> Result<()> {
    // The original ROM stays external and read-only. The bounded loader reads
    // it once for discovery/emission; the generated artifact binds SHA-256 and
    // length and re-reads a caller-supplied path at runtime.
    let cart = Cartridge::from_path(Path::new(&request.rom))?;
    let artifact = match &request.trace {
        Some(path) => {
            let text = std::fs::read_to_string(path)
                .map_err(|error| Error::Args(format!("cannot read --trace {path:?}: {error}")))?;
            let seeds = aot::parse_trace_seeds(&text)?;
            aot::trace_artifact(&cart, ROM_BASE, false, &seeds)?
        }
        None => aot::discover(&cart, ROM_BASE, false, aot::Limits::default())?,
    };
    let source =
        aot::emit_native_module_with_policy(&cart, &artifact, request.budget, request.policy)?;

    // Every run owns a fresh workspace: the default allocates a unique
    // directory, and an explicit --out must be a new path below output/.
    let work = match request.out {
        Some(path) => create_fresh_directory(&path)?,
        None => create_unique_workspace("native-aot-app")?,
    };
    let generated = work.join("generated.rs");
    write_safely(&generated, source.as_bytes())?;

    // The harness crate is another fresh child. Its own target directory keeps
    // the release build offline and independent of the parent Cargo lock.
    let harness = NativeHarness::build_for_target(&source, &work.join("native"), &request.target)?;

    let facts = aot::ToolchainFacts::detect();
    let artifact_meta = aot::artifact_metadata(
        &artifact,
        &request.target,
        request.budget,
        &source,
        harness.executable(),
    )?;
    let build_meta = aot::build_metadata(&request.target, &facts);
    let artifact_meta_path = work.join("artifact.meta");
    let build_meta_path = work.join("build.meta");
    write_safely(&artifact_meta_path, artifact_meta.as_bytes())?;
    write_safely(&build_meta_path, build_meta.as_bytes())?;

    println!("status=built");
    println!("target={}", request.target);
    println!("rom_sha256={}", artifact.rom_sha256_hex());
    println!("rom_bytes={}", artifact.rom_len);
    println!("entry=0x{:08X}", artifact.entry);
    println!(
        "entry_state={}",
        if artifact.entry_thumb { "thumb" } else { "arm" }
    );
    println!("budget={}", request.budget);
    println!("blocks={}", artifact.stats.blocks);
    println!("translated_instructions={}", artifact.stats.instructions);
    println!("sealed_blocks={}", artifact.stats.sealed_blocks);
    println!("rejected_seeds={}", artifact.stats.rejected_seeds);
    println!("bios_data_policy={}", request.policy.bios_zero_fill as u8);
    println!("bios_write_policy={}", request.policy.bios_rom_writes as u8);
    println!(
        "rom_write_policy={}",
        request.policy.rom_writes_dropped as u8
    );
    println!("backup_policy={}", request.policy.backup.name());
    println!(
        "decompression_policy={}",
        request.policy.decompression as u8
    );
    println!("affine_policy={}", request.policy.affine as u8);
    println!("generated_dir={}", work.display());
    println!("generated_source={}", generated.display());
    println!("executable={}", harness.executable().display());
    println!("artifact_meta={}", artifact_meta_path.display());
    println!("build_meta={}", build_meta_path.display());
    println!("embedded_rom_bytes=0");
    println!("requires_external_rom=true");
    println!("app=prototype-not-playable");
    println!(
        "note=experimental headless native AOT artifact from bounded discovery; requires --rom PATH at runtime; not a playable standalone game, GUI frontend or save-aware app; automated tests use synthetic fixtures and building does not execute guest code"
    );
    Ok(())
}

/// WASM branch: ROM-bound execution module plus importable JS glue.
fn build_wasm_module(request: BuildRequest) -> Result<()> {
    if request.trace.is_some() {
        return Err(Error::Args(
            "--trace is supported only for native targets".to_string(),
        ));
    }
    // Resolve and verify the pinned wasm-bindgen CLI before reading the ROM or
    // creating output, so a missing/mismatched tool leaves no side effects.
    let bindgen = aot::wasm_bindgen_cli()?;

    let cart = Cartridge::from_path(Path::new(&request.rom))?;
    let artifact = aot::discover(&cart, ROM_BASE, false, aot::Limits::default())?;
    let source =
        aot::emit_wasm_module_with_policy(&cart, &artifact, request.budget, request.policy)?;

    let work = match request.out {
        Some(path) => create_fresh_directory(&path)?,
        None => create_unique_workspace("wasm-aot-module")?,
    };
    let generated = work.join("generated.rs");
    write_safely(&generated, source.as_bytes())?;

    let module = aot::WasmModule::build_with_bindgen(&source, &work.join("wasm"), &bindgen)?;
    let module_bytes = module.module_bytes()?;
    let glue_bytes = module.glue_bytes()?;

    let facts = aot::ToolchainFacts::detect();
    let artifact_meta = aot::wasm_artifact_metadata(
        &artifact,
        request.budget,
        &source,
        &module_bytes,
        &glue_bytes,
        aot::WASM_BINDGEN_VERSION,
    );
    let build_meta = aot::wasm_build_metadata(&facts, aot::WASM_BINDGEN_VERSION);
    let readme = aot::wasm_readme(&artifact, request.budget);
    let artifact_meta_path = work.join("artifact.meta");
    let build_meta_path = work.join("build.meta");
    let readme_path = work.join("README.md");
    write_safely(&artifact_meta_path, artifact_meta.as_bytes())?;
    write_safely(&build_meta_path, build_meta.as_bytes())?;
    write_safely(&readme_path, readme.as_bytes())?;

    println!("status=built");
    println!("artifact_kind=wasm-module");
    println!("target={}", request.target);
    println!("wasm_bindgen_version={}", aot::WASM_BINDGEN_VERSION);
    println!("rom_sha256={}", artifact.rom_sha256_hex());
    println!("rom_bytes={}", artifact.rom_len);
    println!("entry=0x{:08X}", artifact.entry);
    println!(
        "entry_state={}",
        if artifact.entry_thumb { "thumb" } else { "arm" }
    );
    println!("budget={}", request.budget);
    println!("blocks={}", artifact.stats.blocks);
    println!("translated_instructions={}", artifact.stats.instructions);
    println!("sealed_blocks={}", artifact.stats.sealed_blocks);
    println!("rejected_seeds={}", artifact.stats.rejected_seeds);
    println!("bios_data_policy={}", request.policy.bios_zero_fill as u8);
    println!("bios_write_policy={}", request.policy.bios_rom_writes as u8);
    println!(
        "rom_write_policy={}",
        request.policy.rom_writes_dropped as u8
    );
    println!("backup_policy={}", request.policy.backup.name());
    println!(
        "decompression_policy={}",
        request.policy.decompression as u8
    );
    println!("affine_policy={}", request.policy.affine as u8);
    println!("generated_dir={}", work.display());
    println!("generated_source={}", generated.display());
    println!("wasm_module={}", module.wasm().display());
    println!("js_glue={}", module.js().display());
    println!("artifact_meta={}", artifact_meta_path.display());
    println!("build_meta={}", build_meta_path.display());
    println!("readme={}", readme_path.display());
    println!("embedded_rom_bytes=0");
    println!("requires_external_rom=true");
    println!("app=prototype-not-playable");
    println!(
        "note=experimental ROM-bound WASM execution module (run_rom takes a Uint8Array and checks SHA-256+length); browser-callable module only, not a playable game, browser app/HTML bundle, GPU frontend, input, persistence or final frontend; automated tests use synthetic fixtures and building does not execute guest code"
    );
    Ok(())
}
