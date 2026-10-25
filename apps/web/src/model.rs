//! Host-testable rules for the browser foundation.
//!
//! Everything in this module is pure: backend choice validation, the explicit
//! auto/compatibility resolution, allocation caps and nearest-neighbour
//! letterbox geometry. No browser API is touched here, so the same rules are
//! covered by ordinary host unit tests.

use std::fmt;

/// Guest image geometry produced by the synthetic core demo.
pub const IMAGE_WIDTH: u32 = 240;
pub const IMAGE_HEIGHT: u32 = 160;
/// Independently recorded SHA-256 of the demo's packed RGB888 stream.
pub const EXPECTED_SHA256: &str =
    "b02968d620ee66d7175c29bbde12bc9ff075290f3e8086b26e9d4829b9ccb154";
/// The existing synthetic demo instruction budget. Not frames, not cycles.
pub const EXPECTED_GUEST_BUDGET: u64 = 230_411;
/// Design cap for browser surfaces. WebGL2 guarantees at least 2048, so this
/// is a conservative application cap, not a queried adapter maximum.
pub const MAX_SURFACE_EDGE: u32 = 8192;
pub const MAX_SURFACE_PIXELS: u64 = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BackendChoice {
    Auto,
    WebGpu,
    WebGl2,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BrowserBackend {
    WebGpu,
    WebGl2,
}

impl BrowserBackend {
    pub fn id(self) -> &'static str {
        match self {
            Self::WebGpu => "webgpu",
            Self::WebGl2 => "webgl2",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::WebGpu => "WebGPU",
            Self::WebGl2 => "WebGL2 compatibility path",
        }
    }
}

impl BackendChoice {
    pub fn parse(value: &str) -> Result<Self, ChoiceError> {
        match value {
            "" | "auto" => Ok(Self::Auto),
            "webgpu" => Ok(Self::WebGpu),
            "webgl2" => Ok(Self::WebGl2),
            other => Err(ChoiceError(other.to_owned())),
        }
    }

    pub fn id(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::WebGpu => "webgpu",
            Self::WebGl2 => "webgl2",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceError(pub String);

impl fmt::Display for ChoiceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "unknown backend {:?}; expected auto, webgpu or webgl2",
            self.0
        )
    }
}

impl std::error::Error for ChoiceError {}

/// What the browser actually exposes before a canvas context is created.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BrowserProbe {
    pub webgpu: bool,
    /// `None` means WebGL2 was intentionally not probed because the selected
    /// route did not need a compatibility decision.
    pub webgl2: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Resolution {
    pub backend: BrowserBackend,
    /// True only for `auto` when WebGPU was unavailable and WebGL2 was chosen.
    pub auto_fallback: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResolveError {
    WebGpuUnavailable,
    WebGl2Unavailable,
    NoBrowserBackend,
}

impl fmt::Display for ResolveError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WebGpuUnavailable => write!(
                formatter,
                "WebGPU was requested explicitly but this browser exposes no usable WebGPU \
                 adapter. No WebGL2 fallback was substituted."
            ),
            Self::WebGl2Unavailable => write!(
                formatter,
                "WebGL2 was requested explicitly but this browser exposes no usable WebGL2 \
                 context. No Canvas2D or WebGL1 fallback was substituted."
            ),
            Self::NoBrowserBackend => write!(
                formatter,
                "this browser exposes neither a usable WebGPU adapter nor a WebGL2 context. \
                 No CPU/Canvas2D fallback was substituted."
            ),
        }
    }
}

impl std::error::Error for ResolveError {}

/// Decide the backend exactly once, before any canvas context exists.
///
/// `auto` prefers WebGPU and falls back to the explicit WebGL2 compatibility
/// path only when WebGL2 is actually available. Forced choices never silently
/// degrade to the other backend; the caller must show the error.
pub fn resolve_backend(
    choice: BackendChoice,
    probe: BrowserProbe,
) -> Result<Resolution, ResolveError> {
    match choice {
        BackendChoice::WebGpu => {
            if probe.webgpu {
                Ok(Resolution {
                    backend: BrowserBackend::WebGpu,
                    auto_fallback: false,
                })
            } else {
                Err(ResolveError::WebGpuUnavailable)
            }
        }
        BackendChoice::WebGl2 => {
            if probe.webgl2 == Some(true) {
                Ok(Resolution {
                    backend: BrowserBackend::WebGl2,
                    auto_fallback: false,
                })
            } else {
                Err(ResolveError::WebGl2Unavailable)
            }
        }
        BackendChoice::Auto => {
            if probe.webgpu {
                Ok(Resolution {
                    backend: BrowserBackend::WebGpu,
                    auto_fallback: false,
                })
            } else if probe.webgl2 == Some(true) {
                Ok(Resolution {
                    backend: BrowserBackend::WebGl2,
                    auto_fallback: true,
                })
            } else {
                Err(ResolveError::NoBrowserBackend)
            }
        }
    }
}

/// Whether backend selection needs a real WebGL2 capability probe.
pub fn should_probe_webgl2(choice: BackendChoice, webgpu: bool) -> bool {
    choice == BackendChoice::WebGl2 || (choice == BackendChoice::Auto && !webgpu)
}

/// Integer nearest-neighbour viewport inside a physical surface.
///
/// The largest integer scale fitting both axes is used, at least 1. A surface
/// smaller than the guest image clips instead of inventing a fractional
/// filter. Zero dimensions (hidden or not laid out) yield an empty viewport.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Viewport {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
    pub scale: u32,
    pub clipped: bool,
}

pub fn compute_viewport(
    content_width: u32,
    content_height: u32,
    surface_width: u32,
    surface_height: u32,
) -> Viewport {
    if content_width == 0 || content_height == 0 || surface_width == 0 || surface_height == 0 {
        return Viewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            scale: 0,
            clipped: false,
        };
    }

    let scale = (surface_width / content_width)
        .min(surface_height / content_height)
        .max(1);
    let scaled_width = content_width.saturating_mul(scale);
    let scaled_height = content_height.saturating_mul(scale);
    let width = scaled_width.min(surface_width);
    let height = scaled_height.min(surface_height);
    Viewport {
        x: (surface_width - width) / 2,
        y: (surface_height - height) / 2,
        width,
        height,
        scale,
        clipped: scaled_width > surface_width || scaled_height > surface_height,
    }
}

/// Conservative allocation guard. This is a design cap, not an adapter limit.
pub fn surface_size_allowed(width: u32, height: u32) -> bool {
    width > 0
        && height > 0
        && width <= MAX_SURFACE_EDGE
        && height <= MAX_SURFACE_EDGE
        && u64::from(width) * u64::from(height) <= MAX_SURFACE_PIXELS
}

/// Reconcile a browser resize event with the CSS content box and device scale.
/// Some browser emulations report `devicePixelContentBoxSize` in CSS pixels;
/// Winit's resize value is accepted only when it agrees with the independently
/// measured CSS box times DPR. A backing-store readback is also trusted only
/// when it agrees with that physical expectation, so a CSS-sized readback never
/// gets multiplied twice.
pub fn physical_canvas_size(
    css_width: f64,
    css_height: f64,
    device_pixel_ratio: f64,
    reported: (u32, u32),
    backing: (u32, u32),
) -> (u32, u32) {
    fn expected(css: f64, ratio: f64) -> u32 {
        if !css.is_finite() || css <= 0.0 || !ratio.is_finite() || ratio <= 0.0 {
            return 0;
        }
        (css * ratio).round().clamp(0.0, f64::from(u32::MAX)) as u32
    }
    fn reconcile(expected: u32, reported: u32, backing: u32) -> u32 {
        let close = |value: u32| value.abs_diff(expected) <= 1;
        if close(reported) {
            reported
        } else if close(backing) {
            backing
        } else {
            expected
        }
    }

    let width = expected(css_width, device_pixel_ratio);
    let height = expected(css_height, device_pixel_ratio);
    if width == 0 || height == 0 {
        return (0, 0);
    }
    (
        reconcile(width, reported.0, backing.0),
        reconcile(height, reported.1, backing.1),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backend_choice_parsing_is_explicit() {
        assert_eq!(BackendChoice::parse("").unwrap(), BackendChoice::Auto);
        assert_eq!(BackendChoice::parse("auto").unwrap(), BackendChoice::Auto);
        assert_eq!(
            BackendChoice::parse("webgpu").unwrap(),
            BackendChoice::WebGpu
        );
        assert_eq!(
            BackendChoice::parse("webgl2").unwrap(),
            BackendChoice::WebGl2
        );
        assert!(BackendChoice::parse("webgl").is_err());
        assert!(BackendChoice::parse("canvas2d").is_err());
    }

    #[test]
    fn auto_prefers_webgpu_then_reports_the_compat_path() {
        let both = resolve_backend(
            BackendChoice::Auto,
            BrowserProbe {
                webgpu: true,
                webgl2: None,
            },
        )
        .unwrap();
        assert_eq!(both.backend, BrowserBackend::WebGpu);
        assert!(!both.auto_fallback);

        let compat = resolve_backend(
            BackendChoice::Auto,
            BrowserProbe {
                webgpu: false,
                webgl2: Some(true),
            },
        )
        .unwrap();
        assert_eq!(compat.backend, BrowserBackend::WebGl2);
        assert!(compat.auto_fallback);

        assert_eq!(
            resolve_backend(
                BackendChoice::Auto,
                BrowserProbe {
                    webgpu: false,
                    webgl2: Some(false)
                }
            ),
            Err(ResolveError::NoBrowserBackend)
        );
    }

    #[test]
    fn forced_backends_never_silently_fall_back() {
        assert_eq!(
            resolve_backend(
                BackendChoice::WebGpu,
                BrowserProbe {
                    webgpu: false,
                    webgl2: Some(true)
                }
            ),
            Err(ResolveError::WebGpuUnavailable)
        );
        assert_eq!(
            resolve_backend(
                BackendChoice::WebGl2,
                BrowserProbe {
                    webgpu: true,
                    webgl2: None
                }
            ),
            Err(ResolveError::WebGl2Unavailable)
        );
        let message = ResolveError::WebGpuUnavailable.to_string();
        assert!(message.contains("No WebGL2 fallback"));
        assert!(!message.contains("CPU"));
    }

    #[test]
    fn webgl2_probe_is_limited_to_routes_that_need_its_result() {
        assert!(!should_probe_webgl2(BackendChoice::WebGpu, true));
        assert!(!should_probe_webgl2(BackendChoice::WebGpu, false));
        assert!(!should_probe_webgl2(BackendChoice::Auto, true));
        assert!(should_probe_webgl2(BackendChoice::Auto, false));
        assert!(should_probe_webgl2(BackendChoice::WebGl2, true));
    }

    #[test]
    fn viewport_matches_the_native_pixel_perfect_math() {
        assert_eq!(
            compute_viewport(240, 160, 720, 480),
            Viewport {
                x: 0,
                y: 0,
                width: 720,
                height: 480,
                scale: 3,
                clipped: false
            }
        );
        assert_eq!(
            compute_viewport(240, 160, 800, 600),
            Viewport {
                x: 40,
                y: 60,
                width: 720,
                height: 480,
                scale: 3,
                clipped: false
            }
        );
        assert_eq!(
            compute_viewport(240, 160, 480, 720),
            Viewport {
                x: 0,
                y: 200,
                width: 480,
                height: 320,
                scale: 2,
                clipped: false
            }
        );
        assert_eq!(
            compute_viewport(240, 160, 240, 160),
            Viewport {
                x: 0,
                y: 0,
                width: 240,
                height: 160,
                scale: 1,
                clipped: false
            }
        );
    }

    #[test]
    fn viewport_clips_and_handles_zero_surfaces() {
        assert_eq!(
            compute_viewport(240, 160, 100, 100),
            Viewport {
                x: 0,
                y: 0,
                width: 100,
                height: 100,
                scale: 1,
                clipped: true
            }
        );
        let empty = Viewport {
            x: 0,
            y: 0,
            width: 0,
            height: 0,
            scale: 0,
            clipped: false,
        };
        assert_eq!(compute_viewport(240, 160, 0, 480), empty);
        assert_eq!(compute_viewport(240, 160, 800, 0), empty);
    }

    #[test]
    fn surface_guard_is_a_conservative_design_cap() {
        assert!(surface_size_allowed(720, 480));
        assert!(surface_size_allowed(4096, 4096));
        assert!(!surface_size_allowed(0, 480));
        assert!(!surface_size_allowed(8193, 160));
        assert!(!surface_size_allowed(8192, 8192));
    }

    #[test]
    fn canvas_physical_size_handles_dpr_and_css_sized_browser_reports() {
        assert_eq!(
            physical_canvas_size(332.0, 332.0, 1.0, (332, 332), (332, 332)),
            (332, 332)
        );
        assert_eq!(
            physical_canvas_size(332.0, 332.0, 2.0, (332, 332), (332, 332)),
            (664, 664)
        );
        assert_eq!(
            physical_canvas_size(331.5, 332.25, 1.25, (332, 332), (332, 332)),
            (414, 415)
        );
        assert_eq!(
            physical_canvas_size(332.0, 332.0, 2.0, (664, 664), (664, 664)),
            (664, 664)
        );
        assert_eq!(
            physical_canvas_size(0.0, 332.0, 2.0, (0, 664), (0, 664)),
            (0, 0)
        );
    }

    #[test]
    fn recorded_source_constants_are_the_native_evidence() {
        assert_eq!(EXPECTED_GUEST_BUDGET, 230_411);
        assert_eq!(EXPECTED_SHA256.len(), 64);
        assert_eq!(IMAGE_WIDTH, 240);
        assert_eq!(IMAGE_HEIGHT, 160);
    }
}
