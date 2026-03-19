use egui::{self, Ui};

/// A virtually-scrolled list that tracks selection and auto-scrolls.
///
/// The caller owns keyboard logic and sets `selection` directly.
/// The list handles rendering, virtual scrolling, click detection,
/// and scrolling to keep the selection visible.
pub struct ScrollableList {
    pub selection: usize,
    prev_selection: usize,
    force_scroll: bool,
}

impl ScrollableList {
    pub fn new() -> Self {
        Self {
            selection: 0,
            prev_selection: usize::MAX,
            force_scroll: false,
        }
    }

    /// Render the list.
    ///
    /// - `render_row(ui, index, is_selected)` — draw one row, return its Response
    /// - `on_select(index)` — called when selection changes (arrow scroll or click)
    /// - `on_accept(index)` — called when a row is double-clicked
    ///
    /// Returns true if a click changed the selection.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        id_salt: &str,
        count: usize,
        row_height: f32,
        max_height: Option<f32>,
        render_row: &mut dyn FnMut(&mut Ui, usize, bool) -> egui::Response,
        on_click: &mut dyn FnMut(usize),
    ) {
        if count == 0 {
            return;
        }

        if self.selection >= count {
            self.selection = count.saturating_sub(1);
        }

        let needs_scroll = self.selection != self.prev_selection || self.force_scroll;
        self.prev_selection = self.selection;
        self.force_scroll = false;

        let visible_h = max_height.unwrap_or(f32::MAX);

        // When selection changes, set scroll offset directly so show_rows
        // renders the selected row (scroll_to_me only works for visible rows).
        let scroll_offset = if needs_scroll {
            let target_y = self.selection as f32 * row_height;
            let centered = (target_y - visible_h / 2.0 + row_height / 2.0).max(0.0);
            Some(egui::vec2(0.0, centered))
        } else {
            None
        };

        let mut area = egui::ScrollArea::vertical()
            .id_salt(id_salt)
            .auto_shrink(false);
        if let Some(h) = max_height {
            area = area.max_height(h);
        }
        if let Some(offset) = scroll_offset {
            area = area.vertical_scroll_offset(offset.y);
        }

        let mut clicked_idx: Option<usize> = None;

        area.show_rows(ui, row_height, count, |ui, range| {
            for i in range {
                let is_selected = i == self.selection;
                let r = render_row(ui, i, is_selected);

                if r.clicked() {
                    clicked_idx = Some(i);
                }
            }
        });

        if let Some(idx) = clicked_idx {
            self.selection = idx;
            on_click(idx);
        }
    }

    /// Move selection down by 1. Returns true if it moved.
    pub fn down(&mut self, count: usize) -> bool {
        if count > 0 && self.selection + 1 < count {
            self.selection += 1;
            true
        } else {
            false
        }
    }

    /// Move selection up by 1. Returns true if it moved.
    pub fn up(&mut self) -> bool {
        if self.selection > 0 {
            self.selection -= 1;
            true
        } else {
            false
        }
    }

    /// Force scroll to current selection next frame.
    pub fn scroll_to_selection(&mut self) {
        self.force_scroll = true;
    }

    /// Reset selection to 0.
    pub fn reset(&mut self) {
        self.selection = 0;
        self.prev_selection = usize::MAX;
        self.force_scroll = true;
    }
}
