use egui::{self, RichText, Ui};

use crate::jq_engine::JqEngine;
use crate::theme::CatppuccinMocha;

struct ResultLine {
    text: String,
    color: egui::Color32,
}

pub struct JqView {
    pub query: String,
    pub history: Vec<String>,
    previous_query: Option<String>,
    last_executed_query: String,
    cached_result: Option<crate::jq_engine::JqResult>,
    cached_lines: Vec<ResultLine>,
    completions: Vec<String>,
    completion_index: usize,
    show_completions: bool,
    refocus: bool,
}

impl JqView {
    pub fn new() -> Self {
        Self {
            query: ".".to_string(),
            history: Vec::new(),
            previous_query: None,
            last_executed_query: String::new(),
            cached_result: None,
            cached_lines: Vec::new(),
            completions: Vec::new(),
            completion_index: 0,
            show_completions: false,
            refocus: false,
        }
    }

    pub fn invalidate(&mut self) {
        self.cached_result = None;
        self.cached_lines.clear();
    }

    pub fn show(&mut self, ui: &mut Ui, value: &serde_json::Value) {
        // Auto-execute on first show or when query changes
        if self.cached_result.is_none() || self.query != self.last_executed_query {
            // Save previous query for Ctrl-L swap (skip initial load)
            if !self.last_executed_query.is_empty() && self.last_executed_query != self.query {
                self.previous_query = Some(self.last_executed_query.clone());
            }
            let result = JqEngine::execute(&self.query, value);
            self.rebuild_result_lines(&result);
            self.cached_result = Some(result);
            self.last_executed_query = self.query.clone();
        }

        let mut accepted_completion: Option<String> = None;

        ui.horizontal(|ui| {
            ui.label(RichText::new("jq").color(CatppuccinMocha::MAUVE).strong());

            let response = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .font(egui::FontId::monospace(14.0))
                    .desired_width(ui.available_width() - 80.0)
                    .text_color(CatppuccinMocha::GREEN),
            );

            // Re-focus and move cursor to end after completion was applied
            if self.refocus {
                response.request_focus();
                if let Some(mut state) = egui::TextEdit::load_state(ui.ctx(), response.id) {
                    let ccursor = egui::text::CCursor::new(self.query.len());
                    state.cursor.set_char_range(Some(egui::text::CCursorRange::one(ccursor)));
                    state.store(ui.ctx(), response.id);
                }
                self.refocus = false;
            }

            let has_focus = response.has_focus() || response.lost_focus();

            if has_focus {
                // Consume keybindings only when jq input has focus
                let tab = ui.input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Tab));
                let ctrl_space = ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space));
                let ctrl_period = ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Period));
                let ctrl_h = ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::H));
                let ctrl_l = ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::L));

                if tab && self.show_completions && !self.completions.is_empty() {
                    accepted_completion =
                        Some(self.completions[self.completion_index].clone());
                }
                if tab {
                    response.request_focus();
                }

                if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                    self.show_completions = false;
                }

                // Ctrl-Space / Ctrl-Period: toggle autocompletion
                if ctrl_space || ctrl_period {
                    if self.show_completions {
                        self.show_completions = false;
                    } else {
                        self.rebuild_completions(value);
                    }
                }

                // Ctrl-H: delete rightmost path segment
                if ctrl_h {
                    if let Some(pos) = self.query.rfind(|c: char| c == '.' || c == '|') {
                        self.query.truncate(pos);
                    } else {
                        self.query.clear();
                    }
                    self.show_completions = false;
                    self.refocus = true;
                }

                // Ctrl-L: swap with previous query
                if ctrl_l {
                    if let Some(prev) = self.previous_query.take() {
                        let current = self.query.clone();
                        self.query = prev;
                        self.previous_query = Some(current);
                    }
                    self.refocus = true;
                }

                if self.show_completions && !self.completions.is_empty() {
                    let down = ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                        || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N));
                    let up = ui.input(|i| i.key_pressed(egui::Key::ArrowUp))
                        || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::P));
                    if down {
                        self.completion_index = (self.completion_index + 1)
                            .min(self.completions.len() - 1);
                    }
                    if up {
                        self.completion_index = self.completion_index.saturating_sub(1);
                    }
                    if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                        accepted_completion =
                            Some(self.completions[self.completion_index].clone());
                    }
                }
            }

            // Auto-show completions as user types
            if response.changed() {
                self.rebuild_completions(value);
            }

            if ui.button("Run").clicked() {
                self.show_completions = false;
                let result = JqEngine::execute(&self.query, value);
                self.rebuild_result_lines(&result);
                self.cached_result = Some(result);
                self.last_executed_query = self.query.clone();
                if !self.query.is_empty() && !self.history.contains(&self.query) {
                    self.history.push(self.query.clone());
                }
            }
        });

        // Apply accepted completion
        if let Some(comp) = accepted_completion {
            self.apply_completion(&comp);
            self.show_completions = false;
            self.refocus = true;
            let result = JqEngine::execute(&self.query, value);
            self.rebuild_result_lines(&result);
            self.cached_result = Some(result);
            self.last_executed_query = self.query.clone();
        }

        // --- Completion popup ---
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
                        .id_salt("jq_completions")
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
                let result = JqEngine::execute(&self.query, value);
                self.rebuild_result_lines(&result);
                self.cached_result = Some(result);
                self.last_executed_query = self.query.clone();
            }
        }

        // --- History ---
        if !self.history.is_empty() && !self.show_completions {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("History:")
                        .color(CatppuccinMocha::SUBTEXT0)
                        .small(),
                );
                let mut clicked_history: Option<String> = None;
                for q in self.history.iter().rev().take(10) {
                    if ui
                        .add(
                            egui::Label::new(
                                RichText::new(q)
                                    .color(CatppuccinMocha::BLUE)
                                    .small()
                                    .family(egui::FontFamily::Monospace),
                            )
                            .sense(egui::Sense::click()),
                        )
                        .clicked()
                    {
                        clicked_history = Some(q.clone());
                    }
                    ui.label(
                        RichText::new("·").color(CatppuccinMocha::SURFACE2).small(),
                    );
                }
                if let Some(q) = clicked_history {
                    self.query = q;
                    let result = JqEngine::execute(&self.query, value);
                    self.rebuild_result_lines(&result);
                    self.cached_result = Some(result);
                    self.last_executed_query = self.query.clone();
                }
            });
        }

        ui.add_space(8.0);

        // --- Results ---
        if let Some(result) = &self.cached_result {
            if let Some(err) = &result.error {
                ui.label(
                    RichText::new(format!("Error: {}", err))
                        .color(CatppuccinMocha::RED)
                        .family(egui::FontFamily::Monospace),
                );
            }

            if !self.cached_lines.is_empty() {
                let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
                let num_rows = self.cached_lines.len();

                ui.label(
                    RichText::new(format!("{} lines", num_rows))
                        .color(CatppuccinMocha::OVERLAY0)
                        .small(),
                );

                egui::ScrollArea::vertical()
                    .id_salt("jq_results")
                    .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                    .auto_shrink(false)
                    .show_rows(ui, row_height, num_rows, |ui, row_range| {
                        for row in row_range {
                            let line = &self.cached_lines[row];
                            ui.label(
                                RichText::new(&line.text)
                                    .color(line.color)
                                    .family(egui::FontFamily::Monospace),
                            );
                        }
                    });
            }
        }
    }

    /// Pre-flatten jq result output into colored lines for virtual scrolling.
    fn rebuild_result_lines(&mut self, result: &crate::jq_engine::JqResult) {
        self.cached_lines.clear();
        for (i, output) in result.output.iter().enumerate() {
            if i > 0 {
                self.cached_lines.push(ResultLine {
                    text: "───".to_string(),
                    color: CatppuccinMocha::SURFACE2,
                });
            }
            if let Ok(parsed) = serde_json::from_str::<serde_json::Value>(output) {
                let pretty = serde_json::to_string_pretty(&parsed).unwrap_or_default();
                for line in pretty.lines() {
                    let (color, text) = colorize_json_line(line);
                    self.cached_lines.push(ResultLine { text, color });
                }
            } else {
                self.cached_lines.push(ResultLine {
                    text: output.clone(),
                    color: CatppuccinMocha::TEXT,
                });
            }
        }
    }

    /// Rebuild completions based on current query.
    /// Collect all valid jq paths from root value.
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

    /// Rebuild completions using fuzzy matching against all possible paths.
    fn rebuild_completions(&mut self, root: &serde_json::Value) {
        let segment = extract_current_segment(&self.query);
        let all_paths = Self::collect_all_paths(root);

        let needle = segment.trim();
        if needle.is_empty() || needle == "." {
            // Show all paths
            self.completions = all_paths;
        } else {
            // Fuzzy filter: every char in needle must appear in order in the path
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

    /// Replace the current segment of the query with the completion.
    fn apply_completion(&mut self, completion: &str) {
        let (before, _) = split_at_current_segment(&self.query);
        self.query = format!("{}{}", before, completion);
    }
}

/// Recursively collect all dot-paths from a JSON value.
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

/// Fuzzy match: returns a score > 0 if all chars of needle appear in haystack in order.
/// Higher score = better match (consecutive chars, earlier positions).
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
            // Bonus for consecutive matches
            if let Some(prev) = prev_match {
                if hi == prev + 1 {
                    score += 2;
                }
            }
            // Bonus for matching after separator (., [, ])
            if hi > 0 && matches!(haystack_chars[hi - 1], '.' | '[' | ']') {
                score += 1;
            }
            prev_match = Some(hi);
            ni += 1;
        }
    }

    if ni == needle_chars.len() { score } else { 0 }
}

/// Extract the current jq segment being typed (after last `|`, `(`, `;`).
fn extract_current_segment(query: &str) -> &str {
    let start = query
        .rfind(|c: char| c == '|' || c == '(' || c == ';')
        .map(|i| i + 1)
        .unwrap_or(0);
    query[start..].trim_start()
}

/// Split query into (everything before current segment, current segment).
fn split_at_current_segment(query: &str) -> (&str, &str) {
    let start = query
        .rfind(|c: char| c == '|' || c == '(' || c == ';')
        .map(|i| i + 1)
        .unwrap_or(0);

    // Include any whitespace after the delimiter in "before"
    let segment_start = query[start..]
        .find(|c: char| !c.is_whitespace())
        .map(|offset| start + offset)
        .unwrap_or(query.len());

    (&query[..segment_start], &query[segment_start..])
}

fn colorize_json_line(line: &str) -> (egui::Color32, String) {
    let trimmed = line.trim();
    let color = if trimmed.starts_with('"') {
        if trimmed.contains(':') {
            CatppuccinMocha::BLUE
        } else {
            CatppuccinMocha::GREEN
        }
    } else if trimmed == "null" || trimmed == "null," {
        CatppuccinMocha::OVERLAY0
    } else if trimmed == "true"
        || trimmed == "true,"
        || trimmed == "false"
        || trimmed == "false,"
    {
        CatppuccinMocha::MAUVE
    } else if trimmed.starts_with(|c: char| c.is_ascii_digit() || c == '-') {
        CatppuccinMocha::PEACH
    } else {
        CatppuccinMocha::TEXT
    };

    (color, line.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_segment_simple() {
        assert_eq!(extract_current_segment(".users"), ".users");
        assert_eq!(extract_current_segment(".users.na"), ".users.na");
    }

    #[test]
    fn test_extract_segment_pipe() {
        assert_eq!(extract_current_segment(".users[] | .na"), ".na");
    }

    #[test]
    fn test_split_at_segment() {
        let (before, seg) = split_at_current_segment(".users[] | .na");
        assert_eq!(before, ".users[] | ");
        assert_eq!(seg, ".na");
    }

    #[test]
    fn test_split_at_segment_simple() {
        let (before, seg) = split_at_current_segment(".users");
        assert_eq!(before, "");
        assert_eq!(seg, ".users");
    }

    #[test]
    fn test_completions_from_root_dot() {
        let root: serde_json::Value = serde_json::json!({"users": [], "metadata": {}});
        let mut view = JqView::new();
        view.query = ".".to_string();
        view.rebuild_completions(&root);
        assert!(!view.completions.is_empty(), "rebuild: completions should not be empty for '.'");
        assert!(view.completions.iter().any(|c| c.contains("users")), "should contain .users");
    }

    #[test]
    fn test_completions_from_empty() {
        let root: serde_json::Value = serde_json::json!({"foo": 1, "bar": 2});
        let mut view = JqView::new();
        view.query = String::new();
        view.rebuild_completions(&root);
        assert!(!view.completions.is_empty(), "rebuild: completions should not be empty for empty query");
    }

    #[test]
    fn test_apply_completion_from_dot() {
        let root: serde_json::Value = serde_json::json!({"users": [{"id": 1}]});
        let mut view = JqView::new();
        view.query = ".".to_string();
        view.rebuild_completions(&root);
        assert!(!view.completions.is_empty());
        let comp = view.completions[0].clone();
        view.apply_completion(&comp);
        assert!(view.query.starts_with('.'), "query should start with dot, got: {:?}", view.query);
    }

    #[test]
    fn test_completions_all_start_with_dot() {
        let root: serde_json::Value = serde_json::json!({"users": [{"id": 1}], "metadata": {"version": "1.0"}});
        let mut view = JqView::new();
        view.query = ".".to_string();
        view.rebuild_completions(&root);
        assert!(!view.completions.is_empty());
        for c in &view.completions {
            assert!(c.starts_with('.'), "completion should start with dot: {:?}", c);
        }
    }

    #[test]
    fn test_completions_array_root() {
        let root: serde_json::Value = serde_json::json!([{"alternate2": "val", "name": "test"}]);
        let mut view = JqView::new();
        view.query = ".".to_string();
        view.rebuild_completions(&root);
        assert!(!view.completions.is_empty());
        assert!(view.completions.iter().any(|c| c == ".[]"), "should contain .[], got: {:?}", view.completions);
        assert!(view.completions.iter().any(|c| c == ".[].alternate2"), "should contain .[].alternate2, got: {:?}", view.completions);
        for c in &view.completions {
            assert!(c.starts_with('.'), "completion should start with dot: {:?}", c);
        }
    }

    #[test]
    fn test_fuzzy_search() {
        let root: serde_json::Value = serde_json::json!({"users": [{"id": 1, "username": "test"}], "metadata": {}});
        let mut view = JqView::new();
        view.query = ".usrn".to_string(); // fuzzy for "username"
        view.rebuild_completions(&root);
        assert!(view.completions.iter().any(|c| c.contains("username")),
            "fuzzy should match username, got: {:?}", view.completions);
    }
}
