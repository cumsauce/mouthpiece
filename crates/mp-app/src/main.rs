//! Mouthpiece: a sentence-mixing DAW.

mod actions;
mod app;
mod arranger;
mod browser;
mod devices_ui;
mod editor;
mod export_ui;
mod inspector;
mod jobs;
mod lane;
mod mixer;
mod script;
mod shell;
mod theme;
mod timeline;
mod waveform;
mod widgets;

use eframe::egui;

fn main() -> eframe::Result {
    let args: Vec<std::path::PathBuf> = std::env::args_os().skip(1).map(Into::into).collect();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Mouthpiece")
            .with_inner_size([1480.0, 900.0])
            .with_min_inner_size([960.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "Mouthpiece",
        options,
        Box::new(move |cc| {
            theme::install(&cc.egui_ctx);
            Ok(Box::new(app::App::new(cc.egui_ctx.clone(), args)))
        }),
    )
}
