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
