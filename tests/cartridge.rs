//! Cartridge header acceptance, diagnostics and size bounds. No commercial ROM
//! is required or referenced.

use gba_native::cart::{Cartridge, MAX_ROM_LEN, MIN_ROM_LEN};
use gba_native::error::Error;

fn header_image(len: usize, title: &[u8], game_code: &[u8], maker: &[u8]) -> Vec<u8> {
    let mut bytes = vec![0u8; len];
    bytes[0xA0..0xAC].fill(0);
    bytes[0xA0..0xA0 + title.len()].copy_from_slice(title);
    bytes[0xAC..0xB0].copy_from_slice(game_code);
    bytes[0xB0..0xB2].copy_from_slice(maker);
    bytes[0xB2] = 0x96;
    bytes[0xB3] = 0;
    bytes[0xB4] = 0;
    bytes[0xBC] = 0;
    let sum = bytes[0xA0..=0xBC]
        .iter()
        .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
    bytes[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
    bytes
}

#[test]
fn accepts_header_boundaries_and_decodes_fields() {
    let minimum = Cartridge::from_bytes(header_image(MIN_ROM_LEN, b"MIN", b"ABCD", b"01"))
        .expect("192-byte image");
    assert_eq!(minimum.len(), MIN_ROM_LEN);
    assert_eq!(minimum.header().title, "MIN");
    assert_eq!(minimum.header().game_code, *b"ABCD");
    assert_eq!(minimum.header().maker_code, *b"01");
    assert!(minimum.header().fixed_ok);
    assert!(minimum.header().complement_ok);

    let maximum = Cartridge::from_bytes(header_image(MAX_ROM_LEN, b"MAXIMUM", b"WXYZ", b"99"))
        .expect("32 MiB image");
    assert_eq!(maximum.len(), MAX_ROM_LEN);
    assert!(maximum.header().complement_ok);
}

#[test]
fn rejects_short_and_oversized_inputs_without_panicking() {
    assert!(matches!(
        Cartridge::from_bytes(vec![0u8; MIN_ROM_LEN - 1]),
        Err(Error::CartSize { len: 191 })
    ));
    assert!(matches!(
        Cartridge::from_bytes(vec![0u8; MAX_ROM_LEN + 1]),
        Err(Error::CartSize { .. })
    ));
}

#[test]
fn complement_uses_documented_subtraction_formula() {
    // Independent vector: fill the header, then place the documented checksum
    // 0x02 rather than deriving it with the same helper used by other tests.
    let mut bytes = vec![0u8; 0x200];
    bytes[0xA0..0xA0 + 3].copy_from_slice(b"MIN");
    bytes[0xAC..0xB0].copy_from_slice(b"ABCD");
    bytes[0xB0..0xB2].copy_from_slice(b"01");
    bytes[0xB2] = 0x96;
    bytes[0xBD] = 0x02;
    let cart = Cartridge::from_bytes(bytes).expect("cart");
    assert_eq!(cart.header().complement, 0x02);
    assert_eq!(cart.header().computed_complement, 0x02);
    assert!(cart.header().complement_ok);
}

#[test]
fn reports_fixed_byte_and_complement_independently() {
    let mut bytes = header_image(0x200, b"CHECK", b"GBAD", b"00");
    bytes[0xB2] = 0x00;
    bytes[0xBD] = bytes[0xBD].wrapping_add(1);
    let cart = Cartridge::from_bytes(bytes).expect("size is valid");
    assert!(!cart.header().fixed_ok);
    assert!(!cart.header().complement_ok);
    assert_ne!(cart.header().complement, cart.header().computed_complement);
}

#[test]
fn path_loader_rejects_oversized_sparse_file_without_reading_it() {
    let dir = std::env::temp_dir().join(format!("gba-native-sparse-cart-{}", std::process::id()));
    std::fs::create_dir_all(&dir).expect("temp dir");
    let path = dir.join("sparse.gba");
    let file = std::fs::File::create(&path).expect("create sparse file");
    file.set_len(MAX_ROM_LEN as u64 + 4096)
        .expect("set sparse length");
    drop(file);

    let error = Cartridge::from_path(&path).expect_err("oversized file rejected");
    assert!(matches!(error, Error::CartSize { .. }));

    std::fs::remove_file(&path).expect("remove sparse file");
    std::fs::remove_dir(&dir).expect("remove temp dir");
}

#[test]
fn digest_identifies_the_image() {
    let first = Cartridge::from_bytes(header_image(0x200, b"ONE", b"GBAD", b"00")).expect("cart");
    let mut different = header_image(0x200, b"ONE", b"GBAD", b"00");
    different[0x180] = 1;
    let second = Cartridge::from_bytes(different).expect("cart");
    assert_eq!(first.sha256(), first.sha256());
    assert_ne!(first.sha256(), second.sha256());
    assert_eq!(first.sha256_hex().len(), 64);
}
