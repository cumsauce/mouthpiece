//! The window: top bar, panels, status bar and overlays.

use crate::actions::{shortcut_text, Action, ACTIONS};
use crate::app::{App, ToastKind};
use crate::jobs::file_stem;
use crate::theme::*;
use crate::timeline::{format_bars, format_time};
use crate::widgets::{self, icon_button, icon_toggle, keycap, keys, meter, Icon};
use eframe::egui::{self, Align2, CornerRadius, Key, Rect, Sense, Stroke, Ui};
use mp_model::MusicalKey;

impl eframe::App for App {
    fn raw_input_hook(&mut self, ctx: &egui::Context, raw_input: &mut egui::RawInput) {
        raw_input.dropped_files.append(&mut self.script_dropped);
        if let Some((events, modifiers)) = self.inject_frames.pop_front() {
            raw_input.events.push(egui::Event::ModifiersChanged(modifiers));
            raw_input.events.extend(events);
            ctx.request_repaint();
        }
    }

    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        self.hint.clear();
        self.frame_start();
        crate::script::tick(self, &ctx);
        self.handle_drops(&ctx);
        if !self.palette_open && self.export_dialog.is_none() {
            self.handle_shortcuts(&ctx);
        } else if ctx.input_mut(|i| i.consume_shortcut(&egui::KeyboardShortcut::new(egui::Modifiers::COMMAND, Key::K))) {
            self.palette_open = false;
        }

        let bar = |fill| egui::Frame::NONE.fill(fill).inner_margin(egui::Margin::symmetric(8, 5));
        egui::Panel::top("topbar").frame(bar(BG_2)).exact_size(40.0).show(ui, |ui| self.top_bar(ui));
        egui::Panel::bottom("status").frame(bar(BG_0)).exact_size(24.0).show(ui, |ui| self.status_bar(ui));
        if self.show_bottom {
            egui::Panel::bottom("bottom")
                .frame(egui::Frame::NONE.fill(BG_1).inner_margin(egui::Margin::symmetric(0, 3)))
                .resizable(true)
                .default_size(290.0)
                .size_range(150.0..=720.0)
                .show(ui, |ui| self.bottom_panel(ui));
        }
        if self.show_inspector {
            egui::Panel::left("inspector")
                .frame(egui::Frame::NONE.fill(BG_1).inner_margin(egui::Margin::same(10)))
                .resizable(true)
                .default_size(224.0)
                .size_range(190.0..=360.0)
                .show(ui, |ui| self.inspector_panel(ui));
        }
        if self.show_browser {
            egui::Panel::right("browser")
                .frame(egui::Frame::NONE.fill(BG_1).inner_margin(egui::Margin::same(8)))
                .resizable(true)
                .default_size(290.0)
                .size_range(230.0..=520.0)
                .show(ui, |ui| self.browser_panel(ui));
        }
        egui::CentralPanel::default().frame(egui::Frame::NONE.fill(BG_0)).show(ui, |ui| self.arranger(ui));

        self.export_window(&ctx);
        if self.palette_open {
            self.palette(&ctx);
        }
        if self.show_shortcuts {
            self.shortcuts_window(&ctx);
        }
        self.drag_ghost(&ctx);
        self.drop_overlay(&ctx);
        self.toasts_overlay(&ctx);

        let title = format!("{}{} — Mouthpiece", self.project.name, if self.is_dirty() { " ●" } else { "" });
        ctx.send_viewport_cmd(egui::ViewportCommand::Title(title));

        // Redraw when something on screen is moving. Idle otherwise, to leave the CPU to audio.
        if self.engine.is_playing() || self.engine.shared.audition_total.load(std::sync::atomic::Ordering::Relaxed) > 0 || self.master_hold.iter().any(|p| *p > 0.001) {
            ctx.request_repaint();
        } else if !self.jobs.active.is_empty() || self.cache.pending() > 0 || !self.toasts.is_empty() || self.needs_compile {
            ctx.request_repaint_after(std::time::Duration::from_millis(60));
        }
        if self.script.is_some() {
            ctx.request_repaint();
        }
    }
}

impl App {
    fn handle_drops(&mut self, ctx: &egui::Context) {
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        for file in dropped {
            let path = file.path().to_path_buf();
            if path.extension().is_some_and(|e| e == crate::app::FILE_EXTENSION) {
                self.open_project_at(&path);
            } else {
                self.import_path(path);
            }
        }
    }

    fn top_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            self.file_menu(ui);
            ui.add_space(10.0);

            if icon_button(ui, Icon::ToStart, "Stop and return (Return)").clicked() {
                self.run(Action::Stop);
            }
            let playing = self.engine.is_playing();
            let icon = if playing { Icon::Pause } else { Icon::Play };
            if icon_toggle(ui, icon, playing, GOOD, "Play or pause (Space)").clicked() {
                self.run(Action::PlayPause);
            }
            if icon_toggle(ui, Icon::Loop, self.project.loop_region.enabled, ACCENT, "Loop (L). Shift-drag on the ruler to set it.").clicked() {
                self.run(Action::ToggleLoop);
            }
            if icon_toggle(ui, Icon::Metronome, self.project.metronome, ACCENT, "Metronome (K)").clicked() {
                self.run(Action::ToggleMetronome);
            }
            ui.add_space(8.0);
            self.time_readout(ui);
            ui.add_space(8.0);
            self.tempo_controls(ui);
            ui.add_space(8.0);

            if icon_toggle(ui, Icon::Magnet, self.snap, ACCENT, &keys("Snap (N). Hold ⌘ while dragging to bypass.")).clicked() {
                self.snap = !self.snap;
            }
            let label = grid_label(self.grid_division);
            egui::ComboBox::from_id_salt("grid").selected_text(label).width(62.0).show_ui(ui, |ui| {
                for value in [4.0, 2.0, 1.0, 0.5, 0.25, 0.125] {
                    ui.selectable_value(&mut self.grid_division, value, grid_label(value));
                }
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if widgets::accent_button(ui, "Render").on_hover_text(keys("Render to MP3 or WAV (⌘R)")).clicked() {
                    self.run(Action::Export);
                }
                ui.add_space(8.0);
                let peak = self.engine.shared.master_peak();
                for c in 0..2 {
                    self.master_hold[c] = peak[c].max(self.master_hold[c] * 0.86);
                }
                let (rect, response) = ui.allocate_exact_size(egui::vec2(110.0, 14.0), Sense::hover());
                meter(ui.painter(), rect, self.master_hold);
                response.on_hover_text("Master level");
                ui.add_space(6.0);
                if ui.button(egui::RichText::new(keys("⌘K")).font(mono(10.5)).color(TEXT_DIM)).on_hover_text("All commands").clicked() {
                    self.run(Action::Palette);
                }
                for (on, label, action, tip) in [
                    (self.show_browser, "Words", Action::ToggleBrowser, "Word browser (B)"),
                    (self.show_bottom, "Editor", Action::ToggleEditor, "Bottom panel (Tab)"),
                    (self.show_inspector, "Inspector", Action::ToggleInspector, "Inspector (I)"),
                ] {
                    if ui.selectable_label(on, label).on_hover_text(tip).clicked() {
                        self.run(action);
                    }
                }
            });
        });
    }

    fn file_menu(&mut self, ui: &mut Ui) {
        ui.menu_button(egui::RichText::new("Mouthpiece").strong().color(ACCENT), |ui| {
            let entries = [
                (Action::New, "New project", "⌘N"),
                (Action::Open, "Open…", "⌘O"),
                (Action::Save, "Save", "⌘S"),
                (Action::SaveAs, "Save as…", "⇧⌘S"),
                (Action::Import, "Import audio…", "⌘I"),
                (Action::Export, "Render…", "⌘R"),
            ];
            for (action, name, key) in entries {
                if ui.add(egui::Button::new(name).shortcut_text(keys(key))).clicked() {
                    self.run(action);
                    ui.close();
                }
            }
            ui.separator();
            let undo = self.history.undo_label().map(|l| format!("Undo {l}"));
            let redo = self.history.redo_label().map(|l| format!("Redo {l}"));
            if ui.add_enabled(undo.is_some(), egui::Button::new(undo.unwrap_or_else(|| "Undo".to_string())).shortcut_text(keys("⌘Z"))).clicked() {
                self.undo();
                ui.close();
            }
            if ui.add_enabled(redo.is_some(), egui::Button::new(redo.unwrap_or_else(|| "Redo".to_string())).shortcut_text(keys("⇧⌘Z"))).clicked() {
                self.redo();
                ui.close();
            }
            ui.separator();
            if ui.button("Keyboard shortcuts").clicked() {
                self.show_shortcuts = true;
                ui.close();
            }
        });
    }

    fn time_readout(&mut self, ui: &mut Ui) {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(160.0, 28.0), Sense::hover());
        let painter = ui.painter();
        painter.rect_filled(rect, CornerRadius::same(5), BG_0);
        painter.text(rect.left_center() + egui::vec2(8.0, 0.0), Align2::LEFT_CENTER, format_bars(&self.project, self.playhead), mono(14.0), TEXT);
        painter.text(rect.right_center() - egui::vec2(8.0, 0.0), Align2::RIGHT_CENTER, format_time(self.playhead), mono(10.5), TEXT_DIM);
    }

    fn tempo_controls(&mut self, ui: &mut Ui) {
        let mut bpm = self.project.bpm;
        let r = ui.add(egui::DragValue::new(&mut bpm).range(20.0..=400.0).speed(0.1).fixed_decimals(1).suffix(" BPM"));
        if r.drag_started() || r.gained_focus() {
            self.begin_gesture("Change tempo");
        }
        if r.changed() {
            self.tempo_user_set = true;
            self.project.bpm = bpm;
            self.needs_compile = true;
        }
        if r.drag_stopped() || r.lost_focus() {
            self.end_gesture();
        }
        if ui.button("Tap").on_hover_text("Tap along to set the tempo").clicked() {
            self.tap_tempo();
        }
        let key_label = self.project.key.map_or("No key".to_string(), |k| k.label());
        let current = self.project.key;
        let mut chosen = current;
        egui::ComboBox::from_id_salt("key").selected_text(key_label).width(70.0).show_ui(ui, |ui| {
            ui.selectable_value(&mut chosen, None, "No key");
            for minor in [false, true] {
                for root in 0..12 {
                    let key = MusicalKey { root, minor };
                    ui.selectable_value(&mut chosen, Some(key), key.label());
                }
            }
        });
        if chosen != current {
            self.edit("Change key", |p| p.key = chosen);
        }
    }

    fn status_bar(&mut self, ui: &mut Ui) {
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 8.0;
            let jobs: Vec<(u64, String, Option<f32>)> = self.jobs.active.iter().map(|j| (j.id, j.label.clone(), j.fraction)).collect();
            let hint = self.hint.clone();
            ui.label(egui::RichText::new(hint).size(11.0).color(TEXT_DIM));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let load = self.engine.shared.cpu_load();
                let color = if load > 0.8 { BAD } else if load > 0.5 { WARN } else { TEXT_FAINT };
                ui.label(egui::RichText::new(format!("{:.0}% DSP", load * 100.0)).font(mono(10.0)).color(color));
                ui.label(egui::RichText::new(format!("{} kHz", self.sample_rate as f32 / 1000.0)).font(mono(10.0)).color(TEXT_FAINT));
                if let Some(label) = self.history.undo_label() {
                    ui.label(egui::RichText::new(format!("Undo: {label}")).size(10.5).color(TEXT_FAINT));
                }
                let renders = self.cache.pending();
                if renders > 0 {
                    ui.label(egui::RichText::new(format!("Rendering {renders} clip{}", if renders == 1 { "" } else { "s" })).size(10.5).color(WARN));
                }
                for (id, label, fraction) in jobs {
                    if widgets::tiny_icon(ui, Icon::Close, TEXT_FAINT, "Cancel").clicked() {
                        if let Some(job) = self.jobs.active.iter().find(|j| j.id == id) {
                            job.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
                        }
                    }
                    let bar = egui::ProgressBar::new(fraction.unwrap_or(0.0)).desired_width(70.0).desired_height(6.0);
                    let bar = if fraction.is_none() { bar.animate(true) } else { bar };
                    ui.add(bar.fill(ACCENT));
                    ui.label(egui::RichText::new(label).size(10.5).color(TEXT_DIM));
                }
            });
        });
    }

    fn palette(&mut self, ctx: &egui::Context) {
        let query = self.palette_query.to_lowercase();
        let matches: Vec<usize> = ACTIONS
            .iter()
            .enumerate()
            .filter(|(_, (_, name, _, group))| {
                let hay = format!("{} {}", name.to_lowercase(), group.to_lowercase());
                query.split_whitespace().all(|q| hay.contains(q))
            })
            .map(|(i, _)| i)
            .collect();
        let screen = ctx.content_rect();
        let mut run = None;
        let mut close = false;
        egui::Area::new(egui::Id::unique("palette-scrim")).order(egui::Order::Foreground).fixed_pos(screen.min).show(ctx, |ui| {
            let response = ui.allocate_response(screen.size(), Sense::click());
            ui.painter().rect_filled(screen, 0.0, alpha(BG_0, 140));
            if response.clicked() {
                close = true;
            }
        });
        egui::Area::new(egui::Id::unique("palette"))
            .order(egui::Order::Tooltip)
            .fixed_pos(egui::pos2(screen.center().x - 280.0, screen.top() + 90.0))
            .show(ctx, |ui| {
                egui::Frame::NONE
                    .fill(BG_2)
                    .stroke(Stroke::new(1.0, BG_4))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(egui::Margin::same(8))
                    .shadow(egui::epaint::Shadow { offset: [0, 10], blur: 40, spread: 0, color: alpha(BG_0, 180) })
                    .show(ui, |ui| {
                        ui.set_width(560.0);
                        let edit = ui.add(
                            egui::TextEdit::singleline(&mut self.palette_query)
                                .id(egui::Id::unique("palette-input"))
                                .desired_width(f32::INFINITY)
                                .frame(egui::Frame::NONE)
                                .font(egui::FontId::proportional(16.0))
                                .hint_text("Type a command"),
                        );
                        edit.request_focus();
                        if edit.changed() {
                            self.palette_index = 0;
                        }
                        let (up, down, enter, escape) = ui.input_mut(|i| {
                            (
                                i.consume_key(egui::Modifiers::NONE, Key::ArrowUp),
                                i.consume_key(egui::Modifiers::NONE, Key::ArrowDown),
                                i.key_pressed(Key::Enter),
                                i.key_pressed(Key::Escape),
                            )
                        });
                        if down {
                            self.palette_index = (self.palette_index + 1).min(matches.len().saturating_sub(1));
                        }
                        if up {
                            self.palette_index = self.palette_index.saturating_sub(1);
                        }
                        if escape {
                            close = true;
                        }
                        if enter {
                            run = matches.get(self.palette_index).map(|i| ACTIONS[*i].0);
                            close = true;
                        }
                        ui.add_space(4.0);
                        ui.separator();
                        egui::ScrollArea::vertical().max_height(360.0).show(ui, |ui| {
                            ui.spacing_mut().item_spacing.y = 0.0;
                            for (row, index) in matches.iter().enumerate() {
                                let (action, name, shortcut, group) = ACTIONS[*index];
                                let (rect, response) = ui.allocate_exact_size(egui::vec2(ui.available_width(), 28.0), Sense::click());
                                let selected = row == self.palette_index;
                                if selected || response.hovered() {
                                    ui.painter().rect_filled(rect, CornerRadius::same(5), if selected { ACCENT_SOFT } else { BG_3 });
                                }
                                if selected && (up || down) {
                                    ui.scroll_to_rect(rect, None);
                                }
                                ui.painter().text(rect.left_center() + egui::vec2(8.0, 0.0), Align2::LEFT_CENTER, name, ui_font(13.0), TEXT);
                                let mut right = group.to_string();
                                if let Some(s) = shortcut {
                                    right = format!("{}    {}", group, shortcut_text(&s));
                                }
                                ui.painter().text(rect.right_center() - egui::vec2(8.0, 0.0), Align2::RIGHT_CENTER, right, ui_font(11.0), TEXT_FAINT);
                                if response.clicked() {
                                    run = Some(action);
                                    close = true;
                                }
                            }
                            if matches.is_empty() {
                                ui.add_space(10.0);
                                ui.label(egui::RichText::new("No commands match").color(TEXT_FAINT));
                            }
                        });
                    });
            });
        if close {
            self.palette_open = false;
        }
        if let Some(action) = run {
            self.run(action);
        }
    }

    fn shortcuts_window(&mut self, ctx: &egui::Context) {
        let mut open = true;
        egui::Window::new("Keyboard shortcuts")
            .open(&mut open)
            .collapsible(false)
            .anchor(Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .default_size(egui::vec2(760.0, 560.0))
            .show(ctx, |ui| {
                egui::ScrollArea::vertical().show(ui, |ui| {
                    ui.columns(3, |columns| {
                        let groups = ["Transport", "Edit", "Clip", "Sentence mixing", "View", "File", "Track"];
                        for (i, group) in groups.iter().enumerate() {
                            let ui = &mut columns[(i * 3) / groups.len()];
                            ui.label(egui::RichText::new(*group).strong().color(ACCENT));
                            for (_, name, shortcut, g) in ACTIONS {
                                if g == group {
                                    if let Some(s) = shortcut {
                                        ui.horizontal(|ui| {
                                            keycap(ui, &shortcut_text(s));
                                            ui.label(egui::RichText::new(*name).size(11.0).color(TEXT_DIM));
                                        });
                                    }
                                }
                            }
                            ui.add_space(8.0);
                        }
                    });
                    ui.add_space(6.0);
                    ui.label(egui::RichText::new("Mouse").strong().color(ACCENT));
                    for line in [
                        "⌘-scroll or pinch zooms. Shift-scroll or a horizontal swipe pans.",
                        "Drag a clip edge to trim. Alt-drag an edge to stretch. Alt-drag a clip to copy it.",
                        "Drag the dots at a clip's top corners to set fades. Hold ⌘ to bypass snapping.",
                        "Double-click a clip or track to open it in the editor.",
                    ] {
                        ui.label(egui::RichText::new(keys(line)).size(11.0).color(TEXT_DIM));
                    }
                });
            });
        self.show_shortcuts = open && !ctx.input(|i| i.key_pressed(Key::Escape));
    }

    /// Follows the pointer while a word is being dragged out of the browser or editor.
    fn drag_ghost(&mut self, ctx: &egui::Context) {
        let Some(hit) = &self.hit_drag else { return };
        ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
        if let Some(p) = ctx.pointer_latest_pos() {
            let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("word-ghost")));
            let galley = painter.layout_no_wrap(hit.text.clone(), ui_font(12.0), BG_0);
            let r = Rect::from_min_size(p + egui::vec2(10.0, 10.0), galley.size() + egui::vec2(12.0, 6.0));
            painter.rect_filled(r, CornerRadius::same(4), ACCENT);
            painter.galley(r.min + egui::vec2(6.0, 3.0), galley, BG_0);
        }
        // Released away from the timeline: the arranger only handles it over the lanes area.
        if ctx.input(|i| i.pointer.any_released()) {
            self.hit_drag = None;
        }
    }

    fn drop_overlay(&self, ctx: &egui::Context) {
        let hovering: Vec<String> = ctx.input(|i| {
            i.raw.hovered_files.iter().filter_map(|f| f.path.as_ref().map(|p| file_stem(p))).collect()
        });
        if hovering.is_empty() {
            return;
        }
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, egui::Id::unique("drop")));
        painter.rect_filled(screen, 0.0, alpha(BG_0, 190));
        let inner = screen.shrink(24.0);
        painter.rect_stroke(inner, CornerRadius::same(14), Stroke::new(2.0, ACCENT), egui::StrokeKind::Inside);
        let count = hovering.len();
        let title = format!("Drop to add {count} track{}", if count == 1 { "" } else { "s" });
        painter.text(screen.center(), Align2::CENTER_CENTER, title, ui_font(24.0), TEXT);
    }

    fn toasts_overlay(&self, ctx: &egui::Context) {
        let screen = ctx.content_rect();
        let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("toasts")));
        let mut y = screen.top() + 50.0;
        for toast in &self.toasts {
            let age = toast.born.elapsed().as_secs_f32();
            let fade = ((3.5 - age) / 0.4).clamp(0.0, 1.0);
            let color = match toast.kind {
                ToastKind::Info => TEXT_DIM,
                ToastKind::Good => GOOD,
                ToastKind::Bad => BAD,
            };
            let galley = painter.layout_no_wrap(toast.text.clone(), ui_font(12.5), TEXT);
            let r = Rect::from_center_size(egui::pos2(screen.center().x, y + 14.0), galley.size() + egui::vec2(30.0, 12.0));
            painter.rect_filled(r, CornerRadius::same(7), alpha(BG_3, (fade * 245.0) as u8));
            painter.circle_filled(r.left_center() + egui::vec2(11.0, 0.0), 3.5, alpha(color, (fade * 255.0) as u8));
            painter.galley(r.min + egui::vec2(20.0, 6.0), galley, alpha(TEXT, (fade * 255.0) as u8));
            y += r.height() + 5.0;
        }
    }
}

fn grid_label(division: f64) -> String {
    if division >= 4.0 {
        "1 bar".to_string()
    } else {
        format!("1/{}", (4.0 / division) as u32)
    }
}
