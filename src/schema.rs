use std::collections::{BTreeMap, BTreeSet};

use crate::types::InferredType;

#[derive(Debug, Clone)]
pub struct SharedStruct {
    pub name: String,
    pub fields: BTreeMap<String, InferredType>,
    pub source_files: Vec<String>,
    pub occurrence_count: usize,
}

#[derive(Debug, Clone)]
pub struct SchemaOverview {
    pub structs: Vec<SharedStruct>,
    pub unique_structs: Vec<SharedStruct>,
}

impl SchemaOverview {
    /// All known structs (shared + unique) for type resolution.
    /// Returns references — no cloning.
    pub fn all_structs_ref(&self) -> Vec<&SharedStruct> {
        self.structs.iter().chain(self.unique_structs.iter()).collect()
    }

    /// All known structs (shared + unique) as owned vec. Use sparingly (clones).
    pub fn all_structs(&self) -> Vec<SharedStruct> {
        self.structs.iter().chain(self.unique_structs.iter()).cloned().collect()
    }
}

/// A struct candidate from Phase 2, tagged with its filename group.
struct GroupCandidate {
    name: String,
    fields: BTreeMap<String, InferredType>,
    source_files: Vec<String>,
    occurrence_count: usize,
    group_key: String,
}

impl SchemaOverview {
    pub fn infer(files: &[(String, serde_json::Value)], jaccard_threshold: f32) -> Self {
        // Phase 1: Group files by depluralized first word from filename
        let mut file_groups: BTreeMap<String, Vec<&(String, serde_json::Value)>> = BTreeMap::new();
        for file in files {
            let group_key = crate::codegen::first_normal_word(&file.0)
                .map(|w| crate::codegen::singularize(&w))
                .unwrap_or_else(|| "other".to_string());
            file_groups.entry(group_key).or_default().push(file);
        }

        // Phase 2: Within each group, find structs via Jaccard similarity.
        // Single-file groups still produce candidates (needed for cross-group comparison).
        let mut candidates: Vec<GroupCandidate> = Vec::new();

        for (group_key, group_files) in &file_groups {
            let mut shape_map: BTreeMap<String, Vec<(BTreeMap<String, InferredType>, String)>> =
                BTreeMap::new();

            for (filename, value) in group_files.iter().copied() {
                collect_objects(value, filename, "", &mut shape_map);
            }


            for (context_key, occurrences) in &shape_map {
                if context_key.starts_with("root::") {
                    continue;
                }

                let source_files: Vec<String> = occurrences
                    .iter()
                    .map(|(_, f)| f.clone())
                    .collect::<BTreeSet<_>>()
                    .into_iter()
                    .collect();

                let merged = merge_fields(occurrences);
                let name = derive_struct_name(context_key);


                candidates.push(GroupCandidate {
                    name,
                    fields: merged,
                    source_files,
                    occurrence_count: occurrences.len(),
                    group_key: group_key.clone(),
                });
            }
        }

        // Phase 3: Merge candidates with similar field name sets (Jaccard on names only,
        // ignoring types — types get unified via merge_fields anyway).
        let name_sets: Vec<BTreeSet<String>> = candidates
            .iter()
            .map(|c| c.fields.keys().cloned().collect())
            .collect();

        let mut used = vec![false; candidates.len()];
        let mut shared_structs: Vec<(SharedStruct, String)> = Vec::new();
        let mut deduplicated_unique: Vec<(SharedStruct, String)> = Vec::new();

        for i in 0..candidates.len() {
            if used[i] {
                continue;
            }

            // Find all candidates that should merge: either by Jaccard similarity
            // (single-link clustering) or by having the same derived name within the
            // same group (same parent key = same semantic type with optional fields).
            let mut cluster = vec![i];
            let seed_name = &candidates[i].name;
            let seed_group = &candidates[i].group_key;
            for j in (i + 1)..candidates.len() {
                if used[j] {
                    continue;
                }
                // Same name + same group → always merge (same parent key = same type)
                if candidates[j].name == *seed_name && candidates[j].group_key == *seed_group {
                    cluster.push(j);
                    continue;
                }
                let matches_any = cluster.iter().any(|&ci| {
                    let intersection = name_sets[ci].intersection(&name_sets[j]).count();
                    let union_size = name_sets[ci].union(&name_sets[j]).count();
                    if union_size == 0 {
                        name_sets[ci].is_empty() && name_sets[j].is_empty()
                    } else {
                        (intersection as f64 / union_size as f64) >= jaccard_threshold as f64 - 1e-6
                    }
                });
                if matches_any {
                    cluster.push(j);
                }
            }

            // Determine if shared (spans 2+ groups) or just deduplicated
            let cluster_groups: BTreeSet<&str> = cluster
                .iter()
                .map(|&idx| candidates[idx].group_key.as_str())
                .collect();
            let is_shared = cluster_groups.len() >= 2;

            if cluster.len() < 2 {
                continue;
            }

            for &idx in &cluster {
                used[idx] = true;
            }

            // Merge fields from all cluster members
            let mut all_occurrences: Vec<(BTreeMap<String, InferredType>, String)> = Vec::new();
            let mut all_source_files: BTreeSet<String> = BTreeSet::new();
            let mut total_occurrences: usize = 0;

            for &idx in &cluster {
                total_occurrences += candidates[idx].occurrence_count;
                for f in &candidates[idx].source_files {
                    all_source_files.insert(f.clone());
                    all_occurrences.push((candidates[idx].fields.clone(), f.clone()));
                }
            }

            // Use name and group_key from the highest-occurrence member
            let best_idx = *cluster
                .iter()
                .max_by_key(|&&idx| candidates[idx].occurrence_count)
                .unwrap();
            let best_name = candidates[best_idx].name.clone();
            let best_group_key = candidates[best_idx].group_key.clone();

            let merged_fields = merge_fields(&all_occurrences);
            let merged = (
                SharedStruct {
                    name: best_name,
                    fields: merged_fields,
                    source_files: all_source_files.into_iter().collect(),
                    occurrence_count: total_occurrences,
                },
                best_group_key,
            );
            if is_shared {
                shared_structs.push(merged);
            } else {
                deduplicated_unique.push(merged);
            }
        }


        // Disambiguate shared struct names
        disambiguate_names(&mut shared_structs);
        shared_structs.sort_by(|a, b| b.0.occurrence_count.cmp(&a.0.occurrence_count));

        // Collect unique structs (candidates that weren't part of any cluster)
        let mut unique_structs: Vec<(SharedStruct, String)> = Vec::new();
        for (i, candidate) in candidates.iter().enumerate() {
            if !used[i] {
                unique_structs.push((
                    SharedStruct {
                        name: candidate.name.clone(),
                        fields: candidate.fields.clone(),
                        source_files: candidate.source_files.clone(),
                        occurrence_count: candidate.occurrence_count,
                    },
                    candidate.group_key.clone(),
                ));
            }
        }
        // Add back deduplicated same-group clusters
        unique_structs.extend(deduplicated_unique);

        disambiguate_names(&mut unique_structs);

        // Cross-list disambiguation: if a unique struct name collides with a shared
        // struct name, prefix the unique one with its group key.
        let shared_names: BTreeSet<String> =
            shared_structs.iter().map(|(s, _)| s.name.clone()).collect();
        for (s, group_key) in unique_structs.iter_mut() {
            if shared_names.contains(&s.name) {
                let prefix =
                    crate::codegen::to_pascal_case(&crate::codegen::singularize(group_key));
                s.name = format!("{}{}", prefix, s.name);
            }
        }

        unique_structs.sort_by(|a, b| b.0.occurrence_count.cmp(&a.0.occurrence_count));
        let shared_structs: Vec<SharedStruct> =
            shared_structs.into_iter().map(|(s, _)| s).collect();
        let unique_structs: Vec<SharedStruct> =
            unique_structs.into_iter().map(|(s, _)| s).collect();

        Self {
            structs: shared_structs,
            unique_structs,
        }
    }
}

/// For structs with duplicate names, prepend the PascalCase group key to disambiguate.
fn disambiguate_names(structs: &mut [(SharedStruct, String)]) {
    // Count name occurrences
    let mut name_counts: BTreeMap<String, usize> = BTreeMap::new();
    for (s, _) in structs.iter() {
        *name_counts.entry(s.name.clone()).or_default() += 1;
    }

    // Rename duplicates by prepending group key
    for (s, group_key) in structs.iter_mut() {
        if name_counts[&s.name] > 1 {
            let prefix = crate::codegen::to_pascal_case(&crate::codegen::singularize(group_key));
            s.name = format!("{}{}", prefix, s.name);
        }
    }
}

/// Collect all objects, keyed by their sorted field names.
/// Objects with identical field name sets share the same bucket regardless of
/// parent key or value types.  This is O(1) per object (BTreeMap lookup).
/// Jaccard-based merging of *similar-but-not-identical* field sets is deferred
/// to Phase 3 where the candidate count is small.
fn collect_objects(
    value: &serde_json::Value,
    filename: &str,
    parent_key: &str,
    shapes: &mut BTreeMap<String, Vec<(BTreeMap<String, InferredType>, String)>>,
) {
    match value {
        serde_json::Value::Object(map) => {
            let fields: BTreeMap<String, InferredType> = map
                .iter()
                .map(|(k, v)| (k.clone(), crate::types::infer_type_plain(v)))
                .collect();

            let context = if parent_key.is_empty() {
                "root".to_string()
            } else {
                parent_key.to_string()
            };

            // Key = context::sorted_field_names — groups exact-same-fields objects
            let mut key_sig: Vec<String> = fields.keys().cloned().collect();
            key_sig.sort();
            let sig_key = format!("{}::{}", context, key_sig.join(","));

            shapes
                .entry(sig_key)
                .or_default()
                .push((fields, filename.to_string()));

            for (key, child) in map {
                collect_objects(child, filename, key, shapes);
            }
        }
        serde_json::Value::Array(arr) => {
            for item in arr {
                collect_objects(item, filename, parent_key, shapes);
            }
        }
        _ => {}
    }
}

fn merge_fields(
    occurrences: &[(BTreeMap<String, InferredType>, String)],
) -> BTreeMap<String, InferredType> {
    let all_keys: BTreeSet<String> = occurrences
        .iter()
        .flat_map(|(fields, _)| fields.keys().cloned())
        .collect();

    let total = occurrences.len();
    let mut merged = BTreeMap::new();

    for key in &all_keys {
        let mut types: Vec<InferredType> = Vec::new();
        let mut present_count = 0;

        for (fields, _) in occurrences {
            if let Some(t) = fields.get(key) {
                types.push(t.clone());
                present_count += 1;
            }
        }

        let unified = unify_field_types(types);
        let field_type = if present_count < total {
            // Don't double-wrap — if already Option, keep it as-is
            match unified {
                InferredType::Option(_) | InferredType::Null => unified,
                other => InferredType::Option(Box::new(other)),
            }
        } else {
            unified
        };

        merged.insert(key.clone(), field_type);
    }

    merged
}

fn unify_field_types(types: Vec<InferredType>) -> InferredType {
    if types.is_empty() {
        return InferredType::Unknown;
    }

    let mut unique: Vec<InferredType> = Vec::new();
    let mut has_null = false;
    for t in types {
        // Unwrap Option<T> → set has_null and use T for dedup.
        // This handles re-merging already-merged types (e.g. Option<String> + String → Option<String>).
        let inner = match t {
            InferredType::Null => {
                has_null = true;
                continue;
            }
            InferredType::Unknown => continue, // Unknown = no info, absorb it
            InferredType::Option(inner) => {
                has_null = true;
                *inner
            }
            other => other,
        };
        if inner == InferredType::Unknown {
            continue;
        }
        if !unique.contains(&inner) {
            unique.push(inner);
        }
    }

    if unique.is_empty() {
        return InferredType::Null;
    }

    // Merge structurally similar types (Objects together, Arrays together)
    if unique.len() > 1 {
        unique = crate::types::merge_structural_types(unique);
    }

    if unique.len() == 1 {
        let inner = unique.into_iter().next().unwrap();
        return if has_null {
            InferredType::Option(Box::new(inner))
        } else {
            inner
        };
    }

    if has_null {
        InferredType::Option(Box::new(InferredType::Mixed(unique)))
    } else {
        InferredType::Mixed(unique)
    }
}

fn derive_struct_name(context_key: &str) -> String {
    let parts: Vec<&str> = context_key.split("::").collect();
    let name_part = parts.first().unwrap_or(&"Unknown");

    let singular = crate::codegen::singularize(name_part);

    let mut result = String::new();
    let mut capitalize_next = true;
    for ch in singular.chars() {
        if ch == '_' || ch == '-' || ch == ' ' || ch == '.' {
            capitalize_next = true;
        } else if capitalize_next {
            result.push(ch.to_ascii_uppercase());
            capitalize_next = false;
        } else {
            result.push(ch);
        }
    }

    if result.is_empty() {
        "Unknown".to_string()
    } else if result == "Root" {
        "Root".to_string()
    } else {
        result
    }
}
