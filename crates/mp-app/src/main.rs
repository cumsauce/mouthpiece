//! Mouthpiece: a sentence-mixing DAW.

// On Windows, release builds shouldn't open a console window alongside the app.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

// The test script's state dump is one large JSON literal.
#![recursion_limit = "256"]

mod actions;
mod app;
mod chrome;
mod ds;
mod jobs;
mod open_files;
mod panels;
mod script;
mod settings;
mod shell;

use eframe::egui;

/// Shows the design system's components instead of the app, for working on them.
struct Gallery {
    state: ds::gallery::GalleryState,
    frames: u32,
}

impl eframe::App for Gallery {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        ds::gallery::show(ui, &mut self.state);
        // MOUTHPIECE_GALLERY=out.png saves a screenshot and quits, to check changes without a person.
        self.frames += 1;
        ui.ctx().request_repaint();
        let target = std::env::var("MOUTHPIECE_GALLERY").unwrap_or_default();
        if target.ends_with(".png") && self.frames == 10 {
            let ctx = ui.ctx().clone();
            ui.ctx().request_screenshot(move |image| {
                script::save_png(&target, &image);
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            });
        }
    }
}

fn main() -> eframe::Result {
    open_files::listen();
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mouthpiece")
            .with_inner_size([1480.0, 900.0])
            .with_min_inner_size([1100.0, 640.0])
            .with_drag_and_drop(true)
            // On macOS the top bar is the title bar. See chrome.rs. Elsewhere these do nothing.
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    eframe::run_native(
        "Mouthpiece",
        options,
        Box::new(move |cc| {
            ds::install(&cc.egui_ctx);
            if std::env::var_os("MOUTHPIECE_GALLERY").is_some() {
                return Ok(Box::new(Gallery { state: Default::default(), frames: 0 }) as Box<dyn eframe::App>);
            }
            Ok(Box::new(app::App::new(cc.egui_ctx.clone(), args)))
        }),
    )
}
