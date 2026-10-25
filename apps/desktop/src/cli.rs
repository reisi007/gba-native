//! Command line for the isolated synthetic GPU frontend.
//!
//! There is deliberately no cartridge, save, BIOS or input mode here. The only
//! image source is the independently executed synthetic demo fixture from the
//! std-only core.

use std::fmt;

/// Upper bound for either physical window edge.
pub const MAX_WINDOW_EDGE: u32 = 4096;
/// Upper bound for the initial physical window area (4096x2160, about 35 MiB
/// at four bytes per pixel).
pub const MAX_WINDOW_PIXELS: u64 = 4096 * 2160;
/// Upper bound for redraw requests in smoke mode.
pub const MAX_SMOKE_PRESENTATIONS: u32 = 10_000;

pub const USAGE: &str = "\
gba-native-desktop: synthetic GPU presentation foundation

usage: gba-native-desktop [--synthetic] [--size WxH] [--smoke N]
       gba-native-desktop --help
       gba-native-desktop --version

options:
  --synthetic   Present the existing synthetic demo bitmap (the only mode).
  --size WxH    Initial physical window size, for example 720x480.
                Limits: 1..=4096 per edge and <= 4096x2160 pixels.
  --smoke N     Request redraws until N render acknowledgements; print
                adapter/presentation evidence, exit. N is 1..=10000. Counts are
                Pixels::render acknowledgements, not visible or guest frames.
  -h, --help    Show this help.
  --version     Show the frontend version.

This is synthetic GPU presentation only; it is not a playable retail emulator.
The guest image is static after the existing demo instruction budget. There is
no --rom, input, audio, save, BIOS or retail-game mode in this foundation.
The window runs at nearest/pixel-perfect integer scale, 3:2, black letterbox.
Close with Escape or the window close control. The default event loop waits.

Runtime: Winit 0.30 with Pixels 0.17/WGPU 29, portable native window target.
A browser/WebGPU target is a documented future possibility, not implemented.
";

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Options {
    /// `None` means three times the guest image (720x480 for the demo).
    pub initial_size: Option<(u32, u32)>,
    /// Successful render acknowledgement targets before an automatic clean exit.
    pub smoke: Option<u32>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    Run(Options),
    Help,
    Version,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CliError(String);

impl CliError {
    fn new(message: impl Into<String>) -> Self {
        Self(message.into())
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CliError {}

/// Parse arguments without touching a window, GPU or the synthetic fixture.
pub fn parse<I>(args: I) -> Result<Command, CliError>
where
    I: IntoIterator<Item = String>,
{
    let mut args = args.into_iter();
    let mut initial_size = None;
    let mut smoke = None;

    while let Some(argument) = args.next() {
        match argument.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "--version" => return Ok(Command::Version),
            "--synthetic" => {}
            "--size" => {
                let value = args
                    .next()
                    .ok_or_else(|| CliError::new("--size needs a value such as 720x480"))?;
                initial_size = Some(parse_size(&value)?);
            }
            value if value.starts_with("--size=") => {
                initial_size = Some(parse_size(&value["--size=".len()..])?);
            }
            "--smoke" => {
                let value = args
                    .next()
                    .ok_or_else(|| CliError::new("--smoke needs a presentation count"))?;
                smoke = Some(parse_smoke(&value)?);
            }
            value if value.starts_with("--smoke=") => {
                smoke = Some(parse_smoke(&value["--smoke=".len()..])?);
            }
            "--rom" => {
                return Err(CliError::new(
                    "--rom is not supported: this foundation presents the synthetic demo only",
                ));
            }
            value if value.starts_with("--rom=") => {
                return Err(CliError::new(
                    "--rom is not supported: no cartridge path is accepted here",
                ));
            }
            value if value.starts_with('-') => {
                return Err(CliError::new(format!("unknown option {value:?}")));
            }
            value => {
                return Err(CliError::new(format!(
                    "unexpected positional argument {value:?}; use --size or --smoke"
                )));
            }
        }
    }

    Ok(Command::Run(Options {
        initial_size,
        smoke,
    }))
}

fn parse_size(value: &str) -> Result<(u32, u32), CliError> {
    let (width, height) = value
        .split_once(['x', 'X'])
        .ok_or_else(|| CliError::new(format!("invalid --size {value:?}; expected WxH")))?;
    if height.contains(['x', 'X']) {
        return Err(CliError::new(format!(
            "invalid --size {value:?}; expected exactly one WxH separator"
        )));
    }
    let width = parse_dimension(width, "width")?;
    let height = parse_dimension(height, "height")?;
    if width > MAX_WINDOW_EDGE || height > MAX_WINDOW_EDGE {
        return Err(CliError::new(format!(
            "--size {width}x{height} exceeds the {MAX_WINDOW_EDGE}-pixel edge limit"
        )));
    }
    let pixels = u64::from(width) * u64::from(height);
    if pixels > MAX_WINDOW_PIXELS {
        return Err(CliError::new(format!(
            "--size {width}x{height} requests {pixels} pixels; the limit is {MAX_WINDOW_PIXELS}"
        )));
    }
    Ok((width, height))
}

fn parse_dimension(value: &str, name: &str) -> Result<u32, CliError> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| CliError::new(format!("invalid --size {name} {value:?}")))?;
    if parsed == 0 {
        return Err(CliError::new(format!("--size {name} must be at least 1")));
    }
    Ok(parsed)
}

fn parse_smoke(value: &str) -> Result<u32, CliError> {
    let parsed = value
        .parse::<u32>()
        .map_err(|_| CliError::new(format!("invalid --smoke count {value:?}")))?;
    if !(1..=MAX_SMOKE_PRESENTATIONS).contains(&parsed) {
        return Err(CliError::new(format!(
            "--smoke count must be 1..={MAX_SMOKE_PRESENTATIONS}"
        )));
    }
    Ok(parsed)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_ok(args: &[&str]) -> Command {
        parse(args.iter().map(|value| (*value).to_string())).expect("valid arguments")
    }

    fn parse_error(args: &[&str]) -> String {
        parse(args.iter().map(|value| (*value).to_string()))
            .expect_err("invalid arguments")
            .to_string()
    }

    #[test]
    fn defaults_to_synthetic_run_without_size_or_smoke() {
        assert_eq!(
            parse_ok(&[]),
            Command::Run(Options {
                initial_size: None,
                smoke: None
            })
        );
        assert_eq!(parse_ok(&["--synthetic"]), parse_ok(&[]));
    }

    #[test]
    fn accepts_size_and_smoke_in_both_spellings() {
        let expected = Command::Run(Options {
            initial_size: Some((800, 600)),
            smoke: Some(3),
        });
        assert_eq!(parse_ok(&["--size", "800x600", "--smoke", "3"]), expected);
        assert_eq!(parse_ok(&["--size=800X600", "--smoke=3"]), expected);
    }

    #[test]
    fn help_and_version_exit_before_any_path_check() {
        assert_eq!(parse_ok(&["--help"]), Command::Help);
        assert_eq!(parse_ok(&["-h"]), Command::Help);
        assert_eq!(parse_ok(&["--version"]), Command::Version);
    }

    #[test]
    fn rejects_malformed_or_unbounded_sizes() {
        assert!(parse_error(&["--size"]).contains("needs a value"));
        assert!(parse_error(&["--size", "720"]).contains("expected WxH"));
        assert!(parse_error(&["--size", "720x480x2"]).contains("exactly one"));
        assert!(parse_error(&["--size", "0x480"]).contains("at least 1"));
        assert!(parse_error(&["--size", "720x0"]).contains("at least 1"));
        assert!(parse_error(&["--size", "abcx480"]).contains("width"));
        assert!(parse_error(&["--size", "4097x160"]).contains("edge limit"));
        assert!(parse_error(&["--size", "4096x4096"]).contains("pixels"));
    }

    #[test]
    fn rejects_smoke_outside_bounds() {
        assert!(parse_error(&["--smoke"]).contains("needs a presentation count"));
        assert!(parse_error(&["--smoke", "0"]).contains("1..="));
        assert!(parse_error(&["--smoke", "10001"]).contains("1..="));
        assert!(parse_error(&["--smoke", "many"]).contains("invalid"));
    }

    #[test]
    fn rejects_rom_and_unknown_arguments_with_clear_text() {
        assert!(parse_error(&["--rom", "game.gba"]).contains("not supported"));
        assert!(parse_error(&["--rom=game.gba"]).contains("not supported"));
        assert!(parse_error(&["--fps"]).contains("unknown option"));
        assert!(parse_error(&["game.gba"]).contains("positional"));
    }

    #[test]
    fn usage_states_synthetic_only_and_not_playable() {
        assert!(USAGE.contains("synthetic GPU presentation only"));
        assert!(USAGE.contains("not a playable retail emulator"));
        assert!(USAGE.contains("no --rom"));
    }
}
