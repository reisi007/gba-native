#[cfg(not(target_arch = "wasm32"))]
fn main() {
    gba_native_rune::run_app();
}

#[cfg(target_arch = "wasm32")]
fn main() {}
