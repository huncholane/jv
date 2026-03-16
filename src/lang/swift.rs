use crate::lang::LanguageGenerator;
use crate::types::InferredType;

pub struct SwiftGenerator;

const SWIFT_KEYWORDS: &[&str] = &[
    "class",
    "struct",
    "enum",
    "protocol",
    "func",
    "var",
    "let",
    "import",
    "return",
    "self",
    "super",
    "default",
    "case",
    "switch",
    "where",
    "in",
    "is",
    "as",
    "try",
    "throw",
    "throws",
    "nil",
    "true",
    "false",
    "do",
    "catch",
    "guard",
    "defer",
    "repeat",
    "break",
    "continue",
    "fallthrough",
    "typealias",
    "associatedtype",
    "operator",
    "init",
    "deinit",
    "subscript",
    "private",
    "public",
    "internal",
    "open",
    "fileprivate",
    "static",
    "override",
    "mutating",
    "inout",
    "Any",
    "Type",
    "Self",
];

pub fn to_camel_case(s: &str) -> String {
    let parts: Vec<&str> = s.split(|c| c == '_' || c == '-' || c == ' ').collect();
    let mut result = String::new();
    for (i, part) in parts.iter().enumerate() {
        if part.is_empty() {
            continue;
        }
        if i == 0 {
            result.push_str(part);
        } else {
            let mut chars = part.chars();
            if let Some(first) = chars.next() {
                result.extend(first.to_uppercase());
                result.push_str(chars.as_str());
            }
        }
    }
    result
}

impl LanguageGenerator for SwiftGenerator {
    fn file_extension(&self) -> &str {
        "swift"
    }

    fn file_header(&self) -> String {
        String::new()
    }

    fn imports_header(&self, _code_body: &str, _has_shared: bool) -> String {
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

        let mut out = String::new();
        out.push_str("\n    enum CodingKeys: String, CodingKey {\n");
        for (code_name, json_name) in fields {
            if code_name == json_name {
                out.push_str(&format!("        case {}\n", code_name));
            } else {
                out.push_str(&format!("        case {} = \"{}\"\n", code_name, json_name));
            }
        }
        out.push_str("    }\n");
        out.push_str("}\n");
        out
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
            InferredType::DateTime => "Date".to_string(),
            InferredType::Date => "Date".to_string(),
            InferredType::Time => "Date".to_string(),
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
        self.sanitize_keyword(&to_camel_case(json_name))
    }

    fn file_name(&self, base_name: &str) -> String {
        let mut c = base_name.chars();
        let capitalized = match c.next() {
            None => String::new(),
            Some(f) => f.to_uppercase().collect::<String>() + c.as_str(),
        };
        format!("{}.swift", capitalized)
    }
}
