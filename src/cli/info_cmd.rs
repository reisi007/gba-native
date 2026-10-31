//! Read-only cartridge header report.

use std::path::Path;

use gba_native::cart::Cartridge;
use gba_native::error::Result;

pub(super) fn run(path: &Path) -> Result<()> {
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
