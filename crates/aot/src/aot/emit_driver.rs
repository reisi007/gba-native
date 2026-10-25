use std::fmt::Write as _;

use super::artifact::AotArtifact;
use super::emit::block_name;
use gba_native::error::Result;

/// Emit the harness entry point, ROM binding, dispatch loop and state report.
/// Known blocks are direct calls; only undiscovered targets use the reference
/// interpreter.
pub(super) fn emit_driver(out: &mut String, artifact: &AotArtifact) -> Result<()> {
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
    writeln!(out, "    let mut bus = Bus::new(cart);").unwrap();
    writeln!(out, "    let mut cpu = Cpu::direct_entry();").unwrap();
    writeln!(out, "    cpu.set_pc(ENTRY);").unwrap();
    writeln!(out, "    cpu.set_thumb(ENTRY_THUMB);").unwrap();
    writeln!(out, "    let mut budget = BUDGET;").unwrap();
    writeln!(out, "    let mut fallbacks = 0u64;").unwrap();
    writeln!(out, "    while budget > 0 {{").unwrap();
    writeln!(out, "        let next = match (cpu.pc(), cpu.thumb()) {{").unwrap();
    for block in &artifact.blocks {
        writeln!(
            out,
            "            (0x{:08X}, {}) => {}(&mut cpu, &mut bus, &mut budget)?,",
            block.addr,
            block.thumb,
            block_name(block.addr, block.thumb)
        )
        .unwrap();
    }
    writeln!(out, "            _ => {{").unwrap();
    writeln!(out, "                interpret::step(&mut cpu, &mut bus)?;").unwrap();
    writeln!(out, "                budget -= 1;").unwrap();
    writeln!(out, "                fallbacks += 1;").unwrap();
    writeln!(out, "                Next::Continue").unwrap();
    writeln!(out, "            }}").unwrap();
    writeln!(out, "        }};").unwrap();
    writeln!(out, "        if next == Next::Halt {{").unwrap();
    writeln!(out, "            break;").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    let image = display::snapshot(&bus)?;").unwrap();
    writeln!(out, "    println!(\"status=ok\");").unwrap();
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
    writeln!(out, "    println!(\"budget_remaining={{}}\", budget);").unwrap();
    writeln!(out, "    println!(\"budget_used={{}}\", BUDGET - budget);").unwrap();
    writeln!(out, "    println!(\"fallbacks={{}}\", fallbacks);").unwrap();
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
