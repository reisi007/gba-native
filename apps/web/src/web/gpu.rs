//! Async Pixels/WGPU initialization for the browser.
//!
//! One WGPU instance is created per page by `PixelsBuilder::build_async`; the
//! backend is fixed explicitly from the pre-resolved choice. Retrying with the
//! other backend therefore requires a fresh canvas/page, which is exactly what
//! the caller does.

use std::fmt;
use std::sync::Arc;

use pixels::wgpu::{Backend, Backends, Color, TextureFormat};
use pixels::{Pixels, PixelsBuilder, ScalingMode, SurfaceTexture};
use winit::window::Window;

use crate::model::{BrowserBackend, IMAGE_HEIGHT, IMAGE_WIDTH, Resolution};
use crate::presentation::RgbaImage;

use super::dom;
use super::status::{self, DeviceReport};

const GPU_CONSOLE_TEXT_LIMIT_BYTES: usize = 1_024;
const TRUNCATION_MARKER: &str = "… [truncated]";

fn gpu_console_text(category: &str, detail: &str) -> String {
    let prefix = format!("gba-native-web {category}: ");
    let available = GPU_CONSOLE_TEXT_LIMIT_BYTES.saturating_sub(prefix.len());
    if detail.len() <= available {
        return format!("{prefix}{detail}");
    }

    let keep_bytes = available.saturating_sub(TRUNCATION_MARKER.len());
    let detail_prefix = detail
        .char_indices()
        .take_while(|(index, character)| *index + character.len_utf8() <= keep_bytes)
        .map(|(_, character)| character)
        .collect::<String>();
    format!("{prefix}{detail_prefix}{TRUNCATION_MARKER}")
}

#[derive(Clone, Debug)]
pub enum GpuError {
    Stage {
        stage: &'static str,
        detail: String,
    },
    BackendMismatch {
        requested: BrowserBackend,
        actual: String,
    },
}

impl fmt::Display for GpuError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Stage { stage, detail } => write!(formatter, "{stage} failed: {detail}"),
            Self::BackendMismatch { requested, actual } => write!(
                formatter,
                "requested {} but the created adapter reports backend {actual}; \
                 refusing to present a differently accelerated surface",
                requested.label()
            ),
        }
    }
}

impl std::error::Error for GpuError {}

pub struct BuiltSurface {
    pub pixels: Pixels<'static>,
    pub report: DeviceReport,
}

pub async fn build(
    window: Arc<Window>,
    surface_size: (u32, u32),
    resolved: Resolution,
) -> Result<BuiltSurface, GpuError> {
    let surface = SurfaceTexture::new(surface_size.0, surface_size.1, Arc::clone(&window));
    let backends = match resolved.backend {
        BrowserBackend::WebGpu => Backends::BROWSER_WEBGPU,
        BrowserBackend::WebGl2 => Backends::GL,
    };
    let builder = {
        // Browser WebGPU canvas contexts accept only non-sRGB formats
        // (`bgra8unorm`/`rgba8unorm`), while the core bytes are already
        // sRGB-encoded. Declaring both the pixel texture and the surface
        // non-sRGB keeps the raw bytes untouched. The WebGL2 path can use the
        // sRGB surface format that WGPU reports, so it declares the pixel
        // texture sRGB for an exact decode/encode round-trip.
        let (pixel_format, surface_format) = match resolved.backend {
            BrowserBackend::WebGpu => (TextureFormat::Rgba8Unorm, Some(TextureFormat::Bgra8Unorm)),
            BrowserBackend::WebGl2 => (TextureFormat::Rgba8UnormSrgb, None),
        };
        let mut builder = PixelsBuilder::new(IMAGE_WIDTH, IMAGE_HEIGHT, surface)
            .wgpu_backend(backends)
            .texture_format(pixel_format)
            .enable_vsync(true);
        if let Some(format) = surface_format {
            builder = builder
                .surface_texture_format(format)
                .render_texture_format(format);
        }
        builder
    };

    let mut pixels = builder
        .build_async()
        .await
        .map_err(|error| GpuError::Stage {
            stage: "GPU adapter/device/surface creation",
            detail: error.to_string(),
        })?;
    pixels.set_scaling_mode(ScalingMode::PixelPerfect);
    pixels.clear_color(Color::BLACK);

    let info = pixels.adapter().get_info();
    let expected = match resolved.backend {
        BrowserBackend::WebGpu => Backend::BrowserWebGpu,
        BrowserBackend::WebGl2 => Backend::Gl,
    };
    if info.backend != expected {
        return Err(GpuError::BackendMismatch {
            requested: resolved.backend,
            actual: format!("{:?}", info.backend),
        });
    }

    let limits = pixels.device().limits();
    let report = DeviceReport {
        adapter_name: info.name.clone(),
        backend: format!("{:?}", info.backend),
        device_type: format!("{:?}", info.device_type),
        driver: info.driver.clone(),
        driver_info: info.driver_info.clone(),
        vendor: info.vendor,
        device: info.device,
        surface_format: format!("{:?}", pixels.surface_texture_format()),
        render_format: format!("{:?}", pixels.render_texture_format()),
        present_mode: format!("{:?}", pixels.present_mode()),
        max_texture_dimension_2d: limits.max_texture_dimension_2d,
        max_buffer_size: limits.max_buffer_size,
    };

    pixels.device().on_uncaptured_error(Arc::new(|error| {
        let message = gpu_console_text("uncaptured GPU error", &error.to_string());
        status::console_error(&message);
    }));
    pixels.device().set_device_lost_callback(|reason, message| {
        let detail = format!("WebGPU device lost ({reason:?}): {message}");
        let console_message = gpu_console_text("device loss", &detail);
        status::console_error(&console_message);
        dom::mark_lost(detail);
    });

    Ok(BuiltSurface { pixels, report })
}

pub fn copy_frame(pixels: &mut Pixels<'_>, image: &RgbaImage) -> Result<(), String> {
    let frame = pixels.frame_mut();
    if frame.len() != image.rgba().len() {
        return Err(format!(
            "GPU frame buffer is {} bytes, expected {} for {}x{} RGBA",
            frame.len(),
            image.rgba().len(),
            image.width(),
            image.height()
        ));
    }
    frame.copy_from_slice(image.rgba());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{GPU_CONSOLE_TEXT_LIMIT_BYTES, gpu_console_text};

    #[test]
    fn gpu_console_diagnostics_keep_category_and_bound_long_causes() {
        let short = gpu_console_text(
            "device loss",
            "WebGPU device lost (OutOfMemory): synthetic allocation failure",
        );
        assert!(short.contains("device loss: WebGPU device lost (OutOfMemory)"));
        assert!(short.contains("synthetic allocation failure"));

        let long_loss = gpu_console_text(
            "device loss",
            &format!("WebGPU device lost (OutOfMemory): {}", "x".repeat(2_000)),
        );
        assert!(
            long_loss.starts_with("gba-native-web device loss: WebGPU device lost (OutOfMemory):")
        );
        assert!(long_loss.ends_with("… [truncated]"));
        assert!(long_loss.len() <= GPU_CONSOLE_TEXT_LIMIT_BYTES);

        let long_unicode =
            gpu_console_text("device loss", &format!("原因: {}", "💥".repeat(1_000)));
        assert!(long_unicode.starts_with("gba-native-web device loss: 原因: "));
        assert!(long_unicode.ends_with("… [truncated]"));
        assert!(long_unicode.len() <= GPU_CONSOLE_TEXT_LIMIT_BYTES);

        let long = gpu_console_text(
            "uncaptured GPU error",
            &format!("validation failure: {}", "x".repeat(2_000)),
        );
        assert!(long.starts_with("gba-native-web uncaptured GPU error: validation failure:"));
        assert!(long.ends_with("… [truncated]"));
        assert!(long.len() <= GPU_CONSOLE_TEXT_LIMIT_BYTES);
    }
}
