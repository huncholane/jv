use egui::{self, RichText, Ui};

use crate::schema::{SchemaOverview, SharedStruct};
use crate::theme::CatppuccinMocha;
use crate::types::InferredType;

pub struct SharedBrowserView {
    /// Navigation path: e.g. ["Shared", "Address", "city"]
    path: Vec<String>,
    selection: usize,
    scroll_to_selection: bool,
    restore_key: Option<String>,
    sort_alpha: bool,
    /// Cached all_structs — recomputed only when schema changes
    cached_all: Vec<SharedStruct>,
    cached_all_key: usize,
    /// Cached entries — recomputed only when path changes
    entries_cache_key: Vec<String>,
    cached_current_entries: Vec<Entry>,
    cached_parent_entries: Option<Vec<Entry>>,
    /// Filter for the center column
    filter: crate::widgets::miller::MillerFilter,
    /// Cached values column data: (path, selection_label) -> flattened rows
    values_cache_key: (Vec<String>, String),
    values_rows: Vec<ValueRow>,
}

/// Pre-flattened row for the values column (file headers + values).
enum ValueRow {
    FileHeader(String), // display filename
    Value { text: String, color: egui::Color32, count: Option<usize> },
}

/// An entry at a given level of the schema tree.
struct Entry {
    label: String,
    type_short: String,
    color: egui::Color32,
    is_container: bool,
    count: Option<usize>,
}

impl SharedBrowserView {
    pub fn new() -> Self {
        Self {
            path: Vec::new(),
            selection: 0,
            scroll_to_selection: false,
            restore_key: None,
            sort_alpha: false,
            cached_all: Vec::new(),
            cached_all_key: usize::MAX, // force initial rebuild
            entries_cache_key: vec!["__invalid__".to_string()], // force initial rebuild
            cached_current_entries: Vec::new(),
            cached_parent_entries: None,
            filter: crate::widgets::miller::MillerFilter::new("groups_center_filter"),
            values_cache_key: (Vec::new(), String::new()),
            values_rows: Vec::new(),
        }
    }

    pub fn invalidate(&mut self) {
        self.path.clear();
        self.selection = 0;
        self.cached_all_key = usize::MAX;
        self.entries_cache_key = vec!["__invalid__".to_string()];
        self.values_cache_key = (Vec::new(), String::new());
        self.values_rows.clear();
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        schema: &SchemaOverview,
        files: &[(String, serde_json::Value)],
    ) {
        if schema.structs.is_empty() && schema.unique_structs.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Import multiple files to see shared types")
                        .color(CatppuccinMocha::OVERLAY0)
                        .size(16.0),
                );
            });
            return;
        }

        // Cache all_structs — only recompute when schema changes
        let schema_key = schema.structs.len() + schema.unique_structs.len();
        if self.cached_all_key != schema_key {
            self.cached_all = schema.all_structs();
            self.cached_all_key = schema_key;
        }
        let all = &self.cached_all;

        // Build entries — cached, only recompute when path changes
        if self.entries_cache_key != self.path {
            self.cached_current_entries = build_entries_at_path(&self.path, schema, all);
            if self.sort_alpha && self.path.len() == 1 {
                self.cached_current_entries.sort_by(|a, b| a.label.cmp(&b.label));
            }
            self.cached_parent_entries = if self.path.is_empty() {
                None
            } else {
                Some(build_entries_at_path(
                    &self.path[..self.path.len() - 1],
                    schema,
                    all,
                ))
            };
            self.entries_cache_key = self.path.clone();
        }

        // Restore selection by key after going back
        if let Some(key) = self.restore_key.take() {
            if let Some(idx) = self.cached_current_entries.iter().position(|e| e.label == key) {
                self.selection = idx;
            }
        }

        // Clamp selection
        if !self.cached_current_entries.is_empty() && self.selection >= self.cached_current_entries.len() {
            self.selection = self.cached_current_entries.len().saturating_sub(1);
        }

        // Keyboard navigation (skip when filter has focus)
        let skip_keys = self.filter.has_focus(ui);
        if !skip_keys && ui.input(|i| i.key_pressed(egui::Key::Questionmark)) {
            self.filter.focus();
        }

        let action = crate::widgets::read_miller_keys(ui, skip_keys);
        if crate::widgets::apply_selection(&mut self.selection, action, self.cached_current_entries.len()) {
            self.scroll_to_selection = true;
        }
        if action == crate::widgets::MillerAction::Enter {
            if let Some(entry) = self.cached_current_entries.get(self.selection) {
                if entry.is_container {
                    self.filter.query.clear();
                    self.path.push(entry.label.clone());
                    self.selection = 0;
                    self.scroll_to_selection = true;
                    // Rebuild immediately so this frame renders the new entries
                    self.cached_current_entries = build_entries_at_path(&self.path, schema, all);
                    self.cached_parent_entries = Some(build_entries_at_path(
                        &self.path[..self.path.len() - 1], schema, all,
                    ));
                    self.entries_cache_key = self.path.clone();
                }
            }
        }
        if action == crate::widgets::MillerAction::Back && !self.path.is_empty() {
            self.filter.query.clear();
            let popped = self.path.pop().unwrap();
            self.restore_key = Some(popped);
            self.scroll_to_selection = true;
            // Rebuild immediately
            self.cached_current_entries = build_entries_at_path(&self.path, schema, all);
            self.cached_parent_entries = if self.path.is_empty() {
                None
            } else {
                Some(build_entries_at_path(&self.path[..self.path.len() - 1], schema, all))
            };
            self.entries_cache_key = self.path.clone();
        }

        // 'A' toggles alphabetical sort on the structs pane
        let toggle_sort = ui.input(|i| i.key_pressed(egui::Key::A));
        if toggle_sort && self.path.len() == 1 {
            let selected_label = self.cached_current_entries.get(self.selection).map(|e| e.label.clone());
            self.sort_alpha = !self.sort_alpha;
            // Invalidate + rebuild
            self.entries_cache_key.clear();
            self.cached_current_entries = build_entries_at_path(&self.path, schema, all);
            if self.sort_alpha {
                self.cached_current_entries.sort_by(|a, b| a.label.cmp(&b.label));
            }
            self.entries_cache_key = self.path.clone();
            if let Some(label) = selected_label {
                if let Some(idx) = self.cached_current_entries.iter().position(|e| e.label == label) {
                    self.selection = idx;
                }
            }
        }
        // Re-borrow after potential mutation
        let current_entries = &self.cached_current_entries;
        let parent_entries = &self.cached_parent_entries;

        // --- Three-column miller layout ---
        let avail = ui.available_rect_before_wrap();
        let total_w = avail.width() - 12.0;
        let col_widths = [total_w * 0.22, total_w * 0.38, total_w * 0.38];
        let col_height = avail.height();

        let mut clicked_entry: Option<usize> = None;
        let mut dbl_clicked_entry: Option<usize> = None;
        let mut type_nav: Option<Vec<String>> = None;
        let mut filter_accepted = false;

        // Compute pane titles based on current path
        let (left_title, mid_title, right_title) = pane_titles(
            &self.path,
            current_entries.get(self.selection).map(|e| e.label.as_str()),
        );

        ui.horizontal(|ui| {
            ui.set_height(col_height);

            // Left: parent (history)
            ui.vertical(|ui| {
                ui.set_width(col_widths[0]);
                ui.set_height(col_height);
                if !left_title.is_empty() {
                    render_pane_title(ui, &left_title);
                }
                render_parent_column(ui, &self.path, &parent_entries, col_height);
            });

            draw_separator(ui, col_height);

            // Middle: current entries (with optional filter)
            ui.vertical(|ui| {
                ui.set_width(col_widths[1]);
                ui.set_height(col_height);
                render_pane_title(ui, &mid_title);
                let filter_resp = self.filter.show(ui, "? to filter");

                // Filter + snap selection
                let fr = self.filter.apply(
                    current_entries.iter().map(|e| &e.label), self.selection,
                );
                self.selection = fr.selection;
                let filtered: Vec<(usize, &Entry)> = fr.indices.iter()
                    .map(|&i| (i, &current_entries[i]))
                    .collect();

                if filter_resp.accept {
                    filter_accepted = true;
                }
                if !filtered.is_empty() {
                    if filter_resp.next {
                        let next_pos = (fr.filtered_pos + 1).min(filtered.len() - 1);
                        self.selection = filtered[next_pos].0;
                    }
                    if filter_resp.prev {
                        let prev_pos = fr.filtered_pos.saturating_sub(1);
                        self.selection = filtered[prev_pos].0;
                    }
                }

                let (c, d) = render_current_column(
                    ui,
                    &filtered,
                    fr.filtered_pos,
                    self.scroll_to_selection,
                    col_height,
                );
                // Map filtered index back to original
                clicked_entry = c.and_then(|fi| filtered.get(fi).map(|(orig, _)| *orig));
                dbl_clicked_entry = d.and_then(|fi| filtered.get(fi).map(|(orig, _)| *orig));
            });

            draw_separator(ui, col_height);

            // Right: values for selected entry
            ui.vertical(|ui| {
                let remaining = ui.available_width();
                ui.set_width(remaining);
                ui.set_height(col_height);
                render_pane_title(ui, &right_title);
                if let Some(entry) = current_entries.get(self.selection) {
                    // Cache the expensive value collection + flattening
                    let cache_key = (self.path.clone(), entry.label.clone());
                    if self.values_cache_key != cache_key {
                        self.values_rows = build_value_rows(
                            &self.path, &entry.label, schema, &all, files,
                        );
                        self.values_cache_key = cache_key;
                    }

                    if let Some(nav) = render_values_column(
                        ui,
                        &self.path,
                        &entry.label,
                        schema,
                        &all,
                        &self.values_rows,
                        col_height,
                    ) {
                        type_nav = Some(nav);
                    }
                }
            });
        });

        self.scroll_to_selection = false;

        if let Some(idx) = clicked_entry {
            self.selection = idx;
        }
        if let Some(idx) = dbl_clicked_entry {
            self.selection = idx;
            if let Some(entry) = current_entries.get(idx) {
                if entry.is_container {
                    self.path.push(entry.label.clone());
                    self.selection = 0;
                    self.scroll_to_selection = true;
                    self.cached_current_entries = build_entries_at_path(&self.path, schema, all);
                    self.cached_parent_entries = Some(build_entries_at_path(
                        &self.path[..self.path.len() - 1], schema, all,
                    ));
                    self.entries_cache_key = self.path.clone();
                }
            }
        }
        // Filter Enter: navigate into selected
        if filter_accepted {
            if let Some(entry) = self.cached_current_entries.get(self.selection) {
                if entry.is_container {
                    self.filter.query.clear();
                    self.path.push(entry.label.clone());
                    self.selection = 0;
                    self.scroll_to_selection = true;
                    self.cached_current_entries = build_entries_at_path(&self.path, schema, all);
                    self.cached_parent_entries = Some(build_entries_at_path(
                        &self.path[..self.path.len() - 1], schema, all,
                    ));
                    self.entries_cache_key = self.path.clone();
                }
            }
        }
        // Navigate to a struct via type link click
        if let Some(nav_path) = type_nav {
            self.path = nav_path;
            self.selection = 0;
            self.scroll_to_selection = true;
            self.cached_current_entries = build_entries_at_path(&self.path, schema, all);
            self.cached_parent_entries = if self.path.is_empty() {
                None
            } else {
                Some(build_entries_at_path(&self.path[..self.path.len() - 1], schema, all))
            };
            self.entries_cache_key = self.path.clone();
        }
    }
}

/// Build the list of entries at a given path in the schema tree.
fn build_entries_at_path(path: &[String], schema: &SchemaOverview, all: &[SharedStruct]) -> Vec<Entry> {
    match path.len() {
        0 => {
            // Root: show "Shared" and "Unique" categories
            let mut entries = Vec::new();
            if !schema.structs.is_empty() {
                entries.push(Entry {
                    label: "Shared".to_string(),
                    type_short: String::new(),
                    color: CatppuccinMocha::BLUE,
                    is_container: true,
                    count: Some(schema.structs.len()),
                });
            }
            if !schema.unique_structs.is_empty() {
                entries.push(Entry {
                    label: "Unique".to_string(),
                    type_short: String::new(),
                    color: CatppuccinMocha::LAVENDER,
                    is_container: true,
                    count: Some(schema.unique_structs.len()),
                });
            }
            entries
        }
        1 => {
            // Inside a category: list structs
            let structs = match path[0].as_str() {
                "Shared" => &schema.structs,
                "Unique" => &schema.unique_structs,
                _ => return Vec::new(),
            };
            structs
                .iter()
                .map(|s| Entry {
                    label: s.name.clone(),
                    type_short: format!("{} fields", s.fields.len()),
                    color: CatppuccinMocha::LAVENDER,
                    is_container: true,
                    count: Some(s.occurrence_count),
                })
                .collect()
        }
        _ => {
            // Inside a struct (or nested): list fields
            let structs = match path[0].as_str() {
                "Shared" => &schema.structs,
                "Unique" => &schema.unique_structs,
                _ => return Vec::new(),
            };
            let Some(root_struct) = structs.iter().find(|s| s.name == path[1]) else {
                return Vec::new();
            };

            // Walk remaining path segments to find the current type
            let current_type = resolve_type_at_path(&root_struct.fields, &path[2..], all);

            match current_type {
                Some(fields) => fields
                    .iter()
                    .map(|(name, typ)| {
                        let short = typ.short_name(&all);
                        let color = crate::theme::type_color(&short);
                        let is_container = is_navigable_type(typ, &all);
                        Entry {
                            label: name.clone(),
                            type_short: short,
                            color,
                            is_container,
                            count: None,
                        }
                    })
                    .collect(),
                None => Vec::new(),
            }
        }
    }
}

/// Resolve the fields available at a given sub-path within a struct's fields.
/// Returns the BTreeMap of fields if the path resolves to a struct-like type.
fn resolve_type_at_path<'a>(
    fields: &'a std::collections::BTreeMap<String, InferredType>,
    sub_path: &[String],
    all_structs: &'a [SharedStruct],
) -> Option<&'a std::collections::BTreeMap<String, InferredType>> {
    if sub_path.is_empty() {
        return Some(fields);
    }

    let field_type = fields.get(&sub_path[0])?;
    let inner = unwrap_container(field_type);

    // If this type references another struct, navigate into it
    if let InferredType::Object(obj_fields) = inner {
        return resolve_type_at_path(obj_fields, &sub_path[1..], all_structs);
    }

    // Check if it matches a named struct in the schema
    let type_name = inner.short_name(all_structs);
    if let Some(s) = all_structs.iter().find(|s| s.name == type_name) {
        return resolve_type_at_path(&s.fields, &sub_path[1..], all_structs);
    }

    None
}

/// Unwrap Option<T>, Vec<T> etc. to get the inner type
fn unwrap_container(typ: &InferredType) -> &InferredType {
    match typ {
        InferredType::Option(inner) => unwrap_container(inner),
        InferredType::Array(inner) => unwrap_container(inner),
        _ => typ,
    }
}

/// Check if a type can be navigated into (has sub-fields)
fn is_navigable_type(typ: &InferredType, all_structs: &[SharedStruct]) -> bool {
    let inner = unwrap_container(typ);
    match inner {
        InferredType::Object(_) => true,
        _ => {
            let name = inner.short_name(all_structs);
            all_structs.iter().any(|s| s.name == name)
        }
    }
}

fn draw_separator(ui: &mut Ui, height: f32) {
    crate::widgets::draw_separator(ui, height);
}

/// Compute titles for each of the three panes based on the current navigation path.
fn pane_titles(path: &[String], selected_label: Option<&str>) -> (String, String, String) {
    let selected = selected_label.unwrap_or("");
    match path.len() {
        0 => (
            String::new(),
            "Categories".to_string(),
            format!("{} Preview", selected),
        ),
        1 => (
            "Categories".to_string(),
            format!("{} Structs", path[0]),
            format!("{} Fields", selected),
        ),
        2 => (
            format!("{} Structs", path[0]),
            format!("{} Fields", path[1]),
            format!("{} Values", selected),
        ),
        _ => {
            let parent_field = &path[path.len() - 1];
            let grandparent = &path[path.len() - 2];
            (
                format!("{} Fields", grandparent),
                format!("{} Fields", parent_field),
                format!("{} Values", selected),
            )
        }
    }
}

fn render_pane_title(ui: &mut Ui, title: &str) {
    crate::widgets::miller::pane_title(ui, title);
}

fn render_parent_column(
    ui: &mut Ui,
    path: &[String],
    parent_entries: &Option<Vec<Entry>>,
    height: f32,
) {
    let Some(entries) = parent_entries else {
        return;
    };

    let active_label = path.last().map(|s| s.as_str()).unwrap_or("");
    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;

    egui::ScrollArea::vertical()
        .id_salt("shared_parent")
        .auto_shrink(false)
        .max_height(height)
        .show_rows(ui, row_height, entries.len(), |ui, range| {
            for i in range {
                let entry = &entries[i];
                let is_active = entry.label == active_label;
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
                                RichText::new(&entry.label)
                                    .color(text_color)
                                    .family(egui::FontFamily::Monospace)
                                    .size(12.0),
                            );
                        });
                    });
            }
        });
}

fn render_current_column(
    ui: &mut Ui,
    entries: &[(usize, &Entry)],
    selection: usize,
    scroll_to_selection: bool,
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

    let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;

    egui::ScrollArea::vertical()
        .id_salt("shared_current")
        .auto_shrink(false)
        .max_height(height)
        .show_rows(ui, row_height, entries.len(), |ui, range| {
            for i in range {
                let (_orig_idx, entry) = &entries[i];
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
                let label_color = if is_selected || is_hovered {
                    entry.color
                } else {
                    CatppuccinMocha::SUBTEXT0
                };
                let font_size = if is_hovered && !is_selected { 12.5 } else { 12.0 };
                let type_size = if is_hovered && !is_selected { 11.5 } else { 11.0 };
                let icon_size = if is_hovered && !is_selected { 12.5 } else { 12.0 };

                let r = egui::Frame::new()
                    .fill(bg)
                    .corner_radius(4.0)
                    .inner_margin(egui::Margin::symmetric(6, 2))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            // Container indicator
                            if entry.is_container {
                                ui.label(
                                    RichText::new(egui_phosphor::regular::BRACKETS_CURLY)
                                        .color(entry.color)
                                        .size(icon_size),
                                );
                            }

                            ui.label(
                                RichText::new(&entry.label)
                                    .color(label_color)
                                    .family(egui::FontFamily::Monospace)
                                    .size(font_size),
                            );

                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    if let Some(count) = entry.count {
                                        ui.label(
                                            RichText::new(format!("{}", count))
                                                .color(CatppuccinMocha::OVERLAY0)
                                                .family(egui::FontFamily::Monospace)
                                                .size(10.0),
                                        );
                                    }
                                    if !entry.type_short.is_empty() {
                                        ui.label(
                                            RichText::new(&entry.type_short)
                                                .color(entry.color)
                                                .family(egui::FontFamily::Monospace)
                                                .size(type_size),
                                        );
                                    }
                                },
                            );
                        });
                    })
                    .response
                    .interact(egui::Sense::click());

                // Store hover state for next frame
                crate::widgets::store_hover(ui.ctx(), row_id, r.hovered());

                if scroll_to_selection && is_selected {
                    r.scroll_to_me(Some(egui::Align::Center));
                }

                if r.clicked() {
                    clicked = Some(i);
                }
                if r.double_clicked() {
                    dbl_clicked = Some(i);
                }
            }
        });

    (clicked, dbl_clicked)
}

/// Render the right column showing all values found for the selected entry.
/// Returns an optional navigation path if the user clicks a type link.
/// Build pre-flattened rows for the values column (expensive — cached).
fn build_value_rows(
    path: &[String],
    selected_label: &str,
    schema: &SchemaOverview,
    all: &[SharedStruct],
    files: &[(String, serde_json::Value)],
) -> Vec<ValueRow> {
    if path.len() < 2 {
        return Vec::new();
    }
    let structs = match path[0].as_str() {
        "Shared" => &schema.structs,
        "Unique" => &schema.unique_structs,
        _ => return Vec::new(),
    };
    let Some(root_struct) = structs.iter().find(|s| s.name == path[1]) else {
        return Vec::new();
    };

    let selected_field_type = resolve_type_at_path(&root_struct.fields, &path[2..], all)
        .and_then(|fields| fields.get(selected_label).cloned());
    let is_primitive = selected_field_type
        .as_ref()
        .map(|t| !is_navigable_type(t, all))
        .unwrap_or(true);

    let raw_values = collect_values_for_field(path, selected_label, schema, all, files);

    // Group by file
    let mut per_file: Vec<(&str, Vec<&str>)> = Vec::new();
    for (val, filename) in &raw_values {
        if let Some(entry) = per_file.last_mut().filter(|(f, _)| *f == filename.as_str()) {
            entry.1.push(val.as_str());
        } else {
            per_file.push((filename.as_str(), vec![val.as_str()]));
        }
    }

    let mut rows = Vec::new();
    for (filename, values) in &per_file {
        let display_name = filename.strip_suffix(".json").unwrap_or(filename);
        rows.push(ValueRow::FileHeader(format!("── {} ──", display_name)));

        if is_primitive {
            let mut counts: Vec<(&str, usize)> = Vec::new();
            for val in values {
                if let Some(entry) = counts.iter_mut().find(|(v, _)| *v == *val) {
                    entry.1 += 1;
                } else {
                    counts.push((val, 1));
                }
            }
            counts.sort_by(|a, b| b.1.cmp(&a.1));
            for (val_str, count) in counts {
                let display = if val_str.len() > 60 {
                    format!("{}…", &val_str[..57])
                } else {
                    val_str.to_string()
                };
                let color = value_color(val_str);
                rows.push(ValueRow::Value {
                    text: display,
                    color,
                    count: if count > 1 { Some(count) } else { None },
                });
            }
        } else {
            for val_str in values {
                let display = if val_str.len() > 80 {
                    format!("{}…", &val_str[..77])
                } else {
                    val_str.to_string()
                };
                let color = value_color(val_str);
                rows.push(ValueRow::Value { text: display, color, count: None });
            }
        }
    }
    rows
}

/// Collect all field values across files (expensive — should be cached).
fn collect_values_for_field(
    path: &[String],
    selected_label: &str,
    schema: &SchemaOverview,
    all: &[SharedStruct],
    files: &[(String, serde_json::Value)],
) -> Vec<(String, String)> {
    if path.len() < 2 {
        return Vec::new();
    }
    let structs = match path[0].as_str() {
        "Shared" => &schema.structs,
        "Unique" => &schema.unique_structs,
        _ => return Vec::new(),
    };
    let Some(root_struct) = structs.iter().find(|s| s.name == path[1]) else {
        return Vec::new();
    };
    let mut field_path: Vec<&str> = path[2..].iter().map(|s| s.as_str()).collect();
    field_path.push(selected_label);
    let field_keys: Vec<&str> = root_struct.fields.keys().map(|k| k.as_str()).collect();

    let mut all_values: Vec<(String, String)> = Vec::new();
    for (filename, value) in files {
        if !root_struct.source_files.contains(filename) {
            continue;
        }
        let mut values: Vec<String> = Vec::new();
        collect_nested_field_values(value, &field_keys, &field_path, &mut values);
        for v in values {
            all_values.push((v, filename.clone()));
        }
    }
    all_values
}

fn render_values_column(
    ui: &mut Ui,
    path: &[String],
    selected_label: &str,
    schema: &SchemaOverview,
    all: &[SharedStruct],
    rows: &[ValueRow],
    height: f32,
) -> Option<Vec<String>> {
    let mut nav_target: Option<Vec<String>> = None;
    // Determine what we're looking at
    match path.len() {
        0 => {
            // Root level — selected is "Shared" or "Unique", show summary
            let structs = match selected_label {
                "Shared" => &schema.structs,
                "Unique" => &schema.unique_structs,
                _ => return None,
            };
            ui.label(
                RichText::new(format!("{} — {} structs", selected_label, structs.len()))
                    .color(CatppuccinMocha::BLUE)
                    .small(),
            );
            ui.add_space(4.0);

            let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
            egui::ScrollArea::vertical()
                .id_salt("shared_values")
                .auto_shrink(false)
                .max_height(height - 24.0)
                .show_rows(ui, row_height, structs.len(), |ui, range| {
                    for i in range {
                        let s = &structs[i];
                        ui.horizontal(|ui| {
                            ui.label(
                                RichText::new(egui_phosphor::regular::BRACKETS_CURLY)
                                    .color(CatppuccinMocha::LAVENDER)
                                    .size(11.0),
                            );
                            ui.label(
                                RichText::new(&s.name)
                                    .color(CatppuccinMocha::LAVENDER)
                                    .family(egui::FontFamily::Monospace)
                                    .size(11.0),
                            );
                            ui.label(
                                RichText::new(format!("{}x in {} files", s.occurrence_count, s.source_files.len()))
                                    .color(CatppuccinMocha::OVERLAY0)
                                    .family(egui::FontFamily::Monospace)
                                    .size(10.0),
                            );
                        });
                    }
                });
        }
        1 => {
            // Category level — selected is a struct name, show its fields preview
            let structs = match path[0].as_str() {
                "Shared" => &schema.structs,
                "Unique" => &schema.unique_structs,
                _ => return None,
            };
            let Some(s) = structs.iter().find(|s| s.name == selected_label) else {
                return None;
            };

            ui.label(
                RichText::new(format!(
                    "{} {} — {} fields, {}x",
                    egui_phosphor::regular::BRACKETS_CURLY,
                    s.name,
                    s.fields.len(),
                    s.occurrence_count,
                ))
                .color(CatppuccinMocha::LAVENDER)
                .small(),
            );
            ui.add_space(2.0);
            ui.label(
                RichText::new(format!("in: {}", s.source_files.join(", ")))
                    .color(CatppuccinMocha::OVERLAY0)
                    .size(10.0),
            );
            ui.add_space(4.0);

            let fields_vec: Vec<(&String, &InferredType)> = s.fields.iter().collect();
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 6.0;
            egui::ScrollArea::vertical()
                .id_salt("shared_values")
                .auto_shrink(false)
                .max_height(height - 40.0)
                .show_rows(ui, row_height, fields_vec.len(), |ui, range| {
                    for idx in range {
                        let (name, typ) = fields_vec[idx];
                        let short = typ.short_name(&all);
                        let color = crate::theme::type_color(&short);
                        let is_nav = is_navigable_type(typ, &all);

                        let (is_hovered, row_id) = crate::widgets::prev_frame_hover(ui.ctx(), ui.id(), idx);

                        let bg = if is_hovered {
                            egui::Color32::from_rgba_unmultiplied(
                                color.r(), color.g(), color.b(), 15,
                            )
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let name_color = if is_hovered { color } else { CatppuccinMocha::SUBTEXT0 };
                        let font_size = if is_hovered { 11.5 } else { 11.0 };

                        let row_r = egui::Frame::new()
                            .fill(bg)
                            .corner_radius(3.0)
                            .inner_margin(egui::Margin::symmetric(4, 1))
                            .show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    ui.label(
                                        RichText::new(name)
                                            .color(name_color)
                                            .family(egui::FontFamily::Monospace)
                                            .size(font_size),
                                    );
                                    if is_nav {
                                        let r = ui.add(
                                            egui::Label::new(
                                                RichText::new(&short)
                                                    .color(color)
                                                    .family(egui::FontFamily::Monospace)
                                                    .size(font_size)
                                                    .underline(),
                                            )
                                            .sense(egui::Sense::click()),
                                        );
                                        if r.hovered() {
                                            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                                        }
                                        if r.clicked() {
                                            let inner = unwrap_container(typ);
                                            let struct_name = inner.short_name(&all);
                                            if let Some(cat) = find_struct_category(&struct_name, schema) {
                                                nav_target = Some(vec![cat.to_string(), struct_name]);
                                            }
                                        }
                                    } else {
                                        ui.label(
                                            RichText::new(&short)
                                                .color(color)
                                                .family(egui::FontFamily::Monospace)
                                                .size(font_size),
                                        );
                                    }
                                });
                            })
                            .response;

                        crate::widgets::store_hover(ui.ctx(), row_id, row_r.hovered());
                    }
                });
        }
        _ => {
            // Inside a struct — show pre-flattened value rows with virtual scrolling
            let structs = match path[0].as_str() {
                "Shared" => &schema.structs,
                "Unique" => &schema.unique_structs,
                _ => return None,
            };
            let Some(root_struct) = structs.iter().find(|s| s.name == path[1]) else {
                return None;
            };

            let selected_field_type = resolve_type_at_path(&root_struct.fields, &path[2..], all)
                .and_then(|fields| fields.get(selected_label).cloned());

            let field_type_str = selected_field_type
                .as_ref()
                .map(|t| t.short_name(all))
                .unwrap_or_default();

            ui.label(
                RichText::new(format!("{}: {}", selected_label, field_type_str))
                    .color(CatppuccinMocha::BLUE)
                    .small(),
            );
            ui.add_space(4.0);

            let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
            egui::ScrollArea::vertical()
                .id_salt("shared_values")
                .auto_shrink(false)
                .max_height(height - 24.0)
                .show_rows(ui, row_height, rows.len(), |ui, range| {
                    for i in range {
                        match &rows[i] {
                            ValueRow::FileHeader(name) => {
                                ui.label(
                                    RichText::new(name)
                                        .color(CatppuccinMocha::YELLOW)
                                        .family(egui::FontFamily::Monospace)
                                        .size(11.0),
                                );
                            }
                            ValueRow::Value { text, color, count } => {
                                ui.horizontal(|ui| {
                                    ui.add_space(4.0);
                                    ui.label(
                                        RichText::new(text)
                                            .color(*color)
                                            .family(egui::FontFamily::Monospace)
                                            .size(11.0),
                                    );
                                    if let Some(c) = count {
                                        ui.label(
                                            RichText::new(format!("×{}", c))
                                                .color(CatppuccinMocha::OVERLAY0)
                                                .family(egui::FontFamily::Monospace)
                                                .size(10.0),
                                        );
                                    }
                                });
                            }
                        }
                    }
                });
        }
    }
    nav_target
}

/// Resolve which category ("Shared" or "Unique") a struct name belongs to.
fn find_struct_category(name: &str, schema: &SchemaOverview) -> Option<&'static str> {
    if schema.structs.iter().any(|s| s.name == name) {
        Some("Shared")
    } else if schema.unique_structs.iter().any(|s| s.name == name) {
        Some("Unique")
    } else {
        None
    }
}

/// Recursively collect values at a nested field path from objects matching the struct shape.
fn collect_nested_field_values(
    value: &serde_json::Value,
    struct_field_keys: &[&str],
    field_path: &[&str],
    out: &mut Vec<String>,
) {
    match value {
        serde_json::Value::Object(map) => {
            // Check if this object matches the struct shape
            let matching = struct_field_keys
                .iter()
                .filter(|k| map.contains_key(**k))
                .count();
            let ratio = if struct_field_keys.is_empty() {
                0.0
            } else {
                matching as f64 / struct_field_keys.len() as f64
            };

            if ratio >= 0.5 && !field_path.is_empty() {
                // Navigate through the field path
                let mut current: Option<&serde_json::Value> = Some(value);
                for segment in field_path {
                    match current {
                        Some(serde_json::Value::Object(m)) => {
                            current = m.get(*segment);
                        }
                        Some(serde_json::Value::Array(arr)) => {
                            for item in arr {
                                if let serde_json::Value::Object(m) = item {
                                    if let Some(v) = m.get(*segment) {
                                        out.push(format_value(v));
                                    }
                                }
                            }
                            return;
                        }
                        _ => {
                            current = None;
                            break;
                        }
                    }
                }

                match current {
                    Some(v) => out.push(format_value(v)),
                    None => out.push("(missing)".to_string()),
                }
            }

            // Recurse into all values to find more matching objects
            for (_, v) in map {
                collect_nested_field_values(v, struct_field_keys, field_path, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for v in arr {
                collect_nested_field_values(v, struct_field_keys, field_path, out);
            }
        }
        _ => {}
    }
}

fn format_value(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => format!("\"{}\"", s),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Array(a) => format!("[…] {} items", a.len()),
        serde_json::Value::Object(m) => format!("{{…}} {} fields", m.len()),
    }
}

fn value_color(val_str: &str) -> egui::Color32 {
    if val_str == "(missing)" {
        CatppuccinMocha::OVERLAY0
    } else if val_str.starts_with('"') {
        CatppuccinMocha::GREEN
    } else if val_str == "true" || val_str == "false" {
        CatppuccinMocha::MAUVE
    } else if val_str == "null" {
        CatppuccinMocha::OVERLAY0
    } else if val_str.starts_with('[') || val_str.starts_with('{') {
        CatppuccinMocha::YELLOW
    } else {
        CatppuccinMocha::PEACH
    }
}
