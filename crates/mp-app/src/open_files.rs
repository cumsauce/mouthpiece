//! Opens projects from Finder on macOS. Double-clicking a document doesn't start the app with it as an argument,
//! as on other systems. AppKit asks the application delegate instead, which winit owns and which has no method
//! for this, so one is added to it just before AppKit starts delivering these requests.

use std::path::PathBuf;
use std::sync::Mutex;

static OPENED: Mutex<Vec<PathBuf>> = Mutex::new(Vec::new());

/// What Finder has asked to open since this was last called.
pub fn take() -> Vec<PathBuf> {
    OPENED.lock().map_or_else(|_| Vec::new(), |mut opened| std::mem::take(&mut opened))
}

/// Starts listening. Call before the event loop starts.
pub fn listen() {
    #[cfg(target_os = "macos")]
    mac::listen();
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::rc::Retained;
    use objc2::runtime::{AnyClass, AnyObject, NSObject, Sel};
    use objc2::{define_class, ffi, msg_send, sel, ClassType};
    use objc2_foundation::NSString;
    use std::ffi::CStr;
    use std::os::raw::c_char;

    define_class!(
        #[unsafe(super(NSObject))]
        #[name = "MouthpieceOpenFilesListener"]
        struct Listener;

        impl Listener {
            #[unsafe(method(willFinishLaunching:))]
            fn will_finish_launching(&self, _note: *mut AnyObject) {
                // SAFETY: AppKit sends this on the main thread, with its application running.
                unsafe { add_method_to_delegate() }
            }
        }
    );

    unsafe extern "C-unwind" fn open_files(_this: *mut AnyObject, _cmd: Sel, sender: *mut AnyObject, files: *mut AnyObject) {
        // SAFETY: AppKit calls this with an NSApplication and an NSArray of NSStrings.
        unsafe {
            let count: usize = msg_send![files, count];
            for i in 0..count {
                let file: *mut AnyObject = msg_send![files, objectAtIndex: i];
                let text: *const c_char = msg_send![file, UTF8String];
                if !text.is_null() {
                    if let Ok(mut opened) = super::OPENED.lock() {
                        opened.push(CStr::from_ptr(text).to_string_lossy().into_owned().into());
                    }
                }
            }
            const REPLY_SUCCESS: usize = 0;
            let _: () = msg_send![sender, replyToOpenOrPrint: REPLY_SUCCESS];
        }
    }

    unsafe fn add_method_to_delegate() {
        let Some(application) = AnyClass::get(c"NSApplication") else { return };
        // SAFETY: called on the main thread. The method has the signature its selector calls for, as `types` says.
        unsafe {
            let app: *mut AnyObject = msg_send![application, sharedApplication];
            let delegate: *mut AnyObject = msg_send![app, delegate];
            if delegate.is_null() {
                return;
            }
            let class = (*delegate).class() as *const AnyClass as *mut AnyClass;
            let method = open_files as unsafe extern "C-unwind" fn(*mut AnyObject, Sel, *mut AnyObject, *mut AnyObject);
            ffi::class_addMethod(class, sel!(application:openFiles:), std::mem::transmute(method), c"v@:@@".as_ptr());
        }
    }

    pub fn listen() {
        let Some(center) = AnyClass::get(c"NSNotificationCenter") else { return };
        // SAFETY: plain Objective-C calls on the main thread, before the event loop starts.
        unsafe {
            let listener: Retained<NSObject> = msg_send![Listener::class(), new];
            let center: *mut AnyObject = msg_send![center, defaultCenter];
            let name = NSString::from_str("NSApplicationWillFinishLaunchingNotification");
            let nothing: *mut AnyObject = std::ptr::null_mut();
            let _: () = msg_send![center, addObserver: &*listener, selector: sel!(willFinishLaunching:), name: &*name, object: nothing];
            // Lives as long as the app, and the notification center doesn't keep it alive.
            std::mem::forget(listener);
        }
    }
}
