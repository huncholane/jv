# HAR Import & Multi-Language Codegen Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add HAR file import with intelligent naming, and a language abstraction layer with Swift as the second codegen target alongside Rust.

**Architecture:** HAR parsing is a standalone module that produces `(filename, Value)` pairs fed into the existing session pipeline. Language codegen uses a `LanguageGenerator` trait implemented by `RustGenerator` and `SwiftGenerator`, with `CodeGenerator` delegating all formatting through the trait. The code view gets a language toggle that triggers full rebuild.

**Tech Stack:** Rust, egui 0.31, serde_json, rfd (file dialogs), no new crate dependencies

**Spec:** `docs/superpowers/specs/2026-03-11-har-import-and-swift-codegen-design.md`

---

## Chunk 1: HAR File Import

### Task 1: HAR Parsing Module

**Files:**
- Create: `src/har.rs`

- [ ] **Step 1: Write tests for HAR extraction**

Add `#[cfg(test)]` module at the bottom of `src/har.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_extract_har_files_basic() {
        let har = json!({
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://api.example.com/api/v2/flights?origin=LAX&date=2026-03-01",
                            "queryString": [
                                {"name": "origin", "value": "LAX"},
                                {"name": "date", "value": "2026-03-01"}
                            ]
                        },
                        "response": {
                            "status": 200,
                            "content": {
                                "mimeType": "application/json",
                                "text": "{\"id\": 1, \"origin\": \"LAX\"}"
                            }
                        }
                    }
                ]
            }
        });
        let files = extract_har_files(&har);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "flights_LAX_2026-03-01.json");
    }

    #[test]
    fn test_extract_har_files_post() {
        let har = json!({
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "POST",
                            "url": "https://api.example.com/auth/login",
                            "queryString": [],
                            "postData": {
                                "mimeType": "application/json",
                                "text": "{\"username\": \"alice\"}"
                            }
                        },
                        "response": {
                            "status": 200,
                            "content": {
                                "mimeType": "application/json",
                                "text": "{\"token\": \"abc\"}"
                            }
                        }
                    }
                ]
            }
        });
        let files = extract_har_files(&har);
        assert_eq!(files.len(), 1);
        assert_eq!(files[0].0, "login_alice.json");
    }

    #[test]
    fn test_extract_har_skips_non_json() {
        let har = json!({
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://example.com/image.png",
                            "queryString": []
                        },
                        "response": {
                            "status": 200,
                            "content": {
                                "mimeType": "image/png",
                                "text": ""
                            }
                        }
                    }
                ]
            }
        });
        let files = extract_har_files(&har);
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_extract_har_skips_non_2xx() {
        let har = json!({
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://example.com/api/data",
                            "queryString": []
                        },
                        "response": {
                            "status": 404,
                            "content": {
                                "mimeType": "application/json",
                                "text": "{\"error\": \"not found\"}"
                            }
                        }
                    }
                ]
            }
        });
        let files = extract_har_files(&har);
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_extract_har_skips_base64() {
        let har = json!({
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://example.com/api/data",
                            "queryString": []
                        },
                        "response": {
                            "status": 200,
                            "content": {
                                "mimeType": "application/json",
                                "encoding": "base64",
                                "text": "eyJpZCI6IDF9"
                            }
                        }
                    }
                ]
            }
        });
        let files = extract_har_files(&har);
        assert_eq!(files.len(), 0);
    }

    #[test]
    fn test_extract_path_segment() {
        assert_eq!(extract_path_segment("https://api.example.com/api/v2/flights"), "flights");
        assert_eq!(extract_path_segment("https://api.example.com/users/42/bookings"), "bookings");
        assert_eq!(extract_path_segment("https://api.example.com/health"), "health");
        assert_eq!(extract_path_segment("https://api.example.com/rest/v3/data"), "data");
    }

    #[test]
    fn test_is_hex_like() {
        assert!(is_hex_like("a1b2c3d4e5f6a7b8c9d0"));  // >50% hex, >15 chars
        assert!(is_hex_like("550e8400-e29b-41d4-a716-446655440000"));  // UUID
        assert!(!is_hex_like("LAX"));
        assert!(!is_hex_like("alice"));
        assert!(!is_hex_like("2026-03-01"));
    }

    #[test]
    fn test_deduplication() {
        let har = json!({
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://example.com/api/flights?origin=LAX",
                            "queryString": [{"name": "origin", "value": "LAX"}]
                        },
                        "response": {
                            "status": 200,
                            "content": {
                                "mimeType": "application/json",
                                "text": "{\"id\": 1}"
                            }
                        }
                    },
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://example.com/api/flights?origin=LAX",
                            "queryString": [{"name": "origin", "value": "LAX"}]
                        },
                        "response": {
                            "status": 200,
                            "content": {
                                "mimeType": "application/json",
                                "text": "{\"id\": 2}"
                            }
                        }
                    }
                ]
            }
        });
        let files = extract_har_files(&har);
        assert_eq!(files.len(), 2);
        assert_eq!(files[0].0, "flights_LAX.json");
        assert_eq!(files[1].0, "flights_LAX_2.json");
    }

    #[test]
    fn test_excludes_long_tokens() {
        let har = json!({
            "log": {
                "entries": [
                    {
                        "request": {
                            "method": "GET",
                            "url": "https://example.com/api/data?token=abcdef123456789012345&name=bob",
                            "queryString": [
                                {"name": "token", "value": "abcdef123456789012345"},
                                {"name": "name", "value": "bob"}
                            ]
                        },
                        "response": {
                            "status": 200,
                            "content": {
                                "mimeType": "application/json",
                                "text": "{\"ok\": true}"
                            }
                        }
                    }
                ]
            }
        });
        let files = extract_har_files(&har);
        assert_eq!(files[0].0, "data_bob.json");
    }
}
```

- [ ] **Step 2: Write the HAR module implementation**

Create `src/har.rs`:

```rust
use serde_json::Value;
use std::collections::BTreeSet;

/// Extract JSON response bodies from a HAR file, returning (filename, parsed_json) pairs.
pub fn extract_har_files(har: &Value) -> Vec<(String, Value)> {
    let entries = match har.get("log").and_then(|l| l.get("entries")).and_then(|e| e.as_array()) {
        Some(arr) => arr,
        None => return Vec::new(),
    };

    let mut results: Vec<(String, Value)> = Vec::new();
    let mut used_names: BTreeSet<String> = BTreeSet::new();

    for entry in entries {
        // Check response status is 2xx
        let status = entry
            .pointer("/response/status")
            .and_then(|s| s.as_u64())
            .unwrap_or(0);
        if status < 200 || status >= 300 {
            continue;
        }

        // Check mime type contains "json"
        let mime = entry
            .pointer("/response/content/mimeType")
            .and_then(|m| m.as_str())
            .unwrap_or("");
        if !mime.contains("json") {
            continue;
        }

        // Skip base64-encoded bodies
        let encoding = entry
            .pointer("/response/content/encoding")
            .and_then(|e| e.as_str())
            .unwrap_or("");
        if encoding == "base64" {
            continue;
        }

        // Parse response body
        let text = entry
            .pointer("/response/content/text")
            .and_then(|t| t.as_str())
            .unwrap_or("");
        let parsed: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Build filename
        let url = entry
            .pointer("/request/url")
            .and_then(|u| u.as_str())
            .unwrap_or("");
        let base = extract_path_segment(url);
        let values = collect_distinguishing_values(entry);

        let stem = if values.is_empty() {
            base.clone()
        } else {
            format!("{}_{}", base, values.join("_"))
        };

        // Deduplicate
        let filename = if used_names.contains(&stem) {
            let mut i = 2;
            let deduped = loop {
                let candidate = format!("{}_{}", stem, i);
                if !used_names.contains(&candidate) {
                    used_names.insert(candidate.clone());
                    break candidate;
                }
                i += 1;
            };
            format!("{}.json", deduped)
        } else {
            used_names.insert(stem.clone());
            format!("{}.json", stem)
        };

        results.push((filename, parsed));
    }

    results
}

/// Extract the last meaningful path segment from a URL.
/// Strips /api/, /rest/, /vN/ prefixes and numeric-only segments.
fn extract_path_segment(url: &str) -> String {
    // Split off query string and fragment
    let path_part = url.split('?').next().unwrap_or(url);
    let path_part = path_part.split('#').next().unwrap_or(path_part);

    // Strip scheme + host: find the path after ://host
    let path = if let Some(after_scheme) = path_part.strip_prefix("https://")
        .or_else(|| path_part.strip_prefix("http://"))
    {
        // Find first / after host
        after_scheme.find('/').map(|i| &after_scheme[i..]).unwrap_or("/")
    } else {
        path_part
    };

    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .filter(|s| {
            // Skip api, rest, version segments
            let lower = s.to_ascii_lowercase();
            lower != "api" && lower != "rest" && !is_version_segment(&lower)
        })
        .filter(|s| {
            // Skip numeric-only segments (IDs)
            !s.chars().all(|c| c.is_ascii_digit())
        })
        .collect();

    segments
        .last()
        .unwrap_or(&"response")
        .to_ascii_lowercase()
}

/// Collect short, distinguishing values from query params (GET) or POST body.
fn collect_distinguishing_values(entry: &Value) -> Vec<String> {
    let mut values = Vec::new();

    // Query string params
    if let Some(qs) = entry.pointer("/request/queryString").and_then(|q| q.as_array()) {
        for param in qs {
            if let Some(val) = param.get("value").and_then(|v| v.as_str()) {
                if is_useful_value(val) {
                    values.push(val.to_string());
                }
            }
        }
    }

    // POST body fields (top-level string values only)
    let method = entry
        .pointer("/request/method")
        .and_then(|m| m.as_str())
        .unwrap_or("");
    if matches!(method, "POST" | "PUT" | "PATCH") {
        if let Some(body_text) = entry
            .pointer("/request/postData/text")
            .and_then(|t| t.as_str())
        {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(body_text) {
                for (_, val) in &map {
                    if let Some(s) = val.as_str() {
                        if is_useful_value(s) {
                            values.push(s.to_string());
                        }
                    }
                }
            }
        }
    }

    values
}

fn is_version_segment(s: &str) -> bool {
    s.starts_with('v') && s[1..].chars().all(|c| c.is_ascii_digit()) && s.len() >= 2
}

fn is_useful_value(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 15
        && !s.chars().all(|c| c.is_ascii_digit())
        && !is_hex_like(s)
}

fn is_hex_like(s: &str) -> bool {
    // UUID pattern
    if s.len() == 36 && s.chars().filter(|c| *c == '-').count() == 4 {
        let without_dashes: String = s.chars().filter(|c| *c != '-').collect();
        if without_dashes.len() == 32 && without_dashes.chars().all(|c| c.is_ascii_hexdigit()) {
            return true;
        }
    }
    // >50% hex digits and length > 15
    if s.len() > 15 {
        let hex_count = s.chars().filter(|c| c.is_ascii_hexdigit()).count();
        return hex_count as f64 / s.len() as f64 > 0.5;
    }
    false
}
```

- [ ] **Step 3: Run tests to verify they pass**

Run: `cargo test --lib har`
Expected: All 8 tests pass

- [ ] **Step 4: Commit**

```bash
git add src/har.rs
git commit -m "feat: add HAR file parsing module with intelligent naming"
```

---

### Task 2: Integrate HAR Import Into App

**Files:**
- Modify: `src/main.rs:1-9` (add `mod har;`)
- Modify: `src/app.rs:115-137` (`import_file`, `import_directory`)
- Modify: `src/app.rs:274-284` (file dialog filters)

- [ ] **Step 1: Add `mod har` to `src/main.rs`**

Add after line 2 (`mod codegen;`):

```rust
mod har;
```

- [ ] **Step 2: Update `import_file()` in `src/app.rs` to handle HAR files**

First, check `src/session.rs` — `LoadedSession::add_file()` uses `PathBuf::from(path).file_name()` to derive the display filename. Since HAR synthetic paths like `"harfile.har:flights_LAX.json"` would produce wrong filenames (the colon is not a path separator on Linux), pass just the synthetic filename as the path:

Replace `import_file` (lines 115–125) with:

```rust
fn import_file(&mut self, path: &std::path::Path) {
    if let Ok(content) = std::fs::read_to_string(path) {
        let is_har = path.extension().is_some_and(|e| e == "har");
        if is_har {
            // Parse HAR and extract synthetic JSON files
            if let Ok(har_value) = serde_json::from_str::<serde_json::Value>(&content) {
                let files = crate::har::extract_har_files(&har_value);
                if let Some(loaded) = &mut self.current_session {
                    for (filename, value) in &files {
                        let json_str = serde_json::to_string_pretty(value).unwrap_or_default();
                        // Pass just the filename so add_file derives the correct display name
                        if loaded.add_file(filename, json_str).is_ok() {}
                    }
                    self.session_manager.update_session(&loaded.session);
                    self.rebuild_schema();
                }
            }
        } else {
            let path_str = path.to_string_lossy().to_string();
            if let Some(loaded) = &mut self.current_session {
                if loaded.add_file(&path_str, content).is_ok() {
                    self.session_manager.update_session(&loaded.session);
                    self.rebuild_schema();
                }
            }
        }
    }
}
```

- [ ] **Step 3: Update file dialog filter to accept HAR files**

Find the import file dialog (around line 281–284 in `app.rs`) and update the filter:

Change:
```rust
.add_filter("JSON", &["json"])
```
To:
```rust
.add_filter("JSON & HAR", &["json", "har"])
```

- [ ] **Step 4: Update `import_directory()` to also pick up `.har` files**

In `import_directory` (lines 127–137), change the extension filter:

```rust
if path.is_file() && path.extension().is_some_and(|e| e == "json" || e == "har") {
```

- [ ] **Step 5: Update drag-and-drop handler to accept `.har` files**

In `src/app.rs`, find `handle_dropped_files()` (around line 830) where it checks `path.extension().is_some_and(|e| e == "json")`. Update to also accept `.har`:

```rust
if path.is_file() && path.extension().is_some_and(|e| e == "json" || e == "har") {
```

- [ ] **Step 6: Build and verify**

Run: `cargo build`
Expected: Clean build (warnings OK)

- [ ] **Step 7: Commit**

```bash
git add src/main.rs src/app.rs src/har.rs
git commit -m "feat: integrate HAR file import into session pipeline"
```

---

## Chunk 2: Language Abstraction Layer

### Task 3: Create Language Trait and Rust Generator

**Files:**
- Create: `src/lang/mod.rs`
- Create: `src/lang/rust.rs`
- Modify: `src/main.rs` (add `mod lang;`)

- [ ] **Step 1: Create `src/lang/mod.rs` with trait and enum**

```rust
pub mod rust;
pub mod swift;

use crate::types::InferredType;

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum CodeLanguage {
    Rust,
    Swift,
}

impl CodeLanguage {
    pub fn display_name(&self) -> &str {
        match self {
            Self::Rust => "Rust",
            Self::Swift => "Swift",
        }
    }

    pub fn generator(&self) -> Box<dyn LanguageGenerator> {
        match self {
            Self::Rust => Box::new(rust::RustGenerator),
            Self::Swift => Box::new(swift::SwiftGenerator),
        }
    }
}

/// Language-neutral field info for code generation.
pub struct FieldInfo {
    pub code_name: String,
    pub json_name: String,
    pub code_type: String,
    pub needs_rename: bool,
}

pub trait LanguageGenerator {
    fn file_extension(&self) -> &str;
    fn file_header(&self) -> String;
    fn imports_header(&self, needs_temporal: bool, has_shared: bool) -> String;
    fn struct_open(&self, name: &str) -> String;
    /// Close a struct. `fields` is (code_name, json_name) pairs for CodingKeys etc.
    fn struct_close(&self, fields: &[(String, String)]) -> String;
    fn field_line(&self, code_name: &str, type_name: &str) -> String;
    fn enum_open(&self, name: &str) -> String;
    fn enum_close(&self) -> String;
    fn enum_variant(&self, variant_name: &str, json_value: &str) -> String;
    fn type_name(&self, inferred: &InferredType) -> String;
    fn mod_file(&self, file_names: &[&str]) -> Option<String>;
    fn sanitize_keyword(&self, name: &str) -> String;
    fn field_name(&self, json_name: &str) -> String;
    fn file_name(&self, base_name: &str) -> String;
}
```

- [ ] **Step 2: Create `src/lang/rust.rs`**

Note: `to_snake_case` already exists in `codegen.rs:186-208`. Make it `pub` there and import it in `rust.rs` via `use crate::codegen::to_snake_case;` instead of duplicating. Same for `to_pascal_case` if needed.

Extract existing logic from `codegen.rs`:

```rust
use crate::codegen::to_snake_case;
use crate::lang::LanguageGenerator;
use crate::types::InferredType;

pub struct RustGenerator;

impl LanguageGenerator for RustGenerator {
    fn file_extension(&self) -> &str {
        "rs"
    }

    fn file_header(&self) -> String {
        "#![allow(non_snake_case)]\n".to_string()
    }

    fn imports_header(&self, needs_temporal: bool, has_shared: bool) -> String {
        let mut s = "use serde::{Deserialize, Serialize};\n".to_string();
        if needs_temporal {
            s.push_str("use chrono::{DateTime, FixedOffset, NaiveDate, NaiveTime};\n");
        }
        if has_shared {
            s.push_str("use crate::shared::*;\n");
        }
        s
    }

    fn struct_open(&self, name: &str) -> String {
        format!(
            "#[derive(Debug, Clone, Serialize, Deserialize)]\npub struct {} {{\n",
            name
        )
    }

    fn struct_close(&self, _fields: &[(String, String)]) -> String {
        "}\n".to_string()
    }

    fn field_line(&self, code_name: &str, type_name: &str) -> String {
        format!("    pub {}: {},\n", code_name, type_name)
    }

    fn enum_open(&self, name: &str) -> String {
        format!(
            "#[derive(Debug, Clone, Serialize, Deserialize)]\npub enum {} {{\n",
            name
        )
    }

    fn enum_close(&self) -> String {
        "}\n".to_string()
    }

    fn enum_variant(&self, variant_name: &str, json_value: &str) -> String {
        if variant_name == json_value {
            format!("    {},\n", variant_name)
        } else {
            format!(
                "    #[serde(rename = \"{}\")]\n    {},\n",
                json_value, variant_name
            )
        }
    }

    fn type_name(&self, inferred: &InferredType) -> String {
        inferred.rust_type()
    }

    fn mod_file(&self, file_names: &[&str]) -> Option<String> {
        let mut code = String::new();
        for name in file_names {
            let mod_name = name.trim_end_matches(".rs");
            code.push_str(&format!("mod {};\npub use {}::*;\n\n", mod_name, mod_name));
        }
        Some(code)
    }

    fn sanitize_keyword(&self, name: &str) -> String {
        match name {
            "type" | "struct" | "enum" | "fn" | "let" | "mut" | "ref" | "self" | "super"
            | "mod" | "use" | "pub" | "crate" | "impl" | "trait" | "for" | "loop" | "while"
            | "if" | "else" | "match" | "return" | "break" | "continue" | "move" | "async"
            | "await" | "dyn" | "static" | "const" | "where" | "unsafe" | "extern" | "as"
            | "in" => format!("r#{}", name),
            _ => name.to_string(),
        }
    }

    fn field_name(&self, json_name: &str) -> String {
        self.sanitize_keyword(&to_snake_case(json_name))
    }

    fn file_name(&self, base_name: &str) -> String {
        format!("{}.rs", base_name)
    }
}

// to_snake_case imported from crate::codegen (make it `pub` there)
```

- [ ] **Step 3: Add `mod lang` to `src/main.rs`**

Add after the `mod har;` line:

```rust
mod lang;
```

- [ ] **Step 4: Create stub `src/lang/swift.rs`**

Minimal stub so it compiles:

```rust
use crate::lang::LanguageGenerator;
use crate::types::InferredType;

pub struct SwiftGenerator;

impl LanguageGenerator for SwiftGenerator {
    fn file_extension(&self) -> &str { "swift" }
    fn file_header(&self) -> String { String::new() }
    fn imports_header(&self, _needs_temporal: bool, _has_shared: bool) -> String {
        "import Foundation\n".to_string()
    }
    fn struct_open(&self, name: &str) -> String {
        format!("struct {}: Codable {{\n", name)
    }
    fn struct_close(&self, _fields: &[(String, String)]) -> String {
        "}\n".to_string()
    }
    fn field_line(&self, code_name: &str, type_name: &str) -> String {
        format!("    let {}: {}\n", code_name, type_name)
    }
    fn enum_open(&self, name: &str) -> String {
        format!("enum {}: String, Codable {{\n", name)
    }
    fn enum_close(&self) -> String { "}\n".to_string() }
    fn enum_variant(&self, variant_name: &str, json_value: &str) -> String {
        if variant_name == json_value {
            format!("    case {}\n", variant_name)
        } else {
            format!("    case {} = \"{}\"\n", variant_name, json_value)
        }
    }
    fn type_name(&self, inferred: &InferredType) -> String {
        inferred.rust_type() // placeholder — will be replaced in Task 4
    }
    fn mod_file(&self, _file_names: &[&str]) -> Option<String> { None }
    fn sanitize_keyword(&self, name: &str) -> String { name.to_string() }
    fn field_name(&self, json_name: &str) -> String { json_name.to_string() }
    fn file_name(&self, base_name: &str) -> String {
        format!("{}.swift", capitalize_first(base_name))
    }
}

fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
```

- [ ] **Step 5: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 6: Commit**

```bash
git add src/lang/mod.rs src/lang/rust.rs src/lang/swift.rs src/main.rs
git commit -m "feat: add language abstraction layer with Rust generator and Swift stub"
```

---

### Task 4: Full Swift Generator

**Files:**
- Modify: `src/lang/swift.rs`

- [ ] **Step 1: Write tests for Swift generator**

Add to bottom of `src/lang/swift.rs`:

```rust
#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::InferredType;
    use std::collections::BTreeMap;

    #[test]
    fn test_swift_type_names() {
        let gen = SwiftGenerator;
        assert_eq!(gen.type_name(&InferredType::String), "String");
        assert_eq!(gen.type_name(&InferredType::I64), "Int");
        assert_eq!(gen.type_name(&InferredType::F64), "Double");
        assert_eq!(gen.type_name(&InferredType::Bool), "Bool");
        assert_eq!(gen.type_name(&InferredType::DateTime), "Date");
        assert_eq!(gen.type_name(&InferredType::Date), "Date");
        assert_eq!(gen.type_name(&InferredType::Time), "Date");
        assert_eq!(gen.type_name(&InferredType::Null), "Any?");
        assert_eq!(gen.type_name(&InferredType::Unknown), "Any");
    }

    #[test]
    fn test_swift_array_type() {
        let gen = SwiftGenerator;
        assert_eq!(
            gen.type_name(&InferredType::Array(Box::new(InferredType::String))),
            "[String]"
        );
    }

    #[test]
    fn test_swift_option_type() {
        let gen = SwiftGenerator;
        assert_eq!(
            gen.type_name(&InferredType::Option(Box::new(InferredType::I64))),
            "Int?"
        );
    }

    #[test]
    fn test_swift_object_fallback() {
        let gen = SwiftGenerator;
        assert_eq!(
            gen.type_name(&InferredType::Object(BTreeMap::new())),
            "[String: Any]"
        );
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("departure_date"), "departureDate");
        assert_eq!(to_camel_case("seat_abbreviation"), "seatAbbreviation");
        assert_eq!(to_camel_case("id"), "id");
        assert_eq!(to_camel_case("already_camel"), "alreadyCamel");
        assert_eq!(to_camel_case("origin"), "origin");
    }

    #[test]
    fn test_swift_field_name() {
        let gen = SwiftGenerator;
        assert_eq!(gen.field_name("departure_date"), "departureDate");
        assert_eq!(gen.field_name("class"), "`class`");
        assert_eq!(gen.field_name("origin"), "origin");
    }

    #[test]
    fn test_coding_keys() {
        let gen = SwiftGenerator;
        let fields = vec![
            ("origin".to_string(), "origin".to_string()),
            ("departureDate".to_string(), "departure_date".to_string()),
            ("seatAbbreviation".to_string(), "seat_abbreviation".to_string()),
        ];
        let result = gen.struct_close(&fields);
        assert!(result.contains("enum CodingKeys: String, CodingKey"));
        assert!(result.contains("case departureDate = \"departure_date\""));
        assert!(result.contains("case seatAbbreviation = \"seat_abbreviation\""));
        // origin should have no = since names match
        assert!(result.contains("case origin\n") || result.contains("case origin\r\n"));
    }

    #[test]
    fn test_coding_keys_omitted_when_all_match() {
        let gen = SwiftGenerator;
        let fields = vec![
            ("origin".to_string(), "origin".to_string()),
            ("id".to_string(), "id".to_string()),
        ];
        let result = gen.struct_close(&fields);
        assert!(!result.contains("CodingKeys"));
    }

    #[test]
    fn test_swift_keyword_sanitize() {
        let gen = SwiftGenerator;
        assert_eq!(gen.sanitize_keyword("class"), "`class`");
        assert_eq!(gen.sanitize_keyword("self"), "`self`");
        assert_eq!(gen.sanitize_keyword("origin"), "origin");
    }

    #[test]
    fn test_swift_file_name() {
        let gen = SwiftGenerator;
        assert_eq!(gen.file_name("shared"), "Shared.swift");
        assert_eq!(gen.file_name("flights"), "Flights.swift");
    }
}
```

- [ ] **Step 2: Implement full Swift generator**

Replace the stub in `src/lang/swift.rs`:

```rust
use crate::lang::LanguageGenerator;
use crate::types::InferredType;

pub struct SwiftGenerator;

const SWIFT_KEYWORDS: &[&str] = &[
    "class", "struct", "enum", "protocol", "func", "var", "let", "import", "return",
    "self", "super", "default", "case", "switch", "where", "in", "is", "as", "try",
    "throw", "throws", "nil", "true", "false", "do", "catch", "guard", "defer",
    "repeat", "break", "continue", "fallthrough", "typealias", "associatedtype",
    "operator", "init", "deinit", "subscript", "private", "public", "internal",
    "open", "fileprivate", "static", "override", "mutating", "inout", "Any",
    "Type", "Self",
];

impl LanguageGenerator for SwiftGenerator {
    fn file_extension(&self) -> &str {
        "swift"
    }

    fn file_header(&self) -> String {
        String::new()
    }

    fn imports_header(&self, _needs_temporal: bool, _has_shared: bool) -> String {
        "import Foundation\n".to_string()
    }

    fn struct_open(&self, name: &str) -> String {
        format!("struct {}: Codable {{\n", name)
    }

    fn struct_close(&self, fields: &[(String, String)]) -> String {
        let needs_coding_keys = fields.iter().any(|(code, json)| code != json);
        if !needs_coding_keys {
            return "}\n".to_string();
        }
        let mut s = String::new();
        s.push('\n');
        s.push_str("    enum CodingKeys: String, CodingKey {\n");
        for (code_name, json_name) in fields {
            if code_name == json_name {
                s.push_str(&format!("        case {}\n", code_name));
            } else {
                s.push_str(&format!("        case {} = \"{}\"\n", code_name, json_name));
            }
        }
        s.push_str("    }\n");
        s.push_str("}\n");
        s
    }

    fn field_line(&self, code_name: &str, type_name: &str) -> String {
        format!("    let {}: {}\n", code_name, type_name)
    }

    fn enum_open(&self, name: &str) -> String {
        format!("enum {}: String, Codable {{\n", name)
    }

    fn enum_close(&self) -> String {
        "}\n".to_string()
    }

    fn enum_variant(&self, variant_name: &str, json_value: &str) -> String {
        if variant_name == json_value {
            format!("    case {}\n", variant_name)
        } else {
            format!("    case {} = \"{}\"\n", variant_name, json_value)
        }
    }

    fn type_name(&self, inferred: &InferredType) -> String {
        match inferred {
            InferredType::Null => "Any?".to_string(),
            InferredType::Bool => "Bool".to_string(),
            InferredType::I64 => "Int".to_string(),
            InferredType::F64 => "Double".to_string(),
            InferredType::String => "String".to_string(),
            InferredType::DateTime | InferredType::Date | InferredType::Time => "Date".to_string(),
            InferredType::Array(inner) => format!("[{}]", self.type_name(inner)),
            InferredType::Object(_) => "[String: Any]".to_string(),
            InferredType::Option(inner) => format!("{}?", self.type_name(inner)),
            InferredType::Mixed(_) => "Any".to_string(),
            InferredType::Unknown => "Any".to_string(),
        }
    }

    fn mod_file(&self, _file_names: &[&str]) -> Option<String> {
        None
    }

    fn sanitize_keyword(&self, name: &str) -> String {
        if SWIFT_KEYWORDS.contains(&name) {
            format!("`{}`", name)
        } else {
            name.to_string()
        }
    }

    fn field_name(&self, json_name: &str) -> String {
        let camel = to_camel_case(json_name);
        self.sanitize_keyword(&camel)
    }

    fn file_name(&self, base_name: &str) -> String {
        format!("{}.swift", capitalize_first(base_name))
    }
}

fn to_camel_case(s: &str) -> String {
    let mut result = String::new();
    let mut capitalize_next = false;
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

fn capitalize_first(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
    }
}
```

- [ ] **Step 3: Run tests**

Run: `cargo test --lib lang`
Expected: All Swift generator tests pass

- [ ] **Step 4: Commit**

```bash
git add src/lang/swift.rs
git commit -m "feat: implement full Swift code generator with CodingKeys and camelCase"
```

---

## Chunk 3: Wire Language Into Code View

### Task 5: Refactor `CodeGenerator` to Use `LanguageGenerator`

**Files:**
- Modify: `src/codegen.rs:15-21,35-62,64-143,145-179,182-184`
- Modify: `src/views/schema_diagram.rs` (references `rust_type` on `GeneratedField`)
- Modify: `src/views/code.rs` (references `rust_type` on `GeneratedField`)

The key architectural change: `GeneratedField` stores the raw `InferredType` instead of pre-resolved language-specific strings. Type resolution happens at render time via `lang.type_name()`. This ensures Swift gets `[String]` not `Vec<String>`.

- [ ] **Step 1: Update `GeneratedField` to store `InferredType`**

In `src/codegen.rs`, change the struct (lines 15–21):

```rust
#[derive(Debug, Clone)]
pub struct GeneratedField {
    pub json_name: String,
    pub inferred_type: crate::types::InferredType,
    pub needs_rename: bool,
}
```

Remove the `rust_name` and `rust_type`/`code_type` fields entirely. Language-specific names and types are computed at render time.

- [ ] **Step 2: Update `from_schema` to populate new field structure**

In `from_schema` (lines 35–62), change field construction:

```rust
let rust_name = to_snake_case(key);
let needs_rename = rust_name != *key;
GeneratedField {
    json_name: key.clone(),
    inferred_type: typ.clone(),
    needs_rename,
}
```

- [ ] **Step 3: Update `collect_structs` to populate new field structure**

In `collect_structs` (lines 64–143), the field construction currently stores `rust_type: String`. Change to store `inferred_type`:

For object fields (line 78–82): store the child struct name as a string type won't work anymore since we need the struct name. Add a helper: when the value is an Object or Array-of-Object, store the inferred type but also track that the type refers to a child struct. The simplest approach: keep a separate `child_struct_name: Option<String>` field on `GeneratedField`, or handle this in `generate_code` by checking if the `InferredType` is `Object`.

Actually, the simpler approach: `collect_structs` already handles child struct names by returning them directly as type strings. Since this is codegen-specific naming (PascalCase struct names), keep `collect_structs` storing these as resolved names. Add a `resolved_type: Option<String>` field that `generate_code` uses when present (for struct references), falling back to `lang.type_name(&field.inferred_type)` otherwise:

```rust
#[derive(Debug, Clone)]
pub struct GeneratedField {
    pub json_name: String,
    pub inferred_type: crate::types::InferredType,
    pub resolved_type: Option<String>, // Set when type is a child struct name like "Vec<Passenger>"
    pub needs_rename: bool,
}
```

In `collect_structs`, set `resolved_type = Some(child_name)` for Object/Array-of-Object fields, and `resolved_type = None` for primitive types.

- [ ] **Step 4: Update `generate_code` to use `LanguageGenerator`**

Replace `generate_code()` (lines 145–179):

```rust
    pub fn generate_code(&self, lang: &dyn crate::lang::LanguageGenerator) -> String {
        let mut output = String::new();
        let header = lang.file_header();
        if !header.is_empty() {
            output.push_str(&header);
            output.push('\n');
        }

        let needs_temporal = self.structs.iter().any(|s| {
            s.fields.iter().any(|f| {
                matches!(
                    f.inferred_type,
                    crate::types::InferredType::DateTime
                        | crate::types::InferredType::Date
                        | crate::types::InferredType::Time
                )
            })
        });
        output.push_str(&lang.imports_header(needs_temporal, false));
        output.push('\n');

        for (i, s) in self.structs.iter().rev().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str(&lang.struct_open(&s.name));
            let mut field_pairs: Vec<(String, String)> = Vec::new();
            for field in &s.fields {
                let code_name = lang.field_name(&field.json_name);
                let type_str = field.resolved_type.clone()
                    .unwrap_or_else(|| lang.type_name(&field.inferred_type));
                output.push_str(&lang.field_line(&code_name, &type_str));
                field_pairs.push((code_name, field.json_name.clone()));
            }
            output.push_str(&lang.struct_close(&field_pairs));
        }

        output
    }
```

- [ ] **Step 5: Remove `resolve_type` function**

Delete the `resolve_type` function (lines 182–184) — no longer needed since type resolution goes through the trait.

- [ ] **Step 6: Fix all references to removed fields in other files**

The compiler will report errors in files that reference `rust_type`, `rust_name`, or `code_type` on `GeneratedField`. Known locations:

- `src/views/schema_diagram.rs` — references `f.rust_type` (around line 182). Change to `f.resolved_type.clone().unwrap_or_else(|| f.inferred_type.rust_type())` or use the Rust generator's `type_name()`.
- `src/views/code.rs` — the `generate_file_code_structs` function and `prefix_type` function reference `field.rust_type`. These need to use `field.resolved_type` or `lang.type_name(&field.inferred_type)`.

Fix each compiler error by using the appropriate resolution. For code that needs a Rust-specific type string (like schema_diagram which always displays Rust types), call `field.inferred_type.rust_type()` directly.

- [ ] **Step 7: Build and verify**

Run: `cargo build`
Expected: Clean build with no errors referencing old field names.

- [ ] **Step 8: Commit**

```bash
git add src/codegen.rs src/views/schema_diagram.rs src/views/code.rs
git commit -m "refactor: make CodeGenerator language-neutral with LanguageGenerator trait"
```

---

### Task 6: Add Language Toggle to Code View

**Files:**
- Modify: `src/views/code.rs` (CodeView struct, rebuild_file_mode, show_selected top bar)

- [ ] **Step 1: Add `selected_language` to `CodeView`**

In the `CodeView` struct (around line 55), add:

```rust
    selected_language: crate::lang::CodeLanguage,
```

Initialize in `new()`:

```rust
    selected_language: crate::lang::CodeLanguage::Rust,
```

- [ ] **Step 2: Include language in cache key computation**

In `CodeView::show()`, the cache key formula (around lines 141-145) uses pointer address, file count, etc. Add the selected language to the cache key so switching languages triggers rebuild:

```rust
let lang_hash = self.selected_language as u64;
let new_key = /* existing computation */ ^ lang_hash;
```

- [ ] **Step 3: Add language toggle to the top bar**

In `show_selected()`, after the "Download" button (around line 484), add:

```rust
            ui.separator();
            let lang = &mut self.selected_language;
            let prev_lang = *lang;
            ui.horizontal(|ui| {
                ui.selectable_value(lang, crate::lang::CodeLanguage::Rust, "Rust");
                ui.selectable_value(lang, crate::lang::CodeLanguage::Swift, "Swift");
            });
            if *lang != prev_lang {
                self.cache_key = 0;
            }
```

Note: `self` is borrowed mutably for `lang`, so the implementer may need to restructure slightly — e.g., read `selected_language` into a local, render the toggle, then write back. The exact pattern depends on how the surrounding closure borrows work.

- [ ] **Step 3: Update `rebuild_file_mode()` to use `LanguageGenerator`**

This is the biggest change. In `rebuild_file_mode()`:

1. Get the generator at the top:
```rust
let lang = self.selected_language.generator();
```

2. Replace all `".rs"` extensions with `lang.file_extension()`:
   - `"shared.rs"` → `lang.file_name("shared")`
   - `format!("{}.rs", group_key)` → `lang.file_name(&group_key)`
   - `"enums.rs"` → `lang.file_name("enums")`

3. Replace hardcoded Rust code generation with `lang` trait calls:
   - The `CodeGenerator::generate_code()` calls should pass `lang.as_ref()`
   - Enum generation block should use `lang.enum_open()`, `lang.enum_variant()`, `lang.enum_close()`
   - mod.rs generation: call `lang.mod_file()`, skip if `None`

4. Replace the `generate_mod_rs` call with `lang.mod_file()`:
```rust
if let Some(mod_code) = lang.mod_file(&names) {
    // ... create mod file
}
```

5. File name matching: update string comparisons like `f.name == "enums.rs"` to use the language-aware name or use a flag on `GeneratedFile` to identify file types.

The implementer should work through each section of `rebuild_file_mode()` methodically, replacing hardcoded Rust syntax with trait method calls. The structure of the method (shared → groups → enums → mod) stays the same.

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 5: Update `build_struct_index` for Swift patterns**

The existing `build_struct_index` (code.rs) looks for `pub struct`, `pub enum`, `pub type`. For Swift, structs are `struct Name: Codable {` and enums are `enum Name: String, Codable {` (no `pub` prefix). Update to detect both:

```rust
// Detect struct declarations (Rust: "pub struct X", Swift: "struct X: Codable")
let struct_name = trimmed.strip_prefix("pub struct ")
    .or_else(|| trimmed.strip_prefix("struct "))
    .and_then(|rest| {
        rest.split(|c: char| c == ' ' || c == '{' || c == '<' || c == ':')
            .next()
            .filter(|n| !n.is_empty())
    });
```

Apply the same pattern for `pub enum` / `enum`.

- [ ] **Step 6: Update hidden field filtering for Swift**

The `filter_hidden_fields_from_block` and `filter_hidden_fields_from_code` functions parse `pub field: Type,` patterns. Add Swift pattern detection: `let field: Type` lines. Use the selected language to determine which pattern to match.

- [ ] **Step 7: Manually test**

Run the app, load some JSON files, verify:
1. Rust output looks identical to before
2. Switching to Swift shows Swift output with `import Foundation`, `struct: Codable`, `let` fields
3. CodingKeys appear when field names differ
4. Enum conversion still works in both languages
5. Download creates files with correct extensions
6. Clicking a type name navigates to its definition in Swift mode

- [ ] **Step 8: Commit**

```bash
git add src/views/code.rs
git commit -m "feat: add language toggle with Swift code generation in code view"
```

---

### Task 7: Language-Aware Syntax Highlighting

**Files:**
- Modify: `src/views/code.rs` (`render_rust_line` → `render_code_line`)

- [ ] **Step 1: Add `CodeLanguage` parameter to `render_rust_line`**

Rename `render_rust_line` to `render_code_line` and add a `language: CodeLanguage` parameter.

Update the call site in `show_selected` to pass `self.selected_language`.

- [ ] **Step 2: Add Swift keyword detection**

In `render_code_line`, add Swift-specific line detection alongside the existing Rust patterns:

For Swift, these patterns apply:
- `import Foundation` → MAUVE (like `use` in Rust)
- `struct Name: Codable {` → keyword MAUVE, name YELLOW (like `pub enum` handling)
- `enum Name: String, Codable {` → keyword MAUVE, name YELLOW
- `let field: Type` → field handling (like `pub field: Type,` in Rust)
- `case variant` / `case variant = "value"` → GREEN (like enum variants)
- `enum CodingKeys: String, CodingKey {` → YELLOW (like attribute)

The simplest approach: add a branch early in `render_code_line` that checks `language == Swift` and handles Swift-specific patterns, falling through to existing Rust handling otherwise. Many patterns (empty lines, `}`, comments) are shared.

- [ ] **Step 3: Update field line parsing for Swift**

Rust fields: `pub field: Type,` (split on `:`, field part starts with `pub `)
Swift fields: `let field: Type` (split on `:`, field part starts with `let `)

Add detection for Swift field pattern:
```rust
let is_field_line = match language {
    CodeLanguage::Rust => trimmed.starts_with("pub ") && trimmed.contains(':'),
    CodeLanguage::Swift => trimmed.starts_with("let ") && trimmed.contains(':'),
};
```

The interactive features (clickable types, enum buttons, hide/show) work the same way — they just need the field name extracted from the correct prefix (`pub ` vs `let `).

- [ ] **Step 4: Build and verify**

Run: `cargo build`
Expected: Clean build

- [ ] **Step 5: Commit**

```bash
git add src/views/code.rs
git commit -m "feat: language-aware syntax highlighting for Rust and Swift"
```

---

### Task 8: Final Integration and Cleanup

**Files:**
- Modify: `src/views/code.rs` (enum rewriting for Swift)
- Modify: `src/codegen.rs` (remove now-dead Rust-specific code if any)

- [ ] **Step 1: Update enum type rewriting for Swift**

In `rebuild_file_mode()`, the enum type rewriting block (currently around lines 290–320) replaces `String` → enum name in file code. Make patterns language-aware:

```rust
let patterns = match self.selected_language {
    CodeLanguage::Rust => vec![
        (format!("pub {}: String,", field_name), format!("pub {}: {},", field_name, enum_name)),
        (format!("pub {}: Option<String>,", field_name), format!("pub {}: Option<{}>,", field_name, enum_name)),
    ],
    CodeLanguage::Swift => {
        let swift_name = /* lang.field_name(field_name) */;
        vec![
            (format!("let {}: String", swift_name), format!("let {}: {}", swift_name, enum_name)),
            (format!("let {}: String?", swift_name), format!("let {}: {}?", swift_name, enum_name)),
        ]
    },
};
```

Also include sanitized field name variants (e.g., `r#type` for Rust, `` `class` `` for Swift).

- [ ] **Step 2: Update enum import injection for Swift**

Rust adds `use crate::enums::*;` — Swift doesn't need this (all files in a module are visible).

Wrap the import injection in a language check:
```rust
if self.selected_language == CodeLanguage::Rust && !new_code.contains("use crate::enums::") {
    // inject import
}
```

- [ ] **Step 3: Build and run full test suite**

Run: `cargo test --lib`
Expected: All tests pass (HAR + Swift generator)

Run: `cargo build`
Expected: Clean build

- [ ] **Step 4: Manual smoke test**

Test the full flow:
1. Import a `.har` file — verify files appear with intelligent names
2. Switch to Swift — verify Swift output
3. Convert a field to enum — verify enum appears correctly in both Rust and Swift
4. Download as folder — verify correct extensions and content
5. Switch back to Rust — verify Rust output unchanged

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "feat: complete HAR import and multi-language codegen with Swift support"
```
