//! Isolated desktop frontend for the synthetic gba-native presentation proof.
//!
//! This crate is intentionally outside the std-only default workspace. It owns
//! optional Winit/Pixels/WGPU dependencies and never adds a cartridge, BIOS,
//! save, input or vendor-upscaling mode.

#![forbid(unsafe_code)]

mod app;
mod audio_out;
mod cli;
mod presentation;
mod settings;
mod ui;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::Help) => {
            print!("{}", cli::USAGE);
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Version) => {
            println!("gba-native-desktop {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Ok(cli::Command::Run(options)) => match app::run(options) {
            Ok(()) => ExitCode::SUCCESS,
            Err(message) => {
                eprintln!("gba-native-desktop: {message}");
                eprintln!(
                    "hint: synthetic presentation needs a working desktop GPU surface; \
                     use --smoke N for bounded evidence or --help for limits"
                );
                ExitCode::FAILURE
            }
        },
        Err(error) => {
            eprintln!("gba-native-desktop: {error}");
            eprintln!();
            eprintln!("{}", cli::USAGE);
            ExitCode::from(2)
        }
    }
}
