# jv

A native desktop JSON viewer with intelligent schema inference, cross-file comparison, and code generation.

Built with Rust and [egui](https://github.com/emilk/egui), themed with Catppuccin Mocha.

## Features

### Four Modes

- **Jv** — Browse individual JSON files with a tree view, table view, and jq filter execution
- **Schema** — Interactive entity relationship diagram showing inferred structs and their connections across files
- **Groups** — Miller-column view grouping common structures and strings across loaded files
- **Code** — Auto-generated Rust and Swift struct definitions with proper serde/Codable attributes

### Smart Schema Inference

- Detects shared data structures across multiple JSON files using Jaccard similarity on field sets
- Merges similar structs with configurable threshold (default 80%)
- Depluralization and singularization for struct naming
- Handles optional fields, mixed types, and nested objects

### File Support

- JSON files
- HAR (HTTP Archive) files with automatic response body extraction
- Batch directory import
- Toggle individual source files on/off to dynamically rebuild schema

### Session Based

- Create multiple independent sessions, each with their own set of loaded files and configuration
- Sessions persist across app restarts — reopen and pick up where you left off
- Switch between sessions to compare different datasets side by side

### Rich Data Handling

- Automatic temporal type detection (ISO 8601 dates, times, Unix timestamps)
- Timezone-aware formatting with relative time display
- Enum variant consolidation from string fields

## Build

Requires Rust nightly:

```bash
cargo build --release
```

## Run

```bash
cargo run
```

Or after building:

```bash
./target/release/jv
```

## Private Tests

Tests in `tests/private/` are gitignored so contributors can write tests against their own JSON/HAR data without risking exposing personal or proprietary information to the repository. The test files are included into the main test suite via `include!()` macros — they compile and run locally but never get committed.

To use private tests, add `.json` or `.har` files to `private_samples/` and write test functions in `tests/private/`. Both directories are gitignored.

## Dependencies

| Crate | Purpose |
|---|---|
| `eframe` / `egui` 0.31 | Native desktop UI framework |
| `egui_extras` | Syntax highlighting, images, SVGs |
| `egui-phosphor` | Icon font |
| `serde_json` | JSON parsing |
| `jaq-*` | jq query language execution |
| `chrono` / `chrono-tz` | Temporal types with timezone support |
| `rfd` | Native file dialogs |
| `walkdir` | Directory traversal |

## License

MIT
