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

pub trait LanguageGenerator {
    fn file_extension(&self) -> &str;
    fn file_header(&self) -> String;
    fn imports_header(&self, code_body: &str, has_shared: bool) -> String;
    fn struct_open(&self, name: &str) -> String;
    /// Close a struct. `fields` is (code_name, json_name) pairs for CodingKeys etc.
    fn struct_close(&self, fields: &[(String, String)]) -> String;
    fn field_line(&self, code_name: &str, type_name: &str, json_name: &str) -> String;
    fn enum_open(&self, name: &str) -> String;
    fn enum_close(&self) -> String;
    fn enum_variant(&self, variant_name: &str, json_value: &str) -> String;
    fn type_name(&self, inferred: &InferredType) -> String;
    fn wrap_array(&self, inner: &str) -> String;
    fn wrap_optional(&self, inner: &str) -> String;
    fn mod_file(&self, file_names: &[&str]) -> Option<String>;
    fn sanitize_keyword(&self, name: &str) -> String;
    fn field_name(&self, json_name: &str) -> String;
    fn file_name(&self, base_name: &str) -> String;
}
