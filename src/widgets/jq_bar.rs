use egui::{self, RichText, Ui};

use crate::theme::CatppuccinMocha;

/// Response from the jq bar — tells the caller what happened this frame.
pub struct JqBarResponse {
    /// Enter/Tab/click accepted a completion — navigate to the path
    pub accepted: bool,
    /// Enter with no completions — execute as jq query
    pub run: bool,
    /// Escape was pressed — cancel and reset
    pub escaped: bool,
    /// Query text was edited by the user (typing)
    pub changed: bool,
    /// Cycling through completions — preview the path but don't commit
    pub previewing: bool,
}

/// Reusable jq filter bar with fuzzy autocompletion.
pub struct JqBar {
    pub query: String,
    pub completions: Vec<String>,
    comp_list: super::scrollable_list::ScrollableList,
    show_completions: bool,
    refocus: bool,
}

impl JqBar {
    pub fn new() -> Self {
        Self {
            query: ".".to_string(),
            completions: Vec::new(),
            comp_list: super::scrollable_list::ScrollableList::new(),
            show_completions: false,
            refocus: false,
        }
    }

    /// The egui Id used for the text input (for focus checks).
    /// Uses a stable global id so callers can check focus from any ui context.
    pub fn input_id() -> egui::Id {
        egui::Id::new("jq_bar_input_global")
    }

    /// Returns true if the jq bar input currently has focus.
    pub fn has_focus(ui: &Ui) -> bool {
        let id = Self::input_id();
        ui.ctx().memory(|m| m.focused().map_or(false, |f| f == id))
    }

    /// Request focus on the jq bar input next frame.
    pub fn focus(&mut self) {
        self.refocus = true;
    }

    /// Render the jq bar. `title` is shown to the left of the input (e.g. the current file name).
    pub fn show(&mut self, ui: &mut Ui, title: &str, root: &serde_json::Value) -> JqBarResponse {
        let mut response = JqBarResponse {
            accepted: false,
            run: false,
            escaped: false,
            changed: false,
            previewing: false,
        };

        let mut accepted_completion: Option<String> = None;
        let suppress_completions = self.refocus; // completion was just applied

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(egui_phosphor::regular::FUNNEL)
                    .color(CatppuccinMocha::MAUVE)
                    .size(14.0),
            );
            if !title.is_empty() {
                ui.label(
                    RichText::new(title)
                        .color(CatppuccinMocha::OVERLAY1)
                        .family(egui::FontFamily::Monospace)
                        .size(11.0),
                );
            }

            let input_id = Self::input_id();
            let text_response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id(input_id)
                    .font(egui::FontId::monospace(14.0))
                    .desired_width(ui.available_width() - 10.0)
                    .text_color(CatppuccinMocha::GREEN),
            );

            if self.refocus {
                text_response.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), text_response.id) {
                    let ccursor = egui::text::CCursor::new(self.query.len());
                    state
                        .cursor
                        .set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
                    state.store(ui.ctx(), text_response.id);
                }
                self.refocus = false;
            }

            // Check focus — Enter causes lost_focus, so check both
            let has_focus = text_response.has_focus();
            let just_lost_focus = text_response.lost_focus();

            if has_focus || just_lost_focus {
                if has_focus && text_response.changed() && !suppress_completions {
                    response.changed = true;
                    self.rebuild_completions(root);
                }

                let tab = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
                let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
                let ctrl_space =
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space));

                if self.show_completions && !self.completions.is_empty() {
                    // Ctrl-N/P or Arrow Down/Up: cycle suggestions
                    let cycle_down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                        || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N));
                    let cycle_up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp))
                        || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::P));

                    let moved = if cycle_down {
                        self.comp_list.down(self.completions.len())
                    } else if cycle_up {
                        self.comp_list.up()
                    } else {
                        false
                    };

                    if moved {
                        let comp = self.completions[self.comp_list.selection].clone();
                        self.apply_completion(&comp);
                        self.refocus = true;
                        response.previewing = true;
                    }

                    // Enter/Tab: accept current selection
                    if enter || tab {
                        accepted_completion = Some(self.completions[self.comp_list.selection].clone());
                    }
                } else if enter {
                    response.run = true;
                }

                if tab && !self.show_completions {
                    text_response.request_focus();
                }

                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.show_completions = false;
                    response.escaped = true;
                    text_response.surrender_focus();
                }

                if ctrl_space {
                    if self.show_completions {
                        self.show_completions = false;
                    } else {
                        self.rebuild_completions(root);
                    }
                }
            }
        });

        // Apply completion — sets the query, closes suggestions, signals caller to navigate
        if let Some(comp) = accepted_completion {
            self.apply_completion(&comp);
            self.show_completions = false;
            self.refocus = true;
            response.accepted = true;
        }

        // Show completion popup
        if self.show_completions && !self.completions.is_empty() {
            let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 4.0;
            let count = self.completions.len();
            let mut clicked_comp: Option<String> = None;

            egui::Frame::new()
                .fill(CatppuccinMocha::SURFACE0)
                .inner_margin(6.0)
                .corner_radius(4.0)
                .stroke(egui::Stroke::new(1.0, CatppuccinMocha::SURFACE1))
                .show(ui, |ui| {
                    let completions = &self.completions;
                    self.comp_list.show(
                        ui,
                        "jq_bar_completions",
                        count,
                        row_height,
                        Some(200.0),
                        &mut |ui, i, is_selected| {
                            let text_color = if is_selected {
                                CatppuccinMocha::BLUE
                            } else {
                                CatppuccinMocha::TEXT
                            };
                            let bg = if is_selected {
                                CatppuccinMocha::SURFACE1
                            } else {
                                CatppuccinMocha::SURFACE0
                            };
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&completions[i])
                                        .color(text_color)
                                        .family(egui::FontFamily::Monospace)
                                        .background_color(bg),
                                )
                                .sense(egui::Sense::click()),
                            )
                        },
                        &mut |idx| {
                            clicked_comp = Some(completions[idx].clone());
                        },
                    );
                });

            if let Some(comp) = clicked_comp {
                self.apply_completion(&comp);
                self.show_completions = false;
                self.refocus = true;
                response.accepted = true;
            }
        }

        response
    }

    pub fn rebuild_completions(&mut self, root: &serde_json::Value) {
        let segment = extract_current_segment(&self.query);
        let all_paths = collect_all_paths(root);

        let needle = segment.trim();
        if needle.is_empty() || needle == "." {
            self.completions = all_paths;
        } else {
            let needle_lower = needle.to_lowercase();
            let mut scored: Vec<(i64, String)> = all_paths
                .into_iter()
                .filter_map(|path| {
                    let score = fuzzy_score(&needle_lower, &path.to_lowercase());
                    if score > 0 { Some((score, path)) } else { None }
                })
                .collect();
            scored.sort_by(|a, b| b.0.cmp(&a.0).then(a.1.cmp(&b.1)));
            self.completions = scored.into_iter().map(|(_, p)| p).collect();
        }
        self.comp_list.reset();
        self.show_completions = !self.completions.is_empty();
    }

    pub fn apply_completion(&mut self, completion: &str) {
        let (before, _) = split_at_current_segment(&self.query);
        self.query = format!("{}{}", before, completion);
    }
}

// --- Helper functions ---

fn collect_all_paths(root: &serde_json::Value) -> Vec<String> {
    let mut all_paths = Vec::new();
    match root {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let path = format!(".{}", key);
                all_paths.push(path.clone());
                collect_deep_paths(val, &path, 6, &mut all_paths);
            }
        }
        serde_json::Value::Array(arr) => {
            all_paths.push(".[]".to_string());
            if let Some(first) = arr.first() {
                if first.is_object() {
                    collect_deep_paths(first, ".[]", 6, &mut all_paths);
                }
            }
        }
        _ => {}
    }
    all_paths.sort();
    all_paths.dedup();
    all_paths
}

fn collect_deep_paths(
    value: &serde_json::Value,
    prefix: &str,
    max_depth: usize,
    out: &mut Vec<String>,
) {
    if max_depth == 0 {
        return;
    }
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                let path = format!("{}.{}", prefix, key);
                out.push(path.clone());
                match val {
                    serde_json::Value::Object(_) => {
                        collect_deep_paths(val, &path, max_depth - 1, out);
                    }
                    serde_json::Value::Array(arr) => {
                        let arr_path = format!("{}[]", path);
                        out.push(arr_path.clone());
                        if let Some(first) = arr.first() {
                            if first.is_object() {
                                collect_deep_paths(first, &arr_path, max_depth - 1, out);
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
        serde_json::Value::Array(arr) => {
            let arr_path = format!("{}[]", prefix);
            out.push(arr_path.clone());
            if let Some(first) = arr.first() {
                if first.is_object() {
                    collect_deep_paths(first, &arr_path, max_depth - 1, out);
                }
            }
        }
        _ => {}
    }
}

fn fuzzy_score(needle: &str, haystack: &str) -> i64 {
    let needle_chars: Vec<char> = needle.chars().collect();
    let haystack_chars: Vec<char> = haystack.chars().collect();
    if needle_chars.is_empty() {
        return 1;
    }
    if needle_chars.len() > haystack_chars.len() {
        return 0;
    }
    let mut score: i64 = 0;
    let mut ni = 0;
    let mut prev_match: Option<usize> = None;
    for (hi, &hc) in haystack_chars.iter().enumerate() {
        if ni < needle_chars.len() && hc == needle_chars[ni] {
            score += 1;
            if let Some(prev) = prev_match {
                if hi == prev + 1 {
                    score += 2;
                }
            }
            if hi > 0 && matches!(haystack_chars[hi - 1], '.' | '[' | ']') {
                score += 1;
            }
            prev_match = Some(hi);
            ni += 1;
        }
    }
    if ni == needle_chars.len() { score } else { 0 }
}

pub fn extract_current_segment(query: &str) -> &str {
    let start = query
        .rfind(|c: char| c == '|' || c == '(' || c == ';')
        .map(|i| i + 1)
        .unwrap_or(0);
    query[start..].trim_start()
}

pub fn split_at_current_segment(query: &str) -> (&str, &str) {
    let start = query
        .rfind(|c: char| c == '|' || c == '(' || c == ';')
        .map(|i| i + 1)
        .unwrap_or(0);
    let segment_start = query[start..]
        .find(|c: char| !c.is_whitespace())
        .map(|offset| start + offset)
        .unwrap_or(query.len());
    (&query[..segment_start], &query[segment_start..])
}
