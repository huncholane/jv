use egui::{self, RichText};

use jv::schema::SchemaOverview;
use jv::session::{LoadedSession, SessionManager};
use jv::theme::CatppuccinMocha;
use jv::views::{browser::BrowserView, code::CodeView, schema_diagram::SchemaDiagramView, shared_browser::SharedBrowserView, table::TableView};

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppMode {
    Jv,
    Groups,
    Schema,
    Code,
    Settings,
}

const MODE_ORDER: [AppMode; 5] = [
    AppMode::Jv,
    AppMode::Groups,
    AppMode::Schema,
    AppMode::Code,
    AppMode::Settings,
];

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ViewTab {
    Table,
    Code,
}

pub struct JvApp {
    session_manager: SessionManager,
    current_session: Option<LoadedSession>,
    selected_file_index: usize,
    active_mode: AppMode,
    active_tab: ViewTab,

    // Views
    table_view: TableView,
    code_view: CodeView,
    browser_view: BrowserView,
    schema_diagram_view: SchemaDiagramView,
    shared_browser_view: SharedBrowserView,

    applied_threshold: f32,

    // Track file changes
    last_file_index: usize,
    last_file_count: usize,

    // Disabled (toggled-off) source files by index
    disabled_files: std::collections::BTreeSet<usize>,
    // Cached active files — avoids cloning every frame
    cached_active_files: Vec<(String, serde_json::Value)>,
    cached_active_key: u64,

    // UI state
    new_session_name: String,
    show_new_session_dialog: bool,
    sidebar_width: f32,
    sidebar_visible: bool,
    theme_applied: bool,

    // File drop handling
    dropped_files: Vec<egui::DroppedFile>,

    // FPS tracking
    frame_times: std::collections::VecDeque<f64>,
    fps_display: f64,

    // Async file dialog results
    file_dialog_rx: Option<std::sync::mpsc::Receiver<Vec<ImportedFile>>>,

    // Async sample fetch from GitHub
    samples_rx: Option<std::sync::mpsc::Receiver<Vec<SampleFile>>>,
}

struct ImportedFile {
    filename: String,
    content: String,
    source: jv::session::FileSource,
}

/// Fetched sample file from GitHub
struct SampleFile {
    filename: String,
    content: String,
}

const SAMPLES_API_URL: &str =
    "https://api.github.com/repos/huncholane/jv/contents/samples/public";

/// Fetch public sample files from GitHub in a background thread.
fn fetch_github_samples(
    tx: std::sync::mpsc::Sender<Vec<SampleFile>>,
    ctx: egui::Context,
) {
    std::thread::spawn(move || {
        let result = (|| -> Result<Vec<SampleFile>, Box<dyn std::error::Error + Send + Sync>> {
            let listing = ureq::get(SAMPLES_API_URL)
                .header("User-Agent", "jv-json-viewer")
                .call()?
                .body_mut()
                .read_to_string()?;
            let body: serde_json::Value = serde_json::from_str(&listing)?;

            let entries = body.as_array().ok_or("expected array from GitHub API")?;
            let mut files = Vec::new();

            for entry in entries {
                let name = entry["name"].as_str().unwrap_or("");
                let download_url = entry["download_url"].as_str().unwrap_or("");

                // Only fetch .json and .har files
                if !(name.ends_with(".json") || name.ends_with(".har")) || download_url.is_empty() {
                    continue;
                }

                match ureq::get(download_url)
                    .header("User-Agent", "jv-json-viewer")
                    .call()
                {
                    Ok(mut resp) => {
                        if let Ok(content) = resp.body_mut().read_to_string() {
                            files.push(SampleFile {
                                filename: name.to_string(),
                                content,
                            });
                        }
                    }
                    Err(e) => {
                        tracing::warn!("Failed to fetch sample {}: {}", name, e);
                    }
                }
            }

            Ok(files)
        })();

        match result {
            Ok(files) => {
                tracing::info!("Fetched {} sample files from GitHub", files.len());
                let _ = tx.send(files);
            }
            Err(e) => {
                tracing::warn!("Failed to fetch samples from GitHub: {}", e);
                let _ = tx.send(Vec::new());
            }
        }
        ctx.request_repaint();
    });
}

/// Read a single file into ImportedFile(s). HAR files may produce multiple entries.
fn read_single_file(path: &std::path::Path) -> Option<Vec<ImportedFile>> {
    let content = std::fs::read_to_string(path).ok()?;
    let is_har = path.extension().is_some_and(|e| e == "har");
    if is_har {
        let har_value: serde_json::Value = serde_json::from_str(&content).ok()?;
        let files = jv::har::extract_har_files(&har_value);
        Some(files.into_iter().map(|(filename, value)| {
            ImportedFile {
                filename,
                content: serde_json::to_string_pretty(&value).unwrap_or_default(),
                source: jv::session::FileSource::Har,
            }
        }).collect())
    } else {
        let filename = path.file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("unknown.json")
            .to_string();
        Some(vec![ImportedFile {
            filename,
            content,
            source: jv::session::FileSource::Json,
        }])
    }
}

/// Read all JSON/HAR files from a directory tree.
fn read_directory_files(dir: &std::path::Path) -> Vec<ImportedFile> {
    let mut result = Vec::new();
    for entry in walkdir::WalkDir::new(dir)
        .into_iter()
        .filter_map(|e| e.ok())
    {
        let path = entry.path();
        if path.is_file() && path.extension().is_some_and(|e| e == "json" || e == "har") {
            if let Some(files) = read_single_file(path) {
                result.extend(files);
            }
        }
    }
    result
}

impl JvApp {
    pub fn new(cc: &eframe::CreationContext<'_>) -> Self {
        // Load Phosphor icon font
        let mut fonts = egui::FontDefinitions::default();
        egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
        cc.egui_ctx.set_fonts(fonts);
        egui_extras::install_image_loaders(&cc.egui_ctx);

        let mut manager = SessionManager::new();

        // Auto-create a default session if none exist
        let mut samples_rx = None;
        let current = if manager.sessions.is_empty() {
            let session = manager.create_session("Default Session");
            Some(LoadedSession::new(session))
        } else {
            // Restore last active session, or fall back to first
            let session = if let Some(last_id) = &manager.last_session_id {
                manager.sessions.iter().find(|s| s.id == *last_id)
                    .cloned()
                    .unwrap_or_else(|| manager.sessions[0].clone())
            } else {
                manager.sessions[0].clone()
            };
            Some(LoadedSession::new(session))
        };

        // If the Default Session has no files, fetch samples from GitHub
        if let Some(loaded) = &current {
            if loaded.session.name == "Default Session" && loaded.session.files.is_empty() {
                let (tx, rx) = std::sync::mpsc::channel();
                fetch_github_samples(tx, cc.egui_ctx.clone());
                samples_rx = Some(rx);
            }
        }

        let mut app = Self {
            session_manager: manager,
            current_session: current,
            selected_file_index: 0,
            active_mode: AppMode::Jv,
            active_tab: ViewTab::Table,
            table_view: TableView::new(),
            code_view: CodeView::new(),
            browser_view: BrowserView::new(),
            schema_diagram_view: SchemaDiagramView::new(),
            shared_browser_view: SharedBrowserView::new(),
            applied_threshold: 0.0,
            last_file_index: usize::MAX,
            last_file_count: 0,
            disabled_files: std::collections::BTreeSet::new(),
            cached_active_files: Vec::new(),
            cached_active_key: u64::MAX,
            new_session_name: String::new(),
            show_new_session_dialog: false,
            sidebar_width: 240.0,
            sidebar_visible: true,
            theme_applied: false,
            dropped_files: Vec::new(),
            frame_times: std::collections::VecDeque::with_capacity(60),
            fps_display: 0.0,
            file_dialog_rx: None,
            samples_rx,
        };
        if let Some(loaded) = &app.current_session {
            app.applied_threshold = loaded.session.jaccard_threshold;
            app.browser_view.load_focus_list(&loaded.session.focus_list);
        }
        tracing::info!("UI loaded");
        app
    }

    /// Get only the enabled (non-disabled) parsed files
    fn ensure_active_files_cache(&mut self) {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let file_count = self.current_session.as_ref()
            .map(|l| l.parsed_files.len()).unwrap_or(0);
        file_count.hash(&mut h);
        self.disabled_files.len().hash(&mut h);
        for &i in &self.disabled_files {
            i.hash(&mut h);
        }
        let key = h.finish();
        if self.cached_active_key != key {
            self.cached_active_files = if let Some(loaded) = &self.current_session {
                loaded.parsed_files.iter().enumerate()
                    .filter(|(i, _)| !self.disabled_files.contains(i))
                    .map(|(_, f)| f.clone())
                    .collect()
            } else {
                Vec::new()
            };
            self.cached_active_key = key;
        }
    }

    fn rebuild_schema(&mut self) {
        if let Some(loaded) = &mut self.current_session {
            // Rebuild schema using only enabled files
            let active: Vec<(String, serde_json::Value)> = loaded
                .parsed_files
                .iter()
                .enumerate()
                .filter(|(i, _)| !self.disabled_files.contains(i))
                .map(|(_, f)| f.clone())
                .collect();
            if !active.is_empty() {
                let threshold = loaded.session.jaccard_threshold;
                loaded.schema = Some(SchemaOverview::infer(&active, threshold));
            } else {
                loaded.schema = None;
            }
            self.applied_threshold = loaded.session.jaccard_threshold;
        }
    }

    fn show_sidebar(&mut self, ui: &mut egui::Ui) {
        ui.vertical(|ui| {
            use egui_phosphor::regular;

            // -- Session section --
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} ", regular::FOLDER_OPEN))
                        .color(CatppuccinMocha::OVERLAY0)
                        .size(12.0),
                );
                ui.label(
                    RichText::new("SESSION")
                        .color(CatppuccinMocha::OVERLAY0)
                        .small()
                        .strong(),
                );
            });
            ui.add_space(4.0);

            let session_names: Vec<String> = self
                .session_manager
                .sessions
                .iter()
                .map(|s| s.name.clone())
                .collect();

            let current_name = self
                .current_session
                .as_ref()
                .map(|s| s.session.name.clone())
                .unwrap_or_default();

            ui.horizontal(|ui| {
                egui::ComboBox::from_id_salt("session_selector")
                    .selected_text(&current_name)
                    .width(ui.available_width() - 52.0)
                    .show_ui(ui, |ui| {
                        for (i, name) in session_names.iter().enumerate() {
                            if ui.selectable_label(name == &current_name, name).clicked() {
                                let session = self.session_manager.sessions[i].clone();
                                self.session_manager.save_last_session_id(&session.id);
                                self.browser_view.load_focus_list(&session.focus_list);
                                self.current_session = Some(LoadedSession::new(session));
                                self.selected_file_index = 0;
                                self.rebuild_schema();
                            }
                        }
                    });

                if ui.add(egui::Button::new(
                    RichText::new(regular::PLUS).size(12.0),
                ).frame(false)).on_hover_text("New Session").clicked() {
                    self.show_new_session_dialog = true;
                }
                if ui.add(egui::Button::new(
                    RichText::new(regular::TRASH).color(CatppuccinMocha::SURFACE2).size(12.0),
                ).frame(false)).on_hover_text("Delete Session").clicked() {
                    if let Some(loaded) = &self.current_session {
                        let id = loaded.session.id.clone();
                        self.session_manager.delete_session(&id);
                        let new_session = if self.session_manager.sessions.is_empty() {
                            self.session_manager.create_session("Default Session")
                        } else {
                            self.session_manager.sessions[0].clone()
                        };
                        self.session_manager.save_last_session_id(&new_session.id);
                        self.current_session = Some(LoadedSession::new(new_session));
                        self.selected_file_index = 0;
                        self.rebuild_schema();
                    }
                }
            });

            ui.add_space(16.0);

            // -- Source files section (always visible) --
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(format!("{} ", regular::FILES))
                        .color(CatppuccinMocha::OVERLAY0)
                        .size(12.0),
                );
                ui.label(
                    RichText::new("SOURCE FILES")
                        .color(CatppuccinMocha::OVERLAY0)
                        .small()
                        .strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // Import buttons
                    if ui.add(egui::Button::new(
                        RichText::new(regular::FOLDER_PLUS).size(12.0),
                    ).frame(false)).on_hover_text("Import Directory").clicked() && self.file_dialog_rx.is_none() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            if let Some(dir) = rfd::FileDialog::new().pick_folder() {
                                let _ = tx.send(read_directory_files(&dir));
                                ctx.request_repaint();
                            }
                        });
                        self.file_dialog_rx = Some(rx);
                    }
                    if ui.add(egui::Button::new(
                        RichText::new(regular::FILE_PLUS).size(12.0),
                    ).frame(false)).on_hover_text("Import File").clicked() && self.file_dialog_rx.is_none() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        let ctx = ui.ctx().clone();
                        std::thread::spawn(move || {
                            if let Some(paths) = rfd::FileDialog::new()
                                .add_filter("JSON & HAR", &["json", "har"])
                                .pick_files()
                            {
                                let files = paths.iter().filter_map(|p| read_single_file(p)).flatten().collect();
                                let _ = tx.send(files);
                                ctx.request_repaint();
                            }
                        });
                        self.file_dialog_rx = Some(rx);
                    }
                });
            });

            // Select all / none row
            let file_count = self.current_session.as_ref().map(|l| l.session.files.len()).unwrap_or(0);
            if file_count > 0 {
                let all_enabled = self.disabled_files.is_empty();
                let all_disabled = self.disabled_files.len() >= file_count;
                ui.horizontal(|ui| {
                    let all_color = if all_enabled { CatppuccinMocha::OVERLAY0 } else { CatppuccinMocha::SUBTEXT0 };
                    let none_color = if all_disabled { CatppuccinMocha::OVERLAY0 } else { CatppuccinMocha::SUBTEXT0 };
                    if ui.add(egui::Button::new(
                        RichText::new("All").color(all_color).size(11.0),
                    ).frame(false)).clicked() && !all_enabled {
                        self.disabled_files.clear();
                        self.rebuild_schema();
                        self.code_view.invalidate();
                        self.shared_browser_view.invalidate();
                        self.schema_diagram_view.invalidate();
                        self.browser_view.invalidate();
                    }
                    ui.label(RichText::new("·").color(CatppuccinMocha::SURFACE2).size(11.0));
                    if ui.add(egui::Button::new(
                        RichText::new("None").color(none_color).size(11.0),
                    ).frame(false)).clicked() && !all_disabled {
                        self.disabled_files = (0..file_count).collect();
                        self.rebuild_schema();
                        self.code_view.invalidate();
                        self.shared_browser_view.invalidate();
                        self.schema_diagram_view.invalidate();
                        self.browser_view.invalidate();
                    }
                    ui.label(
                        RichText::new(format!("{}/{}", file_count - self.disabled_files.len(), file_count))
                            .color(CatppuccinMocha::OVERLAY0)
                            .family(egui::FontFamily::Monospace)
                            .size(10.0),
                    );
                });
            }
            ui.add_space(4.0);

            let mut to_remove = None;
            let mut toggled: Option<usize> = None;

            if let Some(loaded) = &self.current_session {
                let mut files: Vec<(usize, String, jv::session::FileSource)> = loaded
                    .session
                    .files
                    .iter()
                    .enumerate()
                    .map(|(i, f)| (i, f.filename.clone(), f.source))
                    .collect();
                files.sort_by(|a, b| a.1.to_lowercase().cmp(&b.1.to_lowercase()));

                egui::ScrollArea::vertical()
                    .id_salt("file_list")
                    .show(ui, |ui| {
                        for (idx, filename, source) in &files {
                            let is_disabled = self.disabled_files.contains(idx);

                            let (is_hovered, row_id) = jv::widgets::check_hover(ui.ctx(), ui.id().with(("file_row", idx)));

                            let bg = if is_hovered {
                                egui::Color32::from_rgba_premultiplied(40, 40, 55, 255)
                            } else {
                                egui::Color32::TRANSPARENT
                            };

                            let eye_icon = if is_disabled {
                                egui_phosphor::regular::EYE_CLOSED
                            } else {
                                egui_phosphor::regular::EYE
                            };
                            let eye_color = if is_disabled {
                                CatppuccinMocha::SURFACE2
                            } else if is_hovered {
                                CatppuccinMocha::GREEN
                            } else {
                                CatppuccinMocha::OVERLAY0
                            };
                            let text_color = if is_disabled {
                                CatppuccinMocha::OVERLAY0
                            } else if is_hovered {
                                CatppuccinMocha::TEXT
                            } else {
                                CatppuccinMocha::SUBTEXT0
                            };

                            let mut trash_rect = egui::Rect::NOTHING;
                            let r = egui::Frame::new()
                                .fill(bg)
                                .corner_radius(6.0)
                                .inner_margin(egui::Margin::symmetric(8, 3))
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        // Trash icon
                                        let (trash_hovered, trash_id) = jv::widgets::check_hover(ui.ctx(), ui.id().with(("trash", idx)));
                                        let trash_color = if trash_hovered {
                                            CatppuccinMocha::RED
                                        } else {
                                            CatppuccinMocha::SURFACE2
                                        };
                                        let rm = ui.label(
                                            RichText::new(regular::TRASH)
                                                .color(trash_color)
                                                .size(12.0),
                                        );
                                        jv::widgets::store_hover(ui.ctx(), trash_id, rm.hovered());
                                        trash_rect = rm.rect;

                                        // Eye icon
                                        ui.label(
                                            RichText::new(eye_icon)
                                                .color(eye_color)
                                                .size(13.0),
                                        );
                                        ui.add_space(2.0);

                                        // Strip extension for display
                                        let display_name = filename
                                            .strip_suffix(".json")
                                            .or_else(|| filename.strip_suffix(".har"))
                                            .unwrap_or(filename);

                                        // Filename without extension
                                        ui.label(
                                            RichText::new(display_name)
                                                .color(text_color)
                                                .size(12.0),
                                        );

                                        // Source tag pill pushed to the right
                                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                            let (tag_label, tag_bg) = match source {
                                                jv::session::FileSource::Har => ("har", CatppuccinMocha::MAUVE),
                                                jv::session::FileSource::Json => ("json", CatppuccinMocha::BLUE),
                                            };
                                            egui::Frame::new()
                                                .fill(tag_bg)
                                                .corner_radius(4.0)
                                                .inner_margin(egui::Margin::symmetric(4, 1))
                                                .show(ui, |ui| {
                                                    ui.label(
                                                        RichText::new(tag_label)
                                                            .color(CatppuccinMocha::CRUST)
                                                            .size(9.0)
                                                            .strong(),
                                                    );
                                                });
                                        });
                                    });
                                })
                                .response
                                .interact(egui::Sense::click());

                            jv::widgets::store_hover(ui.ctx(), row_id, r.hovered());

                            if r.clicked() {
                                if let Some(pos) = ui.input(|i| i.pointer.interact_pos()) {
                                    if trash_rect.contains(pos) {
                                        to_remove = Some(*idx);
                                    } else {
                                        toggled = Some(*idx);
                                    }
                                }
                            }
                        }
                    });
            }

            // Handle toggle (outside borrow of current_session)
            if let Some(idx) = toggled {
                if self.disabled_files.contains(&idx) {
                    self.disabled_files.remove(&idx);
                } else {
                    self.disabled_files.insert(idx);
                }
                self.rebuild_schema();
                self.code_view.invalidate();
                self.shared_browser_view.invalidate();
                self.schema_diagram_view.invalidate();
                self.browser_view.invalidate();
            }

            if let Some(idx) = to_remove {
                if let Some(loaded) = &mut self.current_session {
                    loaded.remove_file(idx);
                    self.session_manager.update_session(&loaded.session);
                    // Clean up disabled_files indices
                    self.disabled_files.remove(&idx);
                    let new_disabled: std::collections::BTreeSet<usize> = self.disabled_files
                        .iter()
                        .map(|&i| if i > idx { i - 1 } else { i })
                        .collect();
                    self.disabled_files = new_disabled;
                    if self.selected_file_index >= loaded.session.files.len()
                        && !loaded.session.files.is_empty()
                    {
                        self.selected_file_index = loaded.session.files.len() - 1;
                    }
                    self.rebuild_schema();
                }
            }
        });
    }

    fn show_settings(&mut self, ui: &mut egui::Ui) {
        use egui_phosphor::regular;

        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                ui.add_space(8.0);

                // Schema section
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("{} Schema", regular::GRAPH))
                            .color(CatppuccinMocha::TEXT)
                            .size(14.0)
                            .strong(),
                    );
                });
                ui.add_space(8.0);

                // Jaccard similarity slider
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new("Similarity threshold")
                            .color(CatppuccinMocha::SUBTEXT0)
                            .size(12.0),
                    );
                });
                ui.add_space(2.0);
                {
                    let mut val = self
                        .current_session
                        .as_ref()
                        .map(|l| l.session.jaccard_threshold)
                        .unwrap_or(0.8);
                    let response = ui.horizontal(|ui| {
                        ui.add_space(8.0);
                        ui.add(
                            egui::Slider::new(&mut val, 0.3..=1.0)
                                .step_by(0.05)
                                .custom_formatter(|v, _| format!("{:.0}%", v * 100.0)),
                        )
                    }).inner;
                    if let Some(loaded) = &mut self.current_session {
                        loaded.session.jaccard_threshold = val;
                    }
                    let pending = (val - self.applied_threshold).abs() > f32::EPSILON;
                    if pending && !response.dragged() {
                        self.applied_threshold = val;
                        self.session_manager.update_session(
                            &self.current_session.as_ref().unwrap().session,
                        );
                        self.rebuild_schema();
                        self.code_view.invalidate();
                        self.shared_browser_view.invalidate();
                    }
                }
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(
                            "Controls how similar two object shapes must be to merge into the same struct. Higher values require more identical fields.",
                        )
                        .color(CatppuccinMocha::OVERLAY0)
                        .size(11.0),
                    );
                });
            });
    }

    fn show_main_content(&mut self, ui: &mut egui::Ui) {
        use egui_phosphor::regular;

        // Mode switcher
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            let jv_icon = if self.browser_view.focus_mode {
                regular::STAR
            } else {
                regular::BROWSERS
            };
            for (mode, label, icon) in [
                (AppMode::Jv, "jv", jv_icon),
                (AppMode::Groups, "Groups", regular::TREE_STRUCTURE),
                (AppMode::Schema, "Schema", regular::GRAPH),
                (AppMode::Code, "Code", regular::CODE),
                (AppMode::Settings, "Settings", regular::GEAR_SIX),
            ] {
                let selected = self.active_mode == mode;
                let text_color = if selected {
                    CatppuccinMocha::BLUE
                } else {
                    CatppuccinMocha::OVERLAY0
                };

                let btn = ui.add(
                    egui::Button::new(
                        RichText::new(format!(" {} {} ", icon, label))
                            .color(text_color)
                            .size(12.0),
                    )
                    .fill(if selected {
                        CatppuccinMocha::SURFACE0
                    } else {
                        egui::Color32::TRANSPARENT
                    })
                    .corner_radius(6.0),
                );

                if btn.clicked() && self.active_mode != mode {
                    self.active_mode = mode;
                    self.active_tab = match mode {
                        AppMode::Jv => ViewTab::Table,
                        AppMode::Schema => ViewTab::Table,
                        AppMode::Groups => ViewTab::Table,
                        AppMode::Code => ViewTab::Code,
                        AppMode::Settings => ViewTab::Table,
                    };
                }
            }
        });

        // Ctrl-H/L: switch between modes
        let ctrl_l = ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::L));
        let ctrl_h = ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::H));
        if ctrl_l || ctrl_h {
            let cur = MODE_ORDER.iter().position(|m| *m == self.active_mode).unwrap_or(0);
            let next = if ctrl_l {
                (cur + 1) % MODE_ORDER.len()
            } else {
                (cur + MODE_ORDER.len() - 1) % MODE_ORDER.len()
            };
            let mode = MODE_ORDER[next];
            self.active_mode = mode;
            self.active_tab = match mode {
                AppMode::Jv => ViewTab::Table,
                AppMode::Schema => ViewTab::Table,
                AppMode::Groups => ViewTab::Table,
                AppMode::Code => ViewTab::Code,
                AppMode::Settings => ViewTab::Table,
            };
        }

        ui.add_space(2.0);

        // Tab bar (conditional per mode)
        let tabs: Vec<(ViewTab, &str, &str)> = match self.active_mode {
            AppMode::Jv => vec![],
            AppMode::Schema => vec![],
            AppMode::Groups => vec![],
            AppMode::Code => vec![],
            AppMode::Settings => vec![],
        };

        if !tabs.is_empty() {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                for (tab, label, icon) in &tabs {
                    let selected = self.active_tab == *tab;
                    let text_color = if selected {
                        CatppuccinMocha::TEXT
                    } else {
                        CatppuccinMocha::OVERLAY0
                    };

                    let btn = ui.add(
                        egui::Button::new(
                            RichText::new(format!(" {} {} ", icon, label))
                                .color(text_color)
                                .size(13.0),
                        )
                        .fill(egui::Color32::TRANSPARENT)
                        .corner_radius(6.0),
                    );

                    if selected {
                        let r = btn.rect;
                        ui.painter().rect_filled(
                            egui::Rect::from_min_max(
                                egui::pos2(r.left() + 8.0, r.bottom() - 2.0),
                                egui::pos2(r.right() - 8.0, r.bottom()),
                            ),
                            1.0,
                            CatppuccinMocha::BLUE,
                        );
                    }

                    if btn.clicked() {
                        self.active_tab = *tab;
                    }
                }
            });
        }

        // Subtle divider under tabs
        let sep_rect = ui.available_rect_before_wrap();
        ui.painter().line_segment(
            [
                egui::pos2(sep_rect.left(), sep_rect.top()),
                egui::pos2(sep_rect.right(), sep_rect.top()),
            ],
            egui::Stroke::new(1.0, CatppuccinMocha::SURFACE0),
        );
        ui.add_space(8.0);

        // Content — routed by mode
        match self.active_mode {
            AppMode::Schema => {
                let has_schema = self.current_session.as_ref()
                    .and_then(|l| l.schema.as_ref())
                    .is_some_and(|s| !s.structs.is_empty() || !s.unique_structs.is_empty());
                if !has_schema {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("Import files to see schema diagram")
                                .color(CatppuccinMocha::OVERLAY0)
                                .size(16.0),
                        );
                    });
                    return;
                }
                self.ensure_active_files_cache();
                let active_files = &self.cached_active_files;
                let loaded = self.current_session.as_ref().unwrap();
                let schema = loaded.schema.as_ref().unwrap();
                let structs = schema.structs.clone();
                let unique_structs = schema.unique_structs.clone();
                let enum_conversions = loaded.session.enum_conversions.clone();
                let hidden_fields = loaded.session.hidden_fields.clone();
                self.schema_diagram_view.show(
                    ui,
                    &active_files,
                    &structs,
                    &unique_structs,
                    &enum_conversions,
                    &hidden_fields,
                );
            }
            AppMode::Jv => {
                let has_file = self
                    .current_session
                    .as_ref()
                    .is_some_and(|l| !l.parsed_files.is_empty());

                if !has_file {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new(
                                "Import a JSON file to get started\n\nDrag & drop files here, or use the Import buttons in the sidebar",
                            )
                            .color(CatppuccinMocha::OVERLAY0)
                            .size(16.0),
                        );
                    });
                    return;
                }

                {
                    let loaded = self.current_session.as_ref().unwrap();
                    let file_idx = self.selected_file_index.min(loaded.parsed_files.len().saturating_sub(1));
                    let file_count = loaded.parsed_files.len();

                    if file_count != self.last_file_count {
                        self.browser_view.invalidate();
                        self.code_view.invalidate();
                        self.schema_diagram_view.invalidate();
                        self.last_file_count = file_count;
                    }
                    self.last_file_index = file_idx;

                    self.browser_view.show(ui, &loaded.parsed_files);
                }

                // Save focus list if it changed
                if self.browser_view.take_focus_dirty() {
                    if let Some(loaded) = &mut self.current_session {
                        loaded.session.focus_list = self.browser_view.save_focus_list();
                        self.session_manager.update_session(&loaded.session);
                    }
                }

                // Sync sidebar selection from browser's current file
                if let Some(loaded) = &self.current_session {
                    if let Some(file_key) = self.browser_view.current_file_key(&loaded.parsed_files) {
                        if let Some(idx) = loaded.parsed_files.iter().position(|(n, _)| {
                            n.strip_suffix(".json").unwrap_or(n) == file_key
                        }) {
                            self.selected_file_index = idx;
                        }
                    }
                }
            }
            AppMode::Groups => {
                let loaded = self.current_session.as_ref();
                let has_schema = loaded
                    .and_then(|l| l.schema.as_ref())
                    .is_some_and(|s| !s.structs.is_empty() || !s.unique_structs.is_empty());
                if !has_schema {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("Import multiple files to see shared types")
                                .color(CatppuccinMocha::OVERLAY0)
                                .size(16.0),
                        );
                    });
                    return;
                }

                self.ensure_active_files_cache();
                let active_files = &self.cached_active_files;
                let loaded = self.current_session.as_ref().unwrap();
                let schema = loaded.schema.as_ref().unwrap();
                self.shared_browser_view.show(ui, schema, &active_files);
            }
            AppMode::Settings => {
                self.show_settings(ui);
            }
            AppMode::Code => {
                let has_schema = self.current_session.as_ref()
                    .and_then(|l| l.schema.as_ref())
                    .is_some_and(|s| !s.structs.is_empty() || !s.unique_structs.is_empty());
                if !has_schema {
                    ui.centered_and_justified(|ui| {
                        ui.label(
                            RichText::new("Import files to generate code")
                                .color(CatppuccinMocha::OVERLAY0)
                                .size(16.0),
                        );
                    });
                    return;
                }
                self.ensure_active_files_cache();
                let active_files = &self.cached_active_files;
                let loaded = self.current_session.as_ref().unwrap();
                let schema = loaded.schema.clone();
                let prev_enums = loaded.session.enum_conversions.clone();
                let prev_hidden_count = loaded.session.hidden_fields.len();
                let loaded = self.current_session.as_mut().unwrap();
                self.code_view.show(
                    ui,
                    &active_files,
                    schema.as_ref(),
                    &mut loaded.session.enum_conversions,
                    &mut loaded.session.hidden_fields,
                );
                // Persist session if enum conversions or hidden fields changed
                if loaded.session.enum_conversions != prev_enums
                    || loaded.session.hidden_fields.len() != prev_hidden_count
                {
                    self.session_manager.update_session(&loaded.session);
                }
            }
        }
    }

    fn handle_dropped_files(&mut self, ctx: &egui::Context) {
        // Collect dropped files
        ctx.input(|i| {
            for f in &i.raw.dropped_files {
                self.dropped_files.push(f.clone());
            }
        });

        // Process drops
        let files: Vec<egui::DroppedFile> = self.dropped_files.drain(..).collect();
        for file in files {
            if let Some(path) = &file.path {
                let imported = if path.is_dir() {
                    read_directory_files(path)
                } else if path.extension().is_some_and(|e| e == "json" || e == "har") {
                    read_single_file(path).unwrap_or_default()
                } else {
                    Vec::new()
                };
                if let Some(loaded) = &mut self.current_session {
                    for f in imported {
                        if loaded.add_file(&f.filename, f.content, f.source).is_ok() {}
                    }
                    self.session_manager.update_session(&loaded.session);
                }
                self.rebuild_schema();
            } else if let Some(bytes) = &file.bytes {
                let content = String::from_utf8_lossy(bytes).to_string();
                let name = file.name.clone();
                if let Some(loaded) = &mut self.current_session {
                    let source = if name.ends_with(".har") {
                        jv::session::FileSource::Har
                    } else {
                        jv::session::FileSource::Json
                    };
                    if loaded.add_file(&name, content, source).is_ok() {
                        self.session_manager.update_session(&loaded.session);
                        self.rebuild_schema();
                    }
                }
            }
        }
    }
}

impl eframe::App for JvApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Poll async file dialog results
        if let Some(rx) = &self.file_dialog_rx {
            match rx.try_recv() {
                Ok(files) => {
                    if let Some(loaded) = &mut self.current_session {
                        for f in files {
                            if loaded.add_file(&f.filename, f.content, f.source).is_ok() {}
                        }
                        self.session_manager.update_session(&loaded.session);
                    }
                    self.rebuild_schema();
                    self.file_dialog_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.file_dialog_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        // Poll for GitHub sample files
        if let Some(rx) = &self.samples_rx {
            match rx.try_recv() {
                Ok(files) => {
                    if let Some(loaded) = &mut self.current_session {
                        for sample in &files {
                            let is_har = sample.filename.ends_with(".har");
                            if is_har {
                                if let Ok(har_value) = serde_json::from_str::<serde_json::Value>(&sample.content) {
                                    let extracted = jv::har::extract_har_files(&har_value);
                                    for (filename, value) in &extracted {
                                        let json_str = serde_json::to_string_pretty(value).unwrap_or_default();
                                        let _ = loaded.add_file(filename, json_str, jv::session::FileSource::Har);
                                    }
                                }
                            } else {
                                let _ = loaded.add_file(&sample.filename, sample.content.clone(), jv::session::FileSource::Json);
                            }
                        }
                        self.session_manager.update_session(&loaded.session);
                    }
                    self.rebuild_schema();
                    self.samples_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.samples_rx = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }

        if !self.theme_applied {
            CatppuccinMocha::apply(ctx);
            ctx.style_mut(|style| {
                style.spacing.scroll.floating = false;
                style.spacing.scroll.bar_width = 8.0;
                style.interaction.tooltip_delay = 0.0;
            });
            self.theme_applied = true;
        }

        // Boost scroll speed by scaling raw scroll events
        ctx.input_mut(|input| {
            for event in &mut input.events {
                if let egui::Event::MouseWheel { delta, .. } = event {
                    delta.y *= 3.0;
                }
            }
        });

        self.handle_dropped_files(ctx);

        // FPS tracking (rolling average of last 60 frames)
        let dt = ctx.input(|i| i.stable_dt) as f64;
        self.frame_times.push_back(dt);
        if self.frame_times.len() > 60 {
            self.frame_times.pop_front();
        }
        if !self.frame_times.is_empty() {
            let avg_dt: f64 = self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64;
            self.fps_display = if avg_dt > 0.0 { 1.0 / avg_dt } else { 0.0 };
        }

        // FPS overlay — subtle pill in top-right
        egui::Area::new(egui::Id::new("fps_overlay"))
            .fixed_pos(egui::pos2(ctx.screen_rect().right() - 72.0, 6.0))
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(CatppuccinMocha::SURFACE0)
                    .corner_radius(10.0)
                    .inner_margin(egui::Margin::symmetric(8, 2))
                    .show(ui, |ui| {
                        let fps = self.fps_display;
                        let color = if fps >= 55.0 {
                            CatppuccinMocha::GREEN
                        } else if fps >= 30.0 {
                            CatppuccinMocha::YELLOW
                        } else {
                            CatppuccinMocha::RED
                        };
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing.x = 3.0;
                            ui.label(
                                RichText::new(format!("{:.0}", fps))
                                    .color(color)
                                    .family(egui::FontFamily::Monospace)
                                    .size(10.0),
                            );
                            ui.label(
                                RichText::new("fps")
                                    .color(CatppuccinMocha::OVERLAY0)
                                    .family(egui::FontFamily::Monospace)
                                    .size(10.0),
                            );
                        });
                    });
            });

        // New session dialog
        if self.show_new_session_dialog {
            egui::Window::new("New Session")
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("Name:");
                        ui.text_edit_singleline(&mut self.new_session_name);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Create").clicked() && !self.new_session_name.is_empty() {
                            let session = self
                                .session_manager
                                .create_session(&self.new_session_name);
                            self.session_manager.save_last_session_id(&session.id);
                            self.current_session = Some(LoadedSession::new(session));
                            self.selected_file_index = 0;
                            self.new_session_name.clear();
                            self.show_new_session_dialog = false;
                        }
                        if ui.button("Cancel").clicked() {
                            self.new_session_name.clear();
                            self.show_new_session_dialog = false;
                        }
                    });
                });
        }

        // Ctrl-B: toggle sidebar
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::B)) {
            self.sidebar_visible = !self.sidebar_visible;
        }

        // Sidebar
        if self.sidebar_visible {
            egui::SidePanel::left("sidebar")
                .default_width(self.sidebar_width)
                .min_width(180.0)
                .max_width(400.0)
                .resizable(true)
                .frame(egui::Frame::new().fill(CatppuccinMocha::MANTLE).inner_margin(12.0))
                .show(ctx, |ui| {
                    self.show_sidebar(ui);
                });
        }

        // Main content
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(CatppuccinMocha::BASE)
                    .inner_margin(16.0),
            )
            .show(ctx, |ui| {
                self.show_main_content(ui);
            });
    }
}
