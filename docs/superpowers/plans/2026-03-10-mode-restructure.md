# Mode Restructure Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Restructure jv from flat 4-tab layout into three modes (File/Shared/All) with per-mode sub-tabs and adaptive sidebar.

**Architecture:** Add `AppMode` enum alongside existing `ViewTab`. Mode switcher renders as pills above the tab bar. Each mode filters which tabs are available and what the sidebar shows. Shared mode gets a new schema matrix table view. All mode gets a combined Rust codegen view.

**Tech Stack:** Rust, egui 0.31, egui-phosphor 0.9, existing schema/codegen infrastructure.

---

## Chunk 1: Core Mode Switching

### Task 1: Add AppMode enum and state

**Files:**
- Modify: `src/app.rs:8-14` (enums) and `src/app.rs:16-47` (struct fields)

- [ ] **Step 1: Add AppMode enum and field**

Add above the existing `ViewTab` enum in `src/app.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum AppMode {
    File,
    Shared,
    All,
}
```

Add to `JvApp` struct:

```rust
active_mode: AppMode,
```

Initialize in `new()`:

```rust
active_mode: AppMode::File,
```

- [ ] **Step 2: Build and verify it compiles**

Run: `cargo build 2>&1 | grep "^error"`
Expected: no errors

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: add AppMode enum (File/Shared/All)"
```

### Task 2: Render mode switcher pills above tab bar

**Files:**
- Modify: `src/app.rs` — `show_main_content()` method (around line 420)

- [ ] **Step 1: Add mode switcher UI**

In `show_main_content()`, before the existing tab bar `ui.horizontal`, add:

```rust
// Mode switcher
ui.horizontal(|ui| {
    ui.spacing_mut().item_spacing.x = 4.0;
    for (mode, label, icon) in [
        (AppMode::File, "File", regular::FILE),
        (AppMode::Shared, "Shared", regular::TREE_STRUCTURE),
        (AppMode::All, "All", regular::CODE),
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
            // Reset to first available tab for the new mode
            self.active_tab = match mode {
                AppMode::File => ViewTab::Table,
                AppMode::Shared => ViewTab::Table,
                AppMode::All => ViewTab::Rust,
            };
        }
    }
});
ui.add_space(2.0);
```

- [ ] **Step 2: Filter tab bar by active mode**

Replace the existing tab loop array with a conditional:

```rust
let tabs: Vec<(ViewTab, &str, &str)> = match self.active_mode {
    AppMode::File => vec![
        (ViewTab::Table, "Table", regular::TABLE),
        (ViewTab::Json, "JSON", regular::BRACKETS_CURLY),
        (ViewTab::Rust, "Rust", regular::GEAR),
        (ViewTab::Jq, "jq", regular::FUNNEL),
    ],
    AppMode::Shared => vec![
        (ViewTab::Table, "Table", regular::TABLE),
        (ViewTab::Rust, "Rust", regular::GEAR),
    ],
    AppMode::All => vec![],  // No tab bar for All mode
};
```

Only render the tab bar horizontal block if `!tabs.is_empty()`.

- [ ] **Step 3: Build and verify**

Run: `cargo build 2>&1 | grep "^error"`
Expected: no errors

- [ ] **Step 4: Commit**

```bash
git add src/app.rs
git commit -m "feat: render mode switcher pills and filter tabs per mode"
```

### Task 3: Route content rendering by mode

**Files:**
- Modify: `src/app.rs` — `show_main_content()` content section (around line 509-535)

- [ ] **Step 1: Branch content by mode**

Replace the single `match self.active_tab` block with mode-aware routing:

```rust
match self.active_mode {
    AppMode::File => {
        // Existing per-file content (current match block)
        // Requires a selected file - show empty state if none
        let has_file = self.current_session
            .as_ref()
            .is_some_and(|l| !l.parsed_files.is_empty());
        if !has_file {
            // existing empty state UI
            return;
        }
        // ... existing file change detection + match self.active_tab
    }
    AppMode::Shared => {
        let has_schema = self.schema.as_ref().is_some_and(|s| !s.structs.is_empty());
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
        let structs = self.schema.as_ref().unwrap().structs.clone();
        match self.active_tab {
            ViewTab::Table => {
                // TODO: Task 5 — schema matrix view
                ui.label("Schema matrix — coming soon");
            }
            ViewTab::Rust => {
                self.rust_view.show_schema(ui, &structs);
            }
            _ => {}
        }
    }
    AppMode::All => {
        let has_schema = self.schema.as_ref().is_some_and(|s| !s.structs.is_empty());
        if !has_schema {
            ui.centered_and_justified(|ui| {
                ui.label(
                    RichText::new("Import files to generate Rust code")
                        .color(CatppuccinMocha::OVERLAY0)
                        .size(16.0),
                );
            });
            return;
        }
        // All mode: show_schema with all structs
        let structs = self.schema.as_ref().unwrap().structs.clone();
        self.rust_view.show_schema(ui, &structs);
    }
}
```

- [ ] **Step 2: Build, launch, verify mode switching works**

Run: `cargo build && cargo run`
Expected: clicking File/Shared/All switches modes, tabs update accordingly

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: route content rendering by active mode"
```

### Task 4: Adaptive sidebar per mode

**Files:**
- Modify: `src/app.rs` — `show_sidebar()` method

- [ ] **Step 1: Branch sidebar by mode**

Wrap the existing sidebar content in a mode check. The SESSION section stays in all modes. Then branch:

```rust
// SESSION section — always shown (existing code for session selector + buttons)
// ...

ui.add_space(16.0);

match self.active_mode {
    AppMode::File => {
        // Existing FILES section + file list + SHARED TYPES section
        // (move current code here)
    }
    AppMode::Shared | AppMode::All => {
        // STRUCTS section — scrollable list of struct names
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(format!("{} ", regular::TREE_STRUCTURE))
                    .color(CatppuccinMocha::OVERLAY0)
                    .size(12.0),
            );
            ui.label(
                RichText::new("STRUCTS")
                    .color(CatppuccinMocha::OVERLAY0)
                    .small()
                    .strong(),
            );
        });
        ui.add_space(4.0);

        if let Some(schema) = &self.schema {
            egui::ScrollArea::vertical()
                .id_salt("struct_list_scroll")
                .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
                .auto_shrink(false)
                .show(ui, |ui| {
                    for s in &schema.structs {
                        let label = format!(
                            "{} ({} fields, {}/{})",
                            s.name,
                            s.fields.len(),
                            s.source_files.len(),
                            self.current_session
                                .as_ref()
                                .map(|l| l.session.files.len())
                                .unwrap_or(0),
                        );
                        ui.add(
                            egui::Label::new(
                                RichText::new(&label)
                                    .color(CatppuccinMocha::LAVENDER)
                                    .size(12.0),
                            ),
                        );
                    }
                });
        } else {
            ui.label(
                RichText::new("Import files to see structs")
                    .color(CatppuccinMocha::OVERLAY0)
                    .small(),
            );
        }
    }
}
```

- [ ] **Step 2: Build, launch, verify sidebar changes per mode**

Run: `cargo build && cargo run`
Expected: File mode shows file list; Shared/All show struct list

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "feat: adaptive sidebar per mode (files vs struct list)"
```

## Chunk 2: Shared Mode Schema Matrix Table

### Task 5: Add schema matrix view to TableView

**Files:**
- Modify: `src/views/table.rs` — add `show_schema_matrix()` method

- [ ] **Step 1: Add the show_schema_matrix method**

Add a new public method to `TableView`:

```rust
pub fn show_schema_matrix(
    &mut self,
    ui: &mut Ui,
    structs: &[crate::schema::SharedStruct],
    filenames: &[String],
) {
    egui::ScrollArea::both()
        .id_salt("schema_matrix_scroll")
        .scroll_bar_visibility(egui::scroll_area::ScrollBarVisibility::AlwaysVisible)
        .auto_shrink(false)
        .show(ui, |ui| {
            for shared in structs {
                let id = ui.id().with(&shared.name);
                let is_expanded = self.expanded.contains(&shared.name);

                // Struct header row
                ui.horizontal(|ui| {
                    let arrow = if is_expanded {
                        egui_phosphor::regular::CARET_DOWN
                    } else {
                        egui_phosphor::regular::CARET_RIGHT
                    };
                    let header = format!(
                        "{} {} — {} fields, in {}/{} files",
                        arrow,
                        shared.name,
                        shared.fields.len(),
                        shared.source_files.len(),
                        filenames.len(),
                    );
                    let r = ui.add(
                        egui::Label::new(
                            RichText::new(&header)
                                .color(CatppuccinMocha::LAVENDER)
                                .strong()
                                .size(14.0),
                        )
                        .sense(egui::Sense::click()),
                    );
                    if r.clicked() {
                        if is_expanded {
                            self.expanded.remove(&shared.name);
                        } else {
                            self.expanded.insert(shared.name.clone());
                        }
                    }
                });

                if is_expanded {
                    ui.add_space(4.0);

                    // Build columns: Field + one per source file
                    let num_cols = shared.source_files.len() + 1;

                    egui::Grid::new(id)
                        .num_columns(num_cols)
                        .striped(true)
                        .spacing([12.0, 4.0])
                        .show(ui, |ui| {
                            // Header row
                            ui.label(
                                RichText::new("Field")
                                    .color(CatppuccinMocha::BLUE)
                                    .strong(),
                            );
                            for file in &shared.source_files {
                                ui.label(
                                    RichText::new(file)
                                        .color(CatppuccinMocha::YELLOW)
                                        .strong()
                                        .size(11.0),
                                );
                            }
                            ui.end_row();

                            // Field rows
                            for (field_name, field_type) in &shared.fields {
                                ui.label(
                                    RichText::new(field_name)
                                        .color(CatppuccinMocha::BLUE),
                                );
                                // For each source file, show the type or —
                                for _file in &shared.source_files {
                                    // All source files have this struct, show the merged type
                                    let type_str = field_type.display_name();
                                    let color = crate::theme::type_color(&type_str);
                                    ui.label(
                                        RichText::new(&type_str).color(color),
                                    );
                                }
                                ui.end_row();
                            }
                        });

                    ui.add_space(12.0);
                }
            }
        });
}
```

- [ ] **Step 2: Wire it up in app.rs Shared mode**

Replace the `"Schema matrix — coming soon"` placeholder in `show_main_content()`:

```rust
ViewTab::Table => {
    let filenames: Vec<String> = self.current_session
        .as_ref()
        .map(|l| l.session.files.iter().map(|f| f.filename.clone()).collect())
        .unwrap_or_default();
    self.table_view.show_schema_matrix(ui, &structs, &filenames);
}
```

- [ ] **Step 3: Check InferredType has display_name**

Verify `InferredType` in `src/types.rs` has a `display_name()` method. If not, add one that returns the Rust type string.

- [ ] **Step 4: Build, launch, test Shared mode Table**

Run: `cargo build && cargo run`
Expected: Shared > Table shows expandable struct sections with file columns

- [ ] **Step 5: Commit**

```bash
git add src/views/table.rs src/app.rs
git commit -m "feat: schema matrix table view for Shared mode"
```

## Chunk 3: Polish and All Mode

### Task 6: All mode combined Rust codegen

**Files:**
- Modify: `src/views/rust.rs` — add `show_all()` method
- Modify: `src/app.rs` — All mode content routing

- [ ] **Step 1: Add show_all method to RustView**

The All mode should show all structs from the schema. The existing `show_schema()` already does this — it generates code from all SharedStructs. So All mode can just reuse `show_schema()` directly.

Verify in `app.rs` that All mode already calls `self.rust_view.show_schema(ui, &structs)` — it should from Task 3.

If additional per-file structs are needed that aren't in the shared schema, add a `show_all()` that combines both. For now, reusing `show_schema()` is sufficient.

- [ ] **Step 2: Build and verify All mode**

Run: `cargo build && cargo run`
Expected: All mode shows full Rust codegen, no tab bar

- [ ] **Step 3: Commit**

```bash
git add src/app.rs src/views/rust.rs
git commit -m "feat: All mode shows combined Rust codegen"
```

### Task 7: Clean up and remove old Shared Types sidebar section

**Files:**
- Modify: `src/app.rs` — remove the SHARED TYPES section from File mode sidebar (it's now its own mode)

- [ ] **Step 1: Remove SHARED TYPES from File mode sidebar**

In the File mode branch of the sidebar, remove the "SHARED TYPES" header and the scrollable struct list at the bottom. This information now lives in Shared/All mode sidebars.

- [ ] **Step 2: Build, launch, verify File mode sidebar is cleaner**

Run: `cargo build && cargo run`
Expected: File mode sidebar only shows SESSION + FILES sections

- [ ] **Step 3: Commit**

```bash
git add src/app.rs
git commit -m "refactor: remove shared types from file mode sidebar (now in Shared mode)"
```

### Task 8: Final integration test

- [ ] **Step 1: Manual test all modes**

1. File mode: verify Table, JSON, Rust, jq all work per-file
2. Shared mode: verify Table shows schema matrix, Rust shows shared codegen
3. All mode: verify Rust shows full codegen, no tab bar
4. Mode switching: verify tabs reset, sidebar adapts
5. Session switching: verify schema rebuilds in all modes

- [ ] **Step 2: Take screenshots to verify UI**

```bash
grim -g "X,Y WxH" -s 2 /tmp/jv_final.png
```

- [ ] **Step 3: Final commit**

```bash
git add -A
git commit -m "feat: complete mode restructure (File/Shared/All)"
```
