use egui::{self, Ui};

/// A virtually-scrolled list that tracks selection and auto-scrolls.
///
/// Caller provides hooks:
/// - `render_row` — draw each row
/// - `on_select` — called when selection changes (keyboard or click)
pub struct ScrollableList {
    pub selection: usize,
    prev_selection: usize,
    force_scroll: bool,
    /// When set, force this scroll offset on next show() to bring an off-screen selection into view.
    pending_offset: Option<f32>,
}

impl ScrollableList {
    pub fn new() -> Self {
        Self {
            selection: 0,
            prev_selection: usize::MAX,
            force_scroll: false,
            pending_offset: None,
        }
    }

    /// Move selection down. Calls `on_select` if it moved.
    pub fn down(&mut self, count: usize, on_select: &mut dyn FnMut(usize)) {
        if count > 0 && self.selection + 1 < count {
            self.selection += 1;
            on_select(self.selection);
        }
    }

    /// Move selection up. Calls `on_select` if it moved.
    pub fn up(&mut self, on_select: &mut dyn FnMut(usize)) {
        if self.selection > 0 {
            self.selection -= 1;
            on_select(self.selection);
        }
    }

    /// Render the list with virtual scrolling.
    ///
    /// - `render_row(ui, index, is_selected)` — draw one row, return its Response
    /// - `on_select(index)` — called when a row is clicked
    pub fn show(
        &mut self,
        ui: &mut Ui,
        id_salt: &str,
        count: usize,
        row_height: f32,
        max_height: Option<f32>,
        render_row: &mut dyn FnMut(&mut Ui, usize, bool) -> egui::Response,
        on_select: &mut dyn FnMut(usize),
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

        let mut area = egui::ScrollArea::vertical()
            .id_salt(id_salt)
            .auto_shrink(false);
        if let Some(h) = max_height {
            area = area.max_height(h);
        }
        // Apply pending offset from a previous out-of-range jump
        if let Some(offset) = self.pending_offset.take() {
            area = area.vertical_scroll_offset(offset);
        }

        let mut clicked_idx: Option<usize> = None;
        let selection = self.selection;

        area.show_rows(ui, row_height, count, |ui, range| {
            let in_range = range.contains(&selection);

            for i in range {
                let is_selected = i == selection;
                let r = render_row(ui, i, is_selected);

                // scroll_to_me works when the item is in/near the rendered range
                if needs_scroll && is_selected {
                    r.scroll_to_me(Some(egui::Align::Center));
                }
                if r.clicked() {
                    clicked_idx = Some(i);
                }
            }

            // If selection was outside rendered range, queue an offset jump for next frame
            if needs_scroll && !in_range {
                if let Some(h) = max_height {
                    let target_y = selection as f32 * row_height;
                    self.pending_offset = Some((target_y - h / 2.0 + row_height / 2.0).max(0.0));
                }
            }
        });

        if let Some(idx) = clicked_idx {
            self.selection = idx;
            on_select(idx);
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
