use egui::Ui;

/// Actions produced by miller column keyboard navigation.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MillerAction {
    Down,
    Up,
    Enter,
    Back,
    GoTop,
    GoBottom,
    None,
}

const KEYBOARD_ACTIVE_ID: &str = "miller_keyboard_active";

/// Returns true if keyboard navigation happened this frame (suppresses hover highlights).
pub fn keyboard_active(ctx: &egui::Context) -> bool {
    ctx.data(|d| d.get_temp::<bool>(egui::Id::new(KEYBOARD_ACTIVE_ID))).unwrap_or(false)
}

/// Read J/K/H/L/Arrow/Home/End/G keys and return the corresponding action.
/// Pass `skip: true` to suppress (e.g., when a text input has focus).
/// When a key is pressed, hover highlights are suppressed for this frame.
pub fn read_miller_keys(ui: &Ui, skip: bool) -> MillerAction {
    if skip {
        return MillerAction::None;
    }

    let down = ui.input(|i| i.key_pressed(egui::Key::J) || i.key_pressed(egui::Key::ArrowDown));
    let up = ui.input(|i| i.key_pressed(egui::Key::K) || i.key_pressed(egui::Key::ArrowUp));
    let right = ui.input(|i| {
        i.key_pressed(egui::Key::L)
            || i.key_pressed(egui::Key::ArrowRight)
            || i.key_pressed(egui::Key::Enter)
    });
    let left = ui.input(|i| {
        i.key_pressed(egui::Key::H)
            || i.key_pressed(egui::Key::ArrowLeft)
            || i.key_pressed(egui::Key::Backspace)
    });
    let go_top = ui.input(|i| i.key_pressed(egui::Key::G) || i.key_pressed(egui::Key::Home));
    let go_bottom = ui.input(|i| i.key_pressed(egui::Key::End));

    let action = if down {
        MillerAction::Down
    } else if up {
        MillerAction::Up
    } else if right {
        MillerAction::Enter
    } else if left {
        MillerAction::Back
    } else if go_top {
        MillerAction::GoTop
    } else if go_bottom {
        MillerAction::GoBottom
    } else {
        MillerAction::None
    };

    if action != MillerAction::None {
        ui.ctx().data_mut(|d| d.insert_temp(egui::Id::new(KEYBOARD_ACTIVE_ID), true));
    }

    action
}

/// Apply a navigation action to a selection index.
/// Returns true if selection changed (caller should set scroll_to_selection).
pub fn apply_selection(selection: &mut usize, action: MillerAction, count: usize) -> bool {
    match action {
        MillerAction::Down => {
            if count > 0 && *selection + 1 < count {
                *selection += 1;
                return true;
            }
        }
        MillerAction::Up => {
            if *selection > 0 {
                *selection -= 1;
                return true;
            }
        }
        MillerAction::GoTop => {
            if *selection != 0 {
                *selection = 0;
                return true;
            }
        }
        MillerAction::GoBottom => {
            if count > 0 && *selection != count - 1 {
                *selection = count - 1;
                return true;
            }
        }
        _ => {}
    }
    false
}

/// Response from the filter bar.
pub struct MillerFilterResponse {
    /// Ctrl-N or ArrowDown pressed — move selection down
    pub next: bool,
    /// Ctrl-P or ArrowUp pressed — move selection up
    pub prev: bool,
    /// Enter pressed — navigate into selected entry
    pub accept: bool,
}

/// Always-visible filter bar for a miller column.
/// Shows a text input with placeholder. Focus with shortcut key, Escape unfocuses.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FilterMode {
    Off,      // no filtering
    Fuzzy,    // chars in order anywhere
    Contains, // case-insensitive substring
    Exact,    // case-insensitive full match
}

impl FilterMode {
    fn next(self) -> Self {
        match self {
            FilterMode::Off => FilterMode::Fuzzy,
            FilterMode::Fuzzy => FilterMode::Contains,
            FilterMode::Contains => FilterMode::Exact,
            FilterMode::Exact => FilterMode::Off,
        }
    }

    fn icon(self) -> &'static str {
        match self {
            FilterMode::Off => egui_phosphor::regular::PROHIBIT,
            FilterMode::Fuzzy => egui_phosphor::regular::WAVES,
            FilterMode::Contains => egui_phosphor::regular::TEXT_T,
            FilterMode::Exact => egui_phosphor::regular::EQUALS,
        }
    }

    fn color(self) -> egui::Color32 {
        match self {
            FilterMode::Off => crate::theme::CatppuccinMocha::OVERLAY0,
            FilterMode::Fuzzy => crate::theme::CatppuccinMocha::PEACH,
            FilterMode::Contains => crate::theme::CatppuccinMocha::BLUE,
            FilterMode::Exact => crate::theme::CatppuccinMocha::GREEN,
        }
    }

    fn tooltip(self) -> &'static str {
        match self {
            FilterMode::Off => "Off (ctrl-f to cycle)",
            FilterMode::Fuzzy => "Fuzzy (ctrl-f to cycle)",
            FilterMode::Contains => "Contains (ctrl-f to cycle)",
            FilterMode::Exact => "Exact (ctrl-f to cycle)",
        }
    }
}

pub struct MillerFilter {
    pub query: String,
    id: &'static str,
    focus_next: bool,
    pub mode: FilterMode,
    // Saved query state
    pub current_name: Option<String>,
    pub saved_queries: Vec<(String, String, FilterMode)>, // (name, query, mode)
    save_popup: SavePopupState,
    load_index: Option<usize>, // current position when cycling saved queries
    pub queries_dirty: bool,
}

#[derive(Default)]
struct SavePopupState {
    open: bool,
    name_input: String,
    overwrite_idx: Option<usize>,
}

impl MillerFilter {
    pub fn new(id: &'static str) -> Self {
        Self {
            query: String::new(),
            id,
            focus_next: false,
            mode: FilterMode::Fuzzy,
            current_name: None,
            saved_queries: Vec::new(),
            save_popup: SavePopupState::default(),
            load_index: None,
            queries_dirty: false,
        }
    }

    /// Load saved queries from session data.
    pub fn load_queries(&mut self, queries: &[crate::session::SavedQuery]) {
        self.saved_queries = queries.iter().map(|sq| {
            let mode = match sq.mode.as_str() {
                "fuzzy" => FilterMode::Fuzzy,
                "contains" => FilterMode::Contains,
                "exact" => FilterMode::Exact,
                _ => FilterMode::Fuzzy,
            };
            (sq.name.clone(), sq.query.clone(), mode)
        }).collect();
    }

    /// Export saved queries for session persistence.
    pub fn export_queries(&self) -> Vec<crate::session::SavedQuery> {
        self.saved_queries.iter().map(|(name, query, mode)| {
            crate::session::SavedQuery {
                name: name.clone(),
                query: query.clone(),
                mode: match mode {
                    FilterMode::Off => "off".to_string(),
                    FilterMode::Fuzzy => "fuzzy".to_string(),
                    FilterMode::Contains => "contains".to_string(),
                    FilterMode::Exact => "exact".to_string(),
                },
            }
        }).collect()
    }

    /// Request focus on this filter's input next frame.
    pub fn focus(&mut self) {
        self.focus_next = true;
    }

    /// Returns true if this filter's text input or any popup has focus.
    pub fn has_focus(&self, ui: &Ui) -> bool {
        if self.save_popup.open {
            return true;
        }
        let id = egui::Id::new(self.id);
        ui.ctx().memory(|m| m.focused().map_or(false, |f| f == id))
    }

    /// Render the filter bar. Always visible.
    /// `shortcut` is shown in the placeholder (e.g. "?" or "ctrl-/").
    pub fn show(&mut self, ui: &mut Ui, shortcut: &str) -> MillerFilterResponse {
        let mut resp = MillerFilterResponse {
            next: false,
            prev: false,
            accept: false,
        };

        // Query name label — always visible, shortcuts only when focused
        let filter_focused = {
            let id = egui::Id::new(self.id);
            ui.ctx().memory(|m| m.focused().map_or(false, |f| f == id))
        } || self.save_popup.open || false;

        let name_color = if self.current_name.is_some() {
            crate::theme::CatppuccinMocha::MAUVE
        } else {
            crate::theme::CatppuccinMocha::SURFACE2
        };
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(egui_phosphor::regular::BOOKMARK_SIMPLE)
                    .color(name_color)
                    .size(10.0),
            );
            let name_text = if let Some(name) = &self.current_name {
                if filter_focused {
                    format!("{}  (ctrl-s save, ctrl-S save as{})",
                        name,
                        if self.saved_queries.is_empty() { "" } else { ", ctrl-p/n cycle" })
                } else {
                    name.clone()
                }
            } else if filter_focused {
                format!("unnamed query  (ctrl-s save{})",
                    if self.saved_queries.is_empty() { "" } else { ", ctrl-p/n cycle" })
            } else {
                "unnamed query".to_string()
            };
            ui.label(
                egui::RichText::new(&name_text)
                    .color(name_color)
                    .family(egui::FontFamily::Monospace)
                    .size(10.0),
            );
        });

        // Build dynamic hint
        let hint = format!("{} to focus | ctrl-space mode", shortcut);

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                    .color(if self.query.is_empty() {
                        crate::theme::CatppuccinMocha::SURFACE2
                    } else {
                        crate::theme::CatppuccinMocha::MAUVE
                    })
                    .size(12.0),
            );

            // Filter mode toggle
            if ui.add(
                egui::Button::new(
                    egui::RichText::new(self.mode.icon())
                        .color(self.mode.color())
                        .size(11.0)
                ).frame(false)
            ).on_hover_text(self.mode.tooltip()).clicked() {
                self.mode = self.mode.next();
            }

            let id = egui::Id::new(self.id);
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id(id)
                    .font(egui::FontId::monospace(12.0))
                    .desired_width(ui.available_width() - 10.0)
                    .text_color(crate::theme::CatppuccinMocha::GREEN)
                    .hint_text(
                        egui::RichText::new(&hint)
                            .color(crate::theme::CatppuccinMocha::SURFACE2)
                            .family(egui::FontFamily::Monospace)
                    ),
            );

            if self.focus_next {
                r.request_focus();
                self.focus_next = false;
            }

            let has_focus = r.has_focus();
            let lost_focus = r.lost_focus();

            if has_focus || lost_focus {
                if has_focus {
                    // Ctrl-Space: cycle filter mode
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::Space)) {
                        self.mode = self.mode.next();
                    }
                    // Ctrl-S / Ctrl-Shift-S: save
                    let shift_held = ui.input(|i| i.modifiers.shift);
                    let ctrl_s = ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::S));
                    if ctrl_s && shift_held {
                        // Ctrl-Shift-S: save as (always opens popup)
                        self.save_popup.open = true;
                        self.save_popup.name_input.clear();
                    } else if ctrl_s {
                        if self.current_name.is_some() {
                            // Overwrite existing
                            let name = self.current_name.clone().unwrap();
                            if let Some(idx) = self.saved_queries.iter().position(|(n, _, _)| *n == name) {
                                self.saved_queries[idx].1 = self.query.clone();
                                self.saved_queries[idx].2 = self.mode;
                            }
                            self.queries_dirty = true;
                        } else {
                            // No name yet — open save popup
                            self.save_popup.open = true;
                            self.save_popup.name_input.clear();
                        }
                    }
                    // Ctrl-P: cycle to next saved query
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::P)) {
                        if !self.saved_queries.is_empty() {
                            let next = match self.load_index {
                                Some(idx) => (idx + 1) % self.saved_queries.len(),
                                None => 0,
                            };
                            let (name, query, mode) = &self.saved_queries[next];
                            self.query = query.clone();
                            self.mode = *mode;
                            self.current_name = Some(name.clone());
                            self.load_index = Some(next);
                        }
                    }
                    // Ctrl-N: cycle to previous saved query (reverse)
                    if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N)) {
                        if !self.saved_queries.is_empty() {
                            let prev = match self.load_index {
                                Some(0) | None => self.saved_queries.len() - 1,
                                Some(idx) => idx - 1,
                            };
                            let (name, query, mode) = &self.saved_queries[prev];
                            self.query = query.clone();
                            self.mode = *mode;
                            self.current_name = Some(name.clone());
                            self.load_index = Some(prev);
                        }
                    }
                    // Escape: close popups or unfocus
                    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                        if self.save_popup.open {
                            self.save_popup.open = false;
                        } else {
                            r.surrender_focus();
                        }
                    }
                    // ArrowDown: next match
                    resp.next = ui.input(|i| i.key_pressed(egui::Key::ArrowDown));
                    // ArrowUp: prev match
                    resp.prev = ui.input(|i| i.key_pressed(egui::Key::ArrowUp));
                }
                // Enter
                if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                    if !self.save_popup.open && !false {
                        resp.accept = true;
                    }
                }
            }
        });

        // Save popup
        if self.save_popup.open {
            egui::Frame::new()
                .fill(crate::theme::CatppuccinMocha::SURFACE0)
                .inner_margin(8.0)
                .corner_radius(4.0)
                .stroke(egui::Stroke::new(1.0, crate::theme::CatppuccinMocha::SURFACE1))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new("Save query as:")
                            .color(crate::theme::CatppuccinMocha::TEXT)
                            .size(12.0),
                    );
                    let enter = ui.add(
                        egui::TextEdit::singleline(&mut self.save_popup.name_input)
                            .font(egui::FontId::monospace(12.0))
                            .desired_width(ui.available_width() - 10.0)
                            .text_color(crate::theme::CatppuccinMocha::GREEN),
                    ).lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));

                    if !self.saved_queries.is_empty() {
                        ui.add_space(4.0);
                        ui.label(
                            egui::RichText::new("Or overwrite:")
                                .color(crate::theme::CatppuccinMocha::OVERLAY0)
                                .size(11.0),
                        );
                        let mut overwrite = None;
                        for (i, (name, _, _)) in self.saved_queries.iter().enumerate() {
                            if ui.add(
                                egui::Button::new(
                                    egui::RichText::new(name)
                                        .color(crate::theme::CatppuccinMocha::BLUE)
                                        .family(egui::FontFamily::Monospace)
                                        .size(11.0)
                                ).frame(false)
                            ).clicked() {
                                overwrite = Some(i);
                            }
                        }
                        if let Some(idx) = overwrite {
                            self.saved_queries[idx].1 = self.query.clone();
                            self.saved_queries[idx].2 = self.mode;
                            self.current_name = Some(self.saved_queries[idx].0.clone());
                            self.save_popup.open = false;
                            self.queries_dirty = true;
                        }
                    }

                    if enter && !self.save_popup.name_input.trim().is_empty() {
                        let name = self.save_popup.name_input.trim().to_string();
                        if let Some(idx) = self.saved_queries.iter().position(|(n, _, _)| *n == name) {
                            self.saved_queries[idx].1 = self.query.clone();
                            self.saved_queries[idx].2 = self.mode;
                        } else {
                            self.saved_queries.push((name.clone(), self.query.clone(), self.mode));
                        }
                        self.current_name = Some(name);
                        self.save_popup.open = false;
                        self.queries_dirty = true;
                    }
                });
        }

        ui.add_space(2.0);

        resp
    }

    /// Match a label against the query using the current filter mode.
    /// Supports `|` to OR multiple terms (e.g. "start|report" matches either).
    pub fn matches(&self, label: &str) -> bool {
        if self.mode == FilterMode::Off || self.query.is_empty() {
            return true;
        }
        self.query.split('|')
            .any(|term| {
                let term = term.trim();
                if term.is_empty() { return false; }
                match self.mode {
                    FilterMode::Off => true,
                    FilterMode::Fuzzy => fuzzy_matches(term, label),
                    FilterMode::Contains => label.to_lowercase().contains(&term.to_lowercase()),
                    FilterMode::Exact => label.to_lowercase() == term.to_lowercase(),
                }
            })
    }

    /// Filter a list of labels and manage selection mapping.
    ///
    /// Takes the original entries' labels and the current selection (original index).
    /// Returns `FilteredResult` with filtered indices, snapped selection, and the
    /// position within the filtered list for rendering.
    pub fn apply(
        &self,
        labels: impl Iterator<Item = impl AsRef<str>>,
        selection: usize,
    ) -> FilteredResult {
        let filtered_indices: Vec<usize> = labels
            .enumerate()
            .filter(|(_, label)| self.matches(label.as_ref()))
            .map(|(i, _)| i)
            .collect();

        let selection = self.snap_selection(selection, &filtered_indices);
        let filtered_pos = filtered_indices.iter()
            .position(|&orig| orig == selection)
            .unwrap_or(0);

        FilteredResult {
            indices: filtered_indices,
            selection,
            filtered_pos,
        }
    }

    fn snap_selection(&self, selection: usize, filtered_indices: &[usize]) -> usize {
        if filtered_indices.is_empty() {
            return selection;
        }
        if filtered_indices.contains(&selection) {
            return selection;
        }
        if let Some(&idx) = filtered_indices.iter().find(|&&orig| orig >= selection) {
            return idx;
        }
        filtered_indices[0]
    }
}

/// Result of applying a filter to entries.
pub struct FilteredResult {
    /// Original indices that passed the filter.
    pub indices: Vec<usize>,
    /// The snapped selection (original index).
    pub selection: usize,
    /// Position of the selection within the filtered list (for rendering).
    pub filtered_pos: usize,
}

/// Simple fuzzy match: all chars in needle appear in order in haystack (case-insensitive).
fn fuzzy_matches(needle: &str, haystack: &str) -> bool {
    let mut needle_chars = needle.chars().flat_map(|c| c.to_lowercase());
    let mut current = match needle_chars.next() {
        Some(c) => c,
        None => return true,
    };
    for h in haystack.chars().flat_map(|c| c.to_lowercase()) {
        if h == current {
            current = match needle_chars.next() {
                Some(c) => c,
                None => return true,
            };
        }
    }
    false
}

/// Render a pane title above a miller column.
pub fn pane_title(ui: &mut Ui, title: &str) {
    if title.is_empty() {
        return;
    }
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(title)
                .color(crate::theme::CatppuccinMocha::OVERLAY1)
                .family(egui::FontFamily::Monospace)
                .size(10.0),
        );
    });
    ui.add_space(2.0);
}

/// Draw a vertical separator line (shared by both browser views).
pub fn draw_separator(ui: &mut Ui, height: f32) {
    let sep_rect = ui.available_rect_before_wrap();
    ui.painter().line_segment(
        [
            egui::pos2(sep_rect.left(), sep_rect.top()),
            egui::pos2(sep_rect.left(), sep_rect.top() + height),
        ],
        egui::Stroke::new(1.0, crate::theme::CatppuccinMocha::SURFACE0),
    );
    ui.add_space(4.0);
}
