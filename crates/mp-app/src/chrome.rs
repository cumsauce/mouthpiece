//! Makes the window's title bar part of the top bar, on macOS. The content runs to the top of the window,
//! and the traffic lights are moved to sit centred in our bar. AppKit puts them back whenever it lays the title
//! bar out again, such as during a resize, so this runs every frame. Other systems keep their own title bar.

use eframe::egui;

/// How much room the window's own buttons need at the left of the top bar.
pub fn buttons_inset(ctx: &egui::Context) -> f32 {
    const TRAFFIC_LIGHTS: f32 = 84.0;
    let fullscreen = ctx.input(|i| i.viewport().fullscreen.unwrap_or(false));
    if cfg!(target_os = "macos") && !fullscreen { TRAFFIC_LIGHTS } else { 0.0 }
}

#[cfg(target_os = "macos")]
mod mac {
    use objc2::msg_send;
    use objc2::runtime::AnyObject;
    use objc2_foundation::{NSPoint, NSRect};

    const LEFT: f64 = 14.0;

    /// Where the buttons ended up, and what a click in the bar lands on, for checking this from a script.
    pub struct Report {
        pub buttons: Vec<NSRect>,
        pub bar_click_lands_on: String,
    }

    /// # Safety
    /// `view` must be the window's live NSView, and this must run on the main thread.
    pub unsafe fn place_buttons(view: *mut AnyObject, bar_height: f64) -> Option<Report> {
        unsafe {
            let window: *mut AnyObject = msg_send![view, window];
            if window.is_null() {
                return None;
            }
            let buttons: Vec<*mut AnyObject> = (0usize..3).map(|kind| msg_send![window, standardWindowButton: kind]).collect();
            if buttons.iter().any(|b| b.is_null()) {
                return None;
            }
            let window_frame: NSRect = msg_send![window, frame];
            // The buttons are clipped to the title bar's own view, which needs to be as tall as the bar.
            let holder: *mut AnyObject = msg_send![buttons[0], superview];
            let container: *mut AnyObject = msg_send![holder, superview];
            if container.is_null() {
                return None;
            }
            let mut frame: NSRect = msg_send![container, frame];
            frame.size.height = bar_height;
            frame.origin.y = window_frame.size.height - bar_height;
            let _: () = msg_send![container, setFrame: frame];

            let first: NSRect = msg_send![buttons[0], frame];
            let second: NSRect = msg_send![buttons[1], frame];
            let spacing = second.origin.x - first.origin.x;
            let y = (bar_height - first.size.height) * 0.5;
            let mut placed = Vec::new();
            for (i, button) in buttons.iter().enumerate() {
                let origin = NSPoint::new(LEFT + spacing * i as f64, y);
                let _: () = msg_send![*button, setFrameOrigin: origin];
                let now: NSRect = msg_send![*button, frame];
                placed.push(now);
            }

            let content: *mut AnyObject = msg_send![window, contentView];
            let root: *mut AnyObject = msg_send![content, superview];
            let probe = NSPoint::new(window_frame.size.width * 0.3, window_frame.size.height - bar_height * 0.5);
            let hit: *mut AnyObject = msg_send![root, hitTest: probe];
            let bar_click_lands_on = if hit.is_null() { "nothing".to_string() } else { (*hit).class().name().to_string_lossy().into_owned() };
            Some(Report { buttons: placed, bar_click_lands_on })
        }
    }
}

/// Keeps the window's buttons in place. Returns a description of them, for checking from a script.
#[cfg(target_os = "macos")]
pub fn place_window_buttons(ctx: &egui::Context, frame: &eframe::Frame, bar_height: f32) -> Option<String> {
    use raw_window_handle::{HasWindowHandle, RawWindowHandle};
    if ctx.input(|i| i.viewport().fullscreen.unwrap_or(false)) {
        return None;
    }
    let handle = frame.window_handle().ok()?;
    let RawWindowHandle::AppKit(appkit) = handle.as_raw() else { return None };
    // SAFETY: the handle is this window's own view, and eframe calls `ui` on the main thread.
    let report = unsafe { mac::place_buttons(appkit.ns_view.as_ptr().cast(), bar_height as f64) }?;
    let spots: Vec<String> = report.buttons.iter().map(|r| format!("x {:.0} y {:.0} size {:.0}", r.origin.x, r.origin.y, r.size.width)).collect();
    Some(format!("buttons [{}], a click in the bar lands on {}", spots.join("; "), report.bar_click_lands_on))
}

#[cfg(not(target_os = "macos"))]
pub fn place_window_buttons(_: &egui::Context, _: &eframe::Frame, _: f32) -> Option<String> {
    None
}
