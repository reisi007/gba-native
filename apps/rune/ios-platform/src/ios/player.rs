use objc2::mutability::MainThreadOnly;
use objc2::rc::{Allocated, Retained};
use objc2::runtime::{NSObject, NSObjectProtocol};
use objc2::{ClassType, DeclaredClass, declare_class, extern_class, extern_methods, msg_send_id};
use objc2_foundation::{MainThreadMarker, NSError, NSURL};
use std::path::Path;
use std::sync::{Mutex, OnceLock};

extern_class!(
    pub struct AVAudioPlayer;

    unsafe impl ClassType for AVAudioPlayer {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
    }
);

extern_methods!(
    unsafe impl AVAudioPlayer {
        #[method_id(@__retain_semantics Init initWithContentsOfURL:error:)]
        pub unsafe fn init_with_contents_of_url_error(
            this: Allocated<Self>,
            url: &NSURL,
            error: *mut *mut NSError,
        ) -> Option<Retained<Self>>;

        #[method(play)]
        pub unsafe fn play(&self) -> bool;

        #[method(setDelegate:)]
        pub unsafe fn set_delegate(&self, delegate: Option<&NSObject>);
    }
);

declare_class!(
    struct TonePlayerDelegate;

    unsafe impl ClassType for TonePlayerDelegate {
        type Super = NSObject;
        type Mutability = MainThreadOnly;
        const NAME: &'static str = "RuneTonePlayerDelegate";
    }

    impl DeclaredClass for TonePlayerDelegate {
        type Ivars = ();
    }

    unsafe impl TonePlayerDelegate {
        #[method(audioPlayerDidFinishPlaying:successfully:)]
        fn audio_player_did_finish_playing(
            &self,
            _player: &AVAudioPlayer,
            _successfully: bool,
        ) {
            schedule_release(self as *const Self as *mut std::ffi::c_void);
        }

        #[method(audioPlayerDecodeErrorDidOccur:error:)]
        fn audio_player_decode_error(
            &self,
            _player: &AVAudioPlayer,
            _error: Option<&NSError>,
        ) {
            schedule_release(self as *const Self as *mut std::ffi::c_void);
        }
    }

    unsafe impl NSObjectProtocol for TonePlayerDelegate {}
);

#[link(name = "AVFoundation", kind = "framework")]
unsafe extern "C" {
    #[link_name = "OBJC_CLASS_$_AVAudioPlayer"]
    static AV_AUDIO_PLAYER_CLASS: &'static objc2::runtime::AnyClass;
}

static PLAYERS: OnceLock<Mutex<Vec<(usize, usize)>>> = OnceLock::new();

#[link(name = "System")]
unsafe extern "C" {
    fn dispatch_get_main_queue() -> *mut std::ffi::c_void;
    fn dispatch_async_f(
        queue: *mut std::ffi::c_void,
        context: *mut std::ffi::c_void,
        work: extern "C" fn(*mut std::ffi::c_void),
    );
}

/// Ask AVFoundation to play the core-generated WAV file at `path`.
pub fn play_test_tone(path: &Path) -> Result<(), String> {
    let mtm = MainThreadMarker::new()
        .ok_or_else(|| "iOS audio playback must be requested on the main thread".to_owned())?;
    debug_assert!(std::ptr::eq(
        unsafe { AV_AUDIO_PLAYER_CLASS },
        AVAudioPlayer::class()
    ));
    let url = file_url(path)?;
    let player = unsafe {
        AVAudioPlayer::init_with_contents_of_url_error(mtm.alloc(), &url, std::ptr::null_mut())
    };
    let player =
        player.ok_or_else(|| "AVAudioPlayer could not open the WAV artifact".to_owned())?;
    let delegate: Retained<TonePlayerDelegate> =
        unsafe { msg_send_id![TonePlayerDelegate::class(), new] };
    let delegate_object: &NSObject = &*delegate;
    unsafe { player.set_delegate(Some(delegate_object)) };
    let delegate_pointer = std::ptr::from_ref(&*delegate) as usize;
    retain_during_playback(player.clone(), delegate.clone());
    let started = unsafe { player.play() };
    if !started {
        release_retained(delegate_pointer);
        return Err("AVAudioPlayer did not accept the WAV playback request".to_owned());
    }

    Ok(())
}

fn file_url(path: &Path) -> Result<Retained<NSURL>, String> {
    use std::os::unix::ffi::OsStrExt;

    let c_path = std::ffi::CString::new(path.as_os_str().as_bytes())
        .map_err(|_| "WAV artifact path contains a NUL byte".to_owned())?;
    let path = std::ptr::NonNull::new(c_path.as_ptr().cast_mut())
        .ok_or_else(|| "WAV artifact path is empty".to_owned())?;
    Ok(unsafe {
        NSURL::fileURLWithFileSystemRepresentation_isDirectory_relativeToURL(path, false, None)
    })
}

fn retain_during_playback(player: Retained<AVAudioPlayer>, delegate: Retained<TonePlayerDelegate>) {
    let players = PLAYERS.get_or_init(|| Mutex::new(Vec::new()));
    players
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push((
            Retained::into_raw(player) as usize,
            Retained::into_raw(delegate) as usize,
        ));
}

fn schedule_release(delegate: *mut std::ffi::c_void) {
    // The callback may arrive on a non-main thread. Queue cleanup so AVAudioPlayer
    // and its MainThreadOnly delegate are released on the main thread, after the
    // delegate callback has returned.
    unsafe { dispatch_async_f(dispatch_get_main_queue(), delegate, release_on_main) };
}

extern "C" fn release_on_main(delegate: *mut std::ffi::c_void) {
    debug_assert!(MainThreadMarker::new().is_some());
    release_retained(delegate as usize);
}

fn release_retained(delegate: usize) {
    let Some(players) = PLAYERS.get() else {
        return;
    };
    let retained = {
        let mut players = players
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(index) = players.iter().position(|(_, pointer)| *pointer == delegate) else {
            return;
        };
        players.swap_remove(index)
    };
    // SAFETY: Both raw references were created from owned `Retained` values
    // above, and removing the pair transfers each ownership back exactly once.
    unsafe {
        drop(Retained::<AVAudioPlayer>::from_raw(
            retained.0 as *mut AVAudioPlayer,
        ));
        drop(Retained::<TonePlayerDelegate>::from_raw(
            retained.1 as *mut TonePlayerDelegate,
        ));
    }
}
