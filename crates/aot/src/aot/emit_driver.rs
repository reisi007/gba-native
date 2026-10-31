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
    writeln!(out, "    let mut rom: Option<&str> = None;").unwrap();
    writeln!(out, "    let mut input: Option<&str> = None;").unwrap();
    writeln!(out, "    let mut snapshot: Option<&str> = None;").unwrap();
    writeln!(out, "    let mut audio: Option<&str> = None;").unwrap();
    writeln!(out, "    let mut save: Option<&str> = None;").unwrap();
    writeln!(out, "    let mut volume: u8 = 80;").unwrap();
    writeln!(out, "    let mut muted = false;").unwrap();
    writeln!(out, "    let mut index = 0usize;").unwrap();
    writeln!(out, "    while index < args.len() {{").unwrap();
    writeln!(out, "        match args[index].as_str() {{").unwrap();
    writeln!(out, "            \"--rom\" => {{ index += 1; rom = Some(args.get(index).ok_or_else(|| Error::Args(\"--rom needs a path\".to_string()))?); }}").unwrap();
    writeln!(out, "            \"--input\" => {{ index += 1; input = Some(args.get(index).ok_or_else(|| Error::Args(\"--input needs a script path\".to_string()))?); }}").unwrap();
    writeln!(out, "            \"--snapshot\" => {{ index += 1; snapshot = Some(args.get(index).ok_or_else(|| Error::Args(\"--snapshot needs a PPM path\".to_string()))?); }}").unwrap();
    writeln!(out, "            \"--audio\" => {{ index += 1; audio = Some(args.get(index).ok_or_else(|| Error::Args(\"--audio needs a WAV path\".to_string()))?); }}").unwrap();
    writeln!(out, "            \"--save\" => {{ index += 1; save = Some(args.get(index).ok_or_else(|| Error::Args(\"--save needs a raw backup path\".to_string()))?); }}").unwrap();
    writeln!(out, "            \"--volume\" => {{ index += 1; volume = args.get(index).ok_or_else(|| Error::Args(\"--volume needs 0..=100\".to_string()))?.parse().map_err(|_| Error::Args(\"invalid --volume\".to_string()))?; if volume > 100 {{ return Err(Error::Args(\"--volume must be 0..=100\".to_string())); }} }}").unwrap();
    writeln!(out, "            \"--mute\" => muted = true,").unwrap();
    writeln!(
        out,
        "            other => return Err(Error::Args(format!(\"usage: harness --rom PATH [--input SCRIPT] [--snapshot PPM] [--audio WAV] [--save BACKUP] [--volume 0..100] [--mute] (the ROM is not embedded); unknown option {{other}}\"))),"
    )
    .unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "        index += 1;").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(
        out,
        "    let rom = rom.ok_or_else(|| Error::Args(\"usage: harness --rom PATH [--input SCRIPT] (the ROM is not embedded)\".to_string()))?;"
    )
    .unwrap();
    writeln!(
        out,
        "    let cart = Cartridge::from_path(std::path::Path::new(rom))?;"
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
    writeln!(out, "    let save_path = save;").unwrap();
    writeln!(out, "    if let Some(path) = save_path {{").unwrap();
    writeln!(out, "        if std::path::Path::new(path).exists() {{").unwrap();
    writeln!(out, "            let bytes = std::fs::read(path).map_err(|error| Error::Args(format!(\"cannot read save {{path}}: {{error}}\")))?;").unwrap();
    writeln!(out, "            bus.load_backup(&bytes)?;").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    let script = match input {{").unwrap();
    writeln!(out, "        Some(path) => {{").unwrap();
    writeln!(out, "            let text = std::fs::read_to_string(path).map_err(|error| Error::Args(format!(\"cannot read input script {{path}}: {{error}}\")))?;").unwrap();
    writeln!(out, "            gba_native::input::InputScript::parse(&text).map_err(|error| Error::Args(format!(\"invalid input script: {{error}}\")))?").unwrap();
    writeln!(out, "        }}").unwrap();
    writeln!(
        out,
        "        None => gba_native::input::InputScript::default(),"
    )
    .unwrap();
    writeln!(out, "    }};").unwrap();
    writeln!(out, "    let audio_path = audio;").unwrap();
    writeln!(out, "    let mut audio_engine = audio_path.map(|_| gba_native::audio::AudioEngine::new(gba_native::audio::AudioSettings::new(volume, muted)));").unwrap();
    writeln!(out, "    let started = std::time::Instant::now();").unwrap();
    writeln!(
        out,
        "    let program = run_program(&mut cpu, &mut bus, BUDGET, &script, &mut audio_engine)?;"
    )
    .unwrap();
    writeln!(out, "    let image = display::snapshot(&bus)?;").unwrap();
    writeln!(out, "    if let Some(path) = snapshot {{").unwrap();
    writeln!(out, "        std::fs::write(path, gba_native::render::to_ppm(&image)).map_err(|error| Error::Args(format!(\"cannot write snapshot {{path}}: {{error}}\")))?;").unwrap();
    writeln!(out, "        println!(\"snapshot={{}}\", path);").unwrap();
    writeln!(out, "    }}").unwrap();
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
        "    println!(\"irq_dispatch_returns={{}}\", program.irq_returns);"
    )
    .unwrap();
    writeln!(
        out,
        "    println!(\"native_instructions={{}}\", BUDGET - program.budget_remaining - program.fallbacks - program.irq_returns);"
    )
    .unwrap();
    writeln!(
        out,
        "    if let (Some(path), Some(engine)) = (audio_path, audio_engine.as_ref()) {{"
    )
    .unwrap();
    writeln!(out, "        let wav = engine.wav();").unwrap();
    writeln!(out, "        std::fs::write(path, &wav).map_err(|error| Error::Args(format!(\"cannot write audio {{path}}: {{error}}\")))?;").unwrap();
    writeln!(out, "        println!(\"audio={{}}\", path);").unwrap();
    writeln!(
        out,
        "        println!(\"audio_sample_rate={{}}\", gba_native::audio::SAMPLE_RATE);"
    )
    .unwrap();
    writeln!(
        out,
        "        println!(\"audio_samples={{}}\", engine.samples().len());"
    )
    .unwrap();
    writeln!(out, "        println!(\"audio_master_volume={{volume}}\");").unwrap();
    writeln!(out, "        println!(\"audio_muted={{muted}}\");").unwrap();
    writeln!(out, "        println!(\"audio_audible={{}}\", !muted && engine.samples().iter().any(|sample| *sample != 0));").unwrap();
    writeln!(out, "        println!(\"audio_wav_sha256={{}}\", gba_native::sha256::hex(&gba_native::sha256::digest(&wav)));").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    if let Some(path) = save_path {{").unwrap();
    writeln!(out, "        let image = bus.backup_image();").unwrap();
    writeln!(out, "        std::fs::write(path, image).map_err(|error| Error::Args(format!(\"cannot write save {{path}}: {{error}}\")))?;").unwrap();
    writeln!(out, "        println!(\"save={{}}\", path);").unwrap();
    writeln!(out, "        println!(\"save_bytes={{}}\", image.len());").unwrap();
    writeln!(out, "        println!(\"save_sha256={{}}\", gba_native::sha256::hex(&gba_native::sha256::digest(image)));").unwrap();
    writeln!(out, "    }}").unwrap();
    writeln!(out, "    let stats = gba_native::perf::PerfStats::new(").unwrap();
    writeln!(out, "        BUDGET - program.budget_remaining,").unwrap();
    writeln!(out, "        program.irq_returns,").unwrap();
    writeln!(out, "        program.fallbacks,").unwrap();
    writeln!(out, "        started.elapsed(),").unwrap();
    writeln!(out, "    );").unwrap();
    writeln!(
        out,
        "    for line in stats.report_lines() {{ println!(\"{{line}}\"); }}"
    )
    .unwrap();
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
