# jv Mode Restructure — Design Spec

## Overview

Restructure jv from a flat 4-tab layout into three top-level modes (File, Shared, All), each with its own sub-tabs and sidebar behavior.

## Modes

### File Mode
- **Sub-tabs**: Table, JSON, Rust, jq
- **Sidebar**: SESSION + FILES list + SHARED TYPES (current behavior)
- **Behavior**: Exactly as today. Per-file views.

### Shared Mode
- **Sub-tabs**: Table, Rust
- **Sidebar**: SESSION + scrollable list of shared struct names (clickable to focus/highlight in main content)
- **Table view**: Expandable sections, one per shared struct. Each expands to a field comparison matrix — files as columns, fields as rows, cells show inferred type or `—` if absent.
- **Rust view**: Only shared struct codegen (from `CodeGenerator::from_schema`). Same Rust view component, fed only shared structs.

### All Mode
- **Sub-tabs**: Rust only (no tab bar shown)
- **Sidebar**: SESSION + scrollable list of struct names for navigation
- **Rust view**: Full codegen combining per-file structs + shared structs, deduplicated. One complete Rust output.

## UI Layout

Mode switcher renders as pill buttons **above the tab bar** in the main content panel. Two rows in main panel header:
1. Mode pills: `[ File ] [ Shared ] [ All ]`
2. Sub-tabs for current mode (hidden for All mode since it only has one view)

## Data Model Changes

### New enums
```rust
enum AppMode {
    File,
    Shared,
    All,
}
```

`ViewTab` stays but is contextual per mode:
- File mode: Table, Json, Rust, Jq
- Shared mode: Table, Rust
- All mode: Rust (implicit, no selector)

### State
- `active_mode: AppMode` added to `JvApp`
- `active_tab` resets to first available tab when mode changes

## File Changes

### `src/app.rs`
- Add `AppMode` enum
- Add `active_mode` field to `JvApp`
- `show_main_content()`: render mode pills above tabs, conditionally show sub-tabs per mode
- `show_sidebar()`: branch on `active_mode` for sidebar content
- File mode sidebar: current behavior
- Shared/All mode sidebar: SESSION section + struct name list

### `src/views/table.rs`
- Add `show_schema_matrix()` method
- Takes `&[SharedStruct]` and list of filenames
- Renders expandable sections per struct
- Each section: files as columns, fields as rows, cells = inferred type or `—`

### `src/views/rust.rs`
- Already has `show_schema()` for shared structs — reuse for Shared mode
- Add `show_all()` method for All mode: generates combined codegen from all files + schema

### No changes needed
- `src/views/json.rs`
- `src/views/jq.rs`
- `src/schema.rs`
- `src/codegen.rs`
- `src/session.rs`
- `src/theme.rs`
