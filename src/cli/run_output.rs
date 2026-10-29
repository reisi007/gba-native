//! Optional output for a static bitmap captured after the bounded `run` command.

use std::path::{Path, PathBuf};

use gba_native::bus::{BiosDataPolicy, BiosWritePolicy, Bus, PALETTE_BASE};
use gba_native::cart::Cartridge;
use gba_native::cpu::Cpu;
use gba_native::display::Image;
use gba_native::error::Result;
use gba_native::{render, sha256};

use super::output::{create_fresh_directory, write_safely};

const IMAGE_FILE: &str = "screen.ppm";
const REPORT_FILE: &str = "report.txt";

pub(super) fn write_snapshot(
    path: &Path,
    cart: &Cartridge,
    cpu: &Cpu,
    bus: &Bus,
    requested_budget: u64,
    used_budget: u64,
    image: &Image,
) -> Result<PathBuf> {
    let ppm = render::to_ppm(image);
    let dispcnt = bus.dispcnt();
    let mode = dispcnt & 0x7;
    let backdrop = bus.read16(PALETTE_BASE)?;
    let stop_reason = if used_budget == requested_budget {
        "instruction_budget"
    } else {
        "execution_returned_before_requested_budget"
    };
    let bios_data_note = match bus.bios_data_policy() {
        BiosDataPolicy::Strict => "fail-closed; BIOS data reads are explicit errors",
        BiosDataPolicy::SyntheticZeroFill => {
            "experimental synthetic zero fill; not authentic BIOS data, opcodes or hardware behavior"
        }
    };
    let bios_write_note = match bus.bios_write_policy() {
        BiosWritePolicy::Strict => "fail-closed; BIOS stores are explicit errors",
        BiosWritePolicy::RomWritesDropped => {
            "experimental documented mask-ROM no-effect writes; no BIOS byte is stored, read or executed"
        }
    };
    let report = format!(
        "capture_kind=post_budget_bitmap_snapshot\n\
         rom_bytes={}\n\
         rom_sha256={}\n\
         bios_data_policy={}\n\
         bios_data_note={}\n\
         bios_write_policy={}\n\
         bios_write_note={}\n\
         rom_write_policy={}\n\
         requested_instruction_budget={}\n\
         used_instruction_budget={}\n\
         stop_reason={}\n\
         final_pc=0x{:08X}\n\
         final_cpsr=0x{:08X}\n\
         final_thumb={}\n\
         display_dispcnt=0x{:04X}\n\
         display_mode={}\n\
         display_forced_blank={}\n\
         display_bg0_enabled={}\n\
         display_bg2_enabled={}\n\
         display_backdrop_rgb555=0x{:04X}\n\
         image_file={}\n\
         image_width={}\n\
         image_height={}\n\
         image_sha256={}\n\
         image_file_sha256={}\n\
         note=static post-budget bitmap snapshot only; not a frame, boot, hardware-timed render, gameplay, or compatibility claim\n",
        cart.len(),
        cart.sha256_hex(),
        bus.bios_data_policy().name(),
        bios_data_note,
        bus.bios_write_policy().name(),
        bios_write_note,
        bus.rom_write_policy().name(),
        requested_budget,
        used_budget,
        stop_reason,
        cpu.pc(),
        cpu.cpsr(),
        u8::from(cpu.thumb()),
        dispcnt,
        mode,
        u8::from(dispcnt & 0x0080 != 0),
        u8::from(dispcnt & 0x0100 != 0),
        u8::from(dispcnt & 0x0400 != 0),
        backdrop,
        IMAGE_FILE,
        image.width,
        image.height,
        image.sha256_hex(),
        sha256::hex(&sha256::digest(&ppm)),
    );

    let directory = create_fresh_directory(path)?;
    write_safely(&directory.join(IMAGE_FILE), &ppm)?;
    write_safely(&directory.join(REPORT_FILE), report.as_bytes())?;
    Ok(directory)
}
