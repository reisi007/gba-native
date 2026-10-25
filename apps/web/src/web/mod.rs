//! Browser entry points.
//!
//! `start` runs once per page. A retry with another backend deliberately
//! reloads the page: a canvas context type cannot be changed after creation and
//! Pixels/WGPU fixes its instance at build time. The page bounds those reloads.

pub mod app;
mod dom;
mod gpu;
mod status;

use std::cell::Cell;
use std::rc::Rc;

use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::wasm_bindgen;
use winit::event_loop::{ControlFlow, EventLoop};
use winit::platform::web::EventLoopExtWebSys;

use crate::model::{BackendChoice, BrowserProbe, resolve_backend};
use crate::presentation;

pub use app::App;

thread_local! {
    static STARTED: Cell<bool> = const { Cell::new(false) };
}

pub fn trace(message: &str) {
    web_sys::console::log_1(&JsValue::from_str(&format!("gba-native-web: {message}")));
}

/// Install the JavaScript status sink. The page owns all DOM text.
#[wasm_bindgen]
pub fn set_status_sink(sink: js_sys::Function) {
    status::set_sink(sink);
}

/// Drain an asynchronous device-loss report recorded by the WGPU callback.
/// Returns the typed reason once; the page then shows the halted state.
#[wasm_bindgen]
pub fn poll_gpu_status() -> Option<String> {
    dom::take_lost()
}

#[wasm_bindgen]
pub async fn start(backend: String) -> Result<(), JsValue> {
    status::install_panic_hook();
    if STARTED.with(|started| started.replace(true)) {
        let status = status::Status::new(
            "error",
            "The synthetic demo is already running. Use Restart to load a fresh canvas.",
            "page",
        );
        status::emit(&status);
        return Ok(());
    }
    start_inner(&backend).await;
    Ok(())
}

async fn start_inner(backend: &str) {
    let requested = backend.to_owned();
    let choice = match BackendChoice::parse(backend) {
        Ok(choice) => choice,
        Err(error) => {
            status::emit(&status::Status::new("error", error.to_string(), &requested));
            return;
        }
    };
    let document = match dom::document() {
        Ok(document) => document,
        Err(error) => {
            status::emit(&status::Status::new("error", error, &requested));
            return;
        }
    };
    let canvas = match dom::canvas(dom::CANVAS_ID) {
        Ok(canvas) => canvas,
        Err(error) => {
            status::emit(&status::Status::new("error", error, &requested));
            return;
        }
    };

    let source = match presentation::synthetic_demo_image() {
        Ok(source) => source,
        Err(error) => {
            status::emit(&status::Status::new(
                "error",
                format!("the synthetic core demo failed: {error}"),
                &requested,
            ));
            return;
        }
    };
    status::set_source(source.rgb_sha256(), source.guest_budget());

    status::emit(&status::Status::new(
        "loading",
        format!(
            "Executed the synthetic fixture: {}x{}, sha256(rgb)={}, budget={} guest instructions. \
             Probing browser GPU support…",
            source.width(),
            source.height(),
            source.rgb_sha256(),
            source.guest_budget()
        ),
        &requested,
    ));

    let webgpu = if choice == BackendChoice::WebGl2 {
        false
    } else {
        pixels::wgpu::util::is_browser_webgpu_supported().await
    };
    let probe = BrowserProbe {
        webgpu,
        webgl2: dom::probe_webgl2(&document),
    };
    let resolution = match resolve_backend(choice, probe) {
        Ok(resolution) => resolution,
        Err(error) => {
            let mut status = status::Status::new("error", error.to_string(), &requested);
            status.detail = Some(format!(
                "probe: webgpu={} webgl2={}",
                probe.webgpu, probe.webgl2
            ));
            status::emit(&status);
            return;
        }
    };
    trace(&format!(
        "backend requested={} resolved={} auto_fallback={}",
        choice.id(),
        resolution.backend.id(),
        resolution.auto_fallback
    ));

    let mut loading = status::Status::new(
        "loading",
        format!(
            "Initializing {} (requested {}). Deferred until the canvas reports a non-zero physical size.",
            resolution.backend.label(),
            choice.id()
        ),
        &requested,
    );
    loading.actual = Some(resolution.backend.id().to_owned());
    loading.auto_fallback = resolution.auto_fallback;
    loading.viewport = None;
    status::emit(&loading);

    dom::watch_context_loss(&canvas, choice.id());

    let event_loop = match EventLoop::new() {
        Ok(event_loop) => event_loop,
        Err(error) => {
            status::emit(&status::Status::new(
                "error",
                format!("the browser event loop could not start: {error}"),
                &requested,
            ));
            return;
        }
    };
    event_loop.set_control_flow(ControlFlow::Wait);
    let app = App::new(canvas, choice, resolution, Rc::new(source));
    event_loop.spawn_app(app);
}
