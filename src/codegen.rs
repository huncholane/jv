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

/// Structured representation of a resolved type, replacing string-based manipulation.
#[derive(Debug, Clone, PartialEq)]
pub enum ResolvedType {
    /// A known struct name (e.g., "Passenger")
    Struct(String),
    /// Vec<inner>
    Array(Box<ResolvedType>),
    /// Option<inner>
    Optional(Box<ResolvedType>),
    /// Primitive type — use lang.type_name() to render
    Inferred(InferredType),
}

impl ResolvedType {
    /// Render to language-specific code (e.g., `Vec<Option<Passenger>>` for Rust, `[Passenger?]` for Swift)
    pub fn to_code(&self, lang: &dyn crate::lang::LanguageGenerator) -> String {
        match self {
            ResolvedType::Struct(name) => name.clone(),
            ResolvedType::Array(inner) => lang.wrap_array(&inner.to_code(lang)),
            ResolvedType::Optional(inner) => lang.wrap_optional(&inner.to_code(lang)),
            ResolvedType::Inferred(t) => lang.type_name(t),
        }
    }

    /// Prefix struct names that aren't in shared_names and aren't the root
    pub fn prefix_structs(
        &self,
        prefix: &str,
        shared_names: &std::collections::BTreeSet<String>,
        root_name: &str,
    ) -> ResolvedType {
        match self {
            ResolvedType::Struct(name) => {
                if !shared_names.contains(name)
                    && name != root_name
                    && !name.starts_with(prefix)
                {
                    ResolvedType::Struct(format!("{}{}", prefix, name))
                } else {
                    self.clone()
                }
            }
            ResolvedType::Array(inner) => {
                ResolvedType::Array(Box::new(inner.prefix_structs(prefix, shared_names, root_name)))
            }
            ResolvedType::Optional(inner) => {
                ResolvedType::Optional(Box::new(inner.prefix_structs(prefix, shared_names, root_name)))
            }
            ResolvedType::Inferred(_) => self.clone(),
        }
    }

    /// Extract all struct names referenced in this type
    pub fn struct_names(&self) -> Vec<&str> {
        match self {
            ResolvedType::Struct(name) => vec![name.as_str()],
            ResolvedType::Array(inner) | ResolvedType::Optional(inner) => inner.struct_names(),
            ResolvedType::Inferred(_) => vec![],
        }
    }

    /// Wrap in Optional if not already optional
    pub fn make_optional(self) -> ResolvedType {
        match self {
            ResolvedType::Optional(_) => self,
            other => ResolvedType::Optional(Box::new(other)),
        }
    }

    pub fn is_optional(&self) -> bool {
        matches!(self, ResolvedType::Optional(_))
    }
}

#[derive(Debug, Clone)]
pub struct GeneratedField {
    pub json_name: String,
    pub inferred_type: InferredType,
    /// Resolved struct type — None means use inferred_type via lang.type_name()
    pub resolved_type: Option<ResolvedType>,
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
                    let resolved_type = resolve_type_to_struct(typ, shared);
                    GeneratedField {
                        json_name: key.clone(),
                        inferred_type: typ.clone(),
                        resolved_type,
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
                            Some(ResolvedType::Struct(child_name))
                        }
                        serde_json::Value::Array(arr) => {
                            if let Some(first) = arr.first() {
                                if first.is_object() {
                                    let child_name = to_pascal_case(&singularize(key));
                                    Self::collect_structs(first, &child_name, structs, seen);
                                    Some(ResolvedType::Array(Box::new(ResolvedType::Struct(child_name))))
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
                    let field_sig: Vec<(&str, &InferredType, Option<&ResolvedType>)> = fields
                        .iter()
                        .map(|f| (f.json_name.as_str(), &f.inferred_type, f.resolved_type.as_ref()))
                        .collect();
                    let already_exists = structs.iter().any(|s| {
                        s.name == name && s.fields.len() == fields.len() && {
                            let existing_sig: Vec<(&str, &InferredType, Option<&ResolvedType>)> = s.fields
                                .iter()
                                .map(|f| (f.json_name.as_str(), &f.inferred_type, f.resolved_type.as_ref()))
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
        // Generate struct bodies first so we know what types are used
        let mut body = String::new();
        for (i, s) in self.structs.iter().rev().enumerate() {
            if i > 0 {
                body.push('\n');
            }
            body.push_str(&lang.struct_open(&s.name));

            let mut field_pairs: Vec<(String, String)> = Vec::new();
            for field in &s.fields {
                let code_name = lang.field_name(&field.json_name);
                let type_str = match &field.resolved_type {
                    Some(rt) => rt.to_code(lang),
                    None => lang.type_name(&field.inferred_type),
                };
                body.push_str(&lang.field_line(&code_name, &type_str, &field.json_name));
                field_pairs.push((code_name, field.json_name.clone()));
            }

            body.push_str(&lang.struct_close(&field_pairs));
        }

        // Build output with imports based on what the body actually uses
        let mut output = String::new();
        output.push_str(&lang.file_header());
        output.push_str(&lang.imports_header(&body, false));
        if !output.is_empty() && !output.ends_with('\n') {
            output.push('\n');
        }
        output.push_str(&body);

        output
    }
}

/// A generated project file (name + code content)
pub struct GeneratedProjectFile {
    pub name: String,
    pub code: String,
    /// Root type for each source file that contributed to this group.
    /// Vec of (source_filename, root_rust_type) — used for deserialization testing.
    pub root_types: Vec<(String, String)>,
}

/// Generate all Rust files for a project: shared.rs, per-group files, mod.rs.
/// This mirrors the code view's `rebuild_file_mode` pipeline exactly.
pub fn generate_project(
    parsed_files: &[(String, serde_json::Value)],
    schema: &crate::schema::SchemaOverview,
    lang: &dyn crate::lang::LanguageGenerator,
) -> Vec<GeneratedProjectFile> {
    use std::collections::{BTreeMap, BTreeSet};

    let shared_names: BTreeSet<String> = schema.structs.iter().map(|s| s.name.clone()).collect();
    let unique_names: BTreeSet<String> = schema.unique_structs.iter().map(|s| s.name.clone()).collect();
    let all_structs = schema.all_structs();

    let mut result = Vec::new();

    // shared.rs
    if !schema.structs.is_empty() {
        let code = CodeGenerator::from_schema(&schema.structs).generate_code(lang);
        result.push(GeneratedProjectFile {
            name: lang.file_name("shared"),
            code,
            root_types: Vec::new(),
        });
    }

    // Group files by depluralized first word
    let mut groups: BTreeMap<String, Vec<(&str, &serde_json::Value)>> = BTreeMap::new();
    for (filename, value) in parsed_files {
        let word = first_normal_word(filename)
            .map(|w| to_pascal_case(&singularize(&w)))
            .unwrap_or_else(|| "other".to_string());
        let key = singularize(&word.to_ascii_lowercase());
        groups.entry(key).or_default().push((filename.as_str(), value));
    }

    let all_schema_names: BTreeSet<String> = shared_names.iter().chain(unique_names.iter()).cloned().collect();

    for (group_key, files) in &groups {
        // Collect all struct definitions, merging fields from multiple files
        let mut struct_order: Vec<String> = Vec::new();
        let mut struct_defs: BTreeMap<String, Vec<GeneratedField>> = BTreeMap::new();
        let mut root_types: Vec<(String, String)> = Vec::new();
        let mut type_aliases: Vec<String> = Vec::new();
        let mut seen_aliases: BTreeSet<String> = BTreeSet::new();

        for (filename, value) in files {
            let prefix = first_normal_word(filename)
                .map(|w| to_pascal_case(&singularize(&w)))
                .unwrap_or_default();
            let is_root_array = value.is_array();

            let (root_name, array_item_name) = if is_root_array {
                let singular = singularize(&prefix);
                let item_name = if singular.is_empty() {
                    "Item".to_string()
                } else {
                    let mut s = String::new();
                    s.push(singular.chars().next().unwrap().to_ascii_uppercase());
                    s.extend(singular.chars().skip(1));
                    s
                };
                (item_name.clone(), Some(item_name))
            } else {
                let name = if prefix.is_empty() { "Root".to_string() } else { format!("{}Root", prefix) };
                (name, None)
            };

            let deser_type = if is_root_array {
                format!("Vec<{}>", root_name)
            } else {
                root_name.clone()
            };
            root_types.push((filename.to_string(), deser_type));

            let mut gen = CodeGenerator::from_value_named(value, &root_name);

            // Schema-aware type resolution
            resolve_codegen_against_schema(&mut gen, &all_structs, &shared_names);

            // Collect structs, merging duplicates
            for s in gen.structs.iter().rev() {
                if shared_names.contains(&s.name) {
                    continue;
                }

                let prefixed = format!("{}{}", prefix, s.name);
                let struct_name = if unique_names.contains(&prefixed) {
                    prefixed
                } else if unique_names.contains(&s.name) {
                    s.name.clone()
                } else if s.name != root_name && !prefix.is_empty() && !s.name.starts_with(&prefix) {
                    prefixed
                } else {
                    s.name.clone()
                };

                if let Some(existing) = struct_defs.get_mut(&struct_name) {
                    // Merge: make fields Optional if missing or Null in this instance
                    merge_generated_fields(existing, &s.fields);
                } else {
                    struct_order.push(struct_name.clone());
                    struct_defs.insert(struct_name, s.fields.clone());
                }
            }

            if let Some(ref item_name) = array_item_name {
                let alias_name = format!("{}Root", prefix);
                if !seen_aliases.contains(&alias_name) {
                    seen_aliases.insert(alias_name.clone());
                    let aliased = if shared_names.contains(item_name) {
                        item_name.clone()
                    } else if !prefix.is_empty() && !item_name.starts_with(&prefix) {
                        format!("{}{}", prefix, item_name)
                    } else {
                        item_name.clone()
                    };
                    type_aliases.push(format!("pub type {} = Vec<{}>;\n", alias_name, aliased));
                }
            }
        }

        // Emit code from merged struct definitions
        let group_prefix = to_pascal_case(&singularize(group_key));
        let mut struct_blocks: Vec<String> = type_aliases;
        for struct_name in &struct_order {
            let fields = &struct_defs[struct_name];
            let prefix = group_prefix.clone();

            let mut code = String::new();
            code.push_str(&lang.struct_open(struct_name));
            let mut field_pairs: Vec<(String, String)> = Vec::new();
            for field in fields {
                let code_name = lang.field_name(&field.json_name);
                let type_str = match &field.resolved_type {
                    Some(rt) => {
                        let prefixed = if !prefix.is_empty() {
                            rt.prefix_structs(&prefix, &all_schema_names, struct_name)
                        } else {
                            rt.clone()
                        };
                        prefixed.to_code(lang)
                    }
                    None => lang.type_name(&field.inferred_type),
                };
                code.push_str(&lang.field_line(&code_name, &type_str, &field.json_name));
                field_pairs.push((code_name, field.json_name.clone()));
            }
            code.push_str(&lang.struct_close(&field_pairs));
            struct_blocks.push(code);
        }

        let mut body = String::new();
        for block in &struct_blocks {
            body.push_str(block);
            body.push('\n');
        }

        let mut code = String::new();
        let header = lang.file_header();
        if !header.is_empty() {
            code.push_str(&header);
            code.push('\n');
        }
        code.push_str(&lang.imports_header(&body, !shared_names.is_empty()));
        code.push('\n');
        code.push_str(&body);

        result.push(GeneratedProjectFile {
            name: lang.file_name(group_key),
            code: code.trim_end().to_string() + "\n",
            root_types,
        });
    }

    // mod.rs
    let mod_names: Vec<&str> = result.iter().map(|f| {
        f.name.strip_suffix(".rs").unwrap_or(&f.name)
    }).collect();
    if let Some(mod_code) = lang.mod_file(&mod_names) {
        result.push(GeneratedProjectFile {
            name: "mod.rs".to_string(),
            code: mod_code,
            root_types: Vec::new(),
        });
    }

    result
}

/// Resolve types in a CodeGenerator against schema structs, and generate
/// missing struct definitions from schema when referenced but not present.
pub fn resolve_codegen_against_schema(
    gen: &mut CodeGenerator,
    all_structs: &[crate::schema::SharedStruct],
    shared_names: &std::collections::BTreeSet<String>,
) {
    for s in &mut gen.structs {
        let schema_match = crate::types::resolve_struct_name(
            &s.fields.iter().map(|f| (f.json_name.clone(), f.inferred_type.clone())).collect(),
            all_structs,
        );
        let schema_fields = schema_match.and_then(|name| {
            all_structs.iter().find(|ss| ss.name == name)
        });

        for field in &mut s.fields {
            if field.resolved_type.is_none() {
                field.resolved_type = resolve_type_to_struct(&field.inferred_type, all_structs);
                if field.resolved_type.is_none() {
                    if let Some(ss) = schema_fields {
                        if let Some(schema_type) = ss.fields.get(&field.json_name) {
                            field.resolved_type = resolve_type_to_struct(schema_type, all_structs);
                            if field.resolved_type.is_some() {
                                field.inferred_type = schema_type.clone();
                            }
                        }
                    }
                }
            }
        }
    }

    // Generate missing structs from schema
    let existing_names: std::collections::BTreeSet<String> =
        gen.structs.iter().map(|s| s.name.clone()).collect();
    let mut needed: Vec<String> = Vec::new();
    for s in &gen.structs {
        for field in &s.fields {
            if let Some(rt) = &field.resolved_type {
                for name in rt.struct_names() {
                    if !existing_names.contains(name) && !shared_names.contains(name) {
                        needed.push(name.to_string());
                    }
                }
            }
        }
    }
    let mut added: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
    while let Some(name) = needed.pop() {
        if added.contains(&name) || existing_names.contains(&name) || shared_names.contains(&name) {
            continue;
        }
        added.insert(name.clone());
        if let Some(ss) = all_structs.iter().find(|ss| ss.name == name) {
            let fields: Vec<GeneratedField> = ss.fields.iter().map(|(key, typ)| {
                let resolved = resolve_type_to_struct(typ, all_structs);
                if let Some(rt) = &resolved {
                    for dep in rt.struct_names() {
                        needed.push(dep.to_string());
                    }
                }
                GeneratedField {
                    json_name: key.clone(),
                    inferred_type: typ.clone(),
                    resolved_type: resolved,
                    needs_rename: false,
                }
            }).collect();
            gen.structs.push(GeneratedStruct {
                name: name.clone(),
                fields,
            });
        }
    }
}

/// Merge a new set of fields into an existing field list.
/// Fields missing from the new set, or whose type is Null, become Option<T>.
fn merge_generated_fields(existing: &mut Vec<GeneratedField>, new_fields: &[GeneratedField]) {
    use std::collections::BTreeMap;

    let new_map: BTreeMap<&str, &GeneratedField> = new_fields
        .iter()
        .map(|f| (f.json_name.as_str(), f))
        .collect();

    for field in existing.iter_mut() {
        let should_optionalize = match new_map.get(field.json_name.as_str()) {
            None => true,
            Some(new_field) => new_field.inferred_type == InferredType::Null,
        };

        if should_optionalize
            && !matches!(field.inferred_type, InferredType::Option(_) | InferredType::Null)
        {
            field.inferred_type = InferredType::Option(Box::new(field.inferred_type.clone()));
            field.resolved_type = field.resolved_type.take().map(|rt| rt.make_optional());
        }
    }

    // Add fields that exist in new but not in existing (as Optional)
    let existing_names: BTreeSet<String> = existing.iter().map(|f| f.json_name.clone()).collect();
    for new_field in new_fields {
        if !existing_names.contains(&new_field.json_name) {
            let mut field = new_field.clone();
            if !matches!(field.inferred_type, InferredType::Option(_) | InferredType::Null) {
                field.inferred_type = InferredType::Option(Box::new(field.inferred_type.clone()));
                field.resolved_type = field.resolved_type.take().map(|rt| rt.make_optional());
            }
            existing.push(field);
        }
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

/// Resolve an InferredType to a ResolvedType if it contains an Object matching a known struct.
/// Recursively handles Vec<Object>, Option<Object>, Option<Vec<Object>>, etc.
pub fn resolve_type_to_struct(
    typ: &InferredType,
    shared: &[crate::schema::SharedStruct],
) -> Option<ResolvedType> {
    match typ {
        InferredType::Object(fields) => {
            crate::types::resolve_struct_name(fields, shared).map(ResolvedType::Struct)
        }
        InferredType::Array(inner) => {
            resolve_type_to_struct(inner, shared)
                .map(|rt| ResolvedType::Array(Box::new(rt)))
        }
        InferredType::Option(inner) => {
            resolve_type_to_struct(inner, shared)
                .map(|rt| ResolvedType::Optional(Box::new(rt)))
        }
        _ => None,
    }
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
        | "dyn" | "static" | "const" | "where" | "unsafe" | "extern" | "as" | "in"
        | "override" | "abstract" | "become" | "box" | "do" | "final" | "macro" | "priv"
        | "try" | "typeof" | "unsized" | "virtual" | "yield" => {
            format!("r#{}", s)
        }
        _ => s.to_string(),
    }
}
