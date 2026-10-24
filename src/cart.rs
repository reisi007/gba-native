//! Cartridge image parsing. Only the documented 192-byte header is inspected.
//! No Nintendo logo bytes are embedded or validated.

use std::io::Read;

use crate::error::{Error, Result};
use crate::sha256;

pub const MIN_ROM_LEN: usize = 192;
pub const MAX_ROM_LEN: usize = 32 * 1024 * 1024;
pub const ROM_BASE: u32 = 0x0800_0000;
pub const ROM_WINDOW_MASK: u32 = 0x01FF_FFFF;

/// Bounds-checked cartridge image. The bytes are never mutated by the bus.
#[derive(Clone, Debug)]
pub struct Cartridge {
    bytes: Vec<u8>,
    header: Header,
    sha256: [u8; 32],
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Header {
    pub title_bytes: [u8; 12],
    pub title: String,
    pub game_code: [u8; 4],
    pub maker_code: [u8; 2],
    pub fixed_byte: u8,
    pub main_unit_code: u8,
    pub device_type: u8,
    pub version: u8,
    pub complement: u8,
    pub computed_complement: u8,
    pub fixed_ok: bool,
    pub complement_ok: bool,
}

impl Cartridge {
    pub fn from_bytes(bytes: Vec<u8>) -> Result<Self> {
        if !(MIN_ROM_LEN..=MAX_ROM_LEN).contains(&bytes.len()) {
            return Err(Error::CartSize { len: bytes.len() });
        }
        let header = Header::parse(&bytes);
        let sha256 = sha256::digest(&bytes);
        Ok(Self {
            bytes,
            header,
            sha256,
        })
    }

    /// Bounded path loader. The file metadata rejects oversized/sparse images
    /// before any payload read, and the read itself is capped at
    /// `MAX_ROM_LEN + 1` bytes so a growing file cannot allocate unboundedly.
    pub fn from_path(path: &std::path::Path) -> Result<Self> {
        let file = std::fs::File::open(path)?;
        let len = file.metadata()?.len();
        if len > MAX_ROM_LEN as u64 {
            return Err(Error::CartSize {
                len: usize::try_from(len).unwrap_or(usize::MAX),
            });
        }
        let mut bytes = Vec::with_capacity(usize::try_from(len).unwrap_or(MAX_ROM_LEN));
        let mut limited = file.take(MAX_ROM_LEN as u64 + 1);
        limited.read_to_end(&mut bytes)?;
        Self::from_bytes(bytes)
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn len(&self) -> usize {
        self.bytes.len()
    }

    pub fn is_empty(&self) -> bool {
        self.bytes.is_empty()
    }

    pub fn header(&self) -> &Header {
        &self.header
    }

    pub fn sha256(&self) -> [u8; 32] {
        self.sha256
    }

    pub fn sha256_hex(&self) -> String {
        sha256::hex(&self.sha256)
    }

    /// Map one of the three documented 32 MiB ROM windows to a cartridge offset.
    /// Returns `None` for non-ROM addresses or offsets past the image.
    pub fn rom_offset(&self, addr: u32) -> Option<usize> {
        if !(ROM_BASE..=0x0DFF_FFFF).contains(&addr) {
            return None;
        }
        let offset = ((addr - ROM_BASE) & ROM_WINDOW_MASK) as usize;
        (offset < self.bytes.len()).then_some(offset)
    }

    pub fn read_u8(&self, addr: u32) -> Option<u8> {
        self.rom_offset(addr).map(|offset| self.bytes[offset])
    }

    pub fn read_u16(&self, addr: u32) -> Option<u16> {
        let offset = self.rom_offset(addr)?;
        let bytes = self.bytes.get(offset..offset + 2)?;
        Some(u16::from_le_bytes([bytes[0], bytes[1]]))
    }

    pub fn read_u32(&self, addr: u32) -> Option<u32> {
        let offset = self.rom_offset(addr)?;
        let bytes = self.bytes.get(offset..offset + 4)?;
        Some(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
    }
}

impl Header {
    fn parse(bytes: &[u8]) -> Self {
        let mut title_bytes = [0u8; 12];
        title_bytes.copy_from_slice(&bytes[0xA0..0xAC]);
        let title_end = title_bytes
            .iter()
            .position(|byte| *byte == 0)
            .unwrap_or(title_bytes.len());
        let title = String::from_utf8_lossy(&title_bytes[..title_end])
            .trim_end_matches(' ')
            .to_string();

        let mut game_code = [0u8; 4];
        game_code.copy_from_slice(&bytes[0xAC..0xB0]);
        let mut maker_code = [0u8; 2];
        maker_code.copy_from_slice(&bytes[0xB0..0xB2]);

        let fixed_byte = bytes[0xB2];
        let complement = bytes[0xBD];
        let sum = bytes[0xA0..=0xBC]
            .iter()
            .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
        let computed_complement = 0u8.wrapping_sub(sum).wrapping_sub(0x19);

        Self {
            title_bytes,
            title,
            game_code,
            maker_code,
            fixed_byte,
            main_unit_code: bytes[0xB3],
            device_type: bytes[0xB4],
            version: bytes[0xBC],
            complement,
            computed_complement,
            fixed_ok: fixed_byte == 0x96,
            complement_ok: complement == computed_complement,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image_with_header() -> Vec<u8> {
        let mut bytes = vec![0u8; 0x200];
        bytes[0xA0..0xAC].copy_from_slice(b"TEST GAME\0\0\0");
        bytes[0xAC..0xB0].copy_from_slice(b"ABCD");
        bytes[0xB0..0xB2].copy_from_slice(b"01");
        bytes[0xB2] = 0x96;
        let sum = bytes[0xA0..=0xBC]
            .iter()
            .fold(0u8, |acc, byte| acc.wrapping_add(*byte));
        bytes[0xBD] = 0u8.wrapping_sub(sum).wrapping_sub(0x19);
        bytes
    }

    #[test]
    fn parses_fields_and_checksum() {
        let cart = Cartridge::from_bytes(image_with_header()).expect("valid image");
        let header = cart.header();
        assert_eq!(header.title, "TEST GAME");
        assert_eq!(header.game_code, *b"ABCD");
        assert_eq!(header.maker_code, *b"01");
        assert!(header.fixed_ok);
        assert!(header.complement_ok);
    }

    #[test]
    fn rejects_out_of_range_sizes() {
        assert!(matches!(
            Cartridge::from_bytes(vec![0; 191]),
            Err(Error::CartSize { .. })
        ));
        let mut too_large = vec![0u8; MAX_ROM_LEN + 1];
        too_large[0xB2] = 0x96;
        assert!(matches!(
            Cartridge::from_bytes(too_large),
            Err(Error::CartSize { .. })
        ));
    }
}
