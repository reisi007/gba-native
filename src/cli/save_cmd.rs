//! Host save-library commands: list, import, export, use, delete.

use std::path::PathBuf;

use gba_native::cart::Cartridge;
use gba_native::error::{Error, Result};
use gba_native::save::SaveLibrary;

const USAGE: &str = "\
usage:
  gba-native save list <rom.gba> [--dir output/saves]
  gba-native save import <rom.gba> <slot> --file <save> [--dir output/saves]
  gba-native save export <rom.gba> <slot> --out <save> [--dir output/saves]
  gba-native save use <rom.gba> <slot> [--dir output/saves]
  gba-native save default <rom.gba> [--dir output/saves]
  gba-native save delete <rom.gba> <slot> [--dir output/saves]";

pub(super) fn run(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let command = args.next().ok_or_else(|| Error::Args(USAGE.to_string()))?;
    let path = args
        .next()
        .ok_or_else(|| Error::Args(format!("save {command} needs a ROM path\n{USAGE}")))?;
    let mut slot = None;
    let mut dir = PathBuf::from("output/saves");
    let mut file = None;
    let mut out = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--dir" => {
                dir = PathBuf::from(
                    args.next()
                        .ok_or_else(|| Error::Args("--dir needs a path".to_string()))?,
                );
            }
            "--file" => {
                file =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        Error::Args("--file needs a path".to_string())
                    })?));
            }
            "--out" => {
                out =
                    Some(PathBuf::from(args.next().ok_or_else(|| {
                        Error::Args("--out needs a path".to_string())
                    })?));
            }
            value if !value.starts_with("--") && slot.is_none() => {
                slot = Some(value.to_string());
            }
            _ => {
                return Err(Error::Args(format!(
                    "unknown save option {argument}\n{USAGE}"
                )));
            }
        }
    }
    let cart = Cartridge::from_path(std::path::Path::new(&path))?;
    let digest = cart.sha256_hex();
    let library = SaveLibrary::new(&dir);
    match command.as_str() {
        "list" => {
            println!("rom_sha256={digest}");
            println!("save_dir={}", dir.display());
            let slots = library.list(&digest)?;
            let default = library.default_slot(&digest)?;
            for name in &slots {
                let bytes = library.load(&digest, name)?;
                let marker = if default.as_deref() == Some(name) {
                    " default=1"
                } else {
                    " default=0"
                };
                println!(
                    "slot={name} bytes={} sha256={}{marker}",
                    bytes.len(),
                    gba_native::sha256::hex(&gba_native::sha256::digest(&bytes))
                );
            }
            println!("slots={}", slots.len());
            Ok(())
        }
        "import" => {
            let slot = required_slot(slot)?;
            let file = file.ok_or_else(|| Error::Args("save import needs --file".to_string()))?;
            let bytes = std::fs::read(&file).map_err(|error| {
                Error::Args(format!("cannot read save {}: {error}", file.display()))
            })?;
            library.store(&digest, &slot, &bytes)?;
            println!("slot={slot}");
            println!("bytes={}", bytes.len());
            println!(
                "sha256={}",
                gba_native::sha256::hex(&gba_native::sha256::digest(&bytes))
            );
            println!(
                "default={}",
                library.default_slot(&digest)?.as_deref().unwrap_or("")
            );
            Ok(())
        }
        "export" => {
            let slot = required_slot(slot)?;
            let out = out.ok_or_else(|| Error::Args("save export needs --out".to_string()))?;
            let bytes = library.load(&digest, &slot)?;
            std::fs::write(&out, &bytes).map_err(|error| {
                Error::Args(format!("cannot write save {}: {error}", out.display()))
            })?;
            println!("slot={slot}");
            println!("out={}", out.display());
            println!("bytes={}", bytes.len());
            println!(
                "sha256={}",
                gba_native::sha256::hex(&gba_native::sha256::digest(&bytes))
            );
            Ok(())
        }
        "use" => {
            let slot = required_slot(slot)?;
            library.set_default(&digest, &slot)?;
            println!("default={slot}");
            Ok(())
        }
        "default" => {
            println!(
                "default={}",
                library.default_slot(&digest)?.as_deref().unwrap_or("")
            );
            Ok(())
        }
        "delete" => {
            let slot = required_slot(slot)?;
            library.delete(&digest, &slot)?;
            println!("deleted={slot}");
            println!(
                "default={}",
                library.default_slot(&digest)?.as_deref().unwrap_or("")
            );
            Ok(())
        }
        _ => Err(Error::Args(format!(
            "unknown save command {command}\n{USAGE}"
        ))),
    }
}

fn required_slot(slot: Option<String>) -> Result<String> {
    slot.ok_or_else(|| Error::Args(format!("missing save slot\n{USAGE}")))
}
