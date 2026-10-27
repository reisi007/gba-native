//! Experimental headless native AOT artifact command.
//!
//! The command compiles an externally ROM-bound release executable for one
//! explicit Cargo target. It does not execute the artifact, copy ROM bytes or
//! claim playability, a GUI, save support or a WASM target.

use std::path::{Path, PathBuf};

use gba_native::aot::{self, NativeHarness};
use gba_native::cart::{Cartridge, ROM_BASE};
use gba_native::error::{Error, Result};

use super::output::{create_fresh_directory, create_unique_workspace, write_safely};

const DEFAULT_BUDGET: u64 = 1_000_000;

pub(super) fn run(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let rom = args
        .next()
        .ok_or_else(|| Error::Args("aot build needs a ROM path".to_string()))?;
    let mut target: Option<String> = None;
    let mut explicit_out: Option<PathBuf> = None;
    let mut budget = DEFAULT_BUDGET;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--target" => {
                target = Some(
                    args.next()
                        .ok_or_else(|| Error::Args("--target needs a triple".to_string()))?,
                );
            }
            "--out" => {
                explicit_out =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
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
            _ => return Err(Error::Args(format!("unknown aot build option {argument}"))),
        }
    }
    let target = target.ok_or_else(|| {
        Error::Args(
            "aot build requires --target <triple> (for example aarch64-apple-darwin)".to_string(),
        )
    })?;
    // Validate the target before loading the ROM or creating any output, so an
    // argument failure leaves neither a workspace nor an outside directory.
    aot::validate_target(&target)
        .map_err(|reason| Error::Args(format!("invalid --target {target:?}: {reason}")))?;

    // The original ROM stays external and read-only. The bounded loader reads
    // it once for discovery/emission; the generated artifact binds SHA-256 and
    // length and re-reads a caller-supplied path at runtime.
    let cart = Cartridge::from_path(Path::new(&rom))?;
    let artifact = aot::discover(&cart, ROM_BASE, false, aot::Limits::default())?;
    let source = aot::emit_native_module(&cart, &artifact, budget)?;

    // Every run owns a fresh workspace: the default allocates a unique
    // directory, and an explicit --out must be a new path below output/.
    let work = match explicit_out {
        Some(path) => create_fresh_directory(&path)?,
        None => create_unique_workspace("native-aot-app")?,
    };
    let generated = work.join("generated.rs");
    write_safely(&generated, source.as_bytes())?;

    // The harness crate is another fresh child. Its own target directory keeps
    // the release build offline and independent of the parent Cargo lock.
    let harness = NativeHarness::build_for_target(&source, &work.join("native"), &target)?;

    let facts = aot::ToolchainFacts::detect();
    let artifact_meta =
        aot::artifact_metadata(&artifact, &target, budget, &source, harness.executable())?;
    let build_meta = aot::build_metadata(&target, &facts);
    let artifact_meta_path = work.join("artifact.meta");
    let build_meta_path = work.join("build.meta");
    write_safely(&artifact_meta_path, artifact_meta.as_bytes())?;
    write_safely(&build_meta_path, build_meta.as_bytes())?;

    println!("status=built");
    println!("target={target}");
    println!("rom_sha256={}", artifact.rom_sha256_hex());
    println!("rom_bytes={}", artifact.rom_len);
    println!("entry=0x{:08X}", artifact.entry);
    println!(
        "entry_state={}",
        if artifact.entry_thumb { "thumb" } else { "arm" }
    );
    println!("budget={budget}");
    println!("blocks={}", artifact.stats.blocks);
    println!("translated_instructions={}", artifact.stats.instructions);
    println!("generated_dir={}", work.display());
    println!("generated_source={}", generated.display());
    println!("executable={}", harness.executable().display());
    println!("artifact_meta={}", artifact_meta_path.display());
    println!("build_meta={}", build_meta_path.display());
    println!("embedded_rom_bytes=0");
    println!("requires_external_rom=true");
    println!("app=prototype-not-playable");
    println!(
        "note=experimental headless native AOT artifact from bounded discovery; requires --rom PATH at runtime; not a playable standalone game, GUI frontend, save-aware app or browser/WASM build, and verification covers synthetic fixtures only"
    );
    Ok(())
}
