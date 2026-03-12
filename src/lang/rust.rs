use crate::codegen::to_snake_case;
use crate::lang::LanguageGenerator;
use crate::types::InferredType;

pub struct RustGenerator;

const RUST_KEYWORDS: &[&str] = &[
    "type", "struct", "enum", "fn", "let", "mut", "ref", "self", "super", "mod", "use", "pub",
    "crate", "impl", "trait", "for", "loop", "while", "if", "else", "match", "return", "break",
    "continue", "move", "async", "await", "dyn", "static", "const", "where", "unsafe", "extern",
    "as", "in",
];

impl LanguageGenerator for RustGenerator {
    fn file_extension(&self) -> &str {
        "rs"
    }

    fn file_header(&self) -> String {
        "#![allow(non_snake_case)]\n".to_string()
    }

    fn imports_header(&self, needs_temporal: bool, has_shared: bool) -> String {
        let mut out = "use serde::{Deserialize, Serialize};\n".to_string();
        if needs_temporal {
            out.push_str("use chrono::{DateTime, NaiveDate, NaiveTime, Utc};\n");
        }
        if has_shared {
            out.push_str("use super::shared::*;\n");
        }
        out
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
        let mut out = String::new();
        for name in file_names {
            out.push_str(&format!("mod {};\npub use {}::*;\n\n", name, name));
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
        self.sanitize_keyword(&to_snake_case(json_name))
    }

    fn file_name(&self, base_name: &str) -> String {
        format!("{}.rs", base_name)
    }
}
