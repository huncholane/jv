use crate::lang::LanguageGenerator;
use crate::types::InferredType;

pub struct RustGenerator;

const RUST_KEYWORDS: &[&str] = &[
    "type", "struct", "enum", "fn", "let", "mut", "ref", "self", "super", "mod", "use", "pub",
    "crate", "impl", "trait", "for", "loop", "while", "if", "else", "match", "return", "break",
    "continue", "move", "async", "await", "dyn", "static", "const", "where", "unsafe", "extern",
    "as", "in", "override", "abstract", "become", "box", "do", "final", "macro", "priv",
    "try", "typeof", "unsized", "virtual", "yield",
];

impl LanguageGenerator for RustGenerator {
    fn file_extension(&self) -> &str {
        "rs"
    }

    fn file_header(&self) -> String {
        "#![allow(non_snake_case)]\n".to_string()
    }

    fn imports_header(&self, code_body: &str, has_shared: bool) -> String {
        let mut out = "use serde::{Deserialize, Serialize};\n".to_string();

        // Only import chrono types that are actually used
        let mut chrono_types = Vec::new();
        if code_body.contains("DateTime<FixedOffset>") {
            chrono_types.push("DateTime");
            chrono_types.push("FixedOffset");
        }
        if code_body.contains("DateTime<Utc>") {
            chrono_types.push("DateTime");
            chrono_types.push("Utc");
        }
        if code_body.contains("NaiveDate") {
            chrono_types.push("NaiveDate");
        }
        if code_body.contains("NaiveTime") {
            chrono_types.push("NaiveTime");
        }
        chrono_types.dedup();
        if !chrono_types.is_empty() {
            out.push_str(&format!("use chrono::{{{}}};\n", chrono_types.join(", ")));
        }

        if has_shared {
            out.push_str("use super::shared::*;\n");
        }
        out
    }

    fn struct_open(&self, name: &str) -> String {
        format!(
            "#[derive(Debug, Clone, Default, Serialize, Deserialize)]\n#[serde(default)]\npub struct {} {{\n",
            name
        )
    }

    fn struct_close(&self, _fields: &[(String, String)]) -> String {
        "}\n".to_string()
    }

    fn field_line(&self, code_name: &str, type_name: &str, _json_name: &str) -> String {
        format!("    pub {}: {},\n", code_name, type_name)
    }

    fn enum_open(&self, name: &str) -> String {
        format!(
            "#[derive(Debug, Clone, Default, Serialize, Deserialize)]\npub enum {} {{\n",
            name
        )
    }

    fn enum_close(&self) -> String {
        "    #[default]\n    #[serde(other)]\n    Unknown,\n}\n".to_string()
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
        match inferred {
            // Temporal types vary in format — String is the safe codegen choice
            InferredType::DateTime | InferredType::Date | InferredType::Time => {
                "String".to_string()
            }
            // Recurse into wrappers so Option<DateTime> → Option<String>, etc.
            InferredType::Option(inner) => format!("Option<{}>", self.type_name(inner)),
            InferredType::Array(inner) => format!("Vec<{}>", self.type_name(inner)),
            _ => inferred.rust_type(),
        }
    }

    fn wrap_array(&self, inner: &str) -> String {
        format!("Vec<{}>", inner)
    }

    fn wrap_optional(&self, inner: &str) -> String {
        format!("Option<{}>", inner)
    }

    fn mod_file(&self, file_names: &[&str]) -> Option<String> {
        let mut out = String::new();
        for name in file_names {
            let stem = name.strip_suffix(".rs").unwrap_or(name);
            let safe = self.sanitize_keyword(stem);
            out.push_str(&format!("pub mod {};\npub use {}::*;\n\n", safe, safe));
        }
        Some(out)
    }

    fn sanitize_keyword(&self, name: &str) -> String {
        if RUST_KEYWORDS.contains(&name) {
            format!("r#{}", name)
        } else {
            name.to_string()
        }
    }

    fn field_name(&self, json_name: &str) -> String {
        self.sanitize_keyword(json_name)
    }

    fn file_name(&self, base_name: &str) -> String {
        format!("{}.rs", base_name)
    }
}
