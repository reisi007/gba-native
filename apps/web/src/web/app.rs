//! Winit web lifecycle around one real Pixels/WGPU surface.
//!
//! The guest bitmap is executed once by the std-only core. This module creates
//! the canvas surface, presents the static image with nearest/pixel-perfect
//! integer scaling and reports typed adapter and error data. It has no game
//! loop, no input binding and no cartridge surface.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use web_sys::HtmlCanvasElement;
use winit::application::ApplicationHandler;
use winit::dpi::PhysicalSize;
use winit::event::WindowEvent;
use winit::event_loop::ActiveEventLoop;
use winit::platform::web::WindowAttributesExtWebSys;
use winit::window::{Window, WindowId};

use crate::model::{
    BackendChoice, BrowserBackend, Resolution, Viewport, compute_viewport, surface_size_allowed,
};
use crate::presentation::RgbaImage;

use super::gpu::{self, GpuError};
use super::status::{self, DeviceReport, Status};
use super::{dom, trace};

pub const TITLE: &str = "gba-native synthetic browser GPU demo (not playable)";
const MAX_LAYOUT_ATTEMPTS: u32 = 240;

pub struct App {
    canvas: Option<HtmlCanvasElement>,
    state: Rc<RefCell<UiState>>,
}

pub struct UiState {
    pub choice: BackendChoice,
    pub resolved: Resolution,
    pub window: Option<Arc<Window>>,
    pub size: (u32, u32),
    pub pixels: Option<pixels::Pixels<'static>>,
    pub report: Option<DeviceReport>,
    pub source: Option<Rc<RgbaImage>>,
    pub init_in_flight: bool,
    pub layout_attempts: u32,
    pub failed: bool,
    pub lost: bool,
    pub presented: u32,
    pub viewport: Option<Viewport>,
}

impl UiState {
    fn status(&self, state: &'static str, message: impl Into<String>) -> Status {
        let mut status = Status::new(state, message, self.choice.id());
        status.actual = Some(self.resolved.backend.id().to_owned());
        status.auto_fallback = self.resolved.auto_fallback;
        status.report = self.report.clone();
        status.viewport = self.viewport;
        status.presented = self.presented;
        status
    }
}

impl App {
    pub fn new(
        canvas: HtmlCanvasElement,
        choice: BackendChoice,
        resolved: Resolution,
        source: Rc<RgbaImage>,
    ) -> Self {
        let state = UiState {
            choice,
            resolved,
            window: None,
            size: (0, 0),
            pixels: None,
            report: None,
            source: Some(source),
            init_in_flight: false,
            layout_attempts: 0,
            failed: false,
            lost: false,
            presented: 0,
            viewport: None,
        };
        Self {
            canvas: Some(canvas),
            state: Rc::new(RefCell::new(state)),
        }
    }

    fn request_redraw(&self) {
        if let Some(window) = self.state.borrow().window.as_ref() {
            window.request_redraw();
        }
    }

    fn emit_error(&self, message: impl Into<String>, detail: Option<String>) {
        let mut status = {
            let state = self.state.borrow();
            state.status("error", message)
        };
        status.detail = detail;
        status::emit(&status);
        let mut state = self.state.borrow_mut();
        state.failed = true;
        state.init_in_flight = false;
    }

    fn begin_init(&self) {
        let initialization = {
            let mut state = self.state.borrow_mut();
            if state.init_in_flight || state.pixels.is_some() || state.failed || state.lost {
                return;
            }
            let (Some(window), Some(source)) = (state.window.clone(), state.source.clone()) else {
                return;
            };
            if state.size.0 == 0 || state.size.1 == 0 {
                return;
            }
            if !surface_size_allowed(state.size.0, state.size.1) {
                Err(surface_cap_detail(state.size))
            } else {
                state.init_in_flight = true;
                Ok((window, state.size, state.resolved, source))
            }
        };
        let (window, size, resolved, source) = match initialization {
            Ok(initialization) => initialization,
            Err(detail) => {
                self.emit_error(
                    "the canvas exceeds the conservative surface allocation cap",
                    Some(detail),
                );
                return;
            }
        };
        trace("initializing Pixels/WGPU surface");
        let state = Rc::clone(&self.state);
        wasm_bindgen_futures::spawn_local(async move {
            match gpu::build(Arc::clone(&window), size, resolved).await {
                Ok(built) => finish_success(&state, built, size, &source),
                Err(error) => emit_gpu_failure(&state, &error),
            }
        });
    }
}

fn finish_success(
    state: &Rc<RefCell<UiState>>,
    built: gpu::BuiltSurface,
    built_size: (u32, u32),
    source: &RgbaImage,
) {
    let mut built = built;
    if let Err(message) = gpu::copy_frame(&mut built.pixels, source) {
        let mut status = state.borrow().status("error", message);
        status.detail =
            Some("the GPU pixel buffer and the validated RGBA image disagree".to_owned());
        status::emit(&status);
        let mut guard = state.borrow_mut();
        guard.failed = true;
        guard.init_in_flight = false;
        return;
    }

    let mut guard = state.borrow_mut();
    if guard.failed || guard.lost {
        return;
    }
    let current = guard.size;
    let resized = if current != built_size && surface_size_allowed(current.0, current.1) {
        built.pixels.resize_surface(current.0, current.1).is_ok()
    } else {
        true
    };
    if !resized {
        drop(guard);
        let mut status = state.borrow().status(
            "error",
            "the GPU surface could not follow the canvas size after initialization",
        );
        status.detail = Some(format!(
            "initialized at {}x{}, current canvas is {}x{}",
            built_size.0, built_size.1, current.0, current.1
        ));
        status::emit(&status);
        let mut guard = state.borrow_mut();
        guard.failed = true;
        guard.init_in_flight = false;
        return;
    }
    guard.report = Some(built.report);
    guard.pixels = Some(built.pixels);
    guard.init_in_flight = false;
    emit_ready(&guard);
    if let Some(window) = guard.window.as_ref() {
        window.request_redraw();
    }
}

fn emit_gpu_failure(state: &Rc<RefCell<UiState>>, error: &GpuError) {
    let (mut status, auto_fallback) = {
        let guard = state.borrow();
        let auto_fallback =
            guard.choice == BackendChoice::Auto && guard.resolved.backend == BrowserBackend::WebGpu;
        (
            guard.status(
                "error",
                "GPU initialization failed; no frame was presented.",
            ),
            auto_fallback,
        )
    };
    status.detail = Some(error.to_string());
    if auto_fallback {
        status.action = Some("reload-webgl2");
        status.message = "WebGPU initialization failed. A single bounded retry with the WebGL2 compatibility path is available.".to_owned();
    }
    status::emit(&status);
    let mut guard = state.borrow_mut();
    guard.failed = true;
    guard.init_in_flight = false;
}

fn emit_ready(state: &UiState) {
    if state.lost || state.failed {
        return;
    }
    let status = state.status(
        "ready",
        "Synthetic demo image presented. The guest image is static after one execution.",
    );
    status::emit(&status);
}

fn surface_cap_detail(size: (u32, u32)) -> String {
    format!(
        "{}x{} exceeds the {} edge / {} pixel design cap; resize the window",
        size.0,
        size.1,
        crate::model::MAX_SURFACE_EDGE,
        crate::model::MAX_SURFACE_PIXELS
    )
}

fn emit_lost(state: &Rc<RefCell<UiState>>, reason: String) {
    let mut status = state.borrow().status(
        "lost",
        "The GPU context/device was lost; rendering is halted. Restart to create a fresh canvas.",
    );
    status.detail = Some(reason);
    status::emit(&status);
    state.borrow_mut().lost = true;
}

fn emit_viewport(state: &UiState) {
    if state.lost || state.failed {
        return;
    }
    let status = state.status(
        "ready",
        "Synthetic demo image presented. The guest image is static after one execution.",
    );
    status::emit(&status);
}

impl ApplicationHandler for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.state.borrow().window.is_some() {
            self.request_redraw();
            return;
        }
        let Some(canvas) = self.canvas.as_ref().cloned() else {
            self.request_redraw();
            return;
        };
        let attributes = Window::default_attributes()
            .with_title(TITLE)
            .with_canvas(Some(canvas.clone()))
            .with_append(false)
            .with_focusable(true);
        match event_loop.create_window(attributes) {
            Ok(window) => {
                let window = Arc::new(window);
                let reported = window.inner_size();
                let size = dom::physical_canvas_size(&canvas, (reported.width, reported.height));
                if canvas.width() != size.0 {
                    canvas.set_width(size.0);
                }
                if canvas.height() != size.1 {
                    canvas.set_height(size.1);
                }
                let mut state = self.state.borrow_mut();
                state.size = size;
                state.window = Some(Arc::clone(&window));
                drop(state);
                window.request_redraw();
            }
            Err(error) => {
                self.emit_error(
                    "the browser could not create a canvas surface",
                    Some(error.to_string()),
                );
            }
        }
    }

    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if self
            .state
            .borrow()
            .window
            .as_ref()
            .is_none_or(|window| window.id() != window_id)
        {
            return;
        }
        match event {
            WindowEvent::Resized(size) => self.on_resized(size),
            WindowEvent::ScaleFactorChanged { .. } => {
                let size = self
                    .state
                    .borrow()
                    .window
                    .as_ref()
                    .map(|window| window.inner_size());
                if let Some(size) = size {
                    self.on_resized(size);
                }
            }
            WindowEvent::RedrawRequested => self.on_redraw(),
            _ => {}
        }
    }
}

impl App {
    fn on_resized(&mut self, size: PhysicalSize<u32>) {
        if let Some(reason) = dom::take_lost() {
            emit_lost(&self.state, reason);
            return;
        }
        let mut failure: Option<(String, Option<String>)> = None;
        let mut ready = false;
        let reported = if let Some(canvas) = self.canvas.as_ref() {
            let physical = dom::physical_canvas_size(canvas, (size.width, size.height));
            if canvas.width() != physical.0 {
                canvas.set_width(physical.0);
            }
            if canvas.height() != physical.1 {
                canvas.set_height(physical.1);
            }
            physical
        } else {
            (size.width, size.height)
        };
        {
            let mut state = self.state.borrow_mut();
            state.size = reported;
            if reported.0 == 0 || reported.1 == 0 {
                // A hidden/zero canvas must not allocate a 0x0 GPU surface.
                state.viewport = None;
                return;
            }
            if !surface_size_allowed(reported.0, reported.1) {
                state.failed = true;
                failure = Some((
                    "the canvas exceeds the conservative surface allocation cap".to_owned(),
                    Some(surface_cap_detail(reported)),
                ));
            } else {
                state.viewport = Some(compute_viewport(
                    crate::model::IMAGE_WIDTH,
                    crate::model::IMAGE_HEIGHT,
                    reported.0,
                    reported.1,
                ));
                if let Some(pixels) = state.pixels.as_mut() {
                    if let Err(error) = pixels.resize_surface(reported.0, reported.1) {
                        state.failed = true;
                        failure = Some((
                            "the GPU surface could not be resized to the canvas size".to_owned(),
                            Some(error.to_string()),
                        ));
                    } else {
                        ready = true;
                    }
                }
            }
        }
        if let Some((message, detail)) = failure {
            self.emit_error(message, detail);
            return;
        }
        if ready {
            emit_viewport(&self.state.borrow());
        }
        self.request_redraw();
    }

    fn on_redraw(&mut self) {
        if let Some(reason) = dom::take_lost() {
            emit_lost(&self.state, reason);
            return;
        }
        let has_pixels = {
            let state = self.state.borrow();
            if state.failed || state.lost {
                return;
            }
            state.pixels.is_some()
        };
        if !has_pixels {
            let (window, size, attempts) = {
                let mut state = self.state.borrow_mut();
                let Some(window) = state.window.clone() else {
                    return;
                };
                state.layout_attempts = state.layout_attempts.saturating_add(1);
                (window, state.size, state.layout_attempts)
            };
            if size.0 == 0 || size.1 == 0 {
                if attempts >= MAX_LAYOUT_ATTEMPTS {
                    self.emit_error(
                        "the canvas reported a zero physical size; layout never settled",
                        Some(
                            "deferred initialization avoids creating a 0x0 GPU surface".to_owned(),
                        ),
                    );
                    return;
                }
                window.request_redraw();
                return;
            }
            self.begin_init();
            return;
        }

        let rendered = {
            let state = self.state.borrow();
            let Some(pixels) = state.pixels.as_ref() else {
                return;
            };
            pixels.render().map_err(|error| error.to_string())
        };
        match rendered {
            Ok(()) => {
                let presented = {
                    let mut state = self.state.borrow_mut();
                    state.presented = state.presented.saturating_add(1);
                    state.presented
                };
                // One acknowledgement per requested redraw; the guest image is
                // static, so this is not a frame loop.
                if presented == 1 {
                    emit_viewport(&self.state.borrow());
                }
            }
            Err(message) => {
                let presented = self.state.borrow().presented;
                self.emit_error(
                    format!("GPU presentation failed after {presented} acknowledgement(s)"),
                    Some(format!(
                        "{message}; the surface has not been reported as a visible frame"
                    )),
                );
            }
        }
    }
}
