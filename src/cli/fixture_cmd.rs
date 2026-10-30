//! Synthetic fixture writer.

use std::path::PathBuf;

use gba_native::demo;
use gba_native::error::{Error, Result};

use super::output::write_safely;

pub(super) fn run(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let kind = args
        .next()
        .ok_or_else(|| Error::Args("fixture needs a kind: input-tester".to_string()))?;
    if kind != "input-tester" {
        return Err(Error::Args(format!("unknown fixture {kind:?}")));
    }
    let mut out = None;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" => {
                out = Some(PathBuf::from(args.next().ok_or_else(|| {
                    Error::Args("--out needs a file path under output/".to_string())
                })?));
            }
            _ => return Err(Error::Args(format!("unknown fixture option {argument}"))),
        }
    }
    let out = out.ok_or_else(|| {
        Error::Args("fixture input-tester needs --out output/<name>.gba".to_string())
    })?;
    let cart = demo::input_tester_cartridge();
    let bytes = cart.bytes();
    write_safely(&out, bytes)?;
    let image = demo::expected_input_tester_image(demo::INPUT_TESTER_SAMPLE_MASK);
    println!("fixture=input-tester");
    println!("file={}", out.display());
    println!("bytes={}", bytes.len());
    println!("sha256={}", cart.sha256_hex());
    println!("entry=0x{:08X}", demo::INPUT_TESTER_ENTRY);
    println!("budget={}", demo::INPUT_TESTER_BUDGET);
    println!("sample_input_mask=0x{:04X}", demo::INPUT_TESTER_SAMPLE_MASK);
    println!("sample_image_sha256={}", image.sha256_hex());
    println!("note=synthetic fixture only; no retail ROM, BIOS or save data");
    Ok(())
}
