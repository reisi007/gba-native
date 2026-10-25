use std::fmt;

/// Result type used throughout the experimental core.
pub type Result<T> = std::result::Result<T, Error>;

/// Every failure is explicit. Unsupported hardware is never treated as a
/// successful no-op.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    CartSize {
        len: usize,
    },
    RomOverread {
        addr: u32,
        len: usize,
    },
    RomWrite {
        addr: u32,
    },
    BiosAccess {
        addr: u32,
    },
    UnsupportedSave {
        addr: u32,
    },
    Unmapped {
        addr: u32,
    },
    UnalignedHalfword {
        addr: u32,
    },
    UnalignedWord {
        addr: u32,
    },
    UnalignedFetch {
        addr: u32,
        thumb: bool,
    },
    UnsupportedIoRead {
        addr: u32,
        width: u8,
    },
    UnsupportedIoWrite {
        addr: u32,
        width: u8,
    },
    NonExecutable {
        addr: u32,
    },
    /// Bounded policy for block/stack transfers: the low bits of the base would
    /// be ignored by hardware, but the documented writeback source is not
    /// established, so no access or writeback is performed.
    UnalignedBlockBase {
        addr: u32,
        thumb: bool,
    },
    UnsupportedInstruction {
        addr: u32,
        thumb: bool,
        raw: u32,
        reason: &'static str,
    },
    /// A structurally valid MRS/MSR whose documented execution is restricted
    /// (for example SPSR access in USER/SYSTEM or a T-bit change).
    Psr(&'static str),
    /// An explicit BIOS HLE service failure. Unsupported, waiting and invalid
    /// requests are never reported as a successful no-op.
    Bios(crate::bios::BiosError),
    UnsupportedDisplayMode {
        mode: u16,
    },
    UnsupportedPpuFeature {
        dispcnt: u16,
    },
    OutputTooSmall {
        width: u32,
        height: u32,
    },
    InvalidOutputSize {
        width: u32,
        height: u32,
    },
    UnsupportedBackend {
        name: String,
    },
    Aot(String),
    Native(String),
    Io(String),
    RomMismatch {
        expected: String,
        actual: String,
    },
    Args(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::CartSize { len } => write!(
                f,
                "cartridge size {len} is outside the accepted 192 B..32 MiB range"
            ),
            Error::RomOverread { addr, len } => {
                write!(f, "ROM overread at 0x{addr:08X} (cartridge is {len} bytes)")
            }
            Error::RomWrite { addr } => write!(f, "write to immutable ROM at 0x{addr:08X}"),
            Error::BiosAccess { addr } => write!(
                f,
                "BIOS access at 0x{addr:08X}: direct-entry mode has no BIOS"
            ),
            Error::UnsupportedSave { addr } => {
                write!(f, "save hardware at 0x{addr:08X} is not implemented")
            }
            Error::Unmapped { addr } => write!(f, "unmapped address 0x{addr:08X}"),
            Error::UnalignedHalfword { addr } => {
                write!(f, "unaligned halfword bus access at 0x{addr:08X}")
            }
            Error::UnalignedWord { addr } => {
                write!(f, "unaligned word bus access at 0x{addr:08X}")
            }
            Error::UnalignedFetch { addr, thumb } => write!(
                f,
                "unaligned {} instruction fetch at 0x{addr:08X}",
                if *thumb { "Thumb" } else { "ARM" }
            ),
            Error::UnsupportedIoRead { addr, width } => {
                write!(f, "unsupported {width}-bit I/O read at 0x{addr:08X}")
            }
            Error::UnsupportedIoWrite { addr, width } => {
                write!(f, "unsupported {width}-bit I/O write at 0x{addr:08X}")
            }
            Error::NonExecutable { addr } => {
                write!(
                    f,
                    "instruction fetch from non-executable memory at 0x{addr:08X}"
                )
            }
            Error::UnalignedBlockBase { addr, thumb } => write!(
                f,
                "unaligned {} block-transfer base 0x{addr:08X}; this prototype rejects it before any access",
                if *thumb { "Thumb" } else { "ARM" }
            ),
            Error::UnsupportedInstruction {
                addr,
                thumb,
                raw,
                reason,
            } => write!(
                f,
                "unsupported {} instruction 0x{raw:0width$X} at 0x{addr:08X}: {reason}",
                if *thumb { "Thumb" } else { "ARM" },
                width = if *thumb { 4 } else { 8 }
            ),
            Error::Psr(reason) => write!(f, "PSR access: {reason}"),
            Error::Bios(source) => write!(f, "BIOS HLE: {source}"),
            Error::UnsupportedDisplayMode { mode } => {
                write!(
                    f,
                    "display mode {mode} is not implemented (only modes 3, 4 and 5)"
                )
            }
            Error::UnsupportedPpuFeature { dispcnt } => write!(
                f,
                "DISPCNT 0x{dispcnt:04X} requests an unsupported PPU feature"
            ),
            Error::OutputTooSmall { width, height } => write!(
                f,
                "output {width}x{height} is smaller than the native 240x160 image"
            ),
            Error::InvalidOutputSize { width, height } => {
                write!(f, "invalid output size {width}x{height}")
            }
            Error::UnsupportedBackend { name } => {
                write!(f, "presentation backend {name:?} is not implemented")
            }
            Error::Aot(message) => write!(f, "AOT: {message}"),
            Error::Native(message) => write!(f, "native harness: {message}"),
            Error::Io(message) => write!(f, "I/O: {message}"),
            Error::RomMismatch { expected, actual } => write!(
                f,
                "ROM mismatch: artifact is bound to SHA-256 {expected}, supplied image is {actual}"
            ),
            Error::Args(message) => write!(f, "arguments: {message}"),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(value: std::io::Error) -> Self {
        Error::Io(value.to_string())
    }
}
