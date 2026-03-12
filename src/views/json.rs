use egui::{self, RichText, Ui};
use std::collections::BTreeSet;

use crate::theme::CatppuccinMocha;

/// A pre-rendered line in the JSON view.
struct JsonLine {
    text: String,
    color: egui::Color32,
    /// If Some, clicking this line toggles the path.
    toggle_path: Option<String>,
}

pub struct JsonView {
    expanded: BTreeSet<String>,
    lines: Vec<JsonLine>,
    dirty: bool,
}

impl JsonView {
    pub fn new() -> Self {
        Self {
            expanded: BTreeSet::new(),
            lines: Vec::new(),
            dirty: true,
        }
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn show(&mut self, ui: &mut Ui, value: &serde_json::Value, filename: &str) {
        ui.horizontal(|ui| {
            if ui.button("Expand All").clicked() {
                self.expand_all(value, "root");
                self.dirty = true;
            }
            if ui.button("Collapse All").clicked() {
                self.expanded.clear();
                self.dirty = true;
            }
            if ui.button("Copy JSON").clicked() {
                let text = serde_json::to_string_pretty(value).unwrap_or_default();
                ui.ctx().copy_text(text);
            }
            ui.label(
                RichText::new(format!("{} lines", self.lines.len()))
                    .color(CatppuccinMocha::OVERLAY0)
                    .small(),
            );
        });

        ui.add_space(8.0);

        // Rebuild line buffer when dirty
        if self.dirty {
            self.lines.clear();
            flatten_value(value, "root", 0, false, &self.expanded, &mut self.lines);
            self.dirty = false;
        }

        let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        let num_rows = self.lines.len();
        let mut toggle: Option<String> = None;

        egui::ScrollArea::vertical()
            .id_salt(format!("json_scroll_{}", filename))
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink(false)
            .show_rows(ui, row_height, num_rows, |ui, row_range| {
                for row in row_range {
                    let line = &self.lines[row];
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!("{:>4} ", row + 1))
                                .color(CatppuccinMocha::SURFACE2)
                                .family(egui::FontFamily::Monospace),
                        );

                        if line.toggle_path.is_some() {
                            let r = ui.add(
                                egui::Label::new(
                                    RichText::new(&line.text)
                                        .color(line.color)
                                        .family(egui::FontFamily::Monospace),
                                )
                                .sense(egui::Sense::click()),
                            );
                            if r.clicked() {
                                toggle = line.toggle_path.clone();
                            }
                        } else {
                            ui.label(
                                RichText::new(&line.text)
                                    .color(line.color)
                                    .family(egui::FontFamily::Monospace),
                            );
                        }
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
                self.expanded.insert(path.to_string());
                for (key, val) in map {
                    self.expand_all(val, &format!("{}.{}", path, key));
                }
            }
            serde_json::Value::Array(arr) => {
                self.expanded.insert(path.to_string());
                for (i, val) in arr.iter().enumerate() {
                    self.expand_all(val, &format!("{}[{}]", path, i));
                }
            }
            _ => {}
        }
    }
}

/// Flatten the JSON tree into a vec of lines based on current expand state.
fn flatten_value(
    value: &serde_json::Value,
    path: &str,
    indent: usize,
    trailing_comma: bool,
    expanded: &BTreeSet<String>,
    out: &mut Vec<JsonLine>,
) {
    let pad = "  ".repeat(indent);
    let comma = if trailing_comma { "," } else { "" };

    match value {
        serde_json::Value::Null => {
            out.push(JsonLine {
                text: format!("{}null{}", pad, comma),
                color: CatppuccinMocha::OVERLAY0,
                toggle_path: None,
            });
        }
        serde_json::Value::Bool(b) => {
            out.push(JsonLine {
                text: format!("{}{}{}", pad, b, comma),
                color: CatppuccinMocha::MAUVE,
                toggle_path: None,
            });
        }
        serde_json::Value::Number(n) => {
            out.push(JsonLine {
                text: format!("{}{}{}", pad, n, comma),
                color: CatppuccinMocha::PEACH,
                toggle_path: None,
            });
        }
        serde_json::Value::String(s) => {
            out.push(JsonLine {
                text: format!("{}\"{}\"{}",  pad, s, comma),
                color: CatppuccinMocha::GREEN,
                toggle_path: None,
            });
        }
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                out.push(JsonLine {
                    text: format!("{}{{}}{}", pad, comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: None,
                });
                return;
            }

            let is_expanded = expanded.contains(path);
            if is_expanded {
                out.push(JsonLine {
                    text: format!("{}v {{", pad),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: Some(path.to_string()),
                });

                let entries: Vec<_> = map.iter().collect();
                let len = entries.len();
                for (i, (key, val)) in entries.iter().enumerate() {
                    let child_path = format!("{}.{}", path, key);
                    let has_comma = i < len - 1;
                    let inner_pad = "  ".repeat(indent + 1);

                    if val.is_object() || val.is_array() {
                        // Key prefix on same line as opener
                        let prefix = format!("{}\"{}\": ", inner_pad, key);
                        flatten_value_with_prefix(val, &child_path, indent + 1, has_comma, expanded, out, &prefix);
                    } else {
                        let c = if has_comma { "," } else { "" };
                        let val_text = format_inline_value(val);
                        let color = inline_value_color(val);
                        out.push(JsonLine {
                            text: format!("{}\"{}\": {}{}", inner_pad, key, val_text, c),
                            color,
                            toggle_path: None,
                        });
                    }
                }

                out.push(JsonLine {
                    text: format!("{}}}{}", pad, comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: None,
                });
            } else {
                out.push(JsonLine {
                    text: format!("{}> {{ {} fields }}{}", pad, map.len(), comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: Some(path.to_string()),
                });
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                out.push(JsonLine {
                    text: format!("{}[]{}", pad, comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: None,
                });
                return;
            }

            let is_expanded = expanded.contains(path);
            if is_expanded {
                out.push(JsonLine {
                    text: format!("{}v [", pad),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: Some(path.to_string()),
                });

                let len = arr.len();
                for (i, val) in arr.iter().enumerate() {
                    let child_path = format!("{}[{}]", path, i);
                    let has_comma = i < len - 1;
                    flatten_value(val, &child_path, indent + 1, has_comma, expanded, out);
                }

                out.push(JsonLine {
                    text: format!("{}]{}", pad, comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: None,
                });
            } else {
                out.push(JsonLine {
                    text: format!("{}> [ {} items ]{}", pad, arr.len(), comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: Some(path.to_string()),
                });
            }
        }
    }
}

/// Like flatten_value but prepends a prefix (key name) to the first line.
fn flatten_value_with_prefix(
    value: &serde_json::Value,
    path: &str,
    indent: usize,
    trailing_comma: bool,
    expanded: &BTreeSet<String>,
    out: &mut Vec<JsonLine>,
    prefix: &str,
) {
    let pad = "  ".repeat(indent);
    let comma = if trailing_comma { "," } else { "" };

    match value {
        serde_json::Value::Object(map) => {
            if map.is_empty() {
                out.push(JsonLine {
                    text: format!("{}{{}}{}", prefix, comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: None,
                });
                return;
            }

            let is_expanded = expanded.contains(path);
            if is_expanded {
                out.push(JsonLine {
                    text: format!("{}v {{", prefix),
                    color: CatppuccinMocha::BLUE,
                    toggle_path: Some(path.to_string()),
                });

                let entries: Vec<_> = map.iter().collect();
                let len = entries.len();
                for (i, (key, val)) in entries.iter().enumerate() {
                    let child_path = format!("{}.{}", path, key);
                    let has_comma = i < len - 1;
                    let inner_pad = "  ".repeat(indent + 1);

                    if val.is_object() || val.is_array() {
                        let child_prefix = format!("{}\"{}\": ", inner_pad, key);
                        flatten_value_with_prefix(val, &child_path, indent + 1, has_comma, expanded, out, &child_prefix);
                    } else {
                        let c = if has_comma { "," } else { "" };
                        let val_text = format_inline_value(val);
                        let color = inline_value_color(val);
                        out.push(JsonLine {
                            text: format!("{}\"{}\": {}{}", inner_pad, key, val_text, c),
                            color,
                            toggle_path: None,
                        });
                    }
                }

                out.push(JsonLine {
                    text: format!("{}}}{}", pad, comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: None,
                });
            } else {
                out.push(JsonLine {
                    text: format!("{}> {{ {} fields }}{}", prefix, map.len(), comma),
                    color: CatppuccinMocha::BLUE,
                    toggle_path: Some(path.to_string()),
                });
            }
        }
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                out.push(JsonLine {
                    text: format!("{}[]{}", prefix, comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: None,
                });
                return;
            }

            let is_expanded = expanded.contains(path);
            if is_expanded {
                out.push(JsonLine {
                    text: format!("{}v [", prefix),
                    color: CatppuccinMocha::BLUE,
                    toggle_path: Some(path.to_string()),
                });

                let len = arr.len();
                for (i, val) in arr.iter().enumerate() {
                    let child_path = format!("{}[{}]", path, i);
                    let has_comma = i < len - 1;
                    flatten_value(val, &child_path, indent + 1, has_comma, expanded, out);
                }

                out.push(JsonLine {
                    text: format!("{}]{}", pad, comma),
                    color: CatppuccinMocha::TEXT,
                    toggle_path: None,
                });
            } else {
                out.push(JsonLine {
                    text: format!("{}> [ {} items ]{}", prefix, arr.len(), comma),
                    color: CatppuccinMocha::BLUE,
                    toggle_path: Some(path.to_string()),
                });
            }
        }
        _ => {
            // Scalar with prefix — shouldn't happen but handle gracefully
            flatten_value(value, path, indent, trailing_comma, expanded, out);
        }
    }
}

fn format_inline_value(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_string(),
        serde_json::Value::Bool(b) => b.to_string(),
        serde_json::Value::Number(n) => n.to_string(),
        serde_json::Value::String(s) => format!("\"{}\"", s),
        _ => String::new(),
    }
}

fn inline_value_color(value: &serde_json::Value) -> egui::Color32 {
    match value {
        serde_json::Value::Null => CatppuccinMocha::OVERLAY0,
        serde_json::Value::Bool(_) => CatppuccinMocha::MAUVE,
        serde_json::Value::Number(_) => CatppuccinMocha::PEACH,
        serde_json::Value::String(_) => CatppuccinMocha::GREEN,
        _ => CatppuccinMocha::TEXT,
    }
}
