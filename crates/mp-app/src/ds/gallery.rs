//! Every component in one screen. Run with MOUTHPIECE_GALLERY=1 to check a change to the design system.

use super::*;
use eframe::egui::{self, Ui};

#[derive(Default)]
pub struct GalleryState {
    choice: u8,
    tab: u8,
    on: bool,
    value: f64,
    pitch: f64,
    text: String,
    knob: f32,
    fader: f32,
}

pub fn show(ui: &mut Ui, s: &mut GalleryState) {
    egui::CentralPanel::default().frame(egui::Frame::NONE.fill(PANEL).inner_margin(egui::Margin::same(SP_6 as i8))).show(ui, |ui| {
        egui::ScrollArea::vertical().auto_shrink(false).show(ui, |ui| {
            ui.set_max_width(ui.available_width().min(760.0));
            ui.label(title("Design system"));
            ui.label(caption("Everything panels may build with. See docs/DESIGN.md."));
            section(ui, "Type");
            ui.label(title("Title: the name of what's selected"));
            ui.label(heading("Heading: panels and groups"));
            ui.label(strong("Strong: emphasis at body size"));
            ui.label(body("Body: default text"));
            ui.label(caption("Caption: secondary information"));
            ui.label(micro("Micro: dense annotations"));
            ui.label(num("Num 01:23.456"));

            section(ui, "Buttons");
            ui.horizontal(|ui| {
                ui.add(Button::primary("Primary"));
                ui.add(Button::secondary("Secondary"));
                ui.add(Button::ghost("Ghost"));
                ui.add(Button::secondary("Active").active(true));
                ui.add(Button::secondary("Disabled").enabled(false));
                ui.add(Button::secondary("With icon").icon(Icon::Wand));
                ui.add(Button::ghost("Small").small());
            });

            section(ui, "Icon buttons");
            ui.horizontal_wrapped(|ui| {
                for icon in [
                    Icon::Play, Icon::Pause, Icon::ToStart, Icon::Loop, Icon::Metronome, Icon::Magnet, Icon::Plus, Icon::Minus,
                    Icon::Close, Icon::Search, Icon::Power, Icon::Reverse, Icon::Undo, Icon::Redo, Icon::PanelLeft, Icon::PanelBottom,
                    Icon::PanelRight, Icon::ChevronLeft, Icon::ChevronRight, Icon::ChevronDown, Icon::Repeat, Icon::Split, Icon::Trash,
                    Icon::Wand, Icon::Note, Icon::Faster, Icon::Slower, Icon::Commands, Icon::Check, Icon::Crossfade, Icon::Link, Icon::Bookmark, Icon::Restore, Icon::Compare, Icon::Keyframe, Icon::Expand,
                ] {
                    ui.add(IconButton::new(icon, "Tooltip").shortcut("⌘K"));
                }
                ui.add(IconButton::new(Icon::Loop, "Active").active(true));
                ui.add(IconButton::new(Icon::Play, "Tinted").active(true).tint(GOOD));
                letter_toggle(ui, "M", s.on, MUTE, "Mute", Some("M"));
                letter_toggle(ui, "S", !s.on, SOLO, "Solo", None);
            });

            section(ui, "Choices");
            ui.horizontal(|ui| {
                segmented(ui, &mut s.choice, &[(0, "Elastic"), (1, "Tape"), (2, "Third")]);
                switch(ui, &mut s.on, "Switch");
                dropdown(ui, "gallery-dropdown", "Dropdown", 90.0, |ui| {
                    menu_choice(ui, &mut s.choice, 0, "First");
                    menu_choice(ui, &mut s.choice, 1, "Second");
                });
            });
            tabs(ui, &mut s.tab, &[(0, "One"), (1, "Two"), (2, "Three")]);

            section(ui, "Values");
            ui.add(ValueField::new("Gain", &mut s.value, -48.0..=24.0).unit(" dB").default(0.0).width(320.0));
            ui.add(ValueField::new("Pitch", &mut s.pitch, -24.0..=24.0).unit(" st").bipolar().default(0.0).width(320.0));
            text_field(ui, &mut s.text, "Text field", 200.0);
            info_row(ui, "Source", "Lead Vocals");

            section(ui, "Status");
            ui.horizontal(|ui| {
                status_dot(ui, Tone::Good, "Good");
                status_dot(ui, Tone::Warn, "Warn");
                status_dot(ui, Tone::Bad, "Bad");
                status_dot(ui, Tone::Neutral, "Neutral");
                keycap(ui, "⌘K");
            });
            progress(ui, Some(0.4), 160.0);
            progress(ui, None, 160.0);
            hint_row(ui, "Return", "A keycap and what it does");

            section(ui, "Audio");
            ui.horizontal(|ui| {
                knob(ui, &mut s.knob, 0.5, "Knob", "0.50", false, track_color(0));
                knob(ui, &mut s.knob, 0.5, "Bipolar", "0.50", true, track_color(1));
                fader(ui, &mut s.fader, egui::vec2(16.0, 66.0));
                fader(ui, &mut s.fader, egui::vec2(96.0, 16.0));
                let (rect, _) = ui.allocate_exact_size(egui::vec2(8.0, 66.0), egui::Sense::hover());
                meter(ui.painter(), rect, [0.3, 0.8]);
                let (rect, _) = ui.allocate_exact_size(egui::vec2(100.0, 12.0), egui::Sense::hover());
                meter(ui.painter(), rect, [0.3, 1.2]);
            });

            section(ui, "Surfaces");
            ui.horizontal(|ui| {
                for (name, color) in [("Base", BASE), ("Panel", PANEL), ("Raised", RAISED), ("Control", CONTROL), ("Hover", CONTROL_HOVER), ("Accent", ACCENT), ("Dim", ACCENT_DIM)] {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(60.0, 40.0), egui::Sense::hover());
                    ui.painter().rect_filled(rect, R_S, color);
                    ui.painter().text(rect.center(), egui::Align2::CENTER_CENTER, name, font_micro(), TEXT);
                }
            });
            ui.horizontal(|ui| {
                for i in 0..8 {
                    let (rect, _) = ui.allocate_exact_size(egui::vec2(24.0, 24.0), egui::Sense::hover());
                    ui.painter().circle_filled(rect.center(), 10.0, track_color(i));
                }
            });
            section(ui, "Floating bar");
            floating_frame().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(strong("word"));
                    divider_v(ui);
                    ui.add(IconButton::new(Icon::Minus, "Down").small());
                    ui.label(num("+2.0"));
                    ui.add(IconButton::new(Icon::Plus, "Up").small());
                });
            });
        });
    });
}
