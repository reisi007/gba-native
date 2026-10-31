//! Native Winit lifecycle around a real Pixels/WGPU surface.
//!
//! The guest bitmap is executed once by the std-only core, validated, converted
//! to RGBA and copied into the GPU texture. Redraws re-upload that static image;
//! this is not a frame loop, emulator runtime or playable frontend.

use std::path::PathBuf;
use std::sync::Arc;

use gba_native::input::{Button, Key, Settings};
use pixels::wgpu::Color;
use pixels::{Pixels, PixelsBuilder, ScalingMode, SurfaceTexture};
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::{MouseButton, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::ModifiersState;
use winit::window::{Fullscreen, Window, WindowId};

use crate::cli::Options;
use crate::presentation::{RgbaImage, compute_viewport, synthetic_demo_image};
use crate::{settings, ui};

mod input;

pub const TITLE: &str = "gba-native synthetic GPU presentation (not playable retail)";

/// Runtime guard for OS-driven surface sizes. The CLI has its own, smaller
/// limit; this only prevents an accidental huge window from allocating an
/// unbounded surface.
const MAX_SURFACE_EDGE: u32 = 8192;
const MAX_SURFACE_PIXELS: u64 = 16 * 1024 * 1024;

pub fn run(options: Options) -> Result<(), String> {
    let image = synthetic_demo_image().map_err(|error| error.to_string())?;
    let settings_path = settings::path(options.settings.as_deref());
    let (mut settings, warning) = settings::load(&settings_path);
    if options.fullscreen {
        settings.fullscreen = true;
    }
    if let Some(warning) = warning {
        eprintln!("frontend: {warning}");
    }
    if let Some(path) = &options.audio_out {
        crate::audio_out::write_tone(path, &settings)?;
    }
    let event_loop =
        EventLoop::new().map_err(|error| format!("cannot create window event loop: {error}"))?;
    event_loop.set_control_flow(ControlFlow::Wait);

    let mut frontend = Frontend::new(options, image, settings, settings_path);
    event_loop
        .run_app(&mut frontend)
        .map_err(|error| format!("window event loop failed: {error}"))?;
    frontend.finish()
}

struct Frontend {
    options: Options,
    image: RgbaImage,
    settings: Settings,
    settings_path: PathBuf,
    show_settings: bool,
    input_mask: u16,
    pressed_keys: Vec<Key>,
    pointer_button: Option<Button>,
    pointer: (f32, f32),
    modifiers: ModifiersState,
    window: Option<Arc<Window>>,
    pixels: Option<Pixels<'static>>,
    adapter: Option<String>,
    surface: (u32, u32),
    presented: u32,
    redraw_requests: u32,
    occluded: bool,
    minimized: bool,
    failure: Option<String>,
}

impl Frontend {
    fn new(options: Options, image: RgbaImage, settings: Settings, settings_path: PathBuf) -> Self {
        Self {
            options,
            image,
            settings,
            settings_path,
            show_settings: false,
            input_mask: 0,
            pressed_keys: Vec::new(),
            pointer_button: None,
            pointer: (0.0, 0.0),
            modifiers: ModifiersState::empty(),
            window: None,
            pixels: None,
            adapter: None,
            surface: (0, 0),
            presented: 0,
            redraw_requests: 0,
            occluded: false,
            minimized: false,
            failure: None,
        }
    }

    fn compose(&self) -> RgbaImage {
        if self.show_settings {
            ui::settings_page(&self.settings)
        } else if self.fullscreen() {
            ui::fullscreen(&self.image)
        } else {
            ui::shell(&self.image, self.input_mask)
        }
    }

    fn fullscreen(&self) -> bool {
        self.window
            .as_ref()
            .is_some_and(|window| window.fullscreen().is_some())
            || (self.window.is_none() && self.settings.fullscreen)
    }

    fn persist_settings(&self) {
        if let Err(error) = settings::save(&self.settings_path, &self.settings) {
            eprintln!(
                "frontend: cannot save settings to {}: {error}",
                self.settings_path.display()
            );
        }
    }

    fn initial_size(&self) -> (u32, u32) {
        self.options.initial_size.unwrap_or((ui::WIDTH, ui::HEIGHT))
    }

    fn request_redraw(&self) {
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }

    fn fail(&mut self, event_loop: &ActiveEventLoop, message: impl Into<String>) {
        if self.failure.is_none() {
            self.failure = Some(message.into());
        }
        event_loop.exit();
    }

    fn finish(mut self) -> Result<(), String> {
        self.persist_settings();
        if let Some(message) = self.failure.take() {
            return Err(message);
        }
        if self.options.smoke.is_none() {
            println!(
                "frontend: closed after {} presentation acknowledgement(s)",
                self.presented
            );
        }
        Ok(())
    }

    fn create_window(&mut self, event_loop: &ActiveEventLoop) -> Result<(), String> {
        let (width, height) = self.initial_size();
        let attributes = Window::default_attributes()
            .with_title(TITLE)
            .with_inner_size(PhysicalSize::new(width, height));
        let window = event_loop
            .create_window(attributes)
            .map_err(|error| format!("cannot create native window: {error}"))?;
        let window = Arc::new(window);
        if self.settings.fullscreen {
            window.set_fullscreen(Some(Fullscreen::Borderless(None)));
        }
        self.window = Some(Arc::clone(&window));

        let size = window.inner_size();
        if size.width == 0 || size.height == 0 {
            return Err(format!(
                "window was created with an empty physical size {}x{}",
                size.width, size.height
            ));
        }
        if !surface_size_allowed(size) {
            return Err(format!(
                "initial surface {}x{} exceeds the safe {MAX_SURFACE_EDGE}-edge / \
                 {MAX_SURFACE_PIXELS}-pixel cap",
                size.width, size.height
            ));
        }

        let display = self.compose();
        let surface_texture = SurfaceTexture::new(size.width, size.height, window);
        let mut pixels = PixelsBuilder::new(display.width(), display.height(), surface_texture)
            .enable_vsync(true)
            .build()
            .map_err(|error| {
                format!("GPU initialization failed (adapter, device or surface): {error}")
            })?;
        pixels.set_scaling_mode(ScalingMode::PixelPerfect);
        pixels.clear_color(Color::BLACK);

        let frame = pixels.frame_mut();
        if frame.len() != display.rgba().len() {
            return Err(format!(
                "GPU frame buffer is {} bytes, expected {} for {}x{} RGBA",
                frame.len(),
                display.rgba().len(),
                display.width(),
                display.height()
            ));
        }
        frame.copy_from_slice(display.rgba());

        self.surface = (size.width, size.height);
        self.adapter = Some(describe_adapter(&pixels));
        self.pixels = Some(pixels);
        self.print_startup();
        self.request_redraw();
        Ok(())
    }

    fn print_startup(&self) {
        let Some(pixels) = &self.pixels else {
            return;
        };
        let viewport = compute_viewport(ui::WIDTH, ui::HEIGHT, self.surface.0, self.surface.1);
        println!("{TITLE}");
        println!(
            "source: synthetic demo {}x{} RGB888, sha256(rgb)={}, budget={} guest instructions \
             (static after execution; not frame timing)",
            self.image.width(),
            self.image.height(),
            self.image.rgb_sha256(),
            self.image.guest_budget()
        );
        println!(
            "ui: windowed_gba_shell={} fullscreen={} settings={} touch_controls={} integer_scaling={} volume={} muted={}",
            !self.fullscreen(),
            self.fullscreen(),
            self.settings_path.display(),
            self.settings.touch_controls,
            self.settings.integer_scaling,
            self.settings.master_volume,
            self.settings.muted
        );
        println!(
            "{}",
            self.adapter.as_deref().unwrap_or("adapter: unavailable")
        );
        println!(
            "gpu: surface_format={:?} pixel_format={:?} present_mode={:?} \
             scaling=pixel-perfect-nearest letterbox=black",
            pixels.surface_texture_format(),
            pixels.render_texture_format(),
            pixels.present_mode()
        );
        self.print_surface(self.surface, viewport);
    }

    fn print_surface(&self, surface: (u32, u32), viewport: crate::presentation::Viewport) {
        println!(
            "surface: {}x{} physical; viewport={}x{} at ({},{}) scale={} letterbox=black{}",
            surface.0,
            surface.1,
            viewport.width,
            viewport.height,
            viewport.x,
            viewport.y,
            viewport.scale,
            if viewport.clipped {
                " (source clipped: surface below 1x)"
            } else {
                ""
            }
        );
    }

    fn on_resized(&mut self, event_loop: &ActiveEventLoop, size: PhysicalSize<u32>) {
        if size.width == 0 || size.height == 0 {
            self.minimized = true;
            return;
        }
        self.minimized = false;
        if !surface_size_allowed(size) {
            eprintln!(
                "frontend: ignoring oversized surface resize to {}x{} (cap {MAX_SURFACE_EDGE} edge / \
                 {MAX_SURFACE_PIXELS} pixels)",
                size.width, size.height
            );
            self.request_redraw();
            return;
        }
        if self.surface == (size.width, size.height) {
            self.request_redraw();
            return;
        }

        let Some(pixels) = self.pixels.as_mut() else {
            return;
        };
        if let Err(error) = pixels.resize_surface(size.width, size.height) {
            self.fail(
                event_loop,
                format!(
                    "GPU surface resize to {}x{} failed: {error}",
                    size.width, size.height
                ),
            );
            return;
        }
        self.surface = (size.width, size.height);
        let viewport = compute_viewport(
            self.image.width(),
            self.image.height(),
            size.width,
            size.height,
        );
        self.print_surface(self.surface, viewport);
        self.request_redraw();
    }

    fn present(&mut self, event_loop: &ActiveEventLoop) {
        if self.minimized || self.occluded {
            return;
        }
        let Some(pixels) = self.pixels.as_ref() else {
            return;
        };
        match pixels.render() {
            Ok(()) => {
                self.presented = self.presented.saturating_add(1);
                if let Some(target) = self.options.smoke
                    && self.presented >= target
                {
                    self.report_smoke(target);
                    event_loop.exit();
                }
            }
            Err(error) => {
                self.fail(
                    event_loop,
                    format!(
                        "GPU surface presentation failed after {} acknowledgement(s): {error}",
                        self.presented
                    ),
                );
            }
        }
    }

    fn report_smoke(&self, target: u32) {
        println!(
            "smoke: presented={} requested={} scheduled_redraws={} \
             (Pixels::render acknowledgements, not guest frames; guest image static)",
            self.presented, target, self.redraw_requests
        );
    }
}

fn surface_size_allowed(size: PhysicalSize<u32>) -> bool {
    size.width > 0
        && size.height > 0
        && size.width <= MAX_SURFACE_EDGE
        && size.height <= MAX_SURFACE_EDGE
        && u64::from(size.width) * u64::from(size.height) <= MAX_SURFACE_PIXELS
}

fn describe_adapter(pixels: &Pixels<'_>) -> String {
    let info = pixels.adapter().get_info();
    format!(
        "adapter: name={:?} backend={:?} type={:?} driver={:?} driver_info={:?} \
         vendor=0x{:04X} device=0x{:04X}",
        info.name,
        info.backend,
        info.device_type,
        info.driver,
        info.driver_info,
        info.vendor,
        info.device
    )
}

impl ApplicationHandler for Frontend {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            self.request_redraw();
            return;
        }
        if let Err(error) = self.create_window(event_loop) {
            self.fail(event_loop, error);
        }
    }

    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        match event {
            WindowEvent::CloseRequested => {
                self.persist_settings();
                event_loop.exit();
            }
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event_loop, event),
            WindowEvent::CursorMoved { position, .. } => self.on_cursor(position),
            WindowEvent::MouseInput {
                state,
                button: MouseButton::Left,
                ..
            } => self.on_pointer(state),
            WindowEvent::Touch(touch) => self.on_touch(touch.phase, touch.location),
            WindowEvent::ModifiersChanged(modifiers) => self.modifiers = modifiers.state(),
            WindowEvent::Resized(size) => self.on_resized(event_loop, size),
            WindowEvent::ScaleFactorChanged { .. } => {
                if let Some(window) = &self.window {
                    self.on_resized(event_loop, window.inner_size());
                }
            }
            WindowEvent::Occluded(occluded) => {
                self.occluded = occluded;
                if !occluded {
                    self.request_redraw();
                }
            }
            WindowEvent::RedrawRequested => self.present(event_loop),
            WindowEvent::Destroyed => event_loop.exit(),
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        match self.options.smoke {
            Some(target) => {
                if self.presented >= target {
                    event_loop.exit();
                    return;
                }
                let limit = target
                    .saturating_mul(4)
                    .saturating_add(120)
                    .max(target.saturating_add(1));
                if self.redraw_requests >= limit {
                    self.fail(
                        event_loop,
                        format!(
                            "smoke target not reached: {} of {} presentation acknowledgement(s) \
                             after {} redraw attempt(s); the surface may be occluded, minimised \
                             or timing out",
                            self.presented, target, self.redraw_requests
                        ),
                    );
                    return;
                }
                self.redraw_requests = self.redraw_requests.saturating_add(1);
                self.request_redraw();
            }
            None => {
                // Two bounded warm-up redraw requests cover first-expose and
                // high-DPI startup without turning the static window into a
                // busy loop. Four scheduled requests cap an occluded window.
                if self.presented < 2 && self.redraw_requests < 4 {
                    self.redraw_requests = self.redraw_requests.saturating_add(1);
                    self.request_redraw();
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn title_is_explicit_about_the_synthetic_non_claim() {
        assert!(TITLE.contains("synthetic"));
        assert!(TITLE.contains("not playable retail"));
    }

    #[test]
    fn surface_cap_rejects_absurd_allocations() {
        assert!(surface_size_allowed(PhysicalSize::new(720, 480)));
        assert!(surface_size_allowed(PhysicalSize::new(4096, 4096)));
        assert!(!surface_size_allowed(PhysicalSize::new(0, 480)));
        assert!(!surface_size_allowed(PhysicalSize::new(8193, 160)));
        assert!(!surface_size_allowed(PhysicalSize::new(8192, 8192)));
    }
}
