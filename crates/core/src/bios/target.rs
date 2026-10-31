//! Shared writable RAM/video destination classification for HLE services.

/// Writable destination regions accepted by the documented memory/video HLE.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Target {
    Ewram,
    Iwram,
    Palette,
    Vram,
    Oam,
}

/// Classifies a CPU address as a writable HLE destination region.
///
/// EWRAM and IWRAM are mirrored across their full 16 MiB CPU windows. Palette,
/// VRAM and OAM use their physical lengths; the remaining windows are not
/// writable HLE destinations.
pub(super) fn target_of(addr: u32) -> Option<Target> {
    if (0x0200_0000..0x0300_0000).contains(&addr) {
        return Some(Target::Ewram);
    }
    if (0x0300_0000..0x0400_0000).contains(&addr) {
        return Some(Target::Iwram);
    }
    if (0x0500_0000..0x0500_0400).contains(&addr) {
        return Some(Target::Palette);
    }
    if (0x0600_0000..0x0601_8000).contains(&addr) {
        return Some(Target::Vram);
    }
    if (0x0700_0000..0x0700_0400).contains(&addr) {
        return Some(Target::Oam);
    }
    None
}
