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

fn to_camel_case(s: &str) -> String {
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    #[test]
    fn test_swift_type_names() {
        let g = SwiftGenerator;
        assert_eq!(g.type_name(&InferredType::Null), "Any?");
        assert_eq!(g.type_name(&InferredType::Bool), "Bool");
        assert_eq!(g.type_name(&InferredType::I64), "Int");
        assert_eq!(g.type_name(&InferredType::F64), "Double");
        assert_eq!(g.type_name(&InferredType::String), "String");
        assert_eq!(g.type_name(&InferredType::DateTime), "Date");
        assert_eq!(g.type_name(&InferredType::Date), "Date");
        assert_eq!(g.type_name(&InferredType::Time), "Date");
        assert_eq!(g.type_name(&InferredType::Unknown), "Any");
        assert_eq!(
            g.type_name(&InferredType::Mixed(vec![
                InferredType::I64,
                InferredType::String
            ])),
            "Any"
        );
    }

    #[test]
    fn test_swift_array_type() {
        let g = SwiftGenerator;
        let arr = InferredType::Array(Box::new(InferredType::String));
        assert_eq!(g.type_name(&arr), "[String]");
    }

    #[test]
    fn test_swift_option_type() {
        let g = SwiftGenerator;
        let opt = InferredType::Option(Box::new(InferredType::I64));
        assert_eq!(g.type_name(&opt), "Int?");
    }

    #[test]
    fn test_swift_object_fallback() {
        let g = SwiftGenerator;
        let obj = InferredType::Object(BTreeMap::new());
        assert_eq!(g.type_name(&obj), "[String: Any]");
    }

    #[test]
    fn test_to_camel_case() {
        assert_eq!(to_camel_case("departure_date"), "departureDate");
        assert_eq!(to_camel_case("id"), "id");
        assert_eq!(to_camel_case("seat_abbreviation"), "seatAbbreviation");
    }

    #[test]
    fn test_swift_field_name() {
        let g = SwiftGenerator;
        assert_eq!(g.field_name("departure_date"), "departureDate");
        assert_eq!(g.field_name("id"), "id");
        assert_eq!(g.field_name("class"), "`class`");
    }

    #[test]
    fn test_coding_keys() {
        let g = SwiftGenerator;
        let fields = vec![
            ("origin".to_string(), "origin".to_string()),
            ("departureDate".to_string(), "departure_date".to_string()),
        ];
        let result = g.struct_close(&fields);
        assert!(result.contains("enum CodingKeys: String, CodingKey {"));
        assert!(result.contains("case origin\n"));
        assert!(result.contains("case departureDate = \"departure_date\"\n"));
        assert!(result.ends_with("}\n"));
    }

    #[test]
    fn test_coding_keys_omitted_when_all_match() {
        let g = SwiftGenerator;
        let fields = vec![
            ("origin".to_string(), "origin".to_string()),
            ("id".to_string(), "id".to_string()),
        ];
        let result = g.struct_close(&fields);
        assert_eq!(result, "}\n");
        assert!(!result.contains("CodingKeys"));
    }

    #[test]
    fn test_swift_keyword_sanitize() {
        let g = SwiftGenerator;
        assert_eq!(g.sanitize_keyword("class"), "`class`");
        assert_eq!(g.sanitize_keyword("origin"), "origin");
    }

    #[test]
    fn test_swift_file_name() {
        let g = SwiftGenerator;
        assert_eq!(g.file_name("shared"), "Shared.swift");
    }
}
