//! Structured status bridge to the static page.
//!
//! Rust builds a small JavaScript object and hands it to a callback installed by
//! `app.js`. The page owns all DOM text; this module owns the facts. Every
//! ready status carries the observed adapter/backend data, the source hash and
//! the instruction budget, so the UI can never invent a successful fallback.

use std::cell::RefCell;

use wasm_bindgen::JsValue;
use wasm_bindgen::prelude::Closure;
use web_sys::console;

use crate::model::Viewport;

thread_local! {
    static SINK: RefCell<Option<js_sys::Function>> = const { RefCell::new(None) };
    static SOURCE: RefCell<Option<(String, u64)>> = const { RefCell::new(None) };
    static PANIC_INSTALLED: RefCell<bool> = const { RefCell::new(false) };
}

type EventListener = Closure<dyn FnMut(web_sys::Event)>;

#[derive(Clone, Debug)]
pub struct DeviceReport {
    pub adapter_name: String,
    pub backend: String,
    pub device_type: String,
    pub driver: String,
    pub driver_info: String,
    pub vendor: u32,
    pub device: u32,
    pub surface_format: String,
    pub render_format: String,
    pub present_mode: String,
    pub max_texture_dimension_2d: u32,
    pub max_buffer_size: u64,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub state: &'static str,
    pub message: String,
    pub requested: String,
    pub actual: Option<String>,
    pub auto_fallback: bool,
    pub action: Option<&'static str>,
    pub detail: Option<String>,
    pub report: Option<DeviceReport>,
    pub viewport: Option<Viewport>,
    pub presented: u32,
}

impl Status {
    pub fn new(state: &'static str, message: impl Into<String>, requested: &str) -> Self {
        Self {
            state,
            message: message.into(),
            requested: requested.to_owned(),
            ..Self::default()
        }
    }
}

pub fn set_sink(sink: js_sys::Function) {
    SINK.with(|slot| *slot.borrow_mut() = Some(sink));
}

pub fn set_source(sha256: &str, guest_budget: u64) {
    SOURCE.with(|slot| *slot.borrow_mut() = Some((sha256.to_owned(), guest_budget)));
    install_panic_hook();
}

pub fn emit(status: &Status) {
    let Some(payload) = to_object(status) else {
        return;
    };
    SINK.with(|slot| {
        if let Some(sink) = slot.borrow().as_ref()
            && let Err(error) = sink.call1(&JsValue::NULL, &payload)
        {
            console::error_1(&error);
        }
    });
}

fn to_object(status: &Status) -> Option<js_sys::Object> {
    let object = js_sys::Object::new();
    set_str(&object, "state", status.state);
    set_str(&object, "message", &status.message);
    set_str(&object, "requested", &status.requested);
    set_opt_str(&object, "actual", status.actual.as_deref());
    set_bool(&object, "autoFallback", status.auto_fallback);
    set_opt_str(&object, "action", status.action);
    set_opt_str(&object, "detail", status.detail.as_deref());
    set_f64(&object, "presented", f64::from(status.presented));

    SOURCE.with(|slot| {
        if let Some((sha256, budget)) = slot.borrow().as_ref() {
            set_str(&object, "sourceSha256", sha256);
            set_f64(&object, "guestBudget", *budget as f64);
        }
    });
    set_f64(&object, "imageWidth", f64::from(crate::model::IMAGE_WIDTH));
    set_f64(
        &object,
        "imageHeight",
        f64::from(crate::model::IMAGE_HEIGHT),
    );
    set_f64(
        &object,
        "surfaceCapEdge",
        f64::from(crate::model::MAX_SURFACE_EDGE),
    );
    set_f64(
        &object,
        "surfaceCapPixels",
        crate::model::MAX_SURFACE_PIXELS as f64,
    );

    if let Some(viewport) = status.viewport {
        set_f64(&object, "viewportX", f64::from(viewport.x));
        set_f64(&object, "viewportY", f64::from(viewport.y));
        set_f64(&object, "viewportWidth", f64::from(viewport.width));
        set_f64(&object, "viewportHeight", f64::from(viewport.height));
        set_f64(&object, "viewportScale", f64::from(viewport.scale));
        set_bool(&object, "viewportClipped", viewport.clipped);
    }

    if let Some(report) = &status.report {
        set_str(&object, "adapterName", &report.adapter_name);
        set_str(&object, "backend", &report.backend);
        set_str(&object, "deviceType", &report.device_type);
        set_str(&object, "driver", &report.driver);
        set_str(&object, "driverInfo", &report.driver_info);
        set_f64(&object, "vendorId", f64::from(report.vendor));
        set_f64(&object, "deviceId", f64::from(report.device));
        set_str(&object, "surfaceFormat", &report.surface_format);
        set_str(&object, "renderFormat", &report.render_format);
        set_str(&object, "presentMode", &report.present_mode);
        set_f64(
            &object,
            "maxTextureDimension2d",
            f64::from(report.max_texture_dimension_2d),
        );
        set_f64(&object, "maxBufferSize", report.max_buffer_size as f64);
    }

    Some(object)
}

fn set_str(object: &js_sys::Object, key: &str, value: &str) {
    let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), &JsValue::from_str(value));
}

fn set_opt_str(object: &js_sys::Object, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        set_str(object, key, value);
    } else {
        let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), &JsValue::NULL);
    }
}

fn set_bool(object: &js_sys::Object, key: &str, value: bool) {
    let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), &JsValue::from_bool(value));
}

fn set_f64(object: &js_sys::Object, key: &str, value: f64) {
    let _ = js_sys::Reflect::set(object, &JsValue::from_str(key), &JsValue::from_f64(value));
}

pub fn console_error(message: &str) {
    console::error_1(&JsValue::from_str(message));
}

/// Replace the default wasm panic output with one console error. A panic still
/// fails the page (and the E2E suite observes it); this only makes the reason
/// readable instead of a bare `unreachable`.
pub fn install_panic_hook() {
    PANIC_INSTALLED.with(|installed| {
        if *installed.borrow() {
            return;
        }
        *installed.borrow_mut() = true;
        let previous = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            console_error(&format!("gba-native-web panic: {info}"));
            previous(info);
        }));
    });
}

/// Keep listener closures alive for the page lifetime.
pub fn keep_listener(closure: EventListener) {
    thread_local! {
        static LISTENERS: RefCell<Vec<EventListener>> = const { RefCell::new(Vec::new()) };
    }
    LISTENERS.with(|slot| slot.borrow_mut().push(closure));
}
