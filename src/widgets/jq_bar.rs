use egui::{self, RichText, Ui};

use crate::theme::CatppuccinMocha;

/// Response from the jq bar — tells the caller what happened this frame.
pub struct JqBarResponse {
    /// Enter was pressed (caller should execute or navigate)
    pub run: bool,
    /// Escape was pressed (caller should cancel/reset)
    pub escaped: bool,
    /// Query text was edited by the user
    pub changed: bool,
    /// A completion was accepted (Tab or click)
    pub completion_applied: bool,
}

/// Reusable jq filter bar with fuzzy autocompletion.
pub struct JqBar {
    pub query: String,
    pub completions: Vec<String>,
    completion_index: usize,
    show_completions: bool,
    refocus: bool,
}

impl JqBar {
    pub fn new() -> Self {
        Self {
            query: ".".to_string(),
            completions: Vec::new(),
            completion_index: 0,
            show_completions: false,
            refocus: false,
        }
    }

    /// The egui Id used for the text input (for focus checks).
    pub fn input_id(ui: &Ui) -> egui::Id {
        ui.id().with("jq_bar_input")
    }

    /// Returns true if the jq bar input currently has focus.
    pub fn has_focus(ui: &Ui) -> bool {
        let id = Self::input_id(ui);
        ui.ctx().memory(|m| m.focused().map_or(false, |f| f == id))
    }

    /// Render the jq bar. Caller provides `root` for autocompletion.
    pub fn show(&mut self, ui: &mut Ui, root: &serde_json::Value) -> JqBarResponse {
        let mut response = JqBarResponse {
            run: false,
            escaped: false,
            changed: false,
            completion_applied: false,
        };

        let mut accepted_completion: Option<String> = None;

        ui.horizontal(|ui| {
            ui.label(
                RichText::new(egui_phosphor::regular::FUNNEL)
                    .color(CatppuccinMocha::MAUVE)
                    .size(14.0),
            );

            let input_id = Self::input_id(ui);
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

            let has_focus = text_response.has_focus();

            if has_focus {
                if text_response.changed() {
                    response.changed = true;
                    self.rebuild_completions(root);
                }

                let tab = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
                let ctrl_space =
                    ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space));

                if tab && self.show_completions && !self.completions.is_empty() {
                    accepted_completion = Some(self.completions[self.completion_index].clone());
                }
                if tab {
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

                // Enter: signal caller to execute/navigate
                if text_response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                {
                    response.run = true;
                }

                // Navigate completions
                if self.show_completions && !self.completions.is_empty() {
                    let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                    let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                    if down {
                        self.completion_index =
                            (self.completion_index + 1).min(self.completions.len() - 1);
                    }
                    if up {
                        self.completion_index = self.completion_index.saturating_sub(1);
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        accepted_completion = Some(self.completions[self.completion_index].clone());
                    }
                }
            }
        });

        // Apply completion
        if let Some(comp) = accepted_completion {
            self.apply_completion(&comp);
            self.show_completions = false;
            self.refocus = true;
            response.completion_applied = true;
        }

        // Show completion popup
        if self.show_completions && !self.completions.is_empty() {
            let mut clicked: Option<String> = None;
            egui::Frame::new()
                .fill(CatppuccinMocha::SURFACE0)
                .inner_margin(6.0)
                .corner_radius(4.0)
                .stroke(egui::Stroke::new(1.0, CatppuccinMocha::SURFACE1))
                .show(ui, |ui| {
                    ui.set_max_height(200.0);
                    egui::ScrollArea::vertical()
                        .id_salt("jq_bar_completions")
                        .show(ui, |ui| {
                            for (i, comp) in self.completions.iter().enumerate() {
                                let selected = i == self.completion_index;
                                let text_color = if selected {
                                    CatppuccinMocha::BLUE
                                } else {
                                    CatppuccinMocha::TEXT
                                };
                                let bg = if selected {
                                    CatppuccinMocha::SURFACE1
                                } else {
                                    CatppuccinMocha::SURFACE0
                                };
                                let r = ui.add(
                                    egui::Label::new(
                                        RichText::new(comp)
                                            .color(text_color)
                                            .family(egui::FontFamily::Monospace)
                                            .background_color(bg),
                                    )
                                    .sense(egui::Sense::click()),
                                );
                                if selected {
                                    r.scroll_to_me(Some(egui::Align::Center));
                                }
                                if r.clicked() {
                                    clicked = Some(comp.clone());
                                }
                            }
                        });
                });
            if let Some(c) = clicked {
                self.apply_completion(&c);
                self.show_completions = false;
                self.refocus = true;
                response.completion_applied = true;
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
        self.completion_index = 0;
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
