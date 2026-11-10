use gba_native::cart::Cartridge;
use std::{env, fs, path::Path};

fn main() {
    println!("cargo:rerun-if-env-changed=GBA_NATIVE_ROM_PATH");
    println!("cargo:rerun-if-env-changed=GBA_NATIVE_PROGRAM_PATH");
    println!("cargo:rerun-if-env-changed=GBA_NATIVE_EMBED_ROM");
    println!("cargo:rustc-check-cfg=cfg(rom_bound)");
    println!("cargo:rustc-check-cfg=cfg(rom_embedded)");
    let output =
        Path::new(&env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("rom_binding.rs");
    match env::var_os("GBA_NATIVE_ROM_PATH") {
        Some(path) => {
            let path = Path::new(&path);
            let cart = Cartridge::from_path(path).unwrap_or_else(|error| {
                panic!("GBA_NATIVE_ROM_PATH is not a valid bounded GBA image: {error}")
            });
            let binding = binding_source(path, &cart);
            fs::write(output, binding).expect("write generated ROM metadata");
            copy_program(&cart);
            embed_rom_if_requested(path);
            println!("cargo:rustc-cfg=rom_bound");
            println!(
                "cargo:warning=building local ROM-bound translated app for {}",
                cart.header().title
            );
        }
        None => {
            if env::var_os("GBA_NATIVE_PROGRAM_PATH").is_some() {
                println!(
                    "cargo:warning=GBA_NATIVE_PROGRAM_PATH is ignored without GBA_NATIVE_ROM_PATH"
                );
            }
            fs::write(output, "pub const BOUND_ROM: Option<RomBinding> = None;")
                .expect("write empty ROM metadata")
        }
    }
}

/// Copies the host-emitted translated program into `OUT_DIR`.
///
/// The host CLI owns AOT discovery (`gba-native aot emit-program`); this crate
/// deliberately has no build-time dependency on the AOT tooling crate. The CLI
/// writes a `<program>.sha256` sidecar for the image it translated, so a stale
/// program cannot be compiled into a build for a different image.
fn copy_program(cart: &Cartridge) {
    let path = env::var_os("GBA_NATIVE_PROGRAM_PATH").unwrap_or_else(|| {
        panic!(
            "GBA_NATIVE_PROGRAM_PATH is required with GBA_NATIVE_ROM_PATH; \
             run scripts/rune-bound.sh or `gba-native aot emit-program <rom> <out.rs>` first"
        )
    });
    let path = Path::new(&path);
    println!("cargo:rerun-if-changed={}", path.display());
    let program = fs::read_to_string(path).unwrap_or_else(|error| {
        panic!(
            "cannot read the pre-emitted translated program {}: {error}",
            path.display()
        )
    });
    let mut sidecar = path.as_os_str().to_owned();
    sidecar.push(".sha256");
    let sidecar = Path::new(&sidecar);
    println!("cargo:rerun-if-changed={}", sidecar.display());
    let record = fs::read_to_string(sidecar).unwrap_or_else(|error| {
        panic!(
            "cannot read the program identity {} next to {}: {error}; \
             re-run `gba-native aot emit-program`",
            sidecar.display(),
            path.display()
        )
    });
    let expected = format!("rom_sha256={}", cart.sha256_hex());
    assert!(
        record.lines().any(|line| line.trim() == expected),
        "the pre-emitted program {} was not translated from this image ({}); \
         re-run `gba-native aot emit-program`",
        path.display(),
        cart.sha256_hex()
    );
    let program = program.replace(
        "#![allow(clippy::all, unused_variables, unreachable_code)]\n",
        "",
    );
    fs::write(
        Path::new(&env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("rom_program.rs"),
        program,
    )
    .expect("write generated translated program");
}

/// Embeds the bound image into the executable for a local single-file build.
///
/// This is intentionally opt-in (`GBA_NATIVE_EMBED_ROM=1`) and refuses to run
/// in CI: the resulting binary contains retail-derived data and must stay
/// local, uncommitted and unpublished.
fn embed_rom_if_requested(path: &Path) {
    let Some(requested) = env::var_os("GBA_NATIVE_EMBED_ROM") else {
        return;
    };
    if requested == "0" || requested.is_empty() {
        return;
    }
    let ci = env::var_os("CI").is_some_and(|value| value != "false" && value != "0");
    assert!(
        !ci,
        "refusing GBA_NATIVE_EMBED_ROM in CI: embedded retail-derived data must stay local"
    );
    let target = env::var("TARGET").unwrap_or_default();
    assert!(
        !target.starts_with("wasm32"),
        "GBA_NATIVE_EMBED_ROM is for native single-file builds; the web bundle stages the image"
    );
    let bytes = fs::read(path).expect("read the bound ROM for embedding");
    let output =
        Path::new(&env::var_os("OUT_DIR").expect("Cargo sets OUT_DIR")).join("bound_rom.bin");
    fs::write(output, bytes).expect("write the embedded ROM copy");
    println!("cargo:rustc-cfg=rom_embedded");
    println!(
        "cargo:warning=embedded the bound ROM for a local single-file build; \
         this binary is retail-derived and must not be committed or published"
    );
}

fn binding_source(path: &Path, cart: &Cartridge) -> String {
    println!("cargo:rerun-if-changed={}", path.display());
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .expect("ROM path needs a UTF-8 file name");
    let title = &cart.header().title;
    let game_code = String::from_utf8_lossy(&cart.header().game_code);
    format!(
        "pub const BOUND_ROM: Option<RomBinding> = Some(RomBinding {{ title: {title:?}, game_code: {game_code:?}, sha256: {:?}, len: {}, file_name: {file_name:?} }});",
        cart.sha256_hex(),
        cart.len(),
    )
}
