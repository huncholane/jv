use egui::{self, RichText, Ui};
use std::collections::BTreeSet;

use crate::temporal::{detect_temporal, detect_unix_timestamp};
use crate::theme::{type_color, CatppuccinMocha};
use crate::types::infer_type;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum SchemaSortMode {
    Name,
    Fields,
    Occurrences,
    Files,
}

enum SchemaRow {
    Header { struct_idx: usize, expanded: bool },
    GridHeader { struct_idx: usize },
    Field { struct_idx: usize, field_idx: usize },
    Spacer,
}

struct TableRow {
    field: String,
    field_color: egui::Color32,
    type_name: String,
    type_color: egui::Color32,
    value_text: String,
    value_color: egui::Color32,
    toggle_path: Option<String>,
    /// Tooltip info for temporal values
    temporal_tip: Option<String>,
    depth: usize,
}

pub struct TableView {
    expanded: BTreeSet<String>,
    rows: Vec<TableRow>,
    dirty: bool,
    schema_sort: SchemaSortMode,
    schema_sort_ascending: bool,
}

impl TableView {
    pub fn new() -> Self {
        Self {
            expanded: BTreeSet::new(),
            rows: Vec::new(),
            dirty: true,
            schema_sort: SchemaSortMode::Occurrences,
            schema_sort_ascending: false,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn show_schema_matrix(
        &mut self,
        ui: &mut Ui,
        structs: &[crate::schema::SharedStruct],
        filenames: &[String],
    ) {
        use egui_phosphor::regular;

        // Sort struct indices
        let mut sorted_indices: Vec<usize> = (0..structs.len()).collect();
        let asc = self.schema_sort_ascending;
        sorted_indices.sort_by(|&a, &b| {
            let cmp = match self.schema_sort {
                SchemaSortMode::Name => structs[a].name.to_lowercase().cmp(&structs[b].name.to_lowercase()),
                SchemaSortMode::Fields => structs[a].fields.len().cmp(&structs[b].fields.len()),
                SchemaSortMode::Occurrences => structs[a].occurrence_count.cmp(&structs[b].occurrence_count),
                SchemaSortMode::Files => structs[a].source_files.len().cmp(&structs[b].source_files.len()),
            };
            if asc { cmp } else { cmp.reverse() }
        });

        // Sortable column header
        let sort_arrow = |mode: SchemaSortMode, current: SchemaSortMode, ascending: bool| -> &str {
            if mode == current {
                if ascending { regular::CARET_UP } else { regular::CARET_DOWN }
            } else {
                ""
            }
        };

        ui.horizontal(|ui| {
            let col_data: [(SchemaSortMode, &str, f32); 4] = [
                (SchemaSortMode::Name, "Struct", 220.0),
                (SchemaSortMode::Fields, "Fields", 70.0),
                (SchemaSortMode::Occurrences, "Occurrences", 100.0),
                (SchemaSortMode::Files, "Files", 80.0),
            ];
            for (mode, label, width) in col_data {
                let arrow = sort_arrow(mode, self.schema_sort, self.schema_sort_ascending);
                let text = if arrow.is_empty() {
                    label.to_string()
                } else {
                    format!("{} {}", label, arrow)
                };
                let r = ui.add_sized(
                    [width, 20.0],
                    egui::Label::new(
                        RichText::new(text)
                            .color(CatppuccinMocha::SUBTEXT0)
                            .strong()
                            .size(12.0),
                    ).sense(egui::Sense::click()),
                );
                if r.clicked() {
                    if self.schema_sort == mode {
                        self.schema_sort_ascending = !self.schema_sort_ascending;
                    } else {
                        self.schema_sort = mode;
                        self.schema_sort_ascending = mode == SchemaSortMode::Name;
                    }
                }
            }
        });

        // Subtle divider
        let sep_rect = ui.available_rect_before_wrap();
        ui.painter().line_segment(
            [
                egui::pos2(sep_rect.left(), sep_rect.top()),
                egui::pos2(sep_rect.right(), sep_rect.top()),
            ],
            egui::Stroke::new(1.0, CatppuccinMocha::SURFACE0),
        );
        ui.add_space(4.0);

        // Pre-compute visible rows using sorted order
        let mut rows: Vec<SchemaRow> = Vec::new();
        for &si in &sorted_indices {
            let shared = &structs[si];
            let is_expanded = self.expanded.contains(&shared.name);
            rows.push(SchemaRow::Header { struct_idx: si, expanded: is_expanded });
            if is_expanded {
                rows.push(SchemaRow::GridHeader { struct_idx: si });
                for field_idx in 0..shared.fields.len() {
                    rows.push(SchemaRow::Field { struct_idx: si, field_idx });
                }
                rows.push(SchemaRow::Spacer);
            }
        }

        let row_height = 20.0;
        let num_rows = rows.len();

        egui::ScrollArea::both()
            .id_salt("schema_matrix_scroll")
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink(false)
            .show_rows(ui, row_height, num_rows, |ui, row_range| {
                let mut toggle = None;
                for idx in row_range {
                    match &rows[idx] {
                        SchemaRow::Header { struct_idx, expanded } => {
                            let shared = &structs[*struct_idx];
                            let arrow = if *expanded {
                                regular::CARET_DOWN
                            } else {
                                regular::CARET_RIGHT
                            };
                            let stripe = if idx % 2 == 0 {
                                CatppuccinMocha::BASE
                            } else {
                                CatppuccinMocha::MANTLE
                            };
                            egui::Frame::new().fill(stripe).show(ui, |ui| {
                                ui.horizontal(|ui| {
                                    // Name column — left-aligned in fixed width
                                    let (rect, _) = ui.allocate_exact_size(
                                        egui::vec2(220.0, row_height),
                                        egui::Sense::click(),
                                    );
                                    let name_text = format!("{} {}", arrow, shared.name);
                                    let galley = ui.painter().layout_no_wrap(
                                        name_text,
                                        egui::FontId::proportional(14.0),
                                        CatppuccinMocha::LAVENDER,
                                    );
                                    ui.painter().galley(
                                        egui::pos2(rect.left(), rect.center().y - galley.size().y / 2.0),
                                        galley,
                                        CatppuccinMocha::LAVENDER,
                                    );
                                    if ui.rect_contains_pointer(rect) && ui.input(|i| i.pointer.any_pressed()) {
                                        toggle = Some(shared.name.clone());
                                    }

                                    // Fields column
                                    ui.add_sized(
                                        [70.0, row_height],
                                        egui::Label::new(
                                            RichText::new(shared.fields.len().to_string())
                                                .color(CatppuccinMocha::TEXT),
                                        ),
                                    );

                                    // Occurrences column
                                    ui.add_sized(
                                        [100.0, row_height],
                                        egui::Label::new(
                                            RichText::new(shared.occurrence_count.to_string())
                                                .color(CatppuccinMocha::TEXT),
                                        ),
                                    );

                                    // Files column
                                    let files_text = format!("{}/{}", shared.source_files.len(), filenames.len());
                                    ui.add_sized(
                                        [80.0, row_height],
                                        egui::Label::new(
                                            RichText::new(&files_text)
                                                .color(CatppuccinMocha::TEXT),
                                        ),
                                    );
                                });
                            });
                        }
                        SchemaRow::GridHeader { struct_idx } => {
                            let shared = &structs[*struct_idx];
                            ui.horizontal(|ui| {
                                ui.add_space(16.0);
                                ui.add_sized([220.0, row_height], egui::Label::new(
                                    RichText::new("Field")
                                        .color(CatppuccinMocha::BLUE)
                                        .strong(),
                                ));
                                for file in &shared.source_files {
                                    ui.add_sized([120.0, row_height], egui::Label::new(
                                        RichText::new(file)
                                            .color(CatppuccinMocha::YELLOW)
                                            .strong()
                                            .size(11.0),
                                    ));
                                }
                            });
                        }
                        SchemaRow::Field { struct_idx, field_idx } => {
                            let shared = &structs[*struct_idx];
                            let fields: Vec<_> = shared.fields.iter().collect();
                            if let Some((field_name, field_type)) = fields.get(*field_idx) {
                                let stripe = if field_idx % 2 == 0 {
                                    CatppuccinMocha::SURFACE0
                                } else {
                                    CatppuccinMocha::BASE
                                };
                                egui::Frame::new().fill(stripe).show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.add_space(16.0);
                                        ui.add_sized([220.0, row_height], egui::Label::new(
                                            RichText::new(*field_name)
                                                .color(CatppuccinMocha::BLUE),
                                        ));
                                        for _file in &shared.source_files {
                                            let type_str = field_type.short_name(structs);
                                            let color = type_color(&type_str);
                                            let r = ui.add_sized([120.0, row_height], egui::Label::new(
                                                RichText::new(&type_str).color(color),
                                            ));
                                            if let Some(tip) = field_type.tooltip(structs) {
                                                r.on_hover_text(tip);
                                            }
                                        }
                                    });
                                });
                            }
                        }
                        SchemaRow::Spacer => {
                            ui.add_space(8.0);
                        }
                    }
                }
                if let Some(name) = toggle {
                    if self.expanded.contains(&name) {
                        self.expanded.remove(&name);
                    } else {
                        self.expanded.insert(name);
                    }
                }
            });
    }

    pub fn show(&mut self, ui: &mut Ui, value: &serde_json::Value, filename: &str) {
        ui.horizontal(|ui| {
            if ui.button("Expand All").clicked() {
                self.expand_all(value, "");
                self.dirty = true;
            }
            if ui.button("Collapse All").clicked() {
                self.expanded.clear();
                self.dirty = true;
            }
            ui.label(
                RichText::new(format!("{} rows", self.rows.len()))
                    .color(CatppuccinMocha::OVERLAY0)
                    .small(),
            );
        });

        ui.add_space(8.0);

        if self.dirty {
            self.rows.clear();
            flatten_table(value, "", 0, &self.expanded, &mut self.rows);
            self.dirty = false;
        }

        let row_height = ui.text_style_height(&egui::TextStyle::Body) + 4.0;
        let num_rows = self.rows.len();
        let mut toggle: Option<String> = None;

        // Header
        ui.horizontal(|ui| {
            ui.set_min_width(ui.available_width());
            ui.columns(3, |cols| {
                cols[0].label(RichText::new("Field").color(CatppuccinMocha::SUBTEXT0).strong());
                cols[1].label(RichText::new("Type").color(CatppuccinMocha::SUBTEXT0).strong());
                cols[2].label(RichText::new("Value").color(CatppuccinMocha::SUBTEXT0).strong());
            });
        });

        egui::ScrollArea::vertical()
            .id_salt(format!("table_scroll_{}", filename))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink(false)
            .show_rows(ui, row_height, num_rows, |ui, row_range| {
                for idx in row_range {
                    let row = &self.rows[idx];
                    let stripe = if idx % 2 == 0 {
                        CatppuccinMocha::BASE
                    } else {
                        CatppuccinMocha::MANTLE
                    };

                    egui::Frame::new()
                        .fill(stripe)
                        .show(ui, |ui| {
                            ui.columns(3, |cols| {
                                // Field column with indentation
                                cols[0].horizontal(|ui| {
                                    if row.depth > 0 {
                                        ui.add_space(row.depth as f32 * 16.0);
                                    }
                                    if row.toggle_path.is_some() {
                                        let r = ui.add(
                                            egui::Label::new(
                                                RichText::new(&row.field).color(row.field_color),
                                            )
                                            .sense(egui::Sense::click()),
                                        );
                                        if r.clicked() {
                                            toggle = row.toggle_path.clone();
                                        }
                                    } else {
                                        ui.label(
                                            RichText::new(&row.field).color(row.field_color),
                                        );
                                    }
                                });

                                // Type column
                                cols[1].label(
                                    RichText::new(&row.type_name).color(row.type_color),
                                );

                                // Value column
                                let r = cols[2].label(
                                    RichText::new(&row.value_text).color(row.value_color),
                                );
                                if let Some(tip) = &row.temporal_tip {
                                    r.on_hover_text(tip);
                                }
                            });
                        });
                }
            });

        if let Some(path) = toggle {
            if self.expanded.contains(&path) {
                self.expanded.remove(&path);
            } else {
                self.expanded.insert(path);
            }
            self.dirty = true;
        }
    }

    fn expand_all(&mut self, value: &serde_json::Value, path: &str) {
        match value {
            serde_json::Value::Object(map) => {
                if !path.is_empty() {
                    self.expanded.insert(path.to_string());
                }
                for (key, val) in map {
                    let child_path = if path.is_empty() {
                        key.clone()
                    } else {
                        format!("{}.{}", path, key)
                    };
                    self.expand_all(val, &child_path);
                }
            }
            serde_json::Value::Array(arr) => {
                if !path.is_empty() {
                    self.expanded.insert(path.to_string());
                }
                for (i, val) in arr.iter().enumerate() {
                    self.expand_all(val, &format!("{}[{}]", path, i));
                }
            }
            _ => {}
        }
    }
}

fn flatten_table(
    value: &serde_json::Value,
    path: &str,
    depth: usize,
    expanded: &BTreeSet<String>,
    out: &mut Vec<TableRow>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let child_path = if path.is_empty() {
                    key.clone()
                } else {
                    format!("{}.{}", path, key)
                };
                flatten_field(key, val, &child_path, depth, expanded, out);
            }
        }
        serde_json::Value::Array(arr) => {
            for (i, val) in arr.iter().enumerate() {
                let child_path = format!("{}[{}]", path, i);
                let key = format!("[{}]", i);
                flatten_field(&key, val, &child_path, depth, expanded, out);
            }
        }
        _ => {}
    }
}

fn flatten_field(
    key: &str,
    value: &serde_json::Value,
    path: &str,
    depth: usize,
    expanded: &BTreeSet<String>,
    out: &mut Vec<TableRow>,
) {
    let is_expandable = value.is_object() || value.is_array();
    let is_expanded = expanded.contains(path);
    let inferred = infer_type(value);

    let field = if is_expandable {
        let arrow = if is_expanded {
            egui_phosphor::regular::CARET_DOWN
        } else {
            egui_phosphor::regular::CARET_RIGHT
        };
        let count = match value {
            serde_json::Value::Object(m) => format!(" ({})", m.len()),
            serde_json::Value::Array(a) => format!(" [{}]", a.len()),
            _ => String::new(),
        };
        format!("{} {}{}", arrow, key, count)
    } else {
        key.to_string()
    };

    let tn = inferred.display_name();
    let tc = type_color(&tn);

    let (value_text, value_color, temporal_tip) = match value {
        serde_json::Value::Null => ("null".to_string(), CatppuccinMocha::OVERLAY0, None),
        serde_json::Value::Bool(b) => (b.to_string(), CatppuccinMocha::MAUVE, None),
        serde_json::Value::Number(n) => {
            let tip = n.as_i64().and_then(|i| {
                detect_unix_timestamp(i).map(|t| {
                    let rel = t.relative_time();
                    if rel.is_empty() {
                        format!("🕐 {}", t.display())
                    } else {
                        format!("🕐 {} ({})", t.display(), rel)
                    }
                })
            });
            (n.to_string(), CatppuccinMocha::PEACH, tip)
        }
        serde_json::Value::String(s) => {
            if let Some(t) = detect_temporal(s) {
                let label = match &t {
                    crate::temporal::TemporalValue::NaiveDate(_) => "d",
                    crate::temporal::TemporalValue::NaiveTime(_) => "t",
                    _ => "dt",
                };
                let mut tip_parts = vec![format!("🕐 {}", t.display())];
                if let Some(tz) = t.timezone_info() {
                    tip_parts.push(format!("Timezone: {}", tz));
                }
                let rel = t.relative_time();
                if !rel.is_empty() {
                    tip_parts.push(rel);
                }
                (format!("{} {}", label, t.display()), CatppuccinMocha::BLUE, Some(tip_parts.join("\n")))
            } else {
                let display = if s.len() > 80 {
                    format!("\"{}...\"", &s[..77])
                } else {
                    format!("\"{}\"", s)
                };
                (display, CatppuccinMocha::GREEN, None)
            }
        }
        serde_json::Value::Object(m) => {
            (format!("{{ {} fields }}", m.len()), CatppuccinMocha::SUBTEXT0, None)
        }
        serde_json::Value::Array(a) => {
            (format!("[ {} items ]", a.len()), CatppuccinMocha::SUBTEXT0, None)
        }
    };

    out.push(TableRow {
        field,
        field_color: CatppuccinMocha::BLUE,
        type_name: tn,
        type_color: tc,
        value_text,
        value_color,
        toggle_path: if is_expandable { Some(path.to_string()) } else { None },
        temporal_tip,
        depth,
    });

    if is_expandable && is_expanded {
        flatten_table(value, path, depth + 1, expanded, out);
    }
}
