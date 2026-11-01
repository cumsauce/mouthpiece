//! The design system. Panels build their interfaces only from what's here.
//! Read docs/DESIGN.md before adding to it or using it.

mod audio;
mod controls;
mod feedback;
pub mod gallery;
mod icons;
mod layout;
mod tokens;

pub use audio::*;
pub use controls::*;
pub use feedback::*;
pub use icons::*;
pub use layout::*;
pub use tokens::*;

#[cfg(test)]
mod conformance {
    //! Enforces docs/DESIGN.md, so the design system can't quietly erode.

    use std::path::{Path, PathBuf};

    fn panel_sources() -> Vec<(PathBuf, String)> {
        let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
        let mut files = vec![src.join("shell.rs")];
        for entry in std::fs::read_dir(src.join("panels")).unwrap() {
            files.push(entry.unwrap().path());
        }
        files.into_iter().map(|p| (p.clone(), std::fs::read_to_string(p).unwrap())).collect()
    }

    /// Panels build from `ds`. Each item here has a replacement there, named in the message.
    #[test]
    fn panels_only_use_design_system_components() {
        const FORBIDDEN: &[(&str, &str)] = &[
            ("Color32::from_rgb", "add a token to ds/tokens.rs"),
            ("Color32::from_gray", "add a token to ds/tokens.rs"),
            ("FontId::", "use a font_* token"),
            ("RichText::new", "use caption(), body(), strong(), heading(), title(), micro() or num()"),
            ("egui::Button", "use ds::Button or ds::IconButton"),
            ("ui.button(", "use ds::Button"),
            ("small_button(", "use ds::Button::small() or ds::IconButton"),
            ("egui::Slider", "use ds::ValueField"),
            ("egui::DragValue", "use ds::ValueField"),
            ("egui::Checkbox", "use ds::switch"),
            (".checkbox(", "use ds::switch"),
            ("selectable_label", "use ds::segmented or ds::Button::active"),
            ("selectable_value", "use ds::segmented or ds::menu_choice"),
            ("ComboBox", "use ds::dropdown"),
            ("ui.menu_button", "use ds::menu_button"),
            ("egui::Window", "use ds::modal"),
            ("ProgressBar", "use ds::progress"),
            ("on_hover_text", "use ds::tip, which can show the shortcut too"),
        ];
        let mut problems = Vec::new();
        for (path, text) in panel_sources() {
            for (needle, fix) in FORBIDDEN {
                if text.contains(needle) {
                    problems.push(format!("{}: `{needle}`: {fix}", path.display()));
                }
            }
        }
        assert!(problems.is_empty(), "\n{}", problems.join("\n"));
    }

    /// Accent means "selected, playing or the main action". Used elsewhere, it stops meaning anything.
    #[test]
    fn accent_stays_scarce() {
        const ALLOWED: &[&str] = &["lane.rs", "arranger.rs", "editor.rs", "say.rs", "overlays.rs"];
        for (path, text) in panel_sources() {
            let name = path.file_name().unwrap().to_str().unwrap().to_string();
            if !ALLOWED.contains(&name.as_str()) {
                assert!(!text.contains("ACCENT"), "{name} uses ACCENT. See \"Colour\" in docs/DESIGN.md.");
            }
        }
    }
}
