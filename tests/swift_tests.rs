use std::collections::BTreeMap;

use jv::lang::swift::{to_camel_case, SwiftGenerator};
use jv::lang::LanguageGenerator;
use jv::types::InferredType;

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
