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
    /// Escape pressed — filter closed
    pub closed: bool,
}

/// Filter state for a miller column. Activated by `?`, fuzzy-filters entries.
pub struct MillerFilter {
    pub active: bool,
    pub query: String,
}

impl MillerFilter {
    pub fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
        }
    }

    /// Check if `?` was pressed (only when no text input has focus).
    /// Returns true if the filter was just activated.
    pub fn check_activate(&mut self, ui: &Ui) -> bool {
        if !self.active && ui.input(|i| i.key_pressed(egui::Key::Questionmark)) {
            self.active = true;
            self.query.clear();
            true
        } else {
            false
        }
    }

    /// Render the filter input bar.
    pub fn show(&mut self, ui: &mut Ui) -> MillerFilterResponse {
        let mut resp = MillerFilterResponse {
            next: false,
            prev: false,
            accept: false,
            closed: false,
        };

        if !self.active {
            return resp;
        }

        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new(egui_phosphor::regular::MAGNIFYING_GLASS)
                    .color(crate::theme::CatppuccinMocha::MAUVE)
                    .size(12.0),
            );
            let r = ui.add(
                egui::TextEdit::singleline(&mut self.query)
                    .id(egui::Id::new("miller_filter_input"))
                    .font(egui::FontId::monospace(12.0))
                    .desired_width(ui.available_width() - 10.0)
                    .text_color(crate::theme::CatppuccinMocha::GREEN),
            );
            r.request_focus();

            // Ctrl-N / ArrowDown: next match
            resp.next = ui.input(|i| i.key_pressed(egui::Key::ArrowDown))
                || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::N));
            // Ctrl-P / ArrowUp: prev match
            resp.prev = ui.input(|i| i.key_pressed(egui::Key::ArrowUp))
                || ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::P));
            // Enter: navigate into selection
            if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                resp.accept = true;
            }
            // Escape: close
            if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                resp.closed = true;
            }
        });
        ui.add_space(2.0);

        if resp.closed {
            self.active = false;
            self.query.clear();
        }

        resp
    }

    /// Returns true if the filter is active (caller should skip miller keys).
    pub fn has_focus(&self) -> bool {
        self.active
    }

    /// Fuzzy match a label against the query. Returns true if it matches.
    pub fn matches(&self, label: &str) -> bool {
        if !self.active || self.query.is_empty() {
            return true;
        }
        fuzzy_matches(&self.query, label)
    }
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
