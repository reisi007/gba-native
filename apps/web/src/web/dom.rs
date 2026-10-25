//! Browser DOM helpers: the canvas, the WebGL2 probe and loss listeners.

use std::sync::Mutex;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::Closure;
use web_sys::{Document, Event, HtmlCanvasElement, WebGlContextEvent, Window};

use super::status;
use crate::model;

pub const CANVAS_ID: &str = "gba-canvas";

static LOST_REASON: Mutex<Option<String>> = Mutex::new(None);

pub fn window() -> Result<Window, String> {
    web_sys::window().ok_or_else(|| "window object is unavailable".to_owned())
}

pub fn document() -> Result<Document, String> {
    window()?
        .document()
        .ok_or_else(|| "document object is unavailable".to_owned())
}

pub fn canvas(id: &str) -> Result<HtmlCanvasElement, String> {
    document()?
        .get_element_by_id(id)
        .ok_or_else(|| format!("canvas element #{id} is missing from the page"))?
        .dyn_into::<HtmlCanvasElement>()
        .map_err(|_| format!("page element #{id} is not a canvas"))
}

/// Return the physical backing size after reconciling Winit's resize event,
/// browser CSS geometry, DPR and the current canvas backing-store readback.
pub fn physical_canvas_size(canvas: &HtmlCanvasElement, reported: (u32, u32)) -> (u32, u32) {
    let rect = canvas.get_bounding_client_rect();
    let ratio = window()
        .map(|window| window.device_pixel_ratio())
        .unwrap_or(1.0);
    model::physical_canvas_size(
        rect.width(),
        rect.height(),
        ratio,
        reported,
        (canvas.width(), canvas.height()),
    )
}

/// Probe WebGL2 on a detached canvas so the real canvas context type stays
/// undecided for WGPU. A negative probe is reported, never faked.
pub fn probe_webgl2(document: &Document) -> bool {
    let Ok(element) = document.create_element("canvas") else {
        return false;
    };
    let Ok(canvas) = element.dyn_into::<HtmlCanvasElement>() else {
        return false;
    };
    matches!(canvas.get_context("webgl2"), Ok(Some(_)))
}

pub fn mark_lost(reason: impl Into<String>) {
    let reason = reason.into();
    if let Ok(mut slot) = LOST_REASON.lock()
        && slot.is_none()
    {
        *slot = Some(reason);
    }
}

pub fn take_lost() -> Option<String> {
    LOST_REASON.lock().ok().and_then(|mut slot| slot.take())
}

/// Register `webglcontextlost` on the real canvas. The handler calls
/// `preventDefault` (as the WebGL specification expects), records the reason and
/// updates the page immediately. Recovery is an explicit restart that creates a
/// fresh canvas by reloading the page once the user asks for it.
pub fn watch_context_loss(canvas: &HtmlCanvasElement, requested: &'static str) {
    let handler = Closure::<dyn FnMut(Event)>::new(move |event: Event| {
        event.prevent_default();
        let detail = event
            .dyn_ref::<WebGlContextEvent>()
            .map(WebGlContextEvent::status_message)
            .filter(|message| !message.is_empty())
            .unwrap_or_else(|| "browser reported webglcontextlost".to_owned());
        let reason = format!("WebGL2 context lost: {detail}");
        super::dom::mark_lost(reason.clone());
        let mut status = super::status::Status::new(
            "lost",
            "The GPU context was lost; rendering is halted. Restart to create a fresh canvas.",
            requested,
        );
        status.detail = Some(reason);
        super::status::emit(&status);
    });
    let _ = canvas
        .add_event_listener_with_callback("webglcontextlost", handler.as_ref().unchecked_ref());
    status::keep_listener(handler);
}
