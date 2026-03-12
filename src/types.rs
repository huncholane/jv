use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::temporal::{detect_temporal, detect_unix_timestamp, TemporalValue};

#[derive(Debug, Clone, PartialEq)]
pub enum InferredType {
    Null,
    Bool,
    I64,
    F64,
    String,
    DateTime,
    Date,
    Time,
    Array(Box<InferredType>),
    Object(BTreeMap<String, InferredType>),
    Option(Box<InferredType>),
    Mixed(Vec<InferredType>),
    Unknown,
}

impl InferredType {
    pub fn rust_type(&self) -> String {
        match self {
            Self::Null => "Option<serde_json::Value>".to_string(),
            Self::Bool => "bool".to_string(),
            Self::I64 => "i64".to_string(),
            Self::F64 => "f64".to_string(),
            Self::String => "String".to_string(),
            Self::DateTime => "DateTime<FixedOffset>".to_string(),
            Self::Date => "NaiveDate".to_string(),
            Self::Time => "NaiveTime".to_string(),
            Self::Array(inner) => format!("Vec<{}>", inner.rust_type()),
            Self::Object(_) => "serde_json::Value".to_string(),
            Self::Option(inner) => format!("Option<{}>", inner.rust_type()),
            Self::Mixed(_) => "serde_json::Value".to_string(),
            Self::Unknown => "serde_json::Value".to_string(),
        }
    }

    /// Recursive type tag for Jaccard similarity — distinguishes structural shape
    pub fn type_tag(&self) -> String {
        match self {
            Self::Null => "null".into(),
            Self::Bool => "bool".into(),
            Self::I64 => "i64".into(),
            Self::F64 => "f64".into(),
            Self::String => "str".into(),
            Self::DateTime => "dt".into(),
            Self::Date => "date".into(),
            Self::Time => "time".into(),
            Self::Array(inner) => format!("[{}]", inner.type_tag()),
            Self::Object(fields) => {
                // Recurse into field types so structurally different objects are distinct
                let mut pairs: Vec<std::string::String> = fields
                    .iter()
                    .map(|(k, v)| format!("{}:{}", k, v.type_tag()))
                    .collect();
                pairs.sort();
                format!("{{{}}}", pairs.join(","))
            }
            Self::Option(inner) => format!("?{}", inner.type_tag()),
            Self::Mixed(_) => "mixed".into(),
            Self::Unknown => "?".into(),
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            Self::Null => "null".to_string(),
            Self::Bool => "bool".to_string(),
            Self::I64 => "i64".to_string(),
            Self::F64 => "f64".to_string(),
            Self::String => "String".to_string(),
            Self::DateTime => "DateTime".to_string(),
            Self::Date => "Date".to_string(),
            Self::Time => "Time".to_string(),
            Self::Array(inner) => format!("Vec<{}>", inner.display_name()),
            Self::Object(fields) => format!("Object({})", fields.len()),
            Self::Option(inner) => format!("Option<{}>", inner.display_name()),
            Self::Mixed(types) => {
                let names: Vec<String> = types.iter().map(|t| t.display_name()).collect();
                format!("Mixed({})", names.join("|"))
            }
            Self::Unknown => "Unknown".to_string(),
        }
    }

    /// Short label for table display — keeps things compact
    pub fn short_name(&self, structs: &[crate::schema::SharedStruct]) -> String {
        match self {
            Self::DateTime => "dt".to_string(),
            Self::Date => "d".to_string(),
            Self::Time => "t".to_string(),
            Self::Option(inner) => format!("{}?", inner.short_name(structs)),
            Self::Mixed(_) => "Mixed".to_string(),
            Self::Array(inner) => format!("[{}]", inner.short_name(structs)),
            Self::Object(fields) => {
                resolve_struct_name(fields, structs)
                    .unwrap_or_else(|| format!("Object({})", fields.len()))
            }
            other => other.display_name(),
        }
    }

    /// Detailed tooltip for complex types, None if short_name is sufficient
    pub fn tooltip(&self, structs: &[crate::schema::SharedStruct]) -> Option<String> {
        match self {
            Self::Mixed(types) => {
                let names: Vec<String> = types.iter().map(|t| t.short_name(structs)).collect();
                Some(names.join(" | "))
            }
            Self::Option(inner) => inner.tooltip(structs),
            Self::Object(fields) => {
                let field_names: Vec<&str> = fields.keys().map(|s| s.as_str()).collect();
                Some(format!("Fields: {}", field_names.join(", ")))
            }
            _ => None,
        }
    }
}

/// Match an Object's field keys against known structs (80%+ key overlap)
fn resolve_struct_name(
    fields: &BTreeMap<String, InferredType>,
    structs: &[crate::schema::SharedStruct],
) -> Option<String> {
    let obj_keys: std::collections::BTreeSet<&str> = fields.keys().map(|s| s.as_str()).collect();
    if obj_keys.is_empty() {
        return None;
    }

    let mut best: Option<(&str, f64)> = None;
    for s in structs {
        let struct_keys: std::collections::BTreeSet<&str> =
            s.fields.keys().map(|k| k.as_str()).collect();
        let intersection = obj_keys.intersection(&struct_keys).count();
        let union = obj_keys.union(&struct_keys).count();
        if union == 0 {
            continue;
        }
        let similarity = intersection as f64 / union as f64;
        if similarity >= 0.8 {
            if best.is_none() || similarity > best.unwrap().1 {
                best = Some((&s.name, similarity));
            }
        }
    }

    best.map(|(name, _)| name.to_string())
}

/// Override state for temporal detection
#[derive(Debug, Clone, PartialEq)]
pub enum TemporalOverride {
    Auto,
    ForceTemporal,
    ForcePlain,
}

impl Default for TemporalOverride {
    fn default() -> Self {
        Self::Auto
    }
}

/// Infer type from a JSON value without temporal detection — all strings are String.
/// Used by schema inference where temporal promotion causes false Mixed types.
pub fn infer_type_plain(value: &Value) -> InferredType {
    match value {
        Value::Null => InferredType::Null,
        Value::Bool(_) => InferredType::Bool,
        Value::Number(n) => {
            if n.is_i64() {
                InferredType::I64
            } else {
                InferredType::F64
            }
        }
        Value::String(_) => InferredType::String,
        Value::Array(arr) => {
            if arr.is_empty() {
                InferredType::Array(Box::new(InferredType::Unknown))
            } else {
                let inner = unify_types(arr.iter().map(infer_type_plain).collect());
                InferredType::Array(Box::new(inner))
            }
        }
        Value::Object(obj) => {
            let fields: BTreeMap<String, InferredType> = obj
                .iter()
                .map(|(k, v)| (k.clone(), infer_type_plain(v)))
                .collect();
            InferredType::Object(fields)
        }
    }
}

pub fn infer_type(value: &Value) -> InferredType {
    match value {
        Value::Null => InferredType::Null,
        Value::Bool(_) => InferredType::Bool,
        Value::Number(n) => {
            if n.is_i64() {
                InferredType::I64
            } else {
                InferredType::F64
            }
        }
        Value::String(s) => {
            if let Some(temporal) = detect_temporal(s) {
                match temporal {
                    TemporalValue::DateTime(_)
                    | TemporalValue::DateTimeUtc(_)
                    | TemporalValue::NaiveDateTime(_) => InferredType::DateTime,
                    TemporalValue::NaiveDate(_) => InferredType::Date,
                    TemporalValue::NaiveTime(_) => InferredType::Time,
                    TemporalValue::UnixTimestamp(_, _) => InferredType::DateTime,
                }
            } else {
                InferredType::String
            }
        }
        Value::Array(arr) => {
            if arr.is_empty() {
                InferredType::Array(Box::new(InferredType::Unknown))
            } else {
                let inner = unify_types(arr.iter().map(infer_type).collect());
                InferredType::Array(Box::new(inner))
            }
        }
        Value::Object(obj) => {
            let fields: BTreeMap<String, InferredType> = obj
                .iter()
                .map(|(k, v)| (k.clone(), infer_type(v)))
                .collect();
            InferredType::Object(fields)
        }
    }
}

pub fn infer_type_with_overrides(
    value: &Value,
    path: &str,
    overrides: &BTreeMap<String, TemporalOverride>,
) -> InferredType {
    let ovr = overrides.get(path).unwrap_or(&TemporalOverride::Auto);
    match value {
        Value::String(s) => match ovr {
            TemporalOverride::ForcePlain => InferredType::String,
            TemporalOverride::ForceTemporal => {
                if detect_temporal(s).is_some() {
                    InferredType::DateTime
                } else {
                    InferredType::DateTime
                }
            }
            TemporalOverride::Auto => {
                if detect_temporal(s).is_some() {
                    InferredType::DateTime
                } else {
                    InferredType::String
                }
            }
        },
        Value::Number(n) => {
            if let TemporalOverride::ForceTemporal = ovr {
                if let Some(i) = n.as_i64() {
                    if detect_unix_timestamp(i).is_some() {
                        return InferredType::DateTime;
                    }
                }
            }
            if let TemporalOverride::Auto = ovr {
                if let Some(i) = n.as_i64() {
                    if detect_unix_timestamp(i).is_some() {
                        return InferredType::DateTime;
                    }
                }
            }
            infer_type(value)
        }
        _ => infer_type(value),
    }
}

fn unify_types(types: Vec<InferredType>) -> InferredType {
    if types.is_empty() {
        return InferredType::Unknown;
    }

    let mut has_null = false;
    let mut non_null: Vec<InferredType> = Vec::new();

    for t in types {
        match t {
            InferredType::Null => has_null = true,
            InferredType::Unknown => {} // Unknown is "no info" — absorb it
            other => {
                if !non_null.contains(&other) {
                    non_null.push(other);
                }
            }
        }
    }

    // If everything was Unknown/Null, fall back to Unknown
    if non_null.is_empty() && !has_null {
        return InferredType::Unknown;
    }

    // Merge structurally similar types before resorting to Mixed
    if non_null.len() > 1 {
        non_null = merge_structural_types(non_null);
    }

    let base = if non_null.is_empty() {
        InferredType::Null
    } else if non_null.len() == 1 {
        non_null.into_iter().next().unwrap()
    } else {
        InferredType::Mixed(non_null)
    };

    if has_null && base != InferredType::Null {
        InferredType::Option(Box::new(base))
    } else {
        base
    }
}

/// Merge Objects together and Arrays together to reduce false Mixed types.
/// E.g., [Object({a,b,c}), Object({a,b,d})] → [Object({a,b,c?,d?})]
/// E.g., [Array(X), Array(Y)] → [Array(unify(X,Y))]
pub fn merge_structural_types(types: Vec<InferredType>) -> Vec<InferredType> {
    let mut objects: Vec<BTreeMap<String, InferredType>> = Vec::new();
    let mut arrays: Vec<InferredType> = Vec::new();
    let mut others: Vec<InferredType> = Vec::new();

    for t in types {
        match t {
            InferredType::Object(fields) => objects.push(fields),
            InferredType::Array(inner) => arrays.push(*inner),
            InferredType::Mixed(inner_types) => {
                // Flatten nested Mixed — pull out Objects and Arrays
                for it in inner_types {
                    match it {
                        InferredType::Object(fields) => objects.push(fields),
                        InferredType::Array(inner) => arrays.push(*inner),
                        other => others.push(other),
                    }
                }
            }
            other => others.push(other),
        }
    }

    let mut result = others;

    if !objects.is_empty() {
        result.push(merge_objects(objects));
    }

    if !arrays.is_empty() {
        // Recursively unify all array inner types
        let unified_inner = unify_types(arrays);
        result.push(InferredType::Array(Box::new(unified_inner)));
    }

    result
}

/// Merge multiple Object types into one, making fields that don't appear in all objects Optional.
fn merge_objects(objects: Vec<BTreeMap<String, InferredType>>) -> InferredType {
    let all_keys: BTreeSet<String> = objects.iter().flat_map(|o| o.keys().cloned()).collect();
    let total = objects.len();
    let mut merged = BTreeMap::new();

    for key in all_keys {
        let mut field_types = Vec::new();
        let mut present = 0;
        for obj in &objects {
            if let Some(t) = obj.get(&key) {
                field_types.push(t.clone());
                present += 1;
            }
        }
        let unified = unify_types(field_types);
        let field_type = if present < total {
            match unified {
                InferredType::Option(_) | InferredType::Null => unified,
                other => InferredType::Option(Box::new(other)),
            }
        } else {
            unified
        };
        merged.insert(key, field_type);
    }

    InferredType::Object(merged)
}
