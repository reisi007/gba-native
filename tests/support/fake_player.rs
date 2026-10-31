// Compiled by scripts/play-tester.sh, never part of the emulator build.
//
// It records the arguments it receives so the playback gate can prove the CLI
// or generated harness invoked the platform player with the generated WAV.
// GBA_NATIVE_FAKE_PLAYER_FAIL makes it exit nonzero for the fail-closed test.

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(path) = std::env::var_os("GBA_NATIVE_FAKE_PLAYER_RECORD") {
        let mut file = std::fs::File::create(path).expect("create player record");
        for arg in &args {
            writeln!(file, "{arg}").expect("write player record");
        }
    }
    if std::env::var_os("GBA_NATIVE_FAKE_PLAYER_FAIL").is_some() {
        std::process::exit(7);
    }
}
