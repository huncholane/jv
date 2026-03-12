# HAR Import & Multi-Language Codegen

## Overview

Two features for jv:

1. **HAR file import** — extract JSON responses from HAR files as synthetic session files with intelligent names
2. **Swift code generation** — language abstraction layer with Rust and Swift as first two implementations, toggle in code view

---

## Feature 1: HAR File Import

### Parsing

HAR files are JSON with structure `{ log: { entries: [...] } }`. Each entry has `request` (method, url, queryString, postData) and `response` (status, content with mimeType and text).

Extract entries where:
- `response.content.mimeType` contains `"json"`
- `response.content.text` parses as valid `serde_json::Value`
- Response status is 2xx
- Skip entries where `response.content.encoding` is `"base64"` (compressed bodies — not worth decoding)

No limit on extracted entry count. Users can remove unwanted files from the session afterward.

### File Naming

Build filename from URL path + param values. Use simple `split('/')` URL parsing (no `url` crate needed — HAR URLs are well-formed).

1. Parse URL path, extract last meaningful path segment:
   - Strip segments matching: `api`, `rest`, or version patterns (`v1`, `v2`, ... i.e. `v\d+`)
   - Skip numeric-only segments (IDs like `/users/42/`)
   - Use the last remaining non-numeric segment as base name
2. Collect short distinguishing values:
   - GET: query string param values
   - POST/PUT: short string values from JSON body (top-level only)
   - Include values that are: strings ≤ 15 chars, or date-like
   - Exclude: values > 15 chars, values where >50% of characters are hex digits, UUIDs (`[0-9a-f]{8}-`...), purely numeric values
3. Join: `{endpoint}_{val1}_{val2}.json`
4. Deduplicate with `_2`, `_3` counters on collision

Examples:
- `GET /api/v2/flights?origin=LAX&date=2026-03-01` → `flights_LAX_2026-03-01.json`
- `POST /auth/login` body `{"username":"alice"}` → `login_alice.json`
- `GET /users/42/bookings?status=confirmed` → `bookings_confirmed.json`
- `GET /health` → `health.json`
- Two identical names → `flights_LAX.json`, `flights_LAX_2.json`

### Integration

- Extend the import file dialog to accept `.har` files alongside `.json`
- When a `.har` file is detected (by extension or by checking for `log.entries` structure), parse and extract synthetic files
- Each extracted JSON response becomes a `SessionFile` in the current session
- The `original_path` field stores the HAR filename + entry index for provenance
- The rest of the pipeline (schema inference, codegen) works unchanged

### New Code

- `src/har.rs` — HAR parsing, file naming logic
  - `pub fn extract_har_files(har_json: &Value) -> Vec<(String, Value)>` — returns `(filename, json_body)` pairs
  - Internal: `fn name_from_request(entry: &Value) -> String`
  - Internal: `fn extract_path_segment(url: &str) -> String`
  - Internal: `fn collect_distinguishing_values(entry: &Value) -> Vec<String>`
  - Internal: `fn is_hex_like(s: &str) -> bool` — >50% hex digits and len > 15, or UUID pattern

### Modified Code

- `src/app.rs` — `import_file()` checks extension, delegates to `har::extract_har_files()` when `.har`
- `src/main.rs` — add `mod har;`

---

## Feature 2: Multi-Language Code Generation

### Language Abstraction

New enum and trait in `src/lang/mod.rs`:

```rust
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CodeLanguage {
    Rust,
    Swift,
}

pub trait LanguageGenerator {
    fn file_extension(&self) -> &str;
    fn file_header(&self) -> String;                    // #![allow(...)] for Rust, empty for Swift
    fn imports_header(&self, needs_temporal: bool, has_shared: bool) -> String;
    fn struct_open(&self, name: &str) -> String;        // "pub struct Foo {" or "struct Foo: Codable {"
    fn struct_close(&self, fields: &[(String, String)]) -> String; // fields=(code_name, json_name) for CodingKeys
    fn field_line(&self, code_name: &str, type_name: &str, json_name: &str) -> String;
    fn enum_open(&self, name: &str) -> String;
    fn enum_close(&self) -> String;
    fn enum_variant(&self, variant_name: &str, json_value: &str) -> String;
    fn type_name(&self, inferred: &InferredType) -> String;
    fn mod_file(&self, file_names: &[&str]) -> Option<String>; // None for Swift
    fn sanitize_keyword(&self, name: &str) -> String;
    fn field_name(&self, json_name: &str) -> String;    // snake_case for Rust, camelCase for Swift
}
```

**Key design decisions:**
- `struct_close()` takes field mappings so Swift can emit CodingKeys before the closing brace. Rust ignores this parameter.
- `field_name()` handles language-specific naming: Rust keeps snake_case, Swift converts to camelCase.
- `type_name()` replaces `InferredType::rust_type()` for all codegen. The existing `rust_type()` method remains for non-codegen display (e.g., schema table tooltip) but codegen always goes through the trait.

### Code generation flow

`CodeGenerator::generate_code()` is refactored to accept `&dyn LanguageGenerator`:

```
generate_code(lang):
  1. lang.file_header()           → "#![allow(non_snake_case)]" or ""
  2. lang.imports_header(...)     → "use serde::..." or "import Foundation"
  3. For each struct:
     a. lang.struct_open(name)    → "pub struct Foo {" or "struct Foo: Codable {"
     b. For each field:
        - code_name = lang.field_name(json_name)
        - type_str  = lang.type_name(inferred_type)
        - lang.field_line(code_name, type_str, json_name)
     c. lang.struct_close(fields) → "}" or CodingKeys block + "}"
  4. For each enum:
     a. lang.enum_open(name)
     b. For each variant: lang.enum_variant(variant, json_value)
     c. lang.enum_close()
```

### Refactoring `GeneratedField`

The existing `GeneratedField` struct has Rust-specific field names (`rust_name`, `rust_type`). Refactor to language-neutral:

```rust
pub struct GeneratedField {
    pub code_name: String,    // was rust_name — language-specific (snake_case or camelCase)
    pub json_name: String,    // unchanged — original JSON key
    pub code_type: String,    // was rust_type — language-specific type string
    pub needs_rename: bool,   // code_name != json_name
}
```

### Rust Generator

`src/lang/rust.rs` — extracts existing logic from `codegen.rs`:
- `type_name`: same mapping as current `InferredType::rust_type()`
- `field_name`: `to_snake_case()` (moved from codegen.rs)
- `sanitize_keyword`: `r#` prefix for Rust reserved words
- `struct_close`: just `"}\n"` (ignores field mappings)
- `mod_file`: generates `mod X; pub use X::*;` pattern

### Swift Generator

`src/lang/swift.rs`

Type mapping:
| InferredType | Swift Type |
|---|---|
| Null | Any? |
| Bool | Bool |
| I64 | Int |
| F64 | Double |
| String | String |
| DateTime | Date |
| Date | Date |
| Time | Date |
| Array(T) | [T] |
| Object(fields) | Named struct (same as Rust — recursive struct generation) |
| Object (fallback) | [String: Any] |
| Option(T) | T? |
| Mixed | Any |
| Unknown | Any |

- `field_name`: `to_camel_case()` — new utility function (e.g. `departure_date` → `departureDate`)
- `sanitize_keyword`: backtick escaping for Swift reserved words (`class`, `struct`, `enum`, `protocol`, `func`, `var`, `let`, `import`, `return`, `self`, `super`, `default`, `case`, `switch`, `where`, `in`, `is`, `as`, `try`, `throw`, `throws`, `nil`, `true`, `false`, `do`, `catch`, `guard`, `defer`, `repeat`, `break`, `continue`, `fallthrough`, `typealias`, `associatedtype`, `operator`, `init`, `deinit`, `subscript`, `private`, `public`, `internal`, `open`, `fileprivate`, `static`, `override`, `mutating`, `inout`, `Any`, `Type`, `Self`)
- `struct_close(fields)`: emits `CodingKeys` enum block before `}` when any field's camelCase name differs from its JSON key
- `mod_file`: returns `None`
- `enum_open`: `enum FooBar: String, Codable {`
- `enum_variant`: `case variantName = "json_value"` (or just `case variantName` when equal)

Swift enum output:
```swift
enum SeatClass: String, Codable {
    case economy
    case business
    case firstClass = "first_class"
}
```

Swift struct output:
```swift
import Foundation

struct Flight: Codable {
    let origin: String
    let departureDate: Date
    let seatAbbreviation: String
    let passengers: [Passenger]?

    enum CodingKeys: String, CodingKey {
        case origin
        case departureDate = "departure_date"
        case seatAbbreviation = "seat_abbreviation"
        case passengers
    }
}
```

### Code View Changes

**Language toggle:** Segmented button in top bar (`Rust | Swift`). Stored as `selected_language: CodeLanguage` in `CodeView`.

**Cache invalidation:** Changing language sets `cache_key = 0`, triggering full rebuild with the new generator.

**File list:** `rebuild_file_mode` uses language for file extension and naming:
- Rust: mod.rs, shared.rs, enums.rs, {group}.rs
- Swift: Shared.swift, Enums.swift, {Group}.swift (no mod file, PascalCase filenames)

**Syntax highlighting:** `render_rust_line` becomes `render_code_line`, language-aware. The rendering logic splits into shared vs language-specific parts:

Shared (both languages):
- Search match highlighting (background + accent bar)
- Line numbers
- Struct block copy buttons
- Clickable type navigation
- Enum convert/revert buttons
- Field hide/show toggles
- Enum usage tooltips

Language-specific (driven by `CodeLanguage`):
- Keyword detection: `pub struct` / `pub enum` / `use` (Rust) vs `struct` / `enum` / `let` / `import` / `case` (Swift)
- Field line parsing: `pub field: Type,` (Rust) vs `let field: Type` (Swift)
- Attribute lines: `#[derive(...)]` (Rust) vs none (Swift — `: Codable` is on the struct line)
- CodingKeys block: rendered like attributes in YELLOW

**Download:** Uses correct extension per language.

### File Structure

```
src/
├── lang/
│   ├── mod.rs         # CodeLanguage enum, LanguageGenerator trait
│   ├── rust.rs        # RustGenerator
│   └── swift.rs       # SwiftGenerator
├── har.rs             # HAR parsing + naming
├── codegen.rs         # CodeGenerator uses &dyn LanguageGenerator
└── views/code.rs      # Language toggle, language-aware rendering
```

### What stays in `codegen.rs`

`CodeGenerator` struct and its struct/field collection logic (`collect_structs`, `from_value`, `from_schema`) stay in `codegen.rs`. These are language-independent — they walk JSON and produce `GeneratedStruct`/`GeneratedField` lists. Only the formatting/output methods (`generate_code`, type resolution, keyword sanitization) delegate to the `LanguageGenerator` trait.

Utility functions `to_pascal_case`, `singularize`, `first_normal_word`, `make_unique_name` stay in `codegen.rs` as they're language-independent.

### Modified Files

- `src/codegen.rs` — `CodeGenerator` takes `&dyn LanguageGenerator`, `GeneratedField` uses neutral names, `generate_code(lang)` delegates formatting
- `src/types.rs` — `InferredType::rust_type()` kept for non-codegen display; codegen uses `lang.type_name()`
- `src/views/code.rs` — language toggle UI, `rebuild_file_mode()` uses selected language, `render_code_line()` replaces `render_rust_line()`
- `src/main.rs` — add `mod lang; mod har;`

---

## Out of Scope

- SwiftLint disable comments
- Other languages beyond Rust and Swift (architecture supports it, implementation deferred)
- HAR request body extraction (only responses)
- HAR filtering UI (all valid JSON responses extracted)
