use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use egui::{self, RichText, Ui};

use crate::jq_engine::JqEngine;
use crate::temporal::{detect_temporal, detect_timezone, detect_unix_timestamp, TemporalValue};
use crate::theme::CatppuccinMocha;

/// Cached result of probing a URL for image content.
#[derive(Debug, Clone)]
enum ImageProbe {
    Pending,
    /// URL serves an image — we fetched the bytes for reliable rendering.
    Loaded(Vec<u8>),
    NotImage,
}

/// Shared cache for URL image probes — persists across frames.
type ImageProbeCache = Arc<Mutex<HashMap<String, ImageProbe>>>;

#[derive(Debug, Clone)]
enum PathSegment {
    Key(String),
    Index(usize),
}

/// An entry at a given level of the JSON tree.
struct Entry {
    label: String,
    type_icon: &'static str,
    type_label: String,
    preview: String,
    color: egui::Color32,
    is_container: bool,
}

pub struct BrowserView {
    path: Vec<PathSegment>,
    selection: usize,
    scroll_to_selection: bool,
    restore_key: Option<String>,
    // jq bar
    jq_bar: crate::widgets::jq_bar::JqBar,
    jq_synced: bool,
    jq_result: Option<String>,
    jq_error: Option<String>,
    cache_key: u64,
}

impl BrowserView {
    pub fn new() -> Self {
        Self {
            path: Vec::new(),
            selection: 0,
            scroll_to_selection: false,
            restore_key: None,
            jq_bar: crate::widgets::jq_bar::JqBar::new(),
            jq_synced: true,
            jq_result: None,
            jq_error: None,
            cache_key: 0,
        }
    }

    /// Returns the display name of the current file:
    /// - If inside a file, returns the first path segment (the file key)
    /// - If at the file list root, returns the selected entry's label
    pub fn current_file_key(&self, files: &[(String, serde_json::Value)]) -> Option<String> {
        if let Some(seg) = self.path.first() {
            match seg {
                PathSegment::Key(k) => Some(k.clone()),
                _ => None,
            }
        } else {
            // At root file list — use selection index
            // The virtual root is a sorted Object, so we need to match by sorted order
            let mut display_names: Vec<String> = files.iter().map(|(n, _)| {
                n.strip_suffix(".json").unwrap_or(n).to_string()
            }).collect();
            display_names.sort();
            display_names.get(self.selection).cloned()
        }
    }

    /// Navigate the browser to a specific file by display name.
    /// Stays at the root file list and moves selection to the given file.
    pub fn navigate_to_file(&mut self, display_name: &str, files: &[(String, serde_json::Value)]) {
        self.path.clear();
        // Find the index in the sorted display names (matching BTreeMap order)
        let mut display_names: Vec<String> = files.iter().map(|(n, _)| {
            n.strip_suffix(".json").unwrap_or(n).to_string()
        }).collect();
        display_names.sort();
        if let Some(idx) = display_names.iter().position(|n| n == display_name) {
            self.selection = idx;
        }
        self.scroll_to_selection = true;
        self.sync_jq_from_path();
    }

    pub fn invalidate(&mut self) {
        self.cache_key = 0;
        self.path.clear();
        self.selection = 0;
        self.jq_synced = true;
        self.jq_result = None;
        self.jq_error = None;
    }

    /// files: all parsed files. The browser treats them as the root level of the tree.
    /// jq operates on whichever file is currently entered (first path segment = file index).
    pub fn show(&mut self, ui: &mut Ui, files: &[(String, serde_json::Value)]) {
        // Build a virtual root: an Object keyed by filename
        let virtual_root = serde_json::Value::Object(
            files.iter().map(|(name, val)| {
                let display = name.strip_suffix(".json").unwrap_or(name).to_string();
                (display, val.clone())
            }).collect()
        );

        let key = files.len() as u64
            ^ files.iter().map(|(n, _)| n.len() as u64).sum::<u64>() << 8;
        if self.cache_key != key {
            self.cache_key = key;
            if resolve_path(&virtual_root, &self.path).is_none() {
                self.path.clear();
                self.selection = 0;
            }
            self.sync_jq_from_path();
        }

        let current = resolve_path(&virtual_root, &self.path).unwrap_or(&virtual_root);
        let parent = if self.path.is_empty() {
            None
        } else {
            resolve_path(&virtual_root, &self.path[..self.path.len() - 1])
        };

        let current_entries = build_entries(current);
        let parent_entries = parent.map(|p| build_entries(p));

        if let Some(key) = self.restore_key.take() {
            if let Some(idx) = current_entries.iter().position(|e| e.label == key) {
                self.selection = idx;
            }
        }

        if !current_entries.is_empty() && self.selection >= current_entries.len() {
            self.selection = current_entries.len() - 1;
        }

        let selected_child = current_entries
            .get(self.selection)
            .and_then(|e| child_value(current, self.selection, &e.label));

        // --- jq bar (operates on the selected file, not the virtual root) ---
        // The first path segment selects the file; jq runs on that file's value
        let jq_value = self.path.first().and_then(|seg| {
            match seg {
                PathSegment::Key(k) => files.iter().find(|(n, _)| {
                    n.strip_suffix(".json").unwrap_or(n) == k
                }).map(|(_, v)| v),
                _ => None,
            }
        });
        if let Some(val) = jq_value {
            self.show_jq_bar(ui, val);
        } else {
            // At file list root — show jq bar disabled/empty
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(egui_phosphor::regular::FUNNEL)
                        .color(CatppuccinMocha::SURFACE1)
                        .size(14.0),
                );
                ui.label(
                    RichText::new("select a file to use jq")
                        .color(CatppuccinMocha::OVERLAY0)
                        .small(),
                );
            });
        }

        ui.add_space(4.0);

        // --- Keyboard handling (only when jq input NOT focused) ---
        let jq_has_focus = crate::widgets::jq_bar::JqBar::has_focus(ui);

        if !jq_has_focus {
            let action = crate::widgets::read_miller_keys(ui, false);
            if crate::widgets::apply_selection(&mut self.selection, action, current_entries.len()) {
                self.scroll_to_selection = true;
                self.sync_jq_from_path();
            }
            if action == crate::widgets::MillerAction::Enter {
                if let Some(entry) = current_entries.get(self.selection) {
                    if entry.is_container {
                        self.enter_selected(current, &current_entries);
                    }
                }
            }
            if action == crate::widgets::MillerAction::Back && !self.path.is_empty() {
                self.go_up();
            }

            // '/' focuses the jq bar
            if ui.input(|i| i.key_pressed(egui::Key::Slash)) {
                self.jq_bar.focus();
            }

            // Copy selected value: c or Ctrl+C
            let copy = ui.input(|i| i.key_pressed(egui::Key::C))
                || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::C));
            if copy {
                if let Some(entry) = current_entries.get(self.selection) {
                    if let Some(v) = child_value(current, self.selection, &entry.label) {
                        ui.ctx().copy_text(copy_value_str(v));
                    }
                }
            }
        }

        // --- Three-column miller layout: parent | current | preview ---
        let avail = ui.available_rect_before_wrap();
        let total_w = avail.width() - 12.0;
        let col_widths = [total_w * 0.22, total_w * 0.38, total_w * 0.38];

        let mut clicked_entry: Option<usize> = None;
        let mut dbl_clicked_entry: Option<usize> = None;

        // Compute pane titles from path
        let seg_label = |seg: &PathSegment| -> String {
            match seg {
                PathSegment::Key(k) => k.clone(),
                PathSegment::Index(i) => format!("[{}]", i),
            }
        };
        let selected_label = current_entries.get(self.selection).map(|e| e.label.as_str()).unwrap_or("");
        let (left_title, mid_title, right_title) = match self.path.len() {
            0 => (String::new(), "Files".to_string(), selected_label.to_string()),
            1 => ("Files".to_string(), seg_label(&self.path[0]), selected_label.to_string()),
            _ => {
                let parent = seg_label(&self.path[self.path.len() - 2]);
                let current_seg = seg_label(&self.path[self.path.len() - 1]);
                (parent, current_seg, selected_label.to_string())
            }
        };

        let col_height = avail.height();
        ui.horizontal(|ui| {
            ui.set_height(col_height);

            // Left: parent
            ui.vertical(|ui| {
                ui.set_width(col_widths[0]);
                ui.set_height(col_height);
                crate::widgets::miller::pane_title(ui, &left_title);
                self.render_parent_column(ui, &parent_entries, col_height);
            });

            Self::draw_separator(ui, col_height);

            // Middle: current
            ui.vertical(|ui| {
                ui.set_width(col_widths[1]);
                ui.set_height(col_height);
                crate::widgets::miller::pane_title(ui, &mid_title);
                let (c, d) = self.render_current_column(ui, &current_entries, current, col_height);
                clicked_entry = c;
                dbl_clicked_entry = d;
            });

            Self::draw_separator(ui, col_height);

            // Right: preview
            ui.vertical(|ui| {
                let remaining = ui.available_width();
                ui.set_width(remaining);
                ui.set_height(col_height);
                crate::widgets::miller::pane_title(ui, &right_title);
                self.render_preview_column(ui, selected_child, &current_entries, col_height);
            });
        });

        // Handle click actions
        if let Some(idx) = clicked_entry {
            self.selection = idx;
            self.scroll_to_selection = false;
            self.sync_jq_from_path();
        }
        if let Some(idx) = dbl_clicked_entry {
            self.selection = idx;
            if let Some(entry) = current_entries.get(idx) {
                if entry.is_container {
                    self.enter_selected(current, &current_entries);
                }
            }
        }
    }

    fn show_jq_bar(&mut self, ui: &mut Ui, root: &serde_json::Value) {
        let title = match self.path.first() {
            Some(PathSegment::Key(k)) => k.as_str(),
            _ => "",
        };
        let resp = self.jq_bar.show(ui, title, root);

        if resp.changed {
            self.jq_synced = false;
            self.jq_result = None;
            self.jq_error = None;
        }

        if resp.escaped {
            self.jq_synced = true;
            self.sync_jq_from_path();
            self.jq_result = None;
            self.jq_error = None;
        }

        // Build a full path from jq segments by prepending the current file segment
        let file_seg = self.path.first().cloned();
        let make_full_path = |jq_segs: Vec<PathSegment>| -> Vec<PathSegment> {
            let mut full = Vec::with_capacity(jq_segs.len() + 1);
            if let Some(ref seg) = file_seg {
                full.push(seg.clone());
            }
            full.extend(jq_segs);
            full
        };

        // Cycling through completions — preview the path without committing
        if resp.previewing {
            if let Some(jq_segs) = jq_path_to_segments(&self.jq_bar.query) {
                if resolve_path(root, &jq_segs).is_some() {
                    self.path = make_full_path(jq_segs);
                    self.selection = 0;
                    self.jq_synced = true;
                    self.jq_result = None;
                    self.jq_error = None;
                    self.scroll_to_selection = true;
                }
            }
        }

        // Final acceptance — Enter/Tab/click on a completion
        if resp.accepted {
            if let Some(jq_segs) = jq_path_to_segments(&self.jq_bar.query) {
                self.path = make_full_path(jq_segs);
                self.selection = 0;
                self.jq_synced = true;
                self.jq_result = None;
                self.jq_error = None;
                self.scroll_to_selection = true;
            }
        } else if resp.run {
            // Manual Enter (no completion) — try as path, then as jq query
            if let Some(jq_segs) = jq_path_to_segments(&self.jq_bar.query) {
                if resolve_path(root, &jq_segs).is_some() {
                    self.path = make_full_path(jq_segs);
                    self.selection = 0;
                    self.jq_synced = true;
                    self.jq_result = None;
                    self.jq_error = None;
                    self.scroll_to_selection = true;
                    return;
                }
            }
            let result = JqEngine::execute(&self.jq_bar.query, root);
            if let Some(err) = &result.error {
                self.jq_error = Some(err.clone());
                self.jq_result = None;
            } else {
                self.jq_error = None;
                self.jq_result = Some(result.output.join("\n"));
            }
        }

        if let Some(err) = &self.jq_error {
            ui.label(
                RichText::new(err)
                    .color(CatppuccinMocha::RED)
                    .family(egui::FontFamily::Monospace)
                    .small(),
            );
        }
    }

    fn draw_separator(ui: &mut Ui, height: f32) {
        crate::widgets::draw_separator(ui, height);
    }

    fn render_parent_column(
        &self,
        ui: &mut Ui,
        parent_entries: &Option<Vec<Entry>>,
        height: f32,
    ) {
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;

        if let Some(entries) = parent_entries {
            // Which entry in the parent corresponds to our current path segment?
            let active_idx = self
                .path
                .last()
                .map(|seg| match seg {
                    PathSegment::Key(k) => entries
                        .iter()
                        .position(|e| e.label == *k)
                        .unwrap_or(0),
                    PathSegment::Index(i) => *i,
                })
                .unwrap_or(0);

            egui::ScrollArea::vertical()
                .id_salt("browser_parent")
                .auto_shrink(false)
                .max_height(height)
                .show_rows(ui, row_height, entries.len(), |ui, range| {
                    for i in range {
                        let entry = &entries[i];
                        let is_active = i == active_idx;
                        let bg = if is_active {
                            CatppuccinMocha::SURFACE0
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let text_color = if is_active {
                            CatppuccinMocha::TEXT
                        } else {
                            CatppuccinMocha::OVERLAY0
                        };
                        egui::Frame::new()
                            .fill(bg)
                            .corner_radius(4.0)
                            .inner_margin(egui::Margin::symmetric(6, 1))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(entry.type_icon)
                                            .color(entry.color)
                                            .family(egui::FontFamily::Monospace)
                                            .size(12.0),
                                    );
                                    ui.label(
                                        RichText::new(&entry.label)
                                            .color(text_color)
                                            .family(egui::FontFamily::Monospace)
                                            .size(12.0),
                                    );
                                });
                            });
                    }
                });
        } else {
            // At root — left column is blank (files are in the sidebar)
        }
    }

    fn render_current_column(
        &mut self,
        ui: &mut Ui,
        entries: &[Entry],
        current_value: &serde_json::Value,
        height: f32,
    ) -> (Option<usize>, Option<usize>) {
        let mut clicked = None;
        let mut dbl_clicked = None;

        if entries.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("(empty)")
                        .color(CatppuccinMocha::OVERLAY0)
                        .family(egui::FontFamily::Monospace),
                );
            });
            return (None, None);
        }

        let scroll_sel = self.scroll_to_selection;
        self.scroll_to_selection = false;
        let selection = self.selection;

        egui::ScrollArea::vertical()
            .id_salt("browser_current")
            .auto_shrink(false)
            .max_height(height)
            .show(ui, |ui| {
                for i in 0..entries.len() {
                    let entry = &entries[i];
                    let is_selected = i == selection;

                    let (is_hovered, row_id) = crate::widgets::prev_frame_hover(ui.ctx(), ui.id(), i);

                    let bg = if is_selected {
                        CatppuccinMocha::SURFACE0
                    } else if is_hovered {
                        egui::Color32::from_rgba_unmultiplied(
                            entry.color.r(), entry.color.g(), entry.color.b(), 15,
                        )
                    } else {
                        egui::Color32::TRANSPARENT
                    };

                    let font_size = if is_hovered && !is_selected { 12.5 } else { 12.0 };
                    let type_size = if is_hovered && !is_selected { 11.5 } else { 11.0 };
                    let preview_size = if is_hovered && !is_selected { 11.5 } else { 11.0 };

                    let mut copy_clicked = false;

                    // Hit-test copy button from previous frame's stored rect
                    let copy_btn_id = ui.id().with(("copy_btn", i));
                    if let Some(prev_rect) = ui.ctx().data(|d| d.get_temp::<egui::Rect>(copy_btn_id)) {
                        if ui.rect_contains_pointer(prev_rect) {
                            ui.painter().rect_filled(prev_rect, 3.0, CatppuccinMocha::SURFACE1);
                            ui.painter().text(
                                prev_rect.center(),
                                egui::Align2::CENTER_CENTER,
                                egui_phosphor::regular::COPY,
                                egui::FontId::proportional(12.0),
                                CatppuccinMocha::TEXT,
                            );
                            egui::show_tooltip_at_pointer(
                                ui.ctx(),
                                ui.layer_id(),
                                ui.id().with(("copy_tip", i)),
                                |ui| {
                                    ui.label("Copy value (c or Ctrl+C)");
                                },
                            );
                            if ui.input(|inp| inp.pointer.any_click()) {
                                copy_clicked = true;
                                if let Some(v) = child_value(current_value, i, &entry.label) {
                                    ui.ctx().copy_text(copy_value_str(v));
                                }
                            }
                        }
                    }

                    let r = egui::Frame::new()
                        .fill(bg)
                        .corner_radius(4.0)
                        .inner_margin(egui::Margin::symmetric(6, 1))
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                // Copy icon — paint only, no space allocation
                                let icon_rect = egui::Rect::from_min_size(
                                    ui.cursor().left_top(),
                                    egui::vec2(16.0, ui.min_rect().height().max(14.0)),
                                );
                                ui.painter().text(
                                    icon_rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    egui_phosphor::regular::COPY,
                                    egui::FontId::proportional(12.0),
                                    CatppuccinMocha::OVERLAY0,
                                );
                                ui.add_space(16.0);
                                ui.ctx().data_mut(|d| d.insert_temp(copy_btn_id, icon_rect));

                                // Type icon
                                ui.label(
                                    RichText::new(entry.type_icon)
                                        .color(entry.color)
                                        .family(egui::FontFamily::Monospace)
                                        .size(font_size),
                                );
                                // Label (key or index)
                                let label_color = if is_selected || is_hovered {
                                    entry.color
                                } else {
                                    CatppuccinMocha::SUBTEXT0
                                };
                                ui.label(
                                    RichText::new(&entry.label)
                                        .color(label_color)
                                        .family(egui::FontFamily::Monospace)
                                        .size(font_size),
                                );
                                // Type label
                                ui.label(
                                    RichText::new(&entry.type_label)
                                        .color(CatppuccinMocha::OVERLAY0)
                                        .family(egui::FontFamily::Monospace)
                                        .size(type_size),
                                );
                                // Value preview (right-aligned)
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        let preview = if entry.preview.len() > 24 {
                                            format!("{}...", &entry.preview[..21])
                                        } else {
                                            entry.preview.clone()
                                        };
                                        ui.label(
                                            RichText::new(preview)
                                                .color(entry.color)
                                                .family(egui::FontFamily::Monospace)
                                                .size(preview_size),
                                        );
                                    },
                                );
                            });
                        });

                    let response = r.response.interact(egui::Sense::click());
                    crate::widgets::store_hover(ui.ctx(), row_id, response.hovered());

                    if response.clicked() && !copy_clicked {
                        clicked = Some(i);
                    }
                    if response.double_clicked() && !copy_clicked {
                        dbl_clicked = Some(i);
                    }

                    if is_selected && scroll_sel {
                        response.scroll_to_me(Some(egui::Align::Center));
                    }
                }
            });

        (clicked, dbl_clicked)
    }

    fn render_preview_column(
        &self,
        ui: &mut Ui,
        selected_child: Option<&serde_json::Value>,
        entries: &[Entry],
        height: f32,
    ) {
        // Clip content to column bounds
        let clip = ui.available_rect_before_wrap();
        ui.set_clip_rect(clip);
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;

        // If we have a jq result (non-synced query), show that instead
        if !self.jq_synced {
            if let Some(result) = &self.jq_result {
                let lines: Vec<&str> = result.lines().collect();
                ui.label(
                    RichText::new(format!("{} jq result — {} lines", egui_phosphor::regular::FUNNEL, lines.len()))
                        .color(CatppuccinMocha::MAUVE)
                        .small(),
                );
                ui.add_space(4.0);
                egui::ScrollArea::vertical()
                    .id_salt("browser_jq_preview")
                    .auto_shrink(false)
                    .max_height(height - 24.0)
                    .show_rows(ui, row_height, lines.len(), |ui, range| {
                        for i in range {
                            let line = lines[i];
                            let color = colorize_json_token(line.trim());
                            ui.label(
                                RichText::new(line)
                                    .color(color)
                                    .family(egui::FontFamily::Monospace)
                                    .size(12.0),
                            );
                        }
                    });
                return;
            }
        }

        let Some(val) = selected_child else {
            if entries.is_empty() {
                ui.centered_and_justified(|ui| {
                    ui.label(
                        RichText::new("Nothing selected")
                            .color(CatppuccinMocha::OVERLAY0),
                    );
                });
            }
            return;
        };

        match val {
            serde_json::Value::Object(map) => {
                ui.label(
                    RichText::new(format!(
                        "{} Object — {} fields",
                        egui_phosphor::regular::BRACKETS_CURLY,
                        map.len()
                    ))
                    .color(CatppuccinMocha::LAVENDER)
                    .small(),
                );
                ui.add_space(4.0);
                let keys: Vec<(&String, &serde_json::Value)> = map.iter().collect();
                egui::ScrollArea::vertical()
                    .id_salt("browser_preview")
                    .auto_shrink(false)
                    .max_height(height - 24.0)
                    .show_rows(ui, row_height, keys.len(), |ui, range| {
                        for i in range {
                            let (k, v) = keys[i];
                            let (icon, color) = type_icon_color(v);
                            let preview = value_preview(v);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(icon)
                                        .color(color)
                                        .family(egui::FontFamily::Monospace)
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new(k)
                                        .color(CatppuccinMocha::SUBTEXT0)
                                        .family(egui::FontFamily::Monospace)
                                        .size(11.0),
                                );
                                let p = if preview.len() > 30 {
                                    format!("{}...", &preview[..27])
                                } else {
                                    preview
                                };
                                ui.label(
                                    RichText::new(p)
                                        .color(color)
                                        .family(egui::FontFamily::Monospace)
                                        .size(11.0),
                                );
                            });
                        }
                    });
            }
            serde_json::Value::Array(arr) => {
                ui.label(
                    RichText::new(format!(
                        "{} Array — {} items",
                        egui_phosphor::regular::BRACKETS_SQUARE,
                        arr.len()
                    ))
                    .color(CatppuccinMocha::YELLOW)
                    .small(),
                );
                ui.add_space(4.0);
                let show_count = arr.len().min(200);
                egui::ScrollArea::vertical()
                    .id_salt("browser_preview")
                    .auto_shrink(false)
                    .max_height(height - 24.0)
                    .show_rows(ui, row_height, show_count, |ui, range| {
                        for i in range {
                            let v = &arr[i];
                            let (icon, color) = type_icon_color(v);
                            let preview = value_preview(v);
                            ui.horizontal(|ui| {
                                ui.label(
                                    RichText::new(icon)
                                        .color(color)
                                        .family(egui::FontFamily::Monospace)
                                        .size(11.0),
                                );
                                ui.label(
                                    RichText::new(format!("[{}]", i))
                                        .color(CatppuccinMocha::OVERLAY0)
                                        .family(egui::FontFamily::Monospace)
                                        .size(11.0),
                                );
                                let p = if preview.len() > 30 {
                                    format!("{}...", &preview[..27])
                                } else {
                                    preview
                                };
                                ui.label(
                                    RichText::new(p)
                                        .color(color)
                                        .family(egui::FontFamily::Monospace)
                                        .size(11.0),
                                );
                            });
                        }
                    });
            }
            _ => {
                // Scalar value — show full content with smart detection
                let color = match val {
                    serde_json::Value::String(_) => CatppuccinMocha::GREEN,
                    serde_json::Value::Number(_) => CatppuccinMocha::PEACH,
                    serde_json::Value::Bool(_) => CatppuccinMocha::MAUVE,
                    serde_json::Value::Null => CatppuccinMocha::OVERLAY0,
                    _ => CatppuccinMocha::TEXT,
                };

                // Image preview — full width, no raw text
                if let serde_json::Value::String(s) = val {
                    if let Some(img) = detect_image_data(s, ui.ctx()) {
                        egui::ScrollArea::vertical()
                            .id_salt("browser_preview")
                            .auto_shrink(false)
                            .max_height(height)
                            .show(ui, |ui| {
                                render_image_previews(ui, &[(&String::new(), img)]);
                                ui.add_space(4.0);
                                if ui
                                    .add(
                                        egui::Button::new(
                                            RichText::new(format!(
                                                " {} Copy raw ",
                                                egui_phosphor::regular::COPY
                                            ))
                                            .size(11.0),
                                        )
                                        .fill(CatppuccinMocha::SURFACE0)
                                        .corner_radius(4.0),
                                    )
                                    .clicked()
                                {
                                    ui.ctx().copy_text(s.clone());
                                }
                            });
                        return;
                    }
                }

                // Datetime preview
                let mut showed_smart = false;

                if let serde_json::Value::String(s) = val {
                    if let Some(tv) = detect_temporal(s) {
                        showed_smart = true;
                        render_datetime_widgets(ui, &[(&String::new(), tv)]);
                    } else if let Some(tz) = detect_timezone(s) {
                        showed_smart = true;
                        render_timezone_globe(ui, tz.offset_hours, &format!("{} ({})", tz.name, tz.display));
                    }
                }
                if let serde_json::Value::Number(n) = val {
                    if let Some(i) = n.as_i64() {
                        if let Some(tv) = detect_unix_timestamp(i) {
                            showed_smart = true;
                            render_datetime_widgets(ui, &[(&String::new(), tv)]);
                        }
                    }
                }

                if showed_smart {
                    ui.add_space(4.0);
                    let sep_rect = ui.available_rect_before_wrap();
                    ui.painter().line_segment(
                        [
                            egui::pos2(sep_rect.left(), sep_rect.top()),
                            egui::pos2(sep_rect.right(), sep_rect.top()),
                        ],
                        egui::Stroke::new(1.0, CatppuccinMocha::SURFACE0),
                    );
                    ui.add_space(4.0);
                }

                // Raw value text
                let pretty = match val {
                    serde_json::Value::String(s) => s.clone(),
                    other => serde_json::to_string_pretty(other).unwrap_or_default(),
                };
                let lines: Vec<&str> = pretty.lines().collect();
                egui::ScrollArea::vertical()
                    .id_salt("browser_preview")
                    .auto_shrink(false)
                    .max_height(height)
                    .show_rows(ui, row_height, lines.len(), |ui, range| {
                        for i in range {
                            ui.label(
                                RichText::new(lines[i])
                                    .color(color)
                                    .family(egui::FontFamily::Monospace)
                                    .size(12.0),
                            );
                        }
                    });
            }
        }
    }

    fn enter_selected(&mut self, current: &serde_json::Value, entries: &[Entry]) {
        if let Some(entry) = entries.get(self.selection) {
            if !entry.is_container {
                return;
            }
            // Don't enter empty containers
            let child = child_value(current, self.selection, &entry.label);
            let is_empty = child.map_or(true, |v| match v {
                serde_json::Value::Object(m) => m.is_empty(),
                serde_json::Value::Array(a) => a.is_empty(),
                _ => true,
            });
            if is_empty {
                return;
            }
            match current {
                serde_json::Value::Object(_) => {
                    self.path.push(PathSegment::Key(entry.label.clone()));
                }
                serde_json::Value::Array(_) => {
                    self.path.push(PathSegment::Index(self.selection));
                }
                _ => return,
            }
            self.selection = 0;
            self.scroll_to_selection = true;
            self.sync_jq_from_path();
        }
    }

    fn go_up(&mut self) {
        if let Some(seg) = self.path.pop() {
            match seg {
                PathSegment::Index(i) => {
                    self.selection = i;
                    self.restore_key = None;
                }
                PathSegment::Key(k) => {
                    self.selection = 0;
                    self.restore_key = Some(k);
                }
            }
            self.scroll_to_selection = true;
            self.sync_jq_from_path();
        }
    }

    fn sync_jq_from_path(&mut self) {
        if self.jq_synced {
            let jq_path = if self.path.len() > 1 {
                &self.path[1..]
            } else {
                &[]
            };
            self.jq_bar.query = path_to_jq(jq_path);
            self.jq_result = None;
            self.jq_error = None;
        }
    }
}

// --- Helper functions ---

fn resolve_path<'a>(
    root: &'a serde_json::Value,
    path: &[PathSegment],
) -> Option<&'a serde_json::Value> {
    let mut current = root;
    for seg in path {
        match seg {
            PathSegment::Key(k) => {
                current = current.as_object()?.get(k)?;
            }
            PathSegment::Index(i) => {
                current = current.as_array()?.get(*i)?;
            }
        }
    }
    Some(current)
}

fn path_to_jq(path: &[PathSegment]) -> String {
    if path.is_empty() {
        return ".".to_string();
    }
    let mut s = String::new();
    for seg in path {
        match seg {
            PathSegment::Key(k) => {
                // Use bracket notation if key has special chars
                if k.chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '_')
                    && !k.is_empty()
                    && !k.chars().next().unwrap().is_ascii_digit()
                {
                    s.push('.');
                    s.push_str(k);
                } else {
                    s.push_str(&format!(".\"{}\"", k));
                }
            }
            PathSegment::Index(i) => {
                s.push_str(&format!("[{}]", i));
            }
        }
    }
    s
}

/// Try to parse a jq path expression into path segments.
/// Only handles simple dot-access and index patterns.
fn jq_path_to_segments(expr: &str) -> Option<Vec<PathSegment>> {
    let expr = expr.trim();
    if expr == "." {
        return Some(Vec::new());
    }
    if !expr.starts_with('.') {
        return None;
    }

    let mut segments = Vec::new();
    let mut rest = &expr[1..]; // skip leading dot

    while !rest.is_empty() {
        if rest.starts_with('[') {
            // Index access: [N]
            let end = rest.find(']')?;
            let idx_str = &rest[1..end];
            let idx: usize = idx_str.parse().ok()?;
            segments.push(PathSegment::Index(idx));
            rest = &rest[end + 1..];
        } else if rest.starts_with('.') {
            rest = &rest[1..];
        } else if rest.starts_with('"') {
            // Quoted key: ."key"
            let end = rest[1..].find('"')? + 1;
            let key = &rest[1..end];
            segments.push(PathSegment::Key(key.to_string()));
            rest = &rest[end + 1..];
        } else {
            // Simple key: letters until next . or [
            let end = rest
                .find(|c: char| c == '.' || c == '[')
                .unwrap_or(rest.len());
            let key = &rest[..end];
            if key.is_empty() {
                return None;
            }
            segments.push(PathSegment::Key(key.to_string()));
            rest = &rest[end..];
        }
    }

    Some(segments)
}

fn copy_value_str(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        serde_json::Value::Null => "null".to_string(),
        other => serde_json::to_string(other).unwrap_or_default(),
    }
}

fn build_entries(value: &serde_json::Value) -> Vec<Entry> {
    match value {
        serde_json::Value::Object(map) => map
            .iter()
            .map(|(k, v)| {
                let (icon, color) = type_icon_color(v);
                Entry {
                    label: k.clone(),
                    type_icon: icon,
                    type_label: type_label(v),
                    preview: value_preview(v),
                    color,
                    is_container: v.is_object() || v.is_array(),
                }
            })
            .collect(),
        serde_json::Value::Array(arr) => arr
            .iter()
            .enumerate()
            .map(|(i, v)| {
                let (icon, color) = type_icon_color(v);
                Entry {
                    label: format!("[{}]", i),
                    type_icon: icon,
                    type_label: type_label(v),
                    preview: value_preview(v),
                    color,
                    is_container: v.is_object() || v.is_array(),
                }
            })
            .collect(),
        _ => Vec::new(),
    }
}

fn child_value<'a>(
    parent: &'a serde_json::Value,
    index: usize,
    label: &str,
) -> Option<&'a serde_json::Value> {
    match parent {
        serde_json::Value::Object(map) => map.get(label),
        serde_json::Value::Array(arr) => arr.get(index),
        _ => None,
    }
}

fn type_icon_color(v: &serde_json::Value) -> (&'static str, egui::Color32) {
    match v {
        serde_json::Value::Object(_) => ("{}", CatppuccinMocha::LAVENDER),
        serde_json::Value::Array(_) => ("[]", CatppuccinMocha::YELLOW),
        serde_json::Value::String(s) => {
            if let Some(tv) = detect_temporal(s) {
                let icon = match tv {
                    TemporalValue::NaiveDate(_) => "d",
                    TemporalValue::NaiveTime(_) => "t",
                    _ => "dt",
                };
                (icon, CatppuccinMocha::BLUE)
            } else if detect_timezone(s).is_some() {
                ("tz", CatppuccinMocha::BLUE)
            } else {
                ("\"\"", CatppuccinMocha::GREEN)
            }
        }
        serde_json::Value::Number(_) => ("#", CatppuccinMocha::PEACH),
        serde_json::Value::Bool(_) => ("?", CatppuccinMocha::MAUVE),
        serde_json::Value::Null => ("~", CatppuccinMocha::OVERLAY0),
    }
}

fn type_label(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(m) => format!("{} fields", m.len()),
        serde_json::Value::Array(a) => format!("{} items", a.len()),
        serde_json::Value::String(s) => {
            match detect_temporal(s) {
                Some(TemporalValue::NaiveDate(_)) => "date".to_string(),
                Some(TemporalValue::NaiveTime(_)) => "time".to_string(),
                Some(_) => "datetime".to_string(),
                None if detect_timezone(s).is_some() => "timezone".to_string(),
                None => "str".to_string(),
            }
        }
        serde_json::Value::Number(n) => {
            if n.is_f64() && !n.is_i64() {
                "f64".to_string()
            } else {
                "i64".to_string()
            }
        }
        serde_json::Value::Bool(_) => "bool".to_string(),
        serde_json::Value::Null => "null".to_string(),
    }
}

fn value_preview(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::Object(m) => {
            if m.is_empty() {
                "{}".to_string()
            } else {
                format!("{{{} fields}}", m.len())
            }
        }
        serde_json::Value::Array(a) => {
            if a.is_empty() {
                "[]".to_string()
            } else {
                format!("[{} items]", a.len())
            }
        }
        serde_json::Value::String(s) => {
            if s.starts_with("data:image/") {
                "image".to_string()
            } else if let Some(tv) = detect_temporal(s) {
                tv.display()
            } else if let Some(tz) = detect_timezone(s) {
                tz.display
            } else if s.len() > 60 {
                format!("\"{}...\"", &s[..57])
            } else {
                format!("\"{}\"", s)
            }
        }
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
    }
}

fn colorize_json_token(s: &str) -> egui::Color32 {
    if s.starts_with('"') {
        if s.contains(':') {
            CatppuccinMocha::BLUE
        } else {
            CatppuccinMocha::GREEN
        }
    } else if s == "null" || s == "null," {
        CatppuccinMocha::OVERLAY0
    } else if s == "true" || s == "true," || s == "false" || s == "false," {
        CatppuccinMocha::MAUVE
    } else if s.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        CatppuccinMocha::PEACH
    } else {
        CatppuccinMocha::TEXT
    }
}

// --- Reused from jq.rs ---

// --- Smart preview helpers ---

enum ImageData {
    Base64 { data: Vec<u8> },
    Url(String),
}

fn detect_image_data(s: &str, ctx: &egui::Context) -> Option<ImageData> {
    use base64::Engine;

    // data:image/... base64
    if s.starts_with("data:image/") && s.contains("base64,") {
        let b64 = s.split("base64,").nth(1)?;
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64) {
            return Some(ImageData::Base64 { data: bytes });
        }
    }

    // Raw base64 with image magic bytes
    if s.len() > 20 && !s.contains(' ') && !s.contains('\n') {
        if let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(s) {
            if is_image_magic(&bytes) {
                return Some(ImageData::Base64 { data: bytes });
            }
        }
    }

    // URL — check if it's an image via extension or async probe
    let lower = s.to_lowercase();
    if lower.starts_with("http://") || lower.starts_with("https://") {
        let has_image_ext = lower.ends_with(".png")
            || lower.ends_with(".jpg")
            || lower.ends_with(".jpeg")
            || lower.ends_with(".gif")
            || lower.ends_with(".webp")
            || lower.ends_with(".svg")
            || lower.contains("/image")
            || lower.contains("imgur.com");

        let cache: ImageProbeCache = ctx.data_mut(|d| {
            d.get_temp_mut_or_default::<ImageProbeCache>(egui::Id::new("image_probe_cache"))
                .clone()
        });

        let probe = {
            let map = cache.lock().unwrap();
            map.get(s).cloned()
        };

        match probe {
            Some(ImageProbe::Loaded(bytes)) => {
                return Some(ImageData::Base64 { data: bytes });
            }
            Some(ImageProbe::NotImage) => return None,
            Some(ImageProbe::Pending) => {
                // Still loading — for known extensions show URL placeholder
                if has_image_ext {
                    return Some(ImageData::Url(s.to_string()));
                }
                return None;
            }
            None => {
                // Fire background GET to fetch image bytes (handles redirects)
                {
                    let mut map = cache.lock().unwrap();
                    map.insert(s.to_string(), ImageProbe::Pending);
                }
                let url = s.to_string();
                let cache_clone = cache.clone();
                let ctx_clone = ctx.clone();
                std::thread::spawn(move || {
                    let result = (|| -> Option<Vec<u8>> {
                        let mut resp = ureq::get(&url)
                            .header("User-Agent", "jv-json-viewer")
                            .call()
                            .ok()?;

                        let ct = resp
                            .headers()
                            .get("content-type")
                            .and_then(|v| v.to_str().ok())
                            .unwrap_or("");
                        if !ct.starts_with("image/") {
                            return None;
                        }

                        resp.body_mut().read_to_vec().ok()
                    })();

                    let mut map = cache_clone.lock().unwrap();
                    match result {
                        Some(bytes) if is_image_magic(&bytes) || !bytes.is_empty() => {
                            map.insert(url, ImageProbe::Loaded(bytes));
                        }
                        _ => {
                            map.insert(url, ImageProbe::NotImage);
                        }
                    }
                    drop(map);
                    ctx_clone.request_repaint();
                });

                // While fetching, show URL image for known extensions
                if has_image_ext {
                    return Some(ImageData::Url(s.to_string()));
                }
            }
        }
    }

    None
}

fn is_image_magic(bytes: &[u8]) -> bool {
    if bytes.len() < 4 {
        return false;
    }
    // PNG
    if bytes.starts_with(&[0x89, 0x50, 0x4E, 0x47]) {
        return true;
    }
    // JPEG
    if bytes.starts_with(&[0xFF, 0xD8]) {
        return true;
    }
    // GIF
    if bytes.starts_with(b"GIF8") {
        return true;
    }
    // WEBP (RIFF....WEBP)
    if bytes.len() >= 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        return true;
    }
    false
}

fn render_datetime_widgets(ui: &mut Ui, fields: &[(&String, TemporalValue)]) -> f32 {
    let start_y = ui.cursor().top();

    for (key, tv) in fields {
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(egui_phosphor::regular::CALENDAR)
                    .color(CatppuccinMocha::TEAL)
                    .size(12.0),
            );
            ui.label(
                RichText::new(*key)
                    .color(CatppuccinMocha::SUBTEXT0)
                    .family(egui::FontFamily::Monospace)
                    .size(11.0),
            );
            ui.label(
                RichText::new(tv.relative_time())
                    .color(CatppuccinMocha::OVERLAY1)
                    .size(10.0),
            );
        });

        // Show formatted datetime
        ui.horizontal(|ui| {
            ui.add_space(20.0);
            ui.label(
                RichText::new(tv.display())
                    .color(CatppuccinMocha::TEAL)
                    .family(egui::FontFamily::Monospace)
                    .size(11.0),
            );
        });

        // Timezone globe if we have tz info
        if let Some(offset_h) = tv.utc_offset_hours() {
            let tz_label = tv.timezone_info().unwrap_or_default();
            ui.add_space(2.0);
            render_timezone_globe(ui, offset_h, &tz_label);
        }

        // Mini calendar if we have a date
        if let Some(date) = tv.to_naive_date() {
            ui.add_space(2.0);
            render_mini_calendar(ui, date);
        }
        ui.add_space(4.0);
    }

    ui.cursor().top() - start_y
}

fn render_mini_calendar(ui: &mut Ui, date: chrono::NaiveDate) {
    use chrono::Datelike;

    let year = date.year();
    let month = date.month();
    let first_of_month = chrono::NaiveDate::from_ymd_opt(year, month, 1).unwrap();
    let days_in_month = if month == 12 {
        chrono::NaiveDate::from_ymd_opt(year + 1, 1, 1)
    } else {
        chrono::NaiveDate::from_ymd_opt(year, month + 1, 1)
    }
    .unwrap()
    .signed_duration_since(first_of_month)
    .num_days() as u32;

    let start_weekday = first_of_month.weekday().num_days_from_monday(); // Mon=0

    let month_name = match month {
        1 => "Jan",
        2 => "Feb",
        3 => "Mar",
        4 => "Apr",
        5 => "May",
        6 => "Jun",
        7 => "Jul",
        8 => "Aug",
        9 => "Sep",
        10 => "Oct",
        11 => "Nov",
        12 => "Dec",
        _ => "",
    };

    let avail_w = ui.available_width();
    let padding = 16.0; // inner_margin symmetric(8, 6) = 8 each side
    let grid_w = avail_w - padding;
    let cell_size = (grid_w / 7.0).floor();
    let font_size = (cell_size * 0.5).clamp(9.0, 14.0);

    egui::Frame::new()
        .fill(CatppuccinMocha::SURFACE0)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(avail_w - padding);

            // Month/year header
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} {}", month_name, year))
                        .color(CatppuccinMocha::TEXT)
                        .size(font_size + 1.0)
                        .strong(),
                );
            });
            ui.add_space(2.0);

            // Day-of-week headers
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 0.0;
                for day in &["Mo", "Tu", "We", "Th", "Fr", "Sa", "Su"] {
                    ui.allocate_ui(egui::vec2(cell_size, cell_size), |ui| {
                        ui.centered_and_justified(|ui| {
                            ui.label(
                                RichText::new(*day)
                                    .color(CatppuccinMocha::OVERLAY0)
                                    .size(font_size),
                            );
                        });
                    });
                }
            });

            // Day grid
            let mut day = 1u32;
            let mut cell = 0u32;
            let total_cells = start_weekday + days_in_month;
            let rows = (total_cells + 6) / 7;

            for _row in 0..rows {
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 0.0;
                    for _col in 0..7u32 {
                        let in_month = cell >= start_weekday && day <= days_in_month;
                        ui.allocate_ui(egui::vec2(cell_size, cell_size), |ui| {
                            if in_month {
                                let is_today = day == date.day();
                                let (bg, fg) = if is_today {
                                    (CatppuccinMocha::TEAL, CatppuccinMocha::CRUST)
                                } else {
                                    (egui::Color32::TRANSPARENT, CatppuccinMocha::SUBTEXT0)
                                };
                                if is_today {
                                    let rect = ui.available_rect_before_wrap();
                                    let center = rect.center();
                                    ui.painter()
                                        .circle_filled(center, cell_size * 0.4, bg);
                                }
                                ui.centered_and_justified(|ui| {
                                    ui.label(
                                        RichText::new(format!("{}", day))
                                            .color(fg)
                                            .size(font_size),
                                    );
                                });
                                day += 1;
                            }
                        });
                        cell += 1;
                    }
                });
            }
        });
}

fn render_timezone_globe(ui: &mut Ui, offset_hours: f32, tz_label: &str) {
    let avail_w = ui.available_width();
    let padding = 16.0;
    let globe_w = avail_w - padding;
    let globe_h = globe_w * 0.5; // ellipse aspect

    egui::Frame::new()
        .fill(CatppuccinMocha::SURFACE0)
        .corner_radius(6.0)
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(globe_w);

            // Label: tz +05:30
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("tz")
                        .color(CatppuccinMocha::BLUE)
                        .size(11.0)
                        .strong(),
                );
                ui.label(
                    RichText::new(tz_label)
                        .color(CatppuccinMocha::TEXT)
                        .family(egui::FontFamily::Monospace)
                        .size(11.0),
                );
            });

            ui.add_space(2.0);

            // Allocate space for globe
            let (rect, _) = ui.allocate_exact_size(
                egui::vec2(globe_w, globe_h),
                egui::Sense::hover(),
            );
            let painter = ui.painter_at(rect);
            let center = rect.center();
            let rx = globe_w * 0.45; // horizontal radius
            let ry = globe_h * 0.45; // vertical radius

            // Globe outline
            let outline_color = CatppuccinMocha::SURFACE2;
            let n_pts = 64;
            let ellipse_pts: Vec<egui::Pos2> = (0..=n_pts)
                .map(|i| {
                    let angle = std::f32::consts::TAU * i as f32 / n_pts as f32;
                    egui::pos2(
                        center.x + rx * angle.cos(),
                        center.y + ry * angle.sin(),
                    )
                })
                .collect();

            // Timezone band: offset_hours maps to longitude fraction
            // UTC=0 is center, -12 is left edge, +12 is right edge
            // Band is ~1 hour wide = 1/24 of the full width
            let frac = offset_hours / 24.0; // -0.5 to 0.5
            let band_center_x = center.x + frac * 2.0 * rx;
            let band_half_w = rx / 12.0; // 1 hour = 1/24 of diameter = 1/12 of radius

            // Draw the timezone band (clipped to ellipse via vertical strips)
            let band_left = band_center_x - band_half_w;
            let band_right = band_center_x + band_half_w;
            let steps = 20;
            for i in 0..steps {
                let x0 = band_left + (band_right - band_left) * i as f32 / steps as f32;
                let x1 = band_left + (band_right - band_left) * (i + 1) as f32 / steps as f32;
                // Compute ellipse y at these x positions
                let dx0 = (x0 - center.x) / rx;
                let dx1 = (x1 - center.x) / rx;
                if dx0.abs() > 1.0 || dx1.abs() > 1.0 {
                    continue;
                }
                let y_half_0 = ry * (1.0 - dx0 * dx0).sqrt();
                let y_half_1 = ry * (1.0 - dx1 * dx1).sqrt();
                let points = vec![
                    egui::pos2(x0, center.y - y_half_0),
                    egui::pos2(x1, center.y - y_half_1),
                    egui::pos2(x1, center.y + y_half_1),
                    egui::pos2(x0, center.y + y_half_0),
                ];
                painter.add(egui::Shape::convex_polygon(
                    points,
                    egui::Color32::from_rgba_unmultiplied(137, 180, 250, 40),
                    egui::Stroke::NONE,
                ));
            }

            // Longitude lines (every 3 hours = 8 lines)
            for i in -4..=4 {
                let f = i as f32 / 12.0; // fraction of full width
                let x = center.x + f * 2.0 * rx;
                let dx = (x - center.x) / rx;
                if dx.abs() >= 1.0 {
                    continue;
                }
                let y_half = ry * (1.0 - dx * dx).sqrt();
                let color = if i == 0 {
                    CatppuccinMocha::OVERLAY0 // prime meridian slightly brighter
                } else {
                    egui::Color32::from_rgba_unmultiplied(108, 112, 134, 80)
                };
                painter.line_segment(
                    [
                        egui::pos2(x, center.y - y_half),
                        egui::pos2(x, center.y + y_half),
                    ],
                    egui::Stroke::new(1.0, color),
                );
            }

            // Latitude lines (equator + 2 tropics)
            for f in [-0.4_f32, 0.0, 0.4] {
                let y = center.y + f * ry;
                // Compute x range at this y
                let dy = f.abs();
                let x_half = rx * (1.0 - dy * dy).sqrt();
                painter.line_segment(
                    [
                        egui::pos2(center.x - x_half, y),
                        egui::pos2(center.x + x_half, y),
                    ],
                    egui::Stroke::new(
                        if f == 0.0 { 1.0 } else { 0.5 },
                        if f == 0.0 { CatppuccinMocha::OVERLAY0 } else { egui::Color32::from_rgba_unmultiplied(108, 112, 134, 80) },
                    ),
                );
            }

            // Globe outline on top
            painter.add(egui::Shape::closed_line(
                ellipse_pts,
                egui::Stroke::new(1.5, outline_color),
            ));

            // Timezone center marker
            let dx_m = (band_center_x - center.x) / rx;
            if dx_m.abs() < 1.0 {
                painter.circle_filled(
                    egui::pos2(band_center_x, center.y),
                    3.0,
                    CatppuccinMocha::BLUE,
                );
            }
        });
}

fn render_image_previews(ui: &mut Ui, fields: &[(&String, ImageData)]) -> f32 {
    let start_y = ui.cursor().top();
    let max_width = ui.available_width();

    for (key, img) in fields {
        if !key.is_empty() {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(egui_phosphor::regular::IMAGE)
                        .color(CatppuccinMocha::PINK)
                        .size(12.0),
                );
                ui.label(
                    RichText::new(*key)
                        .color(CatppuccinMocha::SUBTEXT0)
                        .family(egui::FontFamily::Monospace)
                        .size(11.0),
                );
            });
        }

        match img {
            ImageData::Base64 { data } => {
                // Use a stable ID based on first bytes to cache the texture
                let hash = data.len() as u64
                    ^ data.iter().take(32).enumerate().fold(0u64, |acc, (i, b)| {
                        acc ^ ((*b as u64) << (i % 8 * 8))
                    });
                let tex_id = egui::Id::new(("img_preview", hash));

                let texture: Option<egui::TextureHandle> =
                    ui.ctx().data(|d| d.get_temp(tex_id));

                let texture = if let Some(t) = texture {
                    t
                } else if let Ok(dyn_img) = image::load_from_memory(data) {
                    let rgba = dyn_img.to_rgba8();
                    let size = [rgba.width() as usize, rgba.height() as usize];
                    let color_image =
                        egui::ColorImage::from_rgba_unmultiplied(size, rgba.as_raw());
                    let t = ui.ctx().load_texture(
                        format!("img_{}", hash),
                        color_image,
                        egui::TextureOptions::LINEAR,
                    );
                    ui.ctx().data_mut(|d| d.insert_temp(tex_id, t.clone()));
                    t
                } else {
                    ui.label(
                        RichText::new("Failed to decode image")
                            .color(CatppuccinMocha::RED)
                            .size(11.0),
                    );
                    continue;
                };

                let tex_size = texture.size_vec2();
                let scale = (max_width / tex_size.x).min(1.0);
                let display_size = tex_size * scale;
                ui.add(egui::Image::new(&texture).fit_to_exact_size(display_size));
            }
            ImageData::Url(url) => {
                ui.add(
                    egui::Image::from_uri(url)
                        .max_width(max_width)
                        .maintain_aspect_ratio(true)
                        .corner_radius(4.0),
                );
            }
        }

        ui.add_space(4.0);
    }

    ui.cursor().top() - start_y
}
