use std::collections::BTreeSet;

use crate::types::InferredType;

pub struct CodeGenerator {
    pub structs: Vec<GeneratedStruct>,
}

#[derive(Debug, Clone)]
pub struct GeneratedStruct {
    pub name: String,
    pub fields: Vec<GeneratedField>,
}

#[derive(Debug, Clone)]
pub struct GeneratedField {
    pub json_name: String,
    pub inferred_type: InferredType,
    /// Set when type is a child struct name (e.g., "Passenger", "Vec<Passenger>")
    /// generate_code uses this instead of lang.type_name() when present
    pub resolved_type: Option<String>,
    pub needs_rename: bool,
}

impl CodeGenerator {
    pub fn from_value(value: &serde_json::Value) -> Self {
        Self::from_value_named(value, "Root")
    }

    pub fn from_value_named(value: &serde_json::Value, root_name: &str) -> Self {
        let mut structs = Vec::new();
        let mut seen_names = BTreeSet::new();
        Self::collect_structs(value, root_name, &mut structs, &mut seen_names);
        Self { structs }
    }

    pub fn from_schema(shared: &[crate::schema::SharedStruct]) -> Self {
        let mut structs = Vec::new();

        for s in shared {
            let fields = s
                .fields
                .iter()
                .map(|(key, typ)| {
                    let snake = to_snake_case(key);
                    let needs_rename = snake != *key;
                    GeneratedField {
                        json_name: key.clone(),
                        inferred_type: typ.clone(),
                        resolved_type: None,
                        needs_rename,
                    }
                })
                .collect();

            structs.push(GeneratedStruct {
                name: s.name.clone(),
                fields,
            });
        }

        Self { structs }
    }

    fn collect_structs(
        value: &serde_json::Value,
        name: &str,
        structs: &mut Vec<GeneratedStruct>,
        seen: &mut BTreeSet<String>,
    ) {
        match value {
            serde_json::Value::Object(map) => {
                let mut fields = Vec::new();
                for (key, val) in map {
                    let typ = crate::types::infer_type(val);
                    let snake = to_snake_case(key);
                    let needs_rename = snake != *key;

                    let resolved_type = match val {
                        serde_json::Value::Object(_) => {
                            let child_name = to_pascal_case(key);
                            Self::collect_structs(val, &child_name, structs, seen);
                            Some(child_name)
                        }
                        serde_json::Value::Array(arr) => {
                            if let Some(first) = arr.first() {
                                if first.is_object() {
                                    let child_name = to_pascal_case(&singularize(key));
                                    Self::collect_structs(first, &child_name, structs, seen);
                                    Some(format!("Vec<{}>", child_name))
                                } else {
                                    None
                                }
                            } else {
                                None
                            }
                        }
                        _ => None,
                    };

                    fields.push(GeneratedField {
                        json_name: key.clone(),
                        inferred_type: typ,
                        resolved_type,
                        needs_rename,
                    });
                }

                // Check if an existing struct with this name has identical fields
                if seen.contains(name) {
                    let field_sig: Vec<(&str, &InferredType, Option<&str>)> = fields
                        .iter()
                        .map(|f| (f.json_name.as_str(), &f.inferred_type, f.resolved_type.as_deref()))
                        .collect();
                    let already_exists = structs.iter().any(|s| {
                        s.name == name && s.fields.len() == fields.len() && {
                            let existing_sig: Vec<(&str, &InferredType, Option<&str>)> = s.fields
                                .iter()
                                .map(|f| (f.json_name.as_str(), &f.inferred_type, f.resolved_type.as_deref()))
                                .collect();
                            existing_sig == field_sig
                        }
                    });
                    if already_exists {
                        return;
                    }
                }

                let unique_name = make_unique_name(name, seen);
                seen.insert(unique_name.clone());

                structs.push(GeneratedStruct {
                    name: unique_name,
                    fields,
                });
            }
            serde_json::Value::Array(arr) => {
                if let Some(first) = arr.first() {
                    Self::collect_structs(first, name, structs, seen);
                }
            }
            _ => {}
        }
    }

    pub fn generate_code(&self, lang: &dyn crate::lang::LanguageGenerator) -> String {
        let mut output = String::new();
        output.push_str(&lang.file_header());

        // Check if any field uses temporal types
        let needs_temporal = self.structs.iter().any(|s| {
            s.fields.iter().any(|f| {
                matches!(
                    f.inferred_type,
                    InferredType::DateTime | InferredType::Date | InferredType::Time
                )
            })
        });
        output.push_str(&lang.imports_header(needs_temporal, false));
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }

        for (i, s) in self.structs.iter().rev().enumerate() {
            if i > 0 {
                output.push('\n');
            }
            output.push_str(&lang.struct_open(&s.name));

            let mut field_pairs: Vec<(String, String)> = Vec::new();
            for field in &s.fields {
                let code_name = lang.field_name(&field.json_name);
                let type_str = field
                    .resolved_type
                    .clone()
                    .unwrap_or_else(|| lang.type_name(&field.inferred_type));
                output.push_str(&lang.field_line(&code_name, &type_str));
                field_pairs.push((code_name, field.json_name.clone()));
            }

            output.push_str(&lang.struct_close(&field_pairs));
        }

        output
    }
}

pub fn to_snake_case(s: &str) -> String {
    let mut result = String::new();
    for (i, ch) in s.chars().enumerate() {
        if ch.is_ascii_uppercase() {
            if i > 0 {
                let prev = s.chars().nth(i - 1).unwrap_or('_');
                if prev != '_' && !prev.is_ascii_uppercase() {
                    result.push('_');
                }
            }
            result.push(ch.to_ascii_lowercase());
        } else if ch == '-' || ch == ' ' {
            result.push('_');
        } else {
            result.push(ch);
        }
    }
    // Ensure it's a valid Rust identifier
    if result.starts_with(|c: char| c.is_ascii_digit()) {
        result.insert(0, '_');
    }
    sanitize_keyword(&result)
}

pub fn to_pascal_case(s: &str) -> String {
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

pub fn singularize(s: &str) -> String {
    let lower = s.to_ascii_lowercase();

    // Words that end in 's' but aren't plural
    const NOT_PLURAL: &[&str] = &[
        "status", "address", "bus", "canvas", "atlas", "alias", "basis",
        "radius", "focus", "census", "corpus", "consensus", "osis",
        "sis", "nexus", "plus", "minus", "gas", "class", "pass",
        "process", "access", "success", "progress", "express",
    ];
    for &word in NOT_PLURAL {
        if lower == word || lower.ends_with(word) {
            return s.to_string();
        }
    }

    if lower.ends_with("ies") && lower.len() > 4 {
        // categories -> category, companies -> company
        format!("{}y", &s[..s.len() - 3])
    } else if lower.ends_with("ses") || lower.ends_with("xes") || lower.ends_with("zes") {
        // responses -> response, indexes -> index, buzzes -> buzz
        // but not "ses" alone
        if lower.len() > 4 {
            s[..s.len() - 2].to_string()
        } else {
            s.to_string()
        }
    } else if lower.ends_with("ves") {
        // leaves -> leaf (but this is rare in JSON, just strip the s)
        s[..s.len() - 1].to_string()
    } else if lower.ends_with('s')
        && !lower.ends_with("ss")
        && !lower.ends_with("us")
        && !lower.ends_with("is")
    {
        // trips -> trip, users -> user
        s[..s.len() - 1].to_string()
    } else {
        s.to_string()
    }
}

/// Extract the first "normal" word from a filename (skip dates, numbers, version strings)
/// e.g. "trip_29_feb2026_rv0.json" -> "trip", "users.json" -> "users"
/// Returns lowercase, not PascalCased — caller decides casing.
pub fn first_normal_word(filename: &str) -> Option<String> {
    let stem = std::path::Path::new(filename)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(filename);

    for part in stem.split(|c: char| c == '_' || c == '-' || c == ' ' || c == '.') {
        if part.is_empty() {
            continue;
        }
        if part.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        if part.len() <= 4 && part.chars().any(|c| c.is_ascii_digit()) {
            continue;
        }
        return Some(part.to_ascii_lowercase());
    }
    None
}

fn make_unique_name(name: &str, seen: &BTreeSet<String>) -> String {
    if !seen.contains(name) {
        return name.to_string();
    }
    let mut i = 2;
    loop {
        let candidate = format!("{}{}", name, i);
        if !seen.contains(&candidate) {
            return candidate;
        }
        i += 1;
    }
}

fn sanitize_keyword(s: &str) -> String {
    match s {
        "type" | "struct" | "enum" | "fn" | "let" | "mut" | "ref" | "self" | "super" | "mod"
        | "use" | "pub" | "crate" | "impl" | "trait" | "for" | "loop" | "while" | "if"
        | "else" | "match" | "return" | "break" | "continue" | "move" | "async" | "await"
        | "dyn" | "static" | "const" | "where" | "unsafe" | "extern" | "as" | "in" => {
            format!("r#{}", s)
        }
        _ => s.to_string(),
    }
}
