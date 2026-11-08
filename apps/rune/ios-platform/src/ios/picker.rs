use crate::staging;
use objc2::mutability::MainThreadOnly;
use objc2::rc::Retained;
use objc2::runtime::{NSObjectProtocol, ProtocolObject};
use objc2::{ClassType, DeclaredClass, declare_class, msg_send_id};
use objc2_foundation::{MainThreadMarker, NSArray, NSObject, NSString, NSURL};
#[allow(deprecated)]
use objc2_ui_kit::UIDocumentPickerMode;
use objc2_ui_kit::{
    UIApplication, UIDocumentPickerDelegate, UIDocumentPickerViewController, UIViewController,
};
use objc2_uniform_type_identifiers::{UTType, UTTypeData};
use std::cell::RefCell;
use std::ffi::CStr;
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

#[derive(Default)]
struct PickerState {
    staging_path: Option<PathBuf>,
    lifecycle: staging::ImportLifecycle,
}

fn picker_state() -> &'static Mutex<PickerState> {
    static STATE: OnceLock<Mutex<PickerState>> = OnceLock::new();
    STATE.get_or_init(|| Mutex::new(PickerState::default()))
}

thread_local! {
    static DELEGATE: RefCell<Option<Retained<SavePickerDelegate>>> = const { RefCell::new(None) };
}

declare_class!(
    struct SavePickerDelegate;

    unsafe impl ClassType for SavePickerDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "RuneSavePickerDelegate";
    }

    impl DeclaredClass for SavePickerDelegate {
        type Ivars = ();
    }

    unsafe impl SavePickerDelegate {
        #[method(documentPicker:didPickDocumentsAtURLs:)]
        fn did_pick(&self, picker: &UIDocumentPickerViewController, urls: &NSArray<NSURL>) {
            let result = unsafe { urls.firstObject() }
                .ok_or_else(|| "the document picker returned no file".to_owned())
                .and_then(stage_selected_url);
            complete(result);
            dismiss_picker(picker);
        }

        #[method(documentPickerWasCancelled:)]
        fn was_cancelled(&self, picker: &UIDocumentPickerViewController) {
            complete(Err("save import cancelled".to_owned()));
            dismiss_picker(picker);
        }
    }

    unsafe impl UIDocumentPickerDelegate for SavePickerDelegate {}
    unsafe impl NSObjectProtocol for SavePickerDelegate {}
);

impl SavePickerDelegate {
    fn retained() -> Retained<Self> {
        DELEGATE.with(|slot| {
            let mut slot = slot.borrow_mut();
            if let Some(delegate) = slot.as_ref() {
                return delegate.clone();
            }
            // SAFETY: UIKit delegate objects are created on the verified main
            // thread, and this class inherits NSObject's standard initializer.
            let delegate: Retained<Self> = unsafe { msg_send_id![Self::class(), new] };
            *slot = Some(delegate.clone());
            delegate
        })
    }
}

/// Present a native picker restricted to the dynamic `.sav` uniform type.
#[allow(deprecated)] // The typed iOS 14 initializer is absent in this cached binding release.
pub fn present_save_import_picker(staging_path: &Path) -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "the iOS document picker must be opened on the main thread".to_owned())?;
    let presenter = current_presenter(mtm)?;
    let save_type = unsafe {
        UTType::typeWithFilenameExtension_conformingToType(&NSString::from_str("sav"), UTTypeData)
    }
    .ok_or_else(|| "iOS could not create the .sav document type".to_owned())?;
    let save_identifier = unsafe { save_type.identifier() };
    let allowed_types = unsafe { NSArray::<NSString>::arrayWithObject(&save_identifier) };
    let picker = unsafe {
        UIDocumentPickerViewController::initWithDocumentTypes_inMode(
            mtm.alloc(),
            &allowed_types,
            UIDocumentPickerMode::Open,
        )
    };
    unsafe { picker.setAllowsMultipleSelection(false) };
    let delegate = SavePickerDelegate::retained();
    unsafe {
        picker.setDelegate(Some(ProtocolObject::from_ref(&*delegate)));
    }

    {
        let mut state = picker_state()
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        state.lifecycle.begin(staging_path)?;
        state.staging_path = Some(staging_path.to_path_buf());
    }

    unsafe { presenter.presentViewController_animated_completion(&picker, true, None) };
    Ok(())
}

/// Take the most recent asynchronous picker result after the user returns to Rune.
pub fn take_save_import_result() -> Option<Result<(), String>> {
    picker_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .lifecycle
        .take_result()
}

#[allow(deprecated)] // Rune currently presents a single foreground UIKit scene.
fn current_presenter(mtm: MainThreadMarker) -> Result<Retained<UIViewController>, String> {
    let app = UIApplication::sharedApplication(mtm);
    let window =
        unsafe { app.keyWindow() }.ok_or_else(|| "no active iOS window is available".to_owned())?;
    let mut presenter = window
        .rootViewController()
        .ok_or_else(|| "the iOS window has no root view controller".to_owned())?;
    while let Some(next) = unsafe { presenter.presentedViewController() } {
        presenter = next;
    }
    Ok(presenter)
}

fn stage_selected_url(url: Retained<NSURL>) -> Result<(), String> {
    let access = SecurityScope::begin(&url)?;
    let path = selected_path(&url)?;
    let destination = picker_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .staging_path
        .clone()
        .ok_or_else(|| "save import destination is unavailable".to_owned())?;
    let source = std::fs::File::open(&path)
        .map_err(|error| format!("cannot open selected save: {error}"))?;
    let result = staging::stage_save(source, &path, &destination)
        .map(|_| ())
        .map_err(|error| format!("cannot stage selected save: {error}"));
    if result.is_err() {
        let _ = staging::clear_staging(&destination);
    }
    drop(access);
    result
}

fn selected_path(url: &NSURL) -> Result<PathBuf, String> {
    // SAFETY: NSURL returns a NUL-terminated path string valid while `url` lives.
    let raw = unsafe { url.fileSystemRepresentation() };
    let path = unsafe { CStr::from_ptr(raw.as_ptr()) };
    let path = path
        .to_str()
        .map_err(|_| "selected save path is not valid UTF-8".to_owned())?;
    let path = PathBuf::from(path);
    if !path
        .extension()
        .and_then(std::ffi::OsStr::to_str)
        .is_some_and(|extension| extension.eq_ignore_ascii_case("sav"))
    {
        return Err("select a .sav file".to_owned());
    }
    Ok(path)
}

struct SecurityScope<'a> {
    url: &'a NSURL,
}

impl<'a> SecurityScope<'a> {
    fn begin(url: &'a NSURL) -> Result<Self, String> {
        let started = unsafe { url.startAccessingSecurityScopedResource() };
        if started {
            Ok(Self { url })
        } else {
            Err("iOS did not grant access to the selected document".to_owned())
        }
    }
}

impl Drop for SecurityScope<'_> {
    fn drop(&mut self) {
        // SAFETY: `begin` succeeded, and this guard calls stop exactly once.
        unsafe { self.url.stopAccessingSecurityScopedResource() };
    }
}

fn complete(result: Result<(), String>) {
    let mut state = picker_state()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    state.lifecycle.complete(result);
}

fn dismiss_picker(picker: &UIDocumentPickerViewController) {
    unsafe { picker.dismissViewControllerAnimated_completion(true, None) };
}
