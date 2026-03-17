/// Check if a row was hovered in the previous frame (via cached temp storage).
/// Returns (was_hovered, row_id) — use `store_hover` after rendering.
pub fn prev_frame_hover(
    ctx: &egui::Context,
    parent_id: egui::Id,
    index: usize,
) -> (bool, egui::Id) {
    check_hover(ctx, parent_id.with(("row", index)))
}

/// Check hover for a pre-computed Id.
/// Returns false when keyboard navigation is active (see `miller::keyboard_active`).
pub fn check_hover(ctx: &egui::Context, id: egui::Id) -> (bool, egui::Id) {
    if super::miller::keyboard_active(ctx) {
        return (false, id);
    }
    let hovered = ctx.data(|d| d.get_temp::<bool>(id)).unwrap_or(false);
    (hovered, id)
}

/// Store hover state for the next frame.
pub fn store_hover(ctx: &egui::Context, row_id: egui::Id, hovered: bool) {
    ctx.data_mut(|d| d.insert_temp(row_id, hovered));
}
