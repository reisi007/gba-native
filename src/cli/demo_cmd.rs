//! Synthetic demo image command.

use std::path::PathBuf;

use gba_native::bus::Bus;
use gba_native::cpu::Cpu;
use gba_native::demo;
use gba_native::display;
use gba_native::error::{Error, Result};
use gba_native::interpret;
use gba_native::render::{self, Backend};

use super::{parse_size, write_safely};

pub(super) fn run(mut args: std::iter::Skip<std::env::Args>) -> Result<()> {
    let mut output = PathBuf::from("output/demo.ppm");
    let mut size = (display::WIDTH, display::HEIGHT);
    let mut backend = Backend::Nearest;
    while let Some(argument) = args.next() {
        match argument.as_str() {
            "--out" => {
                output = PathBuf::from(
                    args.next()
                        .ok_or_else(|| Error::Args("--out needs a path".to_string()))?,
                );
            }
            "--size" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--size needs WxH".to_string()))?;
                size = parse_size(&value)?;
            }
            "--backend" => {
                let value = args
                    .next()
                    .ok_or_else(|| Error::Args("--backend needs a name".to_string()))?;
                backend = Backend::parse(&value)?;
            }
            _ => return Err(Error::Args(format!("unknown demo option {argument}"))),
        }
    }

    let cart = demo::demo_cartridge();
    let mut cpu = Cpu::direct_entry();
    cpu.set_pc(demo::ENTRY);
    cpu.set_thumb(false);
    let mut bus = Bus::new(cart);
    let used = interpret::run_budget(&mut cpu, &mut bus, demo::DEMO_BUDGET)?;
    let image = display::snapshot(&bus)?;
    let scaled = render::scale(&image, size.0, size.1, backend)?;
    write_safely(&output, &render::to_ppm(&scaled))?;

    println!("fixture=synthetic-direct-entry");
    println!("instructions={used}");
    println!("pc=0x{:08X}", cpu.pc());
    println!("cpsr=0x{:08X}", cpu.cpsr());
    println!("image={}x{}", image.width, image.height);
    println!("image_sha256={}", image.sha256_hex());
    println!("output={}", output.display());
    println!("output_size={}x{}", scaled.width, scaled.height);
    println!("backend={}", backend.name());
    println!("note=experimental synthetic fixture; not a retail-game compatibility result");
    Ok(())
}
