use std::collections::{BTreeMap, BTreeSet};

use egui::{self, RichText, Ui};

use crate::theme::CatppuccinMocha;

struct StructBlock {
    name: String,
    start_line: usize,
    end_line: usize,
    text: String,
}

/// A generated .rs file
struct GeneratedFile {
    name: String,
    code: String,
    lines: Vec<String>,
    structs: Vec<StructBlock>,
    is_group: bool,
}

/// State for the enum conversion popup
struct EnumPopup {
    open: bool,
    field_name: String,
    enum_name: String,
    values: Vec<String>,
    // Existing enum to tag with (None = create new)
    selected_existing: Option<String>,
    // Similar existing enums sorted by similarity
    similar_enums: Vec<(String, Vec<String>, f32)>, // (name, variants, similarity 0..1)
    // Structs that have this field, with checkbox state
    affected_structs: Vec<(String, bool)>, // (struct_name, checked)
    // Preview of merged variants (when tagging existing)
    preview_variants: Vec<String>,
}

impl EnumPopup {
    fn new() -> Self {
        Self {
            open: false,
            field_name: String::new(),
            enum_name: String::new(),
            values: Vec::new(),
            selected_existing: None,
            similar_enums: Vec::new(),
            affected_structs: Vec::new(),
            preview_variants: Vec::new(),
        }
    }
}

pub struct CodeView {
    files: Vec<GeneratedFile>,
    selected_file: usize,
    cache_key: u64,
    // Search
    search_query: String,
    search_matches: Vec<(usize, usize)>, // (line_idx, col_offset)
    current_match: usize,
    scroll_to_match: Option<usize>, // line to scroll to
    // Navigation: struct_name -> (file_index, line)
    struct_index: BTreeMap<String, (usize, usize)>,
    // Pending navigation from type click
    nav_target: Option<(usize, usize)>, // (file_index, line)
    // Sample string values per field name (for tooltips)
    field_values: BTreeMap<String, Vec<String>>,
    // Enum candidates: field names where values look like a finite set
    enum_candidates: BTreeSet<String>,
    // User-converted enums: field_name -> (enum_name, variants)
    converted_enums: BTreeMap<String, (String, Vec<String>)>,
    // Hidden fields: "StructName.field_name" set
    hidden_fields: BTreeSet<String>,
    // Enum popup state
    enum_popup: EnumPopup,
    // Selected code language
    selected_language: crate::lang::CodeLanguage,
    // Async download state
    download_rx: Option<std::sync::mpsc::Receiver<Result<usize, String>>>,
}

impl CodeView {
    pub fn new() -> Self {
        Self {
            files: Vec::new(),
            selected_file: 0,
            cache_key: 0,
            search_query: String::new(),
            search_matches: Vec::new(),
            current_match: 0,
            scroll_to_match: None,
            struct_index: BTreeMap::new(),
            nav_target: None,
            field_values: BTreeMap::new(),
            enum_candidates: BTreeSet::new(),
            converted_enums: BTreeMap::new(),
            hidden_fields: BTreeSet::new(),
            enum_popup: EnumPopup::new(),
            selected_language: crate::lang::CodeLanguage::Rust,
            download_rx: None,
        }
    }

    pub fn invalidate(&mut self) {
        self.cache_key = 0;
    }

    /// Get list of generated file names and group flags for sidebar
    pub fn file_names(&self) -> Vec<(&str, bool)> {
        self.files.iter().map(|f| (f.name.as_str(), f.is_group)).collect()
    }

    pub fn selected_file_index(&self) -> usize {
        self.selected_file
    }

    pub fn select_file(&mut self, idx: usize) {
        if idx < self.files.len() {
            self.selected_file = idx;
        }
    }

    pub fn selected_language(&self) -> crate::lang::CodeLanguage {
        self.selected_language
    }

    pub fn set_language(&mut self, lang: crate::lang::CodeLanguage) {
        if self.selected_language != lang {
            self.selected_language = lang;
            self.cache_key = 0; // force rebuild
        }
    }

    /// Generate code: per-file .rs + shared.rs
    pub fn show(
        &mut self,
        ui: &mut Ui,
        parsed_files: &[(String, serde_json::Value)],
        schema: Option<&crate::schema::SchemaOverview>,
        enum_conversions: &mut Vec<crate::session::EnumConversion>,
        hidden_fields: &mut Vec<String>,
    ) {
        // Rebuild converted_enums from session state
        self.converted_enums.clear();
        for ec in enum_conversions.iter() {
            self.converted_enums.insert(
                ec.field_name.clone(),
                (ec.enum_name.clone(), ec.variants.clone()),
            );
        }
        // Rebuild hidden_fields from session state
        self.hidden_fields = hidden_fields.iter().cloned().collect();

        // Poll async download result
        if let Some(rx) = &self.download_rx {
            match rx.try_recv() {
                Ok(Ok(_count)) => { self.download_rx = None; }
                Ok(Err(_err)) => { self.download_rx = None; }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => { self.download_rx = None; }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        let enum_count = enum_conversions.len();
        let hidden_count = hidden_fields.len();
        let key = crate::widgets::hash_key(|h| {
            use std::hash::Hash;
            parsed_files.len().hash(h);
            for (name, _) in parsed_files {
                name.hash(h);
            }
            enum_count.hash(h);
            hidden_count.hash(h);
            (self.selected_language as u64).hash(h);
        });
        if self.cache_key != key {
            self.rebuild_file_mode(parsed_files, schema);
            self.build_struct_index();
            self.cache_key = key;
        }
        // Handle pending navigation from type click
        if let Some((file_idx, line)) = self.nav_target.take() {
            self.selected_file = file_idx;
            // Scroll a couple lines above so the struct has breathing room
            self.scroll_to_match = Some(line.saturating_sub(2));
        }
        let prev_enums = self.converted_enums.clone();
        let prev_hidden_count = self.hidden_fields.len();

        // Internal layout: file sidebar + code content
        let avail = ui.available_rect_before_wrap();
        let sidebar_w = 180.0;

        ui.horizontal(|ui| {
            ui.set_height(avail.height());

            // -- Internal file sidebar --
            ui.vertical(|ui| {
                ui.set_width(sidebar_w);
                ui.set_height(avail.height());

                // Language toggle
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    use egui_phosphor::regular;
                    for (lang_option, icon) in [
                        (crate::lang::CodeLanguage::Rust, regular::FILE_RS),
                        (crate::lang::CodeLanguage::Swift, regular::BIRD),
                    ] {
                        let is_selected = self.selected_language == lang_option;
                        let bg = if is_selected {
                            crate::theme::CatppuccinMocha::SURFACE0
                        } else {
                            egui::Color32::TRANSPARENT
                        };
                        let text_color = if is_selected {
                            crate::theme::CatppuccinMocha::BLUE
                        } else {
                            crate::theme::CatppuccinMocha::OVERLAY0
                        };
                        let btn = egui::Frame::new()
                            .fill(bg)
                            .corner_radius(6.0)
                            .inner_margin(egui::Margin::symmetric(6, 3))
                            .show(ui, |ui| {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(format!("{} {}", icon, lang_option.display_name()))
                                            .color(text_color)
                                            .size(11.0),
                                    )
                                    .sense(egui::Sense::click()),
                                )
                            });
                        if btn.inner.clicked() {
                            self.selected_language = lang_option;
                            self.cache_key = 0;
                        }
                    }
                });
                ui.add_space(4.0);

                // File list
                let mut clicked_file = None;
                egui::ScrollArea::vertical()
                    .id_salt("code_file_list")
                    .auto_shrink(false)
                    .max_height(avail.height() - 30.0)
                    .show(ui, |ui| {
                        let mut prev_was_group = false;
                        for (idx, file) in self.files.iter().enumerate() {
                            if file.is_group && !prev_was_group {
                                ui.add_space(4.0);
                                ui.separator();
                                ui.add_space(2.0);
                            }
                            prev_was_group = file.is_group;

                            let is_selected = idx == self.selected_file;
                            let (is_hovered, row_id) = crate::widgets::check_hover(ui.ctx(), ui.id().with(("code_file", idx)));

                            let bg = if is_selected {
                                crate::theme::CatppuccinMocha::SURFACE0
                            } else if is_hovered {
                                egui::Color32::from_rgba_premultiplied(40, 40, 55, 255)
                            } else {
                                egui::Color32::TRANSPARENT
                            };
                            let text_color = if is_selected || is_hovered {
                                crate::theme::CatppuccinMocha::TEXT
                            } else {
                                crate::theme::CatppuccinMocha::SUBTEXT0
                            };
                            let icon_color = if is_selected || is_hovered {
                                crate::theme::CatppuccinMocha::TEXT
                            } else {
                                crate::theme::CatppuccinMocha::SURFACE2
                            };

                            let r = egui::Frame::new()
                                .fill(bg)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(6, 2))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        ui.label(
                                            RichText::new(egui_phosphor::regular::FILE_CODE)
                                                .color(icon_color)
                                                .size(12.0),
                                        );
                                        ui.label(
                                            RichText::new(&file.name)
                                                .color(text_color)
                                                .size(11.0),
                                        );
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());

                            crate::widgets::store_hover(ui.ctx(), row_id, r.hovered());
                            if r.clicked() {
                                clicked_file = Some(idx);
                            }
                        }
                    });

                if let Some(idx) = clicked_file {
                    self.selected_file = idx;
                }
            });

            // Separator
            let sep_rect = ui.available_rect_before_wrap();
            ui.painter().line_segment(
                [
                    egui::pos2(sep_rect.left(), sep_rect.top()),
                    egui::pos2(sep_rect.left(), sep_rect.bottom()),
                ],
                egui::Stroke::new(1.0, crate::theme::CatppuccinMocha::SURFACE0),
            );
            ui.add_space(4.0);

            // -- Code content --
            ui.vertical(|ui| {
                self.show_selected(ui, "code_all_scroll");
            });
        });
        self.show_enum_popup(ui.ctx());

        // Sync enum conversions back to session if changed
        if self.converted_enums != prev_enums {
            *enum_conversions = self
                .converted_enums
                .iter()
                .map(|(field_name, (enum_name, variants))| crate::session::EnumConversion {
                    field_name: field_name.clone(),
                    enum_name: enum_name.clone(),
                    variants: variants.clone(),
                })
                .collect();
        }
        // Sync hidden fields back to session if changed
        if self.hidden_fields.len() != prev_hidden_count {
            *hidden_fields = self.hidden_fields.iter().cloned().collect();
        }
    }

    fn rebuild_file_mode(
        &mut self,
        parsed_files: &[(String, serde_json::Value)],
        schema: Option<&crate::schema::SchemaOverview>,
    ) {
        self.files.clear();
        let lang = self.selected_language.generator();

        let empty_schema = crate::schema::SchemaOverview {
            structs: Vec::new(),
            unique_structs: Vec::new(),
        };
        let schema = schema.unwrap_or(&empty_schema);

        let project = crate::codegen::generate_project(parsed_files, schema, lang.as_ref());
        let shared_file_name = lang.file_name("shared");

        for pf in &project {
            if pf.name == "mod.rs" {
                continue;
            }
            let lines: Vec<String> = pf.code.lines().map(|l| l.to_string()).collect();
            let struct_blocks = extract_struct_blocks(&lines, self.selected_language);
            let is_group = pf.name != shared_file_name;
            self.files.push(GeneratedFile {
                name: pf.name.clone(),
                code: pf.code.clone(),
                lines,
                structs: struct_blocks,
                is_group,
            });
        }

        let enums_file_name = lang.file_name("enums");
        let shared_file_name = lang.file_name("shared");

        // Generate enums file from converted enums and rewrite types in all files
        if !self.converted_enums.is_empty() {
            let mut enums_code = String::new();
            let header = lang.file_header();
            if !header.is_empty() {
                enums_code.push_str(&header);
                enums_code.push('\n');
            }
            // Enums file has no struct bodies, just enum declarations — no chrono imports needed
            enums_code.push_str(&lang.imports_header("", false));
            enums_code.push('\n');
            for (_, (enum_name, variants)) in &self.converted_enums {
                enums_code.push_str(&lang.enum_open(enum_name));
                for v in variants {
                    let variant_name = to_enum_variant(v);
                    enums_code.push_str(&lang.enum_variant(&variant_name, v));
                }
                enums_code.push_str(&lang.enum_close());
                enums_code.push('\n');
            }
            let enums_code = enums_code.trim_end().to_string() + "\n";
            let lines: Vec<String> = enums_code.lines().map(|l| l.to_string()).collect();
            let struct_blocks = extract_struct_blocks(&lines, self.selected_language);
            self.files.push(GeneratedFile {
                name: enums_file_name.clone(),
                code: enums_code,
                lines,
                structs: struct_blocks,
                is_group: false,
            });

            // Rewrite String -> EnumName in all files for converted fields
            let is_rust = self.selected_language == crate::lang::CodeLanguage::Rust;
            for file in &mut self.files {
                if file.name == enums_file_name {
                    continue;
                }
                let mut new_code = file.code.clone();
                for (field_name, (enum_name, _)) in &self.converted_enums {
                    let code_field = lang.field_name(field_name);
                    if is_rust {
                        let patterns = [
                            (format!("pub {}: String,", field_name), format!("pub {}: {},", field_name, enum_name)),
                            (format!("pub {}: String,", code_field), format!("pub {}: {},", code_field, enum_name)),
                            (format!("pub {}: Option<String>,", field_name), format!("pub {}: Option<{}>,", field_name, enum_name)),
                            (format!("pub {}: Option<String>,", code_field), format!("pub {}: Option<{}>,", code_field, enum_name)),
                        ];
                        for (from, to) in &patterns {
                            new_code = new_code.replace(from, to);
                        }
                    } else {
                        // Swift: `let field: String` and `let field: String?`
                        let patterns = [
                            (format!("let {}: String", code_field), format!("let {}: {}", code_field, enum_name)),
                            (format!("let {}: String?", code_field), format!("let {}: {}?", code_field, enum_name)),
                        ];
                        for (from, to) in &patterns {
                            new_code = new_code.replace(from, to);
                        }
                    }
                }
                if new_code != file.code {
                    // Add enum import for Rust only (Swift doesn't need imports)
                    if is_rust && !new_code.contains("use super::enums::") {
                        new_code = new_code.replace(
                            "use serde::{Deserialize, Serialize};\n",
                            "use serde::{Deserialize, Serialize};\nuse super::enums::*;\n",
                        );
                    }
                    file.code = new_code;
                    file.lines = file.code.lines().map(|l| l.to_string()).collect();
                    file.structs = extract_struct_blocks(&file.lines, self.selected_language);
                }
            }
        }

        // Reorder: move enums file right after shared file (before group files)
        if let Some(enum_pos) = self.files.iter().position(|f| f.name == enums_file_name) {
            let enums_file = self.files.remove(enum_pos);
            let insert_at = self.files.iter().position(|f| f.name == shared_file_name)
                .map(|i| i + 1)
                .unwrap_or(0);
            self.files.insert(insert_at, enums_file);
        }

        // Generate mod file (Rust only — Swift returns None)
        {
            let names: Vec<&str> = self.files.iter().map(|f| f.name.as_str()).collect();
            if let Some(mod_code) = lang.mod_file(&names) {
                let lines: Vec<String> = mod_code.lines().map(|l| l.to_string()).collect();
                let struct_blocks = extract_struct_blocks(&lines, self.selected_language);
                self.files.insert(0, GeneratedFile {
                    name: "mod.rs".to_string(),
                    code: mod_code,
                    lines,
                    structs: struct_blocks,
                    is_group: false,
                });
            }
        }

        if self.selected_file >= self.files.len() {
            self.selected_file = 0;
        }

        // Collect sample string values per field name for tooltips + enum detection
        self.field_values.clear();
        self.enum_candidates.clear();
        let mut value_sets: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
        let mut occurrence_counts: BTreeMap<String, usize> = BTreeMap::new();
        for (_, value) in parsed_files {
            collect_string_values(value, &mut value_sets, &mut occurrence_counts);
        }
        for (key, vals) in value_sets {
            let is_candidate = !vals.is_empty()
                && vals.len() <= 30
                && vals.iter().all(|v| v.len() <= 60 && !v.contains(' ') || looks_like_enum_value(v));
            let mut sorted: Vec<String> = vals.into_iter().collect();
            sorted.sort();
            if is_candidate && !self.converted_enums.contains_key(&key) {
                self.enum_candidates.insert(key.clone());
            }
            sorted.truncate(30);
            self.field_values.insert(key, sorted);
        }
    }

    fn build_struct_index(&mut self) {
        self.struct_index.clear();
        for (file_idx, file) in self.files.iter().enumerate() {
            for line_idx in 0..file.lines.len() {
                let trimmed = file.lines[line_idx].trim();
                // Detect struct lines: "pub struct X" (Rust) or "struct X:" (Swift)
                let struct_name = trimmed.strip_prefix("pub struct ")
                    .or_else(|| trimmed.strip_prefix("struct "))
                    .and_then(|rest| rest.split(|c: char| c == ' ' || c == '{' || c == '<' || c == ':').next())
                    .filter(|n| !n.is_empty());
                if let Some(name) = struct_name {
                    let target = if line_idx > 0
                        && file.lines[line_idx - 1].trim().starts_with("#[derive")
                    {
                        line_idx - 1
                    } else {
                        line_idx
                    };
                    self.struct_index.entry(name.to_string()).or_insert((file_idx, target));
                }
                // Detect enum lines: "pub enum X" (Rust) or "enum X:" (Swift)
                let enum_name = trimmed.strip_prefix("pub enum ")
                    .or_else(|| trimmed.strip_prefix("enum "))
                    .and_then(|rest| rest.split(|c: char| c == ' ' || c == '{' || c == '<' || c == ':').next())
                    .filter(|n| !n.is_empty());
                if let Some(name) = enum_name {
                    let target = if line_idx > 0
                        && file.lines[line_idx - 1].trim().starts_with("#[derive")
                    {
                        line_idx - 1
                    } else {
                        line_idx
                    };
                    self.struct_index.entry(name.to_string()).or_insert((file_idx, target));
                }
                if let Some(rest) = trimmed.strip_prefix("pub type ") {
                    let name = rest
                        .split(|c: char| c == ' ' || c == '=')
                        .next()
                        .unwrap_or("")
                        .to_string();
                    if !name.is_empty() {
                        self.struct_index.entry(name).or_insert((file_idx, line_idx));
                    }
                }
            }
        }
    }

    fn rebuild_search(&mut self) {
        self.search_matches.clear();
        self.current_match = 0;
        if self.search_query.is_empty() {
            return;
        }
        if let Some(file) = self.files.get(self.selected_file) {
            let query = self.search_query.to_ascii_lowercase();
            for (line_idx, line) in file.lines.iter().enumerate() {
                let lower = line.to_ascii_lowercase();
                let mut start = 0;
                while let Some(pos) = lower[start..].find(&query) {
                    self.search_matches.push((line_idx, start + pos));
                    start += pos + query.len();
                }
            }
        }
    }

    fn show_selected(&mut self, ui: &mut Ui, scroll_id: &str) {
        if self.files.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("No code generated")
                        .color(CatppuccinMocha::OVERLAY0)
                        .size(16.0),
                );
            });
            return;
        }

        // Top bar: Copy + file info on left, search on right
        ui.horizontal(|ui| {
            if ui.button("Copy All").clicked() {
                let code = filter_hidden_fields_from_code(
                    &self.files[self.selected_file],
                    &self.hidden_fields,
                );
                ui.ctx().copy_text(code);
            }
            let downloading = self.download_rx.is_some();
            if ui.add_enabled(
                !downloading,
                egui::Button::new(
                    RichText::new(format!("{} {}", egui_phosphor::regular::DOWNLOAD_SIMPLE,
                        if downloading { "Downloading..." } else { "Download" }))
                ),
            ).clicked() {
                // Prepare data for background thread
                let file_data: Vec<(String, String)> = self.files.iter()
                    .map(|f| (f.name.clone(), filter_hidden_fields_from_code(f, &self.hidden_fields)))
                    .collect();
                let dl_lang = self.selected_language.generator();
                let names: Vec<String> = self.files.iter()
                    .filter(|f| f.name != "mod.rs")
                    .map(|f| f.name.clone())
                    .collect();
                let mod_code = {
                    let name_refs: Vec<&str> = names.iter().map(|s| s.as_str()).collect();
                    dl_lang.mod_file(&name_refs)
                };

                let (tx, rx) = std::sync::mpsc::channel();
                let ctx = ui.ctx().clone();
                std::thread::spawn(move || {
                    let result = (|| -> Result<usize, String> {
                        let dir = rfd::FileDialog::new()
                            .pick_folder()
                            .ok_or_else(|| "cancelled".to_string())?;
                        let mut count = 0;
                        for (name, code) in &file_data {
                            std::fs::write(dir.join(name), code)
                                .map_err(|e| format!("write {}: {}", name, e))?;
                            count += 1;
                        }
                        if let Some(mod_code) = &mod_code {
                            std::fs::write(dir.join("mod.rs"), mod_code)
                                .map_err(|e| format!("write mod.rs: {}", e))?;
                            count += 1;
                        }
                        Ok(count)
                    })();
                    let _ = tx.send(result);
                    ctx.request_repaint();
                });
                self.download_rx = Some(rx);
            }
            ui.label(
                RichText::new(format!(
                    "{} — {} lines",
                    self.files[self.selected_file].name,
                    self.files[self.selected_file].lines.len()
                ))
                .color(CatppuccinMocha::OVERLAY0)
                .small(),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                // Next/Prev buttons
                let has_matches = !self.search_matches.is_empty();
                if ui.add_enabled(has_matches, egui::Button::new(
                    RichText::new(egui_phosphor::regular::CARET_DOWN).size(14.0),
                ).frame(false)).on_hover_text("Next match").clicked() {
                    self.current_match = (self.current_match + 1) % self.search_matches.len();
                    self.scroll_to_match = Some(self.search_matches[self.current_match].0);
                }
                if ui.add_enabled(has_matches, egui::Button::new(
                    RichText::new(egui_phosphor::regular::CARET_UP).size(14.0),
                ).frame(false)).on_hover_text("Previous match").clicked() {
                    if self.current_match == 0 {
                        self.current_match = self.search_matches.len().saturating_sub(1);
                    } else {
                        self.current_match -= 1;
                    }
                    self.scroll_to_match = Some(self.search_matches[self.current_match].0);
                }

                // Match count (always reserve space so TextEdit ID stays stable)
                let count_text = if self.search_query.is_empty() {
                    String::new()
                } else if self.search_matches.is_empty() {
                    "0/0".to_string()
                } else {
                    format!("{}/{}", self.current_match + 1, self.search_matches.len())
                };
                ui.label(
                    RichText::new(count_text)
                        .color(if !self.search_query.is_empty() && self.search_matches.is_empty() {
                            CatppuccinMocha::RED
                        } else {
                            CatppuccinMocha::OVERLAY0
                        })
                        .small()
                        .family(egui::FontFamily::Monospace),
                );

                // Search input — stable ID so focus persists across frames
                let search_id = ui.id().with("code_search");
                let prev_query = self.search_query.clone();
                let response = ui.add(
                    egui::TextEdit::singleline(&mut self.search_query)
                        .id(search_id)
                        .desired_width(250.0)
                        .hint_text("Search…")
                        .font(egui::TextStyle::Body),
                );
                if self.search_query != prev_query {
                    self.rebuild_search();
                    if !self.search_matches.is_empty() {
                        self.scroll_to_match = Some(self.search_matches[0].0);
                    }
                    response.request_focus();
                }
                // Enter navigates to next match
                if response.lost_focus()
                    && ui.input(|i| i.key_pressed(egui::Key::Enter))
                    && !self.search_matches.is_empty()
                {
                    self.current_match = (self.current_match + 1) % self.search_matches.len();
                    self.scroll_to_match = Some(self.search_matches[self.current_match].0);
                    response.request_focus();
                }
            });
        });

        ui.add_space(8.0);

        // Collect search state for rendering
        let current_match_line = self.search_matches
            .get(self.current_match)
            .map(|(line, _)| *line);
        let match_lines: std::collections::BTreeSet<usize> = self.search_matches
            .iter()
            .map(|(line, _)| *line)
            .collect();
        let scroll_target: Option<usize> = self.scroll_to_match.take();

        let file = &self.files[self.selected_file];
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace) + 2.0;
        let num_rows = file.lines.len();
        let struct_index = &self.struct_index;
        let field_values = &self.field_values;

        let struct_start_lines: Vec<(usize, usize)> = file
            .structs
            .iter()
            .enumerate()
            .map(|(i, s)| (s.start_line, i))
            .collect();

        let scroll_id_salt = format!("{}_{}", scroll_id, self.selected_file);
        let spacing_y = ui.spacing().item_spacing.y;
        let scroll_offset = scroll_target.map(|line| (line as f32 * (row_height + spacing_y)).max(0.0));

        let mut area = egui::ScrollArea::vertical()
            .id_salt(&scroll_id_salt)
            .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
            .auto_shrink(false)
            .animated(false);

        if let Some(y) = scroll_offset {
            area = area.vertical_scroll_offset(y);
        }

        let mut nav_clicked: Option<(usize, usize)> = None;
        let mut pending_enum_convert: Option<String> = None;
        let mut pending_enum_revert: Option<String> = None;
        let mut pending_toggle_field: Option<String> = None;
        let mut pending_enum_edit: Option<String> = None;
        let enum_candidates = &self.enum_candidates;
        let converted_enums = &self.converted_enums;
        let hidden_fields = &self.hidden_fields;

        // Build reverse mapping: enum_name -> Vec<(struct_name, field_name)>
        let lang = self.selected_language.generator();
        let enums_file_name = lang.file_name("enums");
        let is_rust = self.selected_language == crate::lang::CodeLanguage::Rust;
        let enum_usages: BTreeMap<String, Vec<(String, String, String)>> = {
            let mut usages: BTreeMap<String, Vec<(String, String, String)>> = BTreeMap::new();
            // Scan all non-enums files for fields that reference enum types
            for f in &self.files {
                if f.name == enums_file_name || f.name == "mod.rs" {
                    continue;
                }
                let mut cur_struct: Option<String> = None;
                for line in &f.lines {
                    let t = line.trim();
                    // Detect struct start
                    let struct_start = t.strip_prefix("pub struct ")
                        .or_else(|| t.strip_prefix("struct "));
                    if let Some(rest) = struct_start {
                        cur_struct = rest
                            .split(|c: char| c == ' ' || c == '{' || c == ':')
                            .next()
                            .map(|s| s.to_string());
                    } else if t == "}" {
                        cur_struct = None;
                    } else {
                        let is_field = if is_rust {
                            t.starts_with("pub ") && t.contains(':')
                        } else {
                            t.starts_with("let ") && t.contains(':')
                        };
                        if is_field {
                            if let Some(ref sname) = cur_struct {
                                if let Some(colon) = t.find(':') {
                                    let field = if is_rust {
                                        t[..colon].trim().trim_start_matches("pub ").trim_start_matches("r#")
                                    } else {
                                        t[..colon].trim().trim_start_matches("let ").trim_start_matches('`').trim_end_matches('`')
                                    };
                                    let typ = t[colon + 1..].trim().trim_end_matches(',');
                                    let bare = extract_inner_type(typ);
                                    for (_, (enum_name, _)) in converted_enums.iter() {
                                        if bare == enum_name {
                                            usages.entry(enum_name.clone())
                                                .or_default()
                                                .push((f.name.clone(), sname.clone(), field.to_string()));
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            usages
        };

        // Precompute struct name ranges for current file
        let struct_ranges: Vec<(usize, usize, &str)> = file
            .structs
            .iter()
            .map(|s| (s.start_line, s.end_line, s.name.as_str()))
            .collect();

        // Precompute enum name ranges for current file (for current_enum tracking)
        let enum_ranges: Vec<(usize, usize, String)> = {
            let mut ranges = Vec::new();
            let mut i = 0;
            while i < file.lines.len() {
                let t = file.lines[i].trim();
                let enum_name = t.strip_prefix("pub enum ")
                    .or_else(|| t.strip_prefix("enum "))
                    .and_then(|rest| rest.split(|c: char| c == ' ' || c == '{' || c == ':').next())
                    .filter(|n| !n.is_empty());
                if let Some(name) = enum_name {
                    let name = name.to_string();
                    let start = i;
                    while i < file.lines.len() && file.lines[i].trim() != "}" {
                        i += 1;
                    }
                    ranges.push((start, i, name));
                }
                i += 1;
            }
            ranges
        };

        area.show_rows(ui, row_height, num_rows, |ui, row_range| {
                for row in row_range {
                    let line = &file.lines[row];
                    let is_match_line = match_lines.contains(&row);
                    let is_current = current_match_line == Some(row);

                    // Find which struct this row belongs to
                    let current_struct = struct_ranges
                        .iter()
                        .find(|(start, end, _)| row >= *start && row <= *end)
                        .map(|(_, _, name)| *name);

                    // Find which enum this row belongs to (if any)
                    let current_enum = enum_ranges
                        .iter()
                        .find(|(start, end, _)| row >= *start && row <= *end)
                        .map(|(_, _, name)| name.as_str());

                    // Highlight match lines — subtle bg + left accent bar
                    if is_current || is_match_line {
                        let rect = ui.available_rect_before_wrap();
                        let row_rect = egui::Rect::from_min_size(
                            rect.min,
                            egui::vec2(rect.width(), row_height),
                        );
                        // Subtle background tint
                        let bg = if is_current {
                            egui::Color32::from_rgba_unmultiplied(137, 180, 250, 25)
                        } else {
                            egui::Color32::from_rgba_unmultiplied(249, 226, 175, 15)
                        };
                        ui.painter().rect_filled(row_rect, 0.0, bg);
                        // Left accent bar
                        let bar = egui::Rect::from_min_size(
                            rect.min,
                            egui::vec2(3.0, row_height),
                        );
                        let bar_color = if is_current {
                            CatppuccinMocha::BLUE
                        } else {
                            CatppuccinMocha::YELLOW
                        };
                        ui.painter().rect_filled(bar, 0.0, bar_color);
                    }

                    ui.horizontal(|ui| {
                        if let Some((_, struct_idx)) =
                            struct_start_lines.iter().find(|(l, _)| *l == row)
                        {
                            let btn = ui.add(
                                egui::Button::new(
                                    RichText::new(egui_phosphor::regular::COPY)
                                        .color(CatppuccinMocha::OVERLAY0),
                                )
                                .frame(false),
                            );
                            if btn.hovered() {
                                ui.painter().text(
                                    btn.rect.center(),
                                    egui::Align2::CENTER_CENTER,
                                    egui_phosphor::regular::COPY,
                                    egui::FontId::proportional(14.0),
                                    CatppuccinMocha::BLUE,
                                );
                            }
                            if btn.clicked() {
                                // Copy struct with hidden fields filtered out
                                let text = filter_hidden_fields_from_block(
                                    &file.structs[*struct_idx].text,
                                    &file.structs[*struct_idx].name,
                                    hidden_fields,
                                );
                                ui.ctx().copy_text(text);
                            }
                            btn.on_hover_ui_at_pointer(|ui| {
                                ui.label(
                                    RichText::new("Copy struct")
                                        .color(CatppuccinMocha::TEXT)
                                        .small(),
                                );
                            });
                        } else {
                            ui.label(
                                RichText::new("  ")
                                    .family(egui::FontFamily::Monospace),
                            );
                        }

                        ui.label(
                            RichText::new(format!("{:>4} ", row + 1))
                                .color(CatppuccinMocha::SURFACE2)
                                .family(egui::FontFamily::Monospace),
                        );
                        let action = render_code_line(
                            ui, line, struct_index, field_values, enum_candidates,
                            converted_enums, hidden_fields, current_struct,
                            &enum_usages, current_enum, self.selected_language,
                        );
                        match action {
                            LineAction::NavTo(target) => nav_clicked = Some(target),
                            LineAction::ConvertEnum(field) => pending_enum_convert = Some(field),
                            LineAction::RevertEnum(field) => pending_enum_revert = Some(field),
                            LineAction::ToggleField(key) => pending_toggle_field = Some(key),
                            LineAction::EditEnum(field) => pending_enum_edit = Some(field),
                            LineAction::None => {}
                        }
                    });
                }
            });

        if let Some(target) = nav_clicked {
            self.nav_target = Some(target);
        }
        if let Some(field_name) = pending_enum_convert {
            if let Some(values) = self.field_values.get(&field_name) {
                self.open_enum_popup(&field_name, values.clone());
            }
        }
        if let Some(field_name) = pending_enum_revert {
            self.converted_enums.remove(&field_name);
            self.enum_candidates.insert(field_name);
            self.cache_key = 0;
        }
        if let Some(key) = pending_toggle_field {
            if self.hidden_fields.contains(&key) {
                self.hidden_fields.remove(&key);
            } else {
                self.hidden_fields.insert(key);
            }
        }
        if let Some(field_name) = pending_enum_edit {
            let values = self
                .field_values
                .get(&field_name)
                .cloned()
                .or_else(|| self.converted_enums.get(&field_name).map(|(_, v)| v.clone()))
                .unwrap_or_default();
            self.open_enum_popup(&field_name, values);
        }
    }

    fn open_enum_popup(&mut self, field_name: &str, values: Vec<String>) {
        let existing = self.converted_enums.get(field_name);
        let popup = &mut self.enum_popup;
        popup.open = true;
        popup.field_name = field_name.to_string();
        popup.enum_name = existing
            .map(|(name, _)| name.clone())
            .unwrap_or_else(|| field_to_enum_name(field_name));
        popup.values = values.clone();
        popup.selected_existing = None;
        popup.preview_variants = existing
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| values.clone());

        // Find similar existing enums by Jaccard similarity of variants
        let value_set: BTreeSet<&str> = values.iter().map(|s| s.as_str()).collect();
        let mut similar: Vec<(String, Vec<String>, f32)> = self
            .converted_enums
            .iter()
            .map(|(_, (name, variants))| {
                let existing_set: BTreeSet<&str> = variants.iter().map(|s| s.as_str()).collect();
                let intersection = value_set.intersection(&existing_set).count();
                let union = value_set.union(&existing_set).count();
                let sim = if union > 0 {
                    intersection as f32 / union as f32
                } else {
                    0.0
                };
                (name.clone(), variants.clone(), sim)
            })
            .collect();
        similar.sort_by(|a, b| b.2.partial_cmp(&a.2).unwrap_or(std::cmp::Ordering::Equal));
        popup.similar_enums = similar;

        // Find all structs that contain this field name
        let mut affected = Vec::new();
        for file in &self.files {
            for s in &file.structs {
                for line in s.text.lines() {
                    let trimmed = line.trim();
                    // Extract field name from Rust ("pub field:") or Swift ("let field:")
                    let fname_opt = if let Some(rest) = trimmed.strip_prefix("pub ") {
                        rest.find(':').map(|colon| rest[..colon].trim().trim_start_matches("r#"))
                    } else if let Some(rest) = trimmed.strip_prefix("let ") {
                        rest.find(':').map(|colon| rest[..colon].trim().trim_start_matches('`').trim_end_matches('`'))
                    } else {
                        None
                    };
                    if let Some(fname) = fname_opt {
                        if fname == field_name {
                            affected.push((s.name.clone(), true));
                            break;
                        }
                    }
                }
            }
        }
        popup.affected_structs = affected;
    }

    fn show_enum_popup(&mut self, ctx: &egui::Context) {
        if !self.enum_popup.open {
            return;
        }

        let mut open = true;
        let mut confirmed = false;
        let mut cancelled = false;

        egui::Window::new("Convert to Enum")
            .open(&mut open)
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing.y = 6.0;

                // Enum name
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("Enum name")
                            .color(CatppuccinMocha::SUBTEXT0)
                            .small(),
                    );
                    ui.text_edit_singleline(&mut self.enum_popup.enum_name);
                });

                ui.add_space(4.0);

                // Tag with existing enum
                if !self.enum_popup.similar_enums.is_empty() {
                    ui.label(
                        RichText::new("Tag with existing enum")
                            .color(CatppuccinMocha::SUBTEXT0)
                            .small(),
                    );
                    ui.add_space(2.0);

                    let mut new_selection = self.enum_popup.selected_existing.clone();

                    // "New enum" option
                    let is_new = self.enum_popup.selected_existing.is_none();
                    if ui.add(egui::RadioButton::new(is_new,
                        RichText::new("Create new enum").size(12.0)
                    )).clicked() {
                        new_selection = None;
                    }

                    for (name, variants, sim) in &self.enum_popup.similar_enums {
                        let is_selected = self.enum_popup.selected_existing.as_ref() == Some(name);
                        let pct = (sim * 100.0) as u32;
                        let label = format!("{} ({} variants, {}% similar)", name, variants.len(), pct);
                        if ui.add(egui::RadioButton::new(is_selected,
                            RichText::new(label).size(12.0)
                        )).clicked() {
                            new_selection = Some(name.clone());
                        }
                    }

                    if new_selection != self.enum_popup.selected_existing {
                        self.enum_popup.selected_existing = new_selection.clone();
                        // Update enum name and preview
                        if let Some(ref existing_name) = new_selection {
                            self.enum_popup.enum_name = existing_name.clone();
                            // Merge variants
                            if let Some((_, existing_variants, _)) = self.enum_popup.similar_enums
                                .iter()
                                .find(|(n, _, _)| n == existing_name)
                            {
                                let mut merged: BTreeSet<String> = existing_variants.iter().cloned().collect();
                                for v in &self.enum_popup.values {
                                    merged.insert(v.clone());
                                }
                                self.enum_popup.preview_variants = merged.into_iter().collect();
                            }
                        } else {
                            self.enum_popup.enum_name = field_to_enum_name(&self.enum_popup.field_name);
                            self.enum_popup.preview_variants = self.enum_popup.values.clone();
                        }
                    }

                    ui.add_space(4.0);
                }

                // Affected structs
                ui.label(
                    RichText::new("Apply to structs")
                        .color(CatppuccinMocha::SUBTEXT0)
                        .small(),
                );
                ui.add_space(2.0);
                for (name, checked) in &mut self.enum_popup.affected_structs {
                    ui.checkbox(checked,
                        RichText::new(format!("{}.{}", name, self.enum_popup.field_name))
                            .size(12.0)
                            .family(egui::FontFamily::Monospace)
                    );
                }

                ui.add_space(6.0);

                // Preview
                ui.label(
                    RichText::new("Preview")
                        .color(CatppuccinMocha::SUBTEXT0)
                        .small(),
                );
                ui.add_space(2.0);
                egui::Frame::new()
                    .fill(CatppuccinMocha::CRUST)
                    .corner_radius(6.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        let enum_name = &self.enum_popup.enum_name;
                        ui.label(
                            RichText::new(format!("pub enum {} {{", enum_name))
                                .color(CatppuccinMocha::MAUVE)
                                .family(egui::FontFamily::Monospace)
                                .size(11.0),
                        );
                        for v in &self.enum_popup.preview_variants {
                            let variant = to_enum_variant(v);
                            let is_new = self.enum_popup.selected_existing.is_some()
                                && !self.enum_popup.similar_enums
                                    .iter()
                                    .find(|(n, _, _)| Some(n) == self.enum_popup.selected_existing.as_ref())
                                    .map(|(_, existing, _)| existing.contains(v))
                                    .unwrap_or(false);
                            let color = if is_new {
                                CatppuccinMocha::GREEN
                            } else {
                                CatppuccinMocha::TEXT
                            };
                            ui.label(
                                RichText::new(format!("    {},", variant))
                                    .color(color)
                                    .family(egui::FontFamily::Monospace)
                                    .size(11.0),
                            );
                        }
                        ui.label(
                            RichText::new("}")
                                .color(CatppuccinMocha::MAUVE)
                                .family(egui::FontFamily::Monospace)
                                .size(11.0),
                        );
                    });

                ui.add_space(8.0);

                // Buttons
                ui.horizontal(|ui| {
                    if ui.add(
                        egui::Button::new(
                            RichText::new("Convert")
                                .color(CatppuccinMocha::BASE)
                                .strong(),
                        )
                        .fill(CatppuccinMocha::GREEN)
                        .corner_radius(6.0),
                    ).clicked() {
                        confirmed = true;
                    }
                    if ui.add(
                        egui::Button::new("Cancel")
                            .fill(CatppuccinMocha::SURFACE0)
                            .corner_radius(6.0),
                    ).clicked() {
                        cancelled = true;
                    }
                });
            });

        if confirmed {
            // Apply the conversion
            let field_name = self.enum_popup.field_name.clone();
            let enum_name = self.enum_popup.enum_name.clone();
            let variants = self.enum_popup.preview_variants.clone();

            // If tagging with existing, update that enum's variants
            if let Some(ref existing) = self.enum_popup.selected_existing {
                // Find the field_name(s) that use this existing enum and update variants
                let field_keys: Vec<String> = self.converted_enums
                    .iter()
                    .filter(|(_, (name, _))| name == existing)
                    .map(|(k, _)| k.clone())
                    .collect();
                for k in field_keys {
                    self.converted_enums.get_mut(&k).unwrap().1 = variants.clone();
                }
            }

            self.converted_enums.insert(
                field_name.clone(),
                (enum_name, variants),
            );
            self.enum_candidates.remove(&field_name);
            self.cache_key = 0;
            self.enum_popup.open = false;
        } else if !open || cancelled {
            self.enum_popup.open = false;
        }
    }
}


enum LineAction {
    None,
    NavTo((usize, usize)),
    ConvertEnum(String),
    RevertEnum(String),
    ToggleField(String), // "StructName.field_name"
    EditEnum(String),    // field_name key into converted_enums
}

/// Render a line of code with interactive elements, language-aware.
fn render_code_line(
    ui: &mut Ui,
    line: &str,
    struct_index: &BTreeMap<String, (usize, usize)>,
    field_values: &BTreeMap<String, Vec<String>>,
    enum_candidates: &BTreeSet<String>,
    converted_enums: &BTreeMap<String, (String, Vec<String>)>,
    hidden_fields: &BTreeSet<String>,
    current_struct: Option<&str>,
    enum_usages: &BTreeMap<String, Vec<(String, String, String)>>,
    current_enum: Option<&str>,
    language: crate::lang::CodeLanguage,
) -> LineAction {
    let trimmed = line.trim();
    let is_swift = language == crate::lang::CodeLanguage::Swift;

    if trimmed.is_empty() {
        ui.label("");
        return LineAction::None;
    }

    if trimmed.starts_with("//") {
        ui.label(
            RichText::new(line)
                .color(CatppuccinMocha::OVERLAY0)
                .family(egui::FontFamily::Monospace),
        );
        return LineAction::None;
    }

    // Attributes: #[...] / #![...] for Rust
    if trimmed.starts_with("#[") || trimmed.starts_with("#!") {
        ui.label(
            RichText::new(line)
                .color(CatppuccinMocha::YELLOW)
                .family(egui::FontFamily::Monospace),
        );
        return LineAction::None;
    }

    // Swift CodingKeys enum line
    if is_swift && trimmed.starts_with("enum CodingKeys") {
        ui.label(
            RichText::new(line)
                .color(CatppuccinMocha::YELLOW)
                .family(egui::FontFamily::Monospace),
        );
        return LineAction::None;
    }

    if trimmed == "}" || trimmed == "}," {
        ui.label(
            RichText::new(line)
                .color(CatppuccinMocha::TEXT)
                .family(egui::FontFamily::Monospace),
        );
        return LineAction::None;
    }

    // Handle enum declaration lines: "pub enum Name {" (Rust) or "enum Name: String, Codable {" (Swift)
    let enum_prefix = if is_swift { "enum " } else { "pub enum " };
    if trimmed.starts_with(enum_prefix) && !trimmed.starts_with("enum CodingKeys") {
        ui.spacing_mut().item_spacing.x = 0.0;
        let indent = &line[..line.len() - line.trim_start().len()];
        if !indent.is_empty() {
            ui.label(RichText::new(indent).family(egui::FontFamily::Monospace));
        }
        ui.label(
            RichText::new(enum_prefix)
                .color(CatppuccinMocha::MAUVE)
                .family(egui::FontFamily::Monospace),
        );
        let rest = &trimmed[enum_prefix.len()..];
        let name = rest.split(|c: char| c == ' ' || c == '{' || c == ':').next().unwrap_or("");
        let after = &rest[name.len()..];

        // Check if this enum is a converted enum (editable)
        let enum_field_key = converted_enums
            .iter()
            .find(|(_, (en, _))| en == name)
            .map(|(field_name, _)| field_name.clone());

        let mut action = LineAction::None;
        let label = if enum_field_key.is_some() {
            // Check previous-frame hover state via cached rect
            let hover_id = egui::Id::new("enum_hover").with(name);
            let prev_rect: Option<egui::Rect> = ui.ctx().data(|d| d.get_temp(hover_id));
            let is_hovered = prev_rect.map_or(false, |r| {
                ui.input(|i| i.pointer.hover_pos().map_or(false, |p| r.contains(p)))
            });

            let mut text = RichText::new(name)
                .color(CatppuccinMocha::YELLOW)
                .family(egui::FontFamily::Monospace);
            if is_hovered {
                text = text.size(ui.text_style_height(&egui::TextStyle::Body) + 1.5);
            }

            let link = ui.add(egui::Label::new(text).sense(egui::Sense::click()));

            // Cache rect for next frame
            ui.ctx().data_mut(|d| d.insert_temp(hover_id, link.rect));

            if is_hovered {
                // Light green background pill
                ui.painter().rect_filled(
                    link.rect.expand(1.0),
                    3.0,
                    egui::Color32::from_rgba_unmultiplied(166, 227, 161, 35),
                );
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
            if link.clicked() {
                action = LineAction::EditEnum(enum_field_key.unwrap());
            }
            link
        } else {
            ui.label(
                RichText::new(name)
                    .color(CatppuccinMocha::YELLOW)
                    .family(egui::FontFamily::Monospace),
            )
        };
        // Usage tooltip
        if let Some(usages) = enum_usages.get(name) {
            if !usages.is_empty() {
                label.on_hover_ui(|ui| {
                    ui.style_mut().spacing.item_spacing.y = 2.0;
                    ui.label(
                        RichText::new("Used by")
                            .color(CatppuccinMocha::BLUE)
                            .small()
                            .strong(),
                    );
                    ui.separator();
                    for (file_name, struct_name, field_name) in usages {
                        ui.label(
                            RichText::new(format!("{}::{}.{}", file_name.trim_end_matches(".rs").trim_end_matches(".swift"), struct_name, field_name))
                                .color(CatppuccinMocha::TEXT)
                                .family(egui::FontFamily::Monospace)
                                .small(),
                        );
                    }
                });
            }
        }
        if !after.is_empty() {
            ui.label(
                RichText::new(after)
                    .color(CatppuccinMocha::TEXT)
                    .family(egui::FontFamily::Monospace),
            );
        }
        return action;
    }

    // Handle enum variant lines (inside an enum block)
    if let Some(enum_name) = current_enum {
        let is_variant = if is_swift {
            trimmed.starts_with("case ") && !trimmed.starts_with("case ") || trimmed.starts_with("case ")
        } else {
            let stripped = trimmed.trim_end_matches(',');
            !stripped.is_empty()
                && !stripped.starts_with("#[")
                && !stripped.starts_with("//")
                && stripped != "}"
                && !stripped.starts_with("pub ")
                && !stripped.starts_with("use ")
        };
        if is_variant {
            let indent = &line[..line.len() - line.trim_start().len()];
            ui.spacing_mut().item_spacing.x = 0.0;
            let label = ui.label(
                RichText::new(format!("{}{}", indent, trimmed))
                    .color(CatppuccinMocha::GREEN)
                    .family(egui::FontFamily::Monospace),
            );
            // Usage tooltip on variants too
            if let Some(usages) = enum_usages.get(enum_name) {
                if !usages.is_empty() {
                    label.on_hover_ui(|ui| {
                        ui.style_mut().spacing.item_spacing.y = 2.0;
                        ui.label(
                            RichText::new(format!("{} used by", enum_name))
                                .color(CatppuccinMocha::BLUE)
                                .small()
                                .strong(),
                        );
                        ui.separator();
                        for (file_name, struct_name, field_name) in usages {
                            ui.label(
                                RichText::new(format!("{}::{}.{}", file_name.trim_end_matches(".rs").trim_end_matches(".swift"), struct_name, field_name))
                                    .color(CatppuccinMocha::TEXT)
                                    .family(egui::FontFamily::Monospace)
                                    .small(),
                            );
                        }
                    });
                }
            }
            return LineAction::None;
        }
    }

    ui.spacing_mut().item_spacing.x = 0.0;

    // Struct/class header lines: "pub struct Foo {" / "struct Foo: Codable {"
    let struct_keyword = if is_swift { "struct " } else { "pub struct " };
    if trimmed.starts_with(struct_keyword) {
        let indent = &line[..line.len() - line.trim_start().len()];
        if !indent.is_empty() {
            ui.label(RichText::new(indent).family(egui::FontFamily::Monospace));
        }
        ui.label(
            RichText::new(struct_keyword)
                .color(CatppuccinMocha::MAUVE)
                .family(egui::FontFamily::Monospace),
        );
        let rest = &trimmed[struct_keyword.len()..];
        let name = rest.split(|c: char| c == ' ' || c == '{' || c == ':' || c == '<').next().unwrap_or("");
        ui.label(
            RichText::new(name)
                .color(CatppuccinMocha::YELLOW)
                .family(egui::FontFamily::Monospace),
        );
        let after = &rest[name.len()..];
        if !after.is_empty() {
            // Highlight protocol/trait names like ": Codable, Decodable {"
            render_tokens(ui, after, is_swift);
        }
        return LineAction::None;
    }

    // use/import lines: keyword in mauve, path in text
    let import_keyword = if is_swift { "import " } else { "use " };
    if trimmed.starts_with(import_keyword) {
        let indent = &line[..line.len() - line.trim_start().len()];
        if !indent.is_empty() {
            ui.label(RichText::new(indent).family(egui::FontFamily::Monospace));
        }
        ui.label(
            RichText::new(import_keyword)
                .color(CatppuccinMocha::MAUVE)
                .family(egui::FontFamily::Monospace),
        );
        let rest = &trimmed[import_keyword.len()..];
        ui.label(
            RichText::new(rest)
                .color(CatppuccinMocha::TEXT)
                .family(egui::FontFamily::Monospace),
        );
        return LineAction::None;
    }

    // CodingKeys case lines: case name = "value"
    if is_swift && trimmed.starts_with("case ") && trimmed.contains(" = \"") {
        let indent = &line[..line.len() - line.trim_start().len()];
        if !indent.is_empty() {
            ui.label(RichText::new(indent).family(egui::FontFamily::Monospace));
        }
        ui.label(
            RichText::new("case ")
                .color(CatppuccinMocha::MAUVE)
                .family(egui::FontFamily::Monospace),
        );
        let rest = &trimmed["case ".len()..];
        if let Some(eq_pos) = rest.find(" = ") {
            let case_name = &rest[..eq_pos];
            let value = &rest[eq_pos..];
            ui.label(
                RichText::new(case_name)
                    .color(CatppuccinMocha::TEXT)
                    .family(egui::FontFamily::Monospace),
            );
            ui.label(
                RichText::new(value)
                    .color(CatppuccinMocha::GREEN)
                    .family(egui::FontFamily::Monospace),
            );
        } else {
            ui.label(
                RichText::new(rest)
                    .color(CatppuccinMocha::TEXT)
                    .family(egui::FontFamily::Monospace),
            );
        }
        return LineAction::None;
    }

    // Field lines: "pub field: Type," (Rust) or "let field: Type" (Swift)
    let is_field_line = if is_swift {
        trimmed.starts_with("let ") && trimmed.contains(':')
    } else {
        trimmed.starts_with("pub ") && trimmed.contains(':')
    };

    if is_field_line {
        let indent = &line[..line.len() - line.trim_start().len()];
        if let Some(colon_pos) = trimmed.find(':') {
            let field_part = &trimmed[..colon_pos];
            let type_part = trimmed[colon_pos + 1..].trim();

            // Extract the JSON field name
            let json_field_name = if is_swift {
                field_part.trim_start_matches("let ").trim_start_matches('`').trim_end_matches('`')
            } else {
                field_part.trim_start_matches("pub ").trim_start_matches("r#")
            };

            // Check if this field is hidden
            let field_key = current_struct
                .map(|s| format!("{}.{}", s, json_field_name))
                .unwrap_or_default();
            let is_hidden = !field_key.is_empty() && hidden_fields.contains(&field_key);

            // Dim color for hidden fields
            let text_color = if is_hidden {
                CatppuccinMocha::SURFACE2
            } else {
                CatppuccinMocha::TEXT
            };

            // Toggle button on the left
            ui.label(
                RichText::new(indent)
                    .family(egui::FontFamily::Monospace),
            );
            if !field_key.is_empty() {
                if is_hidden {
                    let btn = ui.add(
                        egui::Button::new(
                            RichText::new(egui_phosphor::regular::PLUS_CIRCLE)
                                .color(CatppuccinMocha::GREEN)
                                .size(14.0),
                        )
                        .frame(false),
                    );
                    if btn.clicked() {
                        return LineAction::ToggleField(field_key);
                    }
                    btn.on_hover_text("Restore field");
                } else {
                    let btn = ui.add(
                        egui::Button::new(
                            RichText::new(egui_phosphor::regular::TRASH)
                                .color(CatppuccinMocha::RED)
                                .size(14.0),
                        )
                        .frame(false),
                    );
                    if btn.clicked() {
                        return LineAction::ToggleField(field_key);
                    }
                    btn.on_hover_text("Exclude field");
                }
                ui.spacing_mut().item_spacing.x = 2.0;
            }

            ui.label(
                RichText::new(format!("{}: ", field_part))
                    .color(text_color)
                    .family(egui::FontFamily::Monospace),
            );

            // Make type names clickable if they resolve to a known struct
            let type_for_lookup = if is_swift {
                type_part.trim_end_matches('?')
            } else {
                type_part.trim_end_matches(',')
            };
            let bare_type = extract_inner_type(type_for_lookup);
            if !is_hidden {
                if let Some(&(file_idx, line_idx)) = struct_index.get(bare_type) {
                    let nav = render_clickable_type(ui, type_part, bare_type, file_idx, line_idx);
                    // Show revert button for converted enum fields (before early return)
                    if converted_enums.contains_key(json_field_name) {
                        ui.spacing_mut().item_spacing.x = 4.0;
                        let btn = ui.add(
                            egui::Button::new(
                                RichText::new(format!("{} revert", egui_phosphor::regular::ARROW_U_UP_LEFT))
                                    .color(CatppuccinMocha::PEACH)
                                    .small(),
                            )
                            .frame(false),
                        );
                        if btn.clicked() {
                            return LineAction::RevertEnum(json_field_name.to_string());
                        }
                        btn.on_hover_text("Revert to String");
                    }
                    if let Some(target) = nav {
                        return LineAction::NavTo(target);
                    }
                    return LineAction::None;
                }
            }

            let type_color = if is_hidden {
                CatppuccinMocha::SURFACE2
            } else {
                classify_type_color(type_for_lookup)
            };
            let type_label = ui.label(
                RichText::new(type_part)
                    .color(type_color)
                    .family(egui::FontFamily::Monospace),
            );

            // Show sample values tooltip for String fields
            let clean_type = type_for_lookup;
            let is_string_type = clean_type == "String" || clean_type == "Option<String>" || clean_type == "String?";
            if !is_hidden && is_string_type && !json_field_name.is_empty() {
                if let Some(values) = field_values.get(json_field_name) {
                    if !values.is_empty() {
                        type_label.on_hover_ui(|ui| {
                            ui.style_mut().spacing.item_spacing.y = 2.0;
                            ui.label(
                                RichText::new(format!("Values for \"{}\"", json_field_name))
                                    .color(CatppuccinMocha::BLUE)
                                    .small()
                                    .strong(),
                            );
                            ui.separator();
                            for val in values {
                                let display = if val.len() > 60 {
                                    format!("{}...", &val[..57])
                                } else {
                                    val.clone()
                                };
                                ui.label(
                                    RichText::new(display)
                                        .color(CatppuccinMocha::GREEN)
                                        .family(egui::FontFamily::Monospace)
                                        .small(),
                                );
                            }
                            if values.len() >= 30 {
                                ui.label(
                                    RichText::new("...and more")
                                        .color(CatppuccinMocha::OVERLAY0)
                                        .small(),
                                );
                            }
                        });
                    }
                }

                // Show "-> enum" button for candidates
                if enum_candidates.contains(json_field_name) {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let btn = ui.add(
                        egui::Button::new(
                            RichText::new(format!("{} enum", egui_phosphor::regular::ARROW_RIGHT))
                                .color(CatppuccinMocha::TEAL)
                                .small(),
                        )
                        .frame(false),
                    );
                    if btn.clicked() {
                        return LineAction::ConvertEnum(json_field_name.to_string());
                    }
                    btn.on_hover_text("Convert to enum");
                }
            }

            // Show revert button for converted enum fields
            if !is_hidden && converted_enums.contains_key(json_field_name) {
                ui.spacing_mut().item_spacing.x = 4.0;
                let btn = ui.add(
                    egui::Button::new(
                        RichText::new(format!("{} revert", egui_phosphor::regular::ARROW_U_UP_LEFT))
                            .color(CatppuccinMocha::PEACH)
                            .small(),
                    )
                    .frame(false),
                );
                if btn.clicked() {
                    return LineAction::RevertEnum(json_field_name.to_string());
                }
                btn.on_hover_text("Revert to String");
            }
        } else {
            let indent = &line[..line.len() - line.trim_start().len()];
            if !indent.is_empty() {
                ui.label(RichText::new(indent).family(egui::FontFamily::Monospace));
            }
            render_tokens(ui, trimmed, is_swift);
        }
    } else {
        // Generic fallback — highlight tokens
        let indent = &line[..line.len() - line.trim_start().len()];
        if !indent.is_empty() {
            ui.label(RichText::new(indent).family(egui::FontFamily::Monospace));
        }
        render_tokens(ui, trimmed, is_swift);
    }
    LineAction::None
}

/// Render a code fragment with basic token-level syntax highlighting.
fn render_tokens(ui: &mut Ui, text: &str, is_swift: bool) {
    let keywords: &[&str] = if is_swift {
        &["struct", "enum", "case", "let", "var", "import", "public", "private", "func", "class", "protocol", "typealias"]
    } else {
        &["pub", "struct", "enum", "use", "fn", "let", "mut", "impl", "mod", "type", "crate", "self", "super"]
    };

    let mut chars = text.char_indices().peekable();
    let mut tokens: Vec<(&str, egui::Color32)> = Vec::new();

    while let Some(&(i, ch)) = chars.peek() {
        if ch == '"' {
            // String literal
            chars.next();
            let start = i;
            while let Some(&(_, c)) = chars.peek() {
                chars.next();
                if c == '"' { break; }
            }
            let end = chars.peek().map_or(text.len(), |&(j, _)| j);
            tokens.push((&text[start..end], CatppuccinMocha::GREEN));
        } else if ch.is_alphabetic() || ch == '_' {
            // Identifier/keyword
            let start = i;
            while chars.peek().is_some_and(|&(_, c)| c.is_alphanumeric() || c == '_') {
                chars.next();
            }
            let end = chars.peek().map_or(text.len(), |&(j, _)| j);
            let word = &text[start..end];
            let color = if keywords.contains(&word) {
                CatppuccinMocha::MAUVE
            } else if word.chars().next().is_some_and(|c| c.is_uppercase()) {
                // PascalCase = type name
                CatppuccinMocha::YELLOW
            } else if matches!(word, "true" | "false") {
                CatppuccinMocha::PEACH
            } else {
                CatppuccinMocha::TEXT
            };
            tokens.push((word, color));
        } else if ch.is_ascii_digit() {
            let start = i;
            while chars.peek().is_some_and(|&(_, c)| c.is_ascii_digit() || c == '.') {
                chars.next();
            }
            let end = chars.peek().map_or(text.len(), |&(j, _)| j);
            tokens.push((&text[start..end], CatppuccinMocha::PEACH));
        } else {
            // Punctuation / whitespace — batch consecutive
            let start = i;
            while chars.peek().is_some_and(|&(_, c)| !c.is_alphanumeric() && c != '_' && c != '"') {
                chars.next();
            }
            let end = chars.peek().map_or(text.len(), |&(j, _)| j);
            tokens.push((&text[start..end], CatppuccinMocha::TEXT));
        }
    }

    for (text, color) in tokens {
        ui.label(
            RichText::new(text)
                .color(color)
                .family(egui::FontFamily::Monospace),
        );
    }
}

/// Extract the innermost struct name from a type like "Vec<Option<Foo>>," -> "Foo"
/// Also handles Swift: "[Foo]" -> "Foo", "Foo?" -> "Foo"
fn extract_inner_type(t: &str) -> &str {
    let t = t.trim().trim_end_matches(',');
    if let Some(rest) = t.strip_prefix("Vec<").and_then(|s| s.strip_suffix('>')) {
        extract_inner_type(rest)
    } else if let Some(rest) = t.strip_prefix("Option<").and_then(|s| s.strip_suffix('>')) {
        extract_inner_type(rest)
    } else if let Some(rest) = t.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
        // Swift array [Foo]
        extract_inner_type(rest)
    } else if let Some(rest) = t.strip_suffix('?') {
        // Swift optional Foo?
        extract_inner_type(rest)
    } else {
        t
    }
}

/// Render a type string where the struct name portion is clickable.
/// Returns Some((file_idx, line_idx)) if the user clicked.
fn render_clickable_type(
    ui: &mut Ui,
    full_type: &str,
    struct_name: &str,
    file_idx: usize,
    line_idx: usize,
) -> Option<(usize, usize)> {
    let mut clicked = None;

    // Find where the struct name appears in the full type string
    if let Some(pos) = full_type.find(struct_name) {
        let before = &full_type[..pos];
        let after = &full_type[pos + struct_name.len()..];

        if !before.is_empty() {
            let wrapper_color = classify_type_color(before.trim_end_matches(|c: char| c == '<'));
            ui.label(
                RichText::new(before)
                    .color(wrapper_color)
                    .family(egui::FontFamily::Monospace),
            );
        }

        let type_color = classify_type_color(struct_name);
        let r = ui.add(
            egui::Label::new(
                RichText::new(struct_name)
                    .color(type_color)
                    .family(egui::FontFamily::Monospace)
                    .underline(),
            )
            .sense(egui::Sense::click()),
        );
        if r.clicked() {
            clicked = Some((file_idx, line_idx));
        }
        if r.hovered() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        }

        if !after.is_empty() {
            let suffix_color = if after.trim_end_matches(',').chars().all(|c| c == '>') {
                classify_type_color(before.trim_end_matches(|c: char| c == '<'))
            } else {
                CatppuccinMocha::TEXT
            };
            ui.label(
                RichText::new(after)
                    .color(suffix_color)
                    .family(egui::FontFamily::Monospace),
            );
        }
    } else {
        // Fallback: render as plain
        let type_color = classify_type_color(full_type.trim_end_matches(','));
        ui.label(
            RichText::new(full_type)
                .color(type_color)
                .family(egui::FontFamily::Monospace),
        );
    }

    clicked
}

fn extract_struct_blocks(lines: &[String], language: crate::lang::CodeLanguage) -> Vec<StructBlock> {
    let is_swift = language == crate::lang::CodeLanguage::Swift;
    let mut blocks = Vec::new();
    let mut i = 0;
    while i < lines.len() {
        let trimmed = lines[i].trim();
        let is_block_start = if is_swift {
            trimmed.starts_with("struct ") || (trimmed.starts_with("enum ") && !trimmed.starts_with("enum CodingKeys"))
        } else {
            trimmed.starts_with("#[derive") || trimmed.starts_with("pub struct") || trimmed.starts_with("pub enum")
        };
        if is_block_start {
            let start = i;
            let mut block_lines = Vec::new();
            let mut name = String::new();
            let mut brace_depth: i32 = 0;
            while i < lines.len() {
                let line_trimmed = lines[i].trim();
                if name.is_empty() {
                    let found_name = line_trimmed.strip_prefix("pub struct ")
                        .or_else(|| line_trimmed.strip_prefix("pub enum "))
                        .or_else(|| line_trimmed.strip_prefix("struct "))
                        .or_else(|| {
                            if line_trimmed.starts_with("enum ") && !line_trimmed.starts_with("enum CodingKeys") {
                                line_trimmed.strip_prefix("enum ")
                            } else {
                                None
                            }
                        });
                    if let Some(rest) = found_name {
                        name = rest
                            .split(|c: char| c == ' ' || c == '{' || c == '<' || c == ':')
                            .next()
                            .unwrap_or("")
                            .to_string();
                    }
                }
                for ch in lines[i].chars() {
                    if ch == '{' { brace_depth += 1; }
                    if ch == '}' { brace_depth -= 1; }
                }
                block_lines.push(lines[i].as_str());
                if brace_depth <= 0 && block_lines.len() > 1 {
                    i += 1;
                    break;
                }
                i += 1;
            }
            blocks.push(StructBlock {
                name,
                start_line: start,
                end_line: i.saturating_sub(1),
                text: block_lines.join("\n"),
            });
        } else {
            i += 1;
        }
    }
    blocks
}

fn classify_type_color(type_str: &str) -> egui::Color32 {
    let t = type_str.trim();
    // Rust wrappers
    if t.starts_with("Vec<") || t.starts_with('[') {
        CatppuccinMocha::YELLOW
    } else if t.starts_with("Option<") || t.ends_with('?') {
        CatppuccinMocha::FLAMINGO
    } else if matches!(t, "i64" | "u64" | "f64" | "i32" | "u32" | "f32" | "usize" | "Int" | "Double") {
        CatppuccinMocha::PEACH
    } else if t == "String" {
        CatppuccinMocha::GREEN
    } else if matches!(t, "bool" | "Bool") {
        CatppuccinMocha::MAUVE
    } else {
        CatppuccinMocha::LAVENDER
    }
}

/// Recursively collect string values per JSON field name, plus occurrence counts.
fn collect_string_values(
    value: &serde_json::Value,
    out: &mut BTreeMap<String, BTreeSet<String>>,
    counts: &mut BTreeMap<String, usize>,
) {
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                if let serde_json::Value::String(s) = val {
                    out.entry(key.clone()).or_default().insert(s.clone());
                    *counts.entry(key.clone()).or_default() += 1;
                }
                collect_string_values(val, out, counts);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_string_values(item, out, counts);
            }
        }
        _ => {}
    }
}

/// Convert a string value to a valid Rust enum variant name (PascalCase).
fn to_enum_variant(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for ch in s.chars() {
        if ch == '_' || ch == '-' || ch == ' ' || ch == '.' || ch == '/' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }
    if result.is_empty() {
        "Unknown".to_string()
    } else if result.chars().next().unwrap().is_ascii_digit() {
        format!("V{}", result)
    } else {
        result
    }
}

/// Convert a field name to a PascalCase enum type name.
fn field_to_enum_name(field_name: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = true;
    for ch in field_name.chars() {
        if ch == '_' || ch == '-' {
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

/// Heuristic: does this string look like an enum variant?
/// Short, no newlines, looks like an identifier or code.
fn looks_like_enum_value(s: &str) -> bool {
    if s.is_empty() || s.len() > 60 {
        return false;
    }
    // Allow PascalCase, camelCase, SCREAMING_SNAKE, kebab-case, or short codes
    let has_only_valid = s.chars().all(|c| {
        c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.' || c == '/'
    });
    if has_only_valid {
        return true;
    }
    // Allow short phrases (≤3 words) like "In Progress"
    let words: Vec<&str> = s.split_whitespace().collect();
    words.len() <= 3 && words.iter().all(|w| w.len() <= 20)
}

/// Filter hidden fields from a struct block text for copying.
fn filter_hidden_fields_from_block(
    block_text: &str,
    struct_name: &str,
    hidden_fields: &BTreeSet<String>,
) -> String {
    let mut result = Vec::new();
    let mut skip_serde_rename = false;
    for line in block_text.lines() {
        let trimmed = line.trim();
        // Check if this is a field line: "pub field: Type," (Rust) or "let field: Type" (Swift)
        let field_name = if trimmed.starts_with("pub ") && trimmed.contains(':') {
            trimmed.strip_prefix("pub ")
                .and_then(|s| s.split(':').next())
                .map(|s| s.trim().trim_start_matches("r#"))
        } else if trimmed.starts_with("let ") && trimmed.contains(':') {
            trimmed.strip_prefix("let ")
                .and_then(|s| s.split(':').next())
                .map(|s| s.trim().trim_start_matches('`').trim_end_matches('`'))
        } else {
            None
        };
        if let Some(fname) = field_name {
            let key = format!("{}.{}", struct_name, fname);
            if hidden_fields.contains(&key) {
                skip_serde_rename = false;
                continue;
            }
        }
        // Skip #[serde(rename = ...)] lines that precede a hidden field
        if trimmed.starts_with("#[serde(rename") {
            skip_serde_rename = true;
            result.push(line.to_string());
            continue;
        }
        if skip_serde_rename {
            skip_serde_rename = false;
            let next_field = if trimmed.starts_with("pub ") && trimmed.contains(':') {
                trimmed.strip_prefix("pub ")
                    .and_then(|s| s.split(':').next())
                    .map(|s| s.trim().trim_start_matches("r#"))
            } else {
                None
            };
            if let Some(fname) = next_field {
                let key = format!("{}.{}", struct_name, fname);
                if hidden_fields.contains(&key) {
                    result.pop();
                    continue;
                }
            }
        }
        result.push(line.to_string());
    }
    result.join("\n")
}

/// Filter hidden fields from an entire generated file for "Copy All".
fn filter_hidden_fields_from_code(
    file: &GeneratedFile,
    hidden_fields: &BTreeSet<String>,
) -> String {
    if hidden_fields.is_empty() {
        return file.code.clone();
    }
    let mut result = Vec::new();
    let mut skip_serde_rename = false;

    for (i, line) in file.lines.iter().enumerate() {
        let trimmed = line.trim();

        // Track which struct we're in
        let current_struct: Option<&str> = file
            .structs
            .iter()
            .find(|s| i >= s.start_line && i <= s.end_line)
            .map(|s| s.name.as_str());

        if let Some(struct_name) = current_struct {
            // Detect field lines for both Rust and Swift
            let field_name_opt = if trimmed.starts_with("pub ") && trimmed.contains(':') && !trimmed.contains("struct ") {
                trimmed.strip_prefix("pub ")
                    .and_then(|s| s.split(':').next())
                    .map(|s| s.trim().trim_start_matches("r#"))
            } else if trimmed.starts_with("let ") && trimmed.contains(':') {
                trimmed.strip_prefix("let ")
                    .and_then(|s| s.split(':').next())
                    .map(|s| s.trim().trim_start_matches('`').trim_end_matches('`'))
            } else {
                None
            };
            if let Some(field_name) = field_name_opt {
                let key = format!("{}.{}", struct_name, field_name);
                if hidden_fields.contains(&key) {
                    if skip_serde_rename {
                        result.pop();
                    }
                    skip_serde_rename = false;
                    continue;
                }
            }
            if trimmed.starts_with("#[serde(rename") {
                skip_serde_rename = true;
                result.push(line.clone());
                continue;
            }
        }
        skip_serde_rename = false;
        result.push(line.clone());
    }
    result.join("\n") + "\n"
}

