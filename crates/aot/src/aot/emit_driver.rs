use std::fmt::Write as _;

use super::artifact::AotArtifact;
use super::policy::HarnessPolicy;
use gba_native::error::Result;

/// Emit the native harness driver around the target-neutral program section.
///
/// The driver owns the host boundary: argument parsing, external ROM loading,
/// digest+length binding, the state report and exit codes. The program section
/// owns translated blocks and fallback dispatch. Known blocks are direct calls;
/// only undiscovered targets use the reference interpreter.
pub(super) fn emit_driver(
    out: &mut String,
    artifact: &AotArtifact,
    budget: u64,
    policy: HarnessPolicy,
) -> Result<()> {
    writeln!(out).unwrap();
    writeln!(
        out,
        "// Native harness driver: host boundary only. Arguments, external ROM"
    )
    .unwrap();
    writeln!(
        out,
        "// loading, digest+length binding, reporting and exit codes live here."
    )
    .unwrap();
    writeln!(out, "use gba_native::cart::Cartridge;").unwrap();
    writeln!(out, "use gba_native::display;").unwrap();
    writeln!(out, "use gba_native::error::Error;").unwrap();
    writeln!(out, "use gba_native::sha256;").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "const ROM_SHA256: &str = \"{}\";",
        artifact.rom_sha256_hex()
    )
    .unwrap();
    writeln!(out, "const BUDGET: u64 = {budget};").unwrap();
    // The artifact binds the ROM by digest and length only. The generated
    // source deliberately contains no ROM byte array and no raw copy of the
    // input image; the runtime reads a caller-supplied path.
    writeln!(out, "const ROM_LEN: usize = {};", artifact.rom_len).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "fn main() {{").unwrap();
    writeln!(out, "    if let Err(error) = run() {{").unwrap();
    writeln!(out, "        eprintln!(\"error={{error}}\");").unwrap();
    writeln!(out, "        let code = match error {{").unwrap();
    writeln!(out, "            Error::RomMismatch {{ .. }} => 3,").unwrap();
    writeln!(out, "            _ => 1,").unwrap();
    writeln!(out, "        }};").unwrap();
    writeln!(out, "        std::process::exit(code);").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "fn verify_binding(cart: &Cartridge) -> Result<()> {{").unwrap();
    writeln!(out, "    let actual = cart.sha256_hex();").unwrap();
    writeln!(
        out,
        "    if actual != ROM_SHA256 || cart.len() != ROM_LEN {{"
    )
    .unwrap();
    writeln!(out, "        return Err(Error::RomMismatch {{").unwrap();
    writeln!(out, "            expected: ROM_SHA256.to_string(),").unwrap();
    writeln!(out, "            actual,").unwrap();
    writeln!(out, "        }});").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    Ok(())").unwrap();
    writeln!(out, "}}").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "fn run() -> Result<()> {{").unwrap();
    writeln!(
        out,
        "    let args: Vec<String> = std::env::args().skip(1).collect();"
    )
    .unwrap();
    writeln!(out, "    if args.len() != 2 || args[0] != \"--rom\" {{").unwrap();
    writeln!(
        out,
        "        return Err(Error::Args(\"usage: harness --rom PATH (the ROM is not embedded)\".to_string()));"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(
        out,
        "    let cart = Cartridge::from_path(std::path::Path::new(&args[1]))?;"
    )
    .unwrap();
    writeln!(out, "    verify_binding(&cart)?;").unwrap();
    if policy.bios_zero_fill {
        writeln!(
            out,
            "    let mut bus = Bus::with_synthetic_bios_zero_fill(cart);"
        )
        .unwrap();
    } else {
        writeln!(out, "    let mut bus = Bus::new(cart);").unwrap();
    }
    if policy.bios_rom_writes {
        writeln!(
            out,
            "    bus = bus.with_bios_write_policy(gba_native::bus::BiosWritePolicy::RomWritesDropped);"
        )
        .unwrap();
    }
    if policy.rom_writes_dropped {
        writeln!(
            out,
            "    bus = bus.with_rom_write_policy(gba_native::bus::RomWritePolicy::Dropped);"
        )
        .unwrap();
    }
    if policy.backup != gba_native::bus::BackupPolicy::Strict {
        writeln!(
            out,
            "    bus = bus.with_backup_policy(gba_native::bus::BackupPolicy::{});",
            backup_variant(policy)
        )
        .unwrap();
    }
    if policy.decompression {
        writeln!(
            out,
            "    bus = bus.with_decompression_policy(gba_native::bios::DecompressionPolicy::DocumentedFormats);"
        )
        .unwrap();
    }
    if policy.affine {
        writeln!(
            out,
            "    bus = bus.with_affine_policy(gba_native::bios::AffinePolicy::Documented);"
        )
        .unwrap();
    }
    writeln!(out, "    let mut cpu = Cpu::direct_entry();").unwrap();
    writeln!(out, "    cpu.initialize_bios_stacks();").unwrap();
    writeln!(out, "    cpu.set_pc(ENTRY);").unwrap();
    writeln!(out, "    cpu.set_thumb(ENTRY_THUMB);").unwrap();
    writeln!(
        out,
        "    let program = run_program(&mut cpu, &mut bus, BUDGET)?;"
    )
    .unwrap();
    writeln!(out, "    let image = display::snapshot(&bus)?;").unwrap();
    writeln!(out, "    println!(\"status=ok\");").unwrap();
    writeln!(
        out,
        "    println!(\"bios_data_policy={{}}\", bus.bios_data_policy().name());"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"bios_write_policy={{}}\", bus.bios_write_policy().name());"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"rom_write_policy={{}}\", bus.rom_write_policy().name());"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"backup_policy={{}}\", bus.backup_policy().name());"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"decompression_policy={{}}\", bus.decompression_policy().name());"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"affine_policy={{}}\", bus.affine_policy().name());"
    )
    .unwrap();
    writeln!(out, "    println!(\"pc=0x{{:08X}}\", cpu.pc());").unwrap();
    writeln!(out, "    println!(\"cpsr=0x{{:08X}}\", cpu.cpsr());").unwrap();
    writeln!(
        out,
        "    println!(\"thumb={{}}\", if cpu.thumb() {{ 1 }} else {{ 0 }});"
    )
    .unwrap();
    writeln!(out, "    for index in 0..16u8 {{").unwrap();
    writeln!(
        out,
        "        println!(\"r{{}}=0x{{:08X}}\", index, cpu.reg(index));"
    )
    .unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(
        out,
        "    println!(\"budget_remaining={{}}\", program.budget_remaining);"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"budget_used={{}}\", BUDGET - program.budget_remaining);"
    )
    .unwrap();
    writeln!(out, "    println!(\"fallbacks={{}}\", program.fallbacks);").unwrap();
    writeln!(
        out,
        "    println!(\"cpu_sha256={{}}\", sha256::hex(&cpu.state_digest()));"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"rom_sha256={{}}\", bus.cart().sha256_hex());"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"vram_sha256={{}}\", sha256::hex(&bus.vram_digest()));"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"bus_sha256={{}}\", sha256::hex(&bus.state_digest()));"
    )
    .unwrap();
    writeln!(out, "    println!(\"image_width={{}}\", image.width);").unwrap();
    writeln!(out, "    println!(\"image_height={{}}\", image.height);").unwrap();
    writeln!(
        out,
        "    println!(\"image_sha256={{}}\", image.sha256_hex());"
    )
    .unwrap();
    writeln!(out, "    Ok(())").unwrap();
    writeln!(out, "}}").unwrap();
    Ok(())
}

fn backup_variant(policy: HarnessPolicy) -> &'static str {
    use gba_native::bus::BackupPolicy;
    match policy.backup {
        BackupPolicy::Strict => "Strict",
        BackupPolicy::Sram32KVolatile => "Sram32KVolatile",
        BackupPolicy::Sram64KVolatile => "Sram64KVolatile",
        BackupPolicy::Flash128KVolatile => "Flash128KVolatile",
    }
}
