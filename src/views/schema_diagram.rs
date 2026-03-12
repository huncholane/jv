use std::collections::{BTreeMap, BTreeSet};

use egui::{self, pos2, vec2, Color32, Pos2, Rect, RichText, Ui, Vec2};

use crate::schema::SharedStruct;
use crate::session::EnumConversion;
use crate::theme::CatppuccinMocha;

/// A node in the diagram representing a struct or enum
#[derive(Clone)]
struct DiagramNode {
    name: String,
    fields: Vec<(String, String, Color32)>, // (field_name, type_display, color)
    kind: NodeKind,
    pos: Pos2,
    size: Vec2,
    is_shared: bool,
}

#[derive(Clone, PartialEq)]
enum NodeKind {
    Struct,
    Enum,
}

/// An edge connecting two nodes
#[derive(Clone)]
struct DiagramEdge {
    from: String,
    to: String,
    label: String,
}

pub struct SchemaDiagramView {
    // Diagram state
    nodes: Vec<DiagramNode>,
    edges: Vec<DiagramEdge>,
    mermaid_code: String,

    // Interaction state
    pan_offset: Vec2,
    zoom: f32,
    dragging_node: Option<usize>,
    drag_start: Option<Pos2>,
    node_drag_origin: Option<Pos2>,
    is_panning: bool,
    pan_start: Option<Pos2>,
    pan_origin: Option<Vec2>,

    // Cache
    cache_key: u64,

    // Initial layout stored for reset
    initial_positions: Vec<Pos2>,
    initial_pan: Vec2,
    initial_zoom: f32,
}

impl SchemaDiagramView {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            edges: Vec::new(),
            mermaid_code: String::new(),
            pan_offset: Vec2::ZERO,
            zoom: 1.0,
            dragging_node: None,
            drag_start: None,
            node_drag_origin: None,
            is_panning: false,
            pan_start: None,
            pan_origin: None,
            cache_key: 0,
            initial_positions: Vec::new(),
            initial_pan: Vec2::ZERO,
            initial_zoom: 1.0,
        }
    }

    pub fn invalidate(&mut self) {
        self.cache_key = 0;
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        files: &[(String, serde_json::Value)],
        structs: &[SharedStruct],
        unique_structs: &[SharedStruct],
        enum_conversions: &[EnumConversion],
        hidden_fields: &[String],
    ) {
        let key = Self::compute_cache_key(files, structs, enum_conversions, hidden_fields);
        if key != self.cache_key {
            self.rebuild(files, structs, unique_structs, enum_conversions, hidden_fields);
            self.cache_key = key;
        }

        self.show_toolbar(ui);
        ui.add_space(4.0);
        self.show_diagram(ui);
    }

    fn compute_cache_key(
        files: &[(String, serde_json::Value)],
        structs: &[SharedStruct],
        enum_conversions: &[EnumConversion],
        hidden_fields: &[String],
    ) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        files.len().hash(&mut hasher);
        for (name, _) in files {
            name.hash(&mut hasher);
        }
        structs.len().hash(&mut hasher);
        for s in structs {
            s.name.hash(&mut hasher);
            s.fields.len().hash(&mut hasher);
            s.occurrence_count.hash(&mut hasher);
        }
        enum_conversions.len().hash(&mut hasher);
        for e in enum_conversions {
            e.field_name.hash(&mut hasher);
            e.enum_name.hash(&mut hasher);
        }
        hidden_fields.len().hash(&mut hasher);
        for h in hidden_fields {
            h.hash(&mut hasher);
        }
        hasher.finish()
    }

    fn rebuild(
        &mut self,
        files: &[(String, serde_json::Value)],
        structs: &[SharedStruct],
        unique_structs: &[SharedStruct],
        enum_conversions: &[EnumConversion],
        hidden_fields: &[String],
    ) {
        let hidden_set: BTreeSet<&str> = hidden_fields.iter().map(|s| s.as_str()).collect();
        let enum_map: BTreeMap<&str, &EnumConversion> = enum_conversions
            .iter()
            .map(|e| (e.field_name.as_str(), e))
            .collect();

        self.nodes.clear();
        self.edges.clear();

        let shared_names: BTreeSet<String> = structs.iter().map(|s| s.name.clone()).collect();
        let unique_names: BTreeSet<String> = unique_structs.iter().map(|s| s.name.clone()).collect();

        // Combine all structs for type resolution
        let all_structs: Vec<SharedStruct> = structs.iter().chain(unique_structs).cloned().collect();

        // Build unique struct nodes from schema (using disambiguated names)
        for s in unique_structs {
            let mut fields = Vec::new();
            for (field_name, field_type) in &s.fields {
                let hidden_key = format!("{}.{}", s.name, field_name);
                if hidden_set.contains(hidden_key.as_str()) {
                    continue;
                }
                let (type_display, color) = if let Some(ec) = enum_map.get(field_name.as_str()) {
                    (ec.enum_name.clone(), CatppuccinMocha::MAUVE)
                } else {
                    let display = field_type.short_name(&all_structs);
                    let color = crate::theme::type_color(&display);
                    (display, color)
                };
                fields.push((field_name.clone(), type_display, color));
            }

            self.nodes.push(DiagramNode {
                name: s.name.clone(),
                fields,
                kind: NodeKind::Struct,
                pos: Pos2::ZERO,
                size: Vec2::ZERO,
                is_shared: false,
            });
        }

        // Build per-file struct nodes using CodeGenerator (only for structs not in schema)
        let mut added_names: BTreeSet<String> = unique_names.clone();
        for (filename, value) in files {
            let root_name = crate::codegen::first_normal_word(filename)
                .map(|w| to_pascal_case(&crate::codegen::singularize(&w)))
                .unwrap_or_else(|| "Root".to_string());

            let gen = crate::codegen::CodeGenerator::from_value_named(value, &root_name);

            for gs in &gen.structs {
                // Skip if this struct name matches a shared or unique struct
                if shared_names.contains(&gs.name) || unique_names.contains(&gs.name) {
                    continue;
                }
                // Skip duplicates from other files
                if added_names.contains(&gs.name) {
                    continue;
                }
                added_names.insert(gs.name.clone());

                let fields: Vec<(String, String, Color32)> = gs
                    .fields
                    .iter()
                    .filter(|f| {
                        let hidden_key = format!("{}.{}", gs.name, f.json_name);
                        !hidden_set.contains(hidden_key.as_str())
                    })
                    .map(|f| {
                        let type_display = if let Some(ec) = enum_map.get(f.json_name.as_str()) {
                            ec.enum_name.clone()
                        } else {
                            f.resolved_type.clone().unwrap_or_else(|| f.inferred_type.rust_type())
                        };
                        let color = crate::theme::type_color(&type_display);
                        (f.json_name.clone(), type_display, color)
                    })
                    .collect();

                self.nodes.push(DiagramNode {
                    name: gs.name.clone(),
                    fields,
                    kind: NodeKind::Struct,
                    pos: Pos2::ZERO,
                    size: Vec2::ZERO,
                    is_shared: false,
                });
            }
        }

        // Build shared struct nodes
        for s in structs {
            let mut fields = Vec::new();
            for (field_name, field_type) in &s.fields {
                let hidden_key = format!("{}.{}", s.name, field_name);
                if hidden_set.contains(hidden_key.as_str()) {
                    continue;
                }

                let (type_display, color) = if let Some(ec) = enum_map.get(field_name.as_str()) {
                    (ec.enum_name.clone(), CatppuccinMocha::MAUVE)
                } else {
                    let display = field_type.short_name(&all_structs);
                    let color = crate::theme::type_color(&display);
                    (display, color)
                };

                fields.push((field_name.clone(), type_display, color));
            }

            self.nodes.push(DiagramNode {
                name: s.name.clone(),
                fields,
                kind: NodeKind::Struct,
                pos: Pos2::ZERO,
                size: Vec2::ZERO,
                is_shared: s.source_files.len() > 1,
            });
        }

        // Build enum nodes from conversions
        let mut enum_names_added: BTreeSet<String> = BTreeSet::new();
        for ec in enum_conversions {
            if enum_names_added.contains(&ec.enum_name) {
                continue;
            }
            enum_names_added.insert(ec.enum_name.clone());

            let fields: Vec<(String, String, Color32)> = ec
                .variants
                .iter()
                .map(|v| (v.clone(), String::new(), CatppuccinMocha::GREEN))
                .collect();

            self.nodes.push(DiagramNode {
                name: ec.enum_name.clone(),
                fields,
                kind: NodeKind::Enum,
                pos: Pos2::ZERO,
                size: Vec2::ZERO,
                is_shared: false,
            });
        }

        // Build edges: struct field -> other struct or enum
        let node_names: BTreeSet<String> = self.nodes.iter().map(|n| n.name.clone()).collect();
        for node in &self.nodes {
            if node.kind != NodeKind::Struct {
                continue;
            }
            for (field_name, type_display, _) in &node.fields {
                let target = extract_inner_type(type_display);
                if target != node.name && node_names.contains(&target) {
                    self.edges.push(DiagramEdge {
                        from: node.name.clone(),
                        to: target,
                        label: field_name.clone(),
                    });
                }
            }
        }

        // Layout nodes
        self.layout_nodes();

        // Store initial state for reset
        self.initial_positions = self.nodes.iter().map(|n| n.pos).collect();
        self.initial_pan = Vec2::ZERO;
        self.initial_zoom = 1.0;
        self.pan_offset = Vec2::ZERO;
        self.zoom = 1.0;

        // Generate mermaid
        self.mermaid_code =
            self.generate_mermaid(files, structs, enum_conversions, hidden_fields);
    }

    fn layout_nodes(&mut self) {
        if self.nodes.is_empty() {
            return;
        }

        // Compute node sizes based on content
        for node in &mut self.nodes {
            let title_width = node.name.len() as f32 * 8.0 + 40.0;
            let max_field_width = node
                .fields
                .iter()
                .map(|(name, typ, _)| {
                    if typ.is_empty() {
                        name.len() as f32 * 7.0 + 24.0
                    } else {
                        (name.len() + typ.len() + 2) as f32 * 7.0 + 24.0
                    }
                })
                .fold(0.0f32, f32::max);

            let width = title_width.max(max_field_width).max(140.0);
            let header_height = 32.0;
            let field_height = node.fields.len() as f32 * 22.0;
            let height = header_height + field_height + 12.0;

            node.size = vec2(width, height);
        }

        // Grid layout: structs first, then enums
        let padding = 40.0;
        let cols = ((self.nodes.len() as f32).sqrt().ceil() as usize).max(1);

        let mut x = 0.0f32;
        let mut y = 0.0f32;
        let mut row_height = 0.0f32;
        let mut col = 0;

        for node in &mut self.nodes {
            node.pos = pos2(x, y);
            row_height = row_height.max(node.size.y);
            col += 1;
            if col >= cols {
                col = 0;
                x = 0.0;
                y += row_height + padding;
                row_height = 0.0;
            } else {
                x += node.size.x + padding;
            }
        }
    }

    fn generate_mermaid(
        &self,
        _files: &[(String, serde_json::Value)],
        _structs: &[SharedStruct],
        _enum_conversions: &[EnumConversion],
        _hidden_fields: &[String],
    ) -> String {
        let mut lines = vec!["classDiagram".to_string()];

        // All nodes (per-file + shared + enums) are already built — use them
        for node in &self.nodes {
            lines.push(format!("    class {} {{", node.name));
            if node.kind == NodeKind::Enum {
                lines.push("        <<enumeration>>".to_string());
            }
            for (field_name, type_display, _) in &node.fields {
                if node.kind == NodeKind::Enum {
                    lines.push(format!("        {}", field_name));
                } else {
                    lines.push(format!("        +{} {}", type_display, field_name));
                }
            }
            lines.push("    }".to_string());
        }

        // Relationships from pre-built edges
        for edge in &self.edges {
            // Check if it's an array relationship by looking at the source node's field type
            let is_array = self.nodes.iter()
                .find(|n| n.name == edge.from)
                .and_then(|n| n.fields.iter().find(|(fname, _, _)| fname == &edge.label))
                .map(|(_, typ, _)| typ.starts_with('[') || typ.starts_with("Vec<"))
                .unwrap_or(false);

            let arrow = if is_array {
                format!("{} \"1\" --> \"*\" {} : {}", edge.from, edge.to, edge.label)
            } else {
                format!("{} --> {} : {}", edge.from, edge.to, edge.label)
            };
            lines.push(format!("    {}", arrow));
        }

        lines.join("\n")
    }

    fn show_toolbar(&mut self, ui: &mut Ui) {
        use egui_phosphor::regular;

        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // Copy Mermaid button
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(format!(" {} Copy Mermaid ", regular::CLIPBOARD_TEXT))
                            .size(12.0),
                    )
                    .fill(CatppuccinMocha::SURFACE0)
                    .corner_radius(6.0),
                )
                .clicked()
            {
                ui.ctx().copy_text(self.mermaid_code.clone());
            }

            // Reset view button
            if ui
                .add(
                    egui::Button::new(
                        RichText::new(format!(" {} Reset View ", regular::ARROWS_COUNTER_CLOCKWISE))
                            .size(12.0),
                    )
                    .fill(CatppuccinMocha::SURFACE0)
                    .corner_radius(6.0),
                )
                .clicked()
            {
                self.pan_offset = self.initial_pan;
                self.zoom = self.initial_zoom;
                for (i, node) in self.nodes.iter_mut().enumerate() {
                    if let Some(pos) = self.initial_positions.get(i) {
                        node.pos = *pos;
                    }
                }
            }

            // Zoom display
            ui.label(
                RichText::new(format!("{:.0}%", self.zoom * 100.0))
                    .color(CatppuccinMocha::OVERLAY0)
                    .size(11.0),
            );

            // Node count info
            let struct_count = self.nodes.iter().filter(|n| n.kind == NodeKind::Struct).count();
            let enum_count = self.nodes.iter().filter(|n| n.kind == NodeKind::Enum).count();
            let edge_count = self.edges.len();
            let mut info = format!("{} structs", struct_count);
            if enum_count > 0 {
                info.push_str(&format!(", {} enums", enum_count));
            }
            if edge_count > 0 {
                info.push_str(&format!(", {} relations", edge_count));
            }
            ui.label(
                RichText::new(info)
                    .color(CatppuccinMocha::OVERLAY0)
                    .size(11.0),
            );
        });
    }

    fn show_diagram(&mut self, ui: &mut Ui) {
        let avail = ui.available_size();
        let (response, mut painter) =
            ui.allocate_painter(avail, egui::Sense::click_and_drag());
        let canvas_rect = response.rect;

        // Handle zoom with scroll wheel
        if ui.rect_contains_pointer(canvas_rect) {
            let scroll_delta = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll_delta != 0.0 {
                let factor = 1.0 + scroll_delta * 0.002;
                let new_zoom = (self.zoom * factor).clamp(0.2, 3.0);
                // Zoom toward pointer position
                if let Some(pointer) = ui.input(|i| i.pointer.hover_pos()) {
                    let pointer_in_canvas = pointer - canvas_rect.left_top().to_vec2();
                    let old_world = (pointer_in_canvas - self.pan_offset) / self.zoom;
                    self.zoom = new_zoom;
                    self.pan_offset = pointer_in_canvas - old_world * self.zoom;
                } else {
                    self.zoom = new_zoom;
                }
            }
        }

        // Handle drag interactions
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());

        if response.drag_started() {
            if let Some(pos) = pointer_pos {
                // Check if we're clicking on a node
                let world_pos = self.screen_to_world(pos, canvas_rect);
                let mut found_node = None;
                for (i, node) in self.nodes.iter().enumerate().rev() {
                    let node_rect = Rect::from_min_size(node.pos, node.size);
                    if node_rect.contains(world_pos) {
                        found_node = Some(i);
                        break;
                    }
                }

                if let Some(idx) = found_node {
                    self.dragging_node = Some(idx);
                    self.drag_start = Some(pos);
                    self.node_drag_origin = Some(self.nodes[idx].pos);
                } else {
                    // Panning
                    self.is_panning = true;
                    self.pan_start = Some(pos);
                    self.pan_origin = Some(self.pan_offset);
                }
            }
        }

        if response.dragged() {
            if let Some(pos) = pointer_pos {
                if let Some(node_idx) = self.dragging_node {
                    if let (Some(start), Some(origin)) = (self.drag_start, self.node_drag_origin) {
                        let delta = (pos - start) / self.zoom;
                        self.nodes[node_idx].pos = origin + delta;
                    }
                } else if self.is_panning {
                    if let (Some(start), Some(origin)) = (self.pan_start, self.pan_origin) {
                        let delta = pos - start;
                        self.pan_offset = origin + delta;
                    }
                }
            }
        }

        if response.drag_stopped() {
            self.dragging_node = None;
            self.drag_start = None;
            self.node_drag_origin = None;
            self.is_panning = false;
            self.pan_start = None;
            self.pan_origin = None;
        }

        // Draw background
        painter.rect_filled(canvas_rect, 0.0, CatppuccinMocha::CRUST);

        // Draw grid dots
        self.draw_grid(&painter, canvas_rect);

        // Clip to canvas
        let clip = painter.clip_rect();
        painter.set_clip_rect(canvas_rect);

        // Draw edges first (behind nodes)
        for edge in &self.edges {
            self.draw_edge(&painter, canvas_rect, edge);
        }

        // Draw nodes
        let hover_world = pointer_pos.map(|p| self.screen_to_world(p, canvas_rect));
        for node in &self.nodes {
            self.draw_node(&painter, canvas_rect, node, hover_world);
        }

        // Restore clip
        painter.set_clip_rect(clip);
    }

    fn screen_to_world(&self, screen_pos: Pos2, canvas_rect: Rect) -> Pos2 {
        let local = screen_pos - canvas_rect.left_top().to_vec2();
        pos2(
            (local.x - self.pan_offset.x) / self.zoom,
            (local.y - self.pan_offset.y) / self.zoom,
        )
    }

    fn world_to_screen(&self, world_pos: Pos2, canvas_rect: Rect) -> Pos2 {
        pos2(
            world_pos.x * self.zoom + self.pan_offset.x + canvas_rect.left(),
            world_pos.y * self.zoom + self.pan_offset.y + canvas_rect.top(),
        )
    }

    fn draw_grid(&self, painter: &egui::Painter, canvas_rect: Rect) {
        let grid_spacing = 30.0 * self.zoom;
        if grid_spacing < 8.0 {
            return; // Too zoomed out for grid
        }

        let dot_color = Color32::from_rgba_premultiplied(255, 255, 255, 12);
        let offset_x = self.pan_offset.x % grid_spacing;
        let offset_y = self.pan_offset.y % grid_spacing;

        let mut x = canvas_rect.left() + offset_x;
        while x < canvas_rect.right() {
            let mut y = canvas_rect.top() + offset_y;
            while y < canvas_rect.bottom() {
                painter.circle_filled(pos2(x, y), 1.0, dot_color);
                y += grid_spacing;
            }
            x += grid_spacing;
        }
    }

    fn draw_edge(&self, painter: &egui::Painter, canvas_rect: Rect, edge: &DiagramEdge) {
        let from_node = self.nodes.iter().find(|n| n.name == edge.from);
        let to_node = self.nodes.iter().find(|n| n.name == edge.to);

        let (Some(from), Some(to)) = (from_node, to_node) else {
            return;
        };

        let from_center = from.pos + from.size / 2.0;
        let to_center = to.pos + to.size / 2.0;

        // Find best connection points on node edges
        let from_screen = self.world_to_screen(
            edge_point(from.pos, from.size, to_center),
            canvas_rect,
        );
        let to_screen = self.world_to_screen(
            edge_point(to.pos, to.size, from_center),
            canvas_rect,
        );

        let edge_color = Color32::from_rgba_premultiplied(137, 180, 250, 100);
        painter.line_segment(
            [from_screen, to_screen],
            egui::Stroke::new(1.5 * self.zoom, edge_color),
        );

        // Arrowhead
        let dir = (to_screen - from_screen).normalized();
        let arrow_size = 8.0 * self.zoom;
        let perp = vec2(-dir.y, dir.x);
        let tip = to_screen;
        let left = tip - dir * arrow_size + perp * arrow_size * 0.4;
        let right = tip - dir * arrow_size - perp * arrow_size * 0.4;
        painter.add(egui::Shape::convex_polygon(
            vec![tip, left, right],
            edge_color,
            egui::Stroke::NONE,
        ));

        // Edge label
        let mid = pos2(
            (from_screen.x + to_screen.x) / 2.0,
            (from_screen.y + to_screen.y) / 2.0,
        );
        let font = egui::FontId::proportional(10.0 * self.zoom);
        painter.text(
            mid,
            egui::Align2::CENTER_CENTER,
            &edge.label,
            font,
            CatppuccinMocha::OVERLAY0,
        );
    }

    fn draw_node(
        &self,
        painter: &egui::Painter,
        canvas_rect: Rect,
        node: &DiagramNode,
        hover_world: Option<Pos2>,
    ) {
        let screen_pos = self.world_to_screen(node.pos, canvas_rect);
        let screen_size = node.size * self.zoom;
        let node_rect = Rect::from_min_size(screen_pos, screen_size);

        // Check if visible
        if !canvas_rect.intersects(node_rect) {
            return;
        }

        let world_rect = Rect::from_min_size(node.pos, node.size);
        let is_hovered = hover_world
            .map(|p| world_rect.contains(p))
            .unwrap_or(false);

        // Node background
        let bg = if is_hovered {
            CatppuccinMocha::SURFACE1
        } else {
            CatppuccinMocha::SURFACE0
        };
        let border_color = match node.kind {
            NodeKind::Struct if node.is_shared => CatppuccinMocha::BLUE,
            NodeKind::Struct => CatppuccinMocha::SURFACE2,
            NodeKind::Enum => CatppuccinMocha::MAUVE,
        };

        let corner = 8.0 * self.zoom;
        painter.rect_filled(node_rect, corner, bg);
        painter.rect_stroke(
            node_rect,
            corner,
            egui::Stroke::new(1.5 * self.zoom, border_color),
            egui::StrokeKind::Outside,
        );

        // Header
        let header_height = 32.0 * self.zoom;
        let header_rect = Rect::from_min_size(screen_pos, vec2(screen_size.x, header_height));

        // Header background
        let header_bg = match node.kind {
            NodeKind::Struct if node.is_shared => Color32::from_rgba_premultiplied(137, 180, 250, 25),
            NodeKind::Struct => Color32::from_rgba_premultiplied(69, 71, 90, 180),
            NodeKind::Enum => Color32::from_rgba_premultiplied(203, 166, 247, 25),
        };
        let corner_u8 = corner as u8;
        painter.rect_filled(
            Rect::from_min_size(screen_pos, vec2(screen_size.x, header_height)),
            egui::CornerRadius {
                nw: corner_u8,
                ne: corner_u8,
                sw: 0,
                se: 0,
            },
            header_bg,
        );

        // Kind badge
        let badge_text = match node.kind {
            NodeKind::Struct if node.is_shared => "shared",
            NodeKind::Struct => "struct",
            NodeKind::Enum => "enum",
        };
        let badge_color = match node.kind {
            NodeKind::Struct if node.is_shared => CatppuccinMocha::BLUE,
            NodeKind::Struct => CatppuccinMocha::OVERLAY0,
            NodeKind::Enum => CatppuccinMocha::MAUVE,
        };
        let badge_font = egui::FontId::proportional(9.0 * self.zoom);
        painter.text(
            pos2(
                header_rect.left() + 10.0 * self.zoom,
                header_rect.top() + 8.0 * self.zoom,
            ),
            egui::Align2::LEFT_TOP,
            badge_text,
            badge_font,
            badge_color,
        );

        // Node name
        let name_font = egui::FontId::proportional(13.0 * self.zoom);
        let name_color = match node.kind {
            NodeKind::Struct if node.is_shared => CatppuccinMocha::BLUE,
            NodeKind::Struct => CatppuccinMocha::TEXT,
            NodeKind::Enum => CatppuccinMocha::MAUVE,
        };
        painter.text(
            pos2(
                header_rect.center().x,
                header_rect.top() + 16.0 * self.zoom,
            ),
            egui::Align2::CENTER_CENTER,
            &node.name,
            name_font,
            name_color,
        );

        // Divider line under header
        painter.line_segment(
            [
                pos2(header_rect.left() + 4.0 * self.zoom, header_rect.bottom()),
                pos2(header_rect.right() - 4.0 * self.zoom, header_rect.bottom()),
            ],
            egui::Stroke::new(1.0 * self.zoom, CatppuccinMocha::SURFACE2),
        );

        // Fields
        let field_start_y = header_rect.bottom() + 6.0 * self.zoom;
        let field_height = 22.0 * self.zoom;
        let field_font = egui::FontId::proportional(11.0 * self.zoom);
        let type_font = egui::FontId::proportional(10.0 * self.zoom);

        for (i, (name, typ, color)) in node.fields.iter().enumerate() {
            let y = field_start_y + i as f32 * field_height;

            if node.kind == NodeKind::Enum {
                // Enum variant — just the name
                painter.text(
                    pos2(screen_pos.x + 12.0 * self.zoom, y),
                    egui::Align2::LEFT_TOP,
                    name,
                    field_font.clone(),
                    *color,
                );
            } else {
                // Struct field — name : type
                painter.text(
                    pos2(screen_pos.x + 12.0 * self.zoom, y),
                    egui::Align2::LEFT_TOP,
                    name,
                    field_font.clone(),
                    CatppuccinMocha::SUBTEXT0,
                );

                if !typ.is_empty() {
                    painter.text(
                        pos2(screen_pos.x + screen_size.x - 12.0 * self.zoom, y),
                        egui::Align2::RIGHT_TOP,
                        typ,
                        type_font.clone(),
                        *color,
                    );
                }
            }
        }
    }
}

/// Find the point on the edge of a rectangle closest to a target point
fn edge_point(rect_pos: Pos2, rect_size: Vec2, target: Pos2) -> Pos2 {
    let center = rect_pos + rect_size / 2.0;
    let dir = target - center;

    if dir.x.abs() < 0.001 && dir.y.abs() < 0.001 {
        return center;
    }

    let half_w = rect_size.x / 2.0;
    let half_h = rect_size.y / 2.0;

    // Scale to find intersection with rectangle edge
    let scale_x = if dir.x.abs() > 0.001 {
        half_w / dir.x.abs()
    } else {
        f32::MAX
    };
    let scale_y = if dir.y.abs() > 0.001 {
        half_h / dir.y.abs()
    } else {
        f32::MAX
    };

    let scale = scale_x.min(scale_y);
    center + dir * scale
}

fn to_pascal_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '_' || ch == '-' || ch == ' ' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    result
}

/// Extract the inner type name from wrapper types like Vec<X>, Option<X>, [X]
fn extract_inner_type(type_str: &str) -> String {
    let s = type_str.trim();
    if let Some(inner) = s.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        return extract_inner_type(inner);
    }
    if let Some(inner) = s.strip_suffix('?') {
        return extract_inner_type(inner);
    }
    if let Some(rest) = s.strip_prefix("Vec<") {
        if let Some(inner) = rest.strip_suffix('>') {
            return extract_inner_type(inner);
        }
    }
    if let Some(rest) = s.strip_prefix("Option<") {
        if let Some(inner) = rest.strip_suffix('>') {
            return extract_inner_type(inner);
        }
    }
    s.to_string()
}
