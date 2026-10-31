//! Shared per-pixel layer candidate for the static Mode 0 compositor.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct LayerPixel {
    pub color: u32,
    pub priority: u8,
    /// BLDCNT target index: 0..3 = BG0..BG3, 4 = OBJ.
    pub layer: u8,
    /// Lower wins a priority tie: OBJ=0, BG0=1, BG1=2, BG2=3, BG3=4.
    pub order: u8,
}

/// Keeps the highest-priority candidate; ties keep the earlier layer.
pub(super) fn choose(best: &mut Option<LayerPixel>, candidate: Option<LayerPixel>) {
    let Some(candidate) = candidate else {
        return;
    };
    let replace = match *best {
        None => true,
        Some(current) => {
            candidate.priority < current.priority
                || (candidate.priority == current.priority && candidate.order < current.order)
        }
    };
    if replace {
        *best = Some(candidate);
    }
}

/// Highest-priority candidate in a per-layer array.
pub(super) fn best(candidates: &[Option<LayerPixel>; 5]) -> Option<LayerPixel> {
    let mut best = None;
    for candidate in candidates.iter().flatten() {
        choose(&mut best, Some(*candidate));
    }
    best
}

/// Highest-priority candidate immediately below `top` in the compositing
/// order. The backdrop (layer 5) is not part of the array; callers fall back to
/// it explicitly.
pub(super) fn next(candidates: &[Option<LayerPixel>; 5], top: LayerPixel) -> Option<LayerPixel> {
    let mut best: Option<LayerPixel> = None;
    for candidate in candidates.iter().flatten() {
        if candidate.layer == top.layer {
            continue;
        }
        let replace = match best {
            None => true,
            Some(current) => {
                candidate.priority < current.priority
                    || (candidate.priority == current.priority && candidate.order < current.order)
            }
        };
        if replace {
            best = Some(*candidate);
        }
    }
    best
}
