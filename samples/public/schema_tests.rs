// Public sample tests — run against committed data in samples/public/

const SAMPLES_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/samples/public");

fn load_samples() -> Vec<(String, serde_json::Value)> {
    let dir = std::path::Path::new(SAMPLES_DIR);
    let mut files: Vec<(String, serde_json::Value)> = Vec::new();
    for entry in std::fs::read_dir(dir).expect("read samples dir") {
        let entry = entry.expect("read dir entry");
        let path = entry.path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let content = std::fs::read_to_string(&path).expect("read file");
            let value: serde_json::Value =
                serde_json::from_str(&content).expect("parse JSON");
            files.push((
                path.file_name().unwrap().to_string_lossy().to_string(),
                value,
            ));
        }
    }
    assert!(!files.is_empty(), "No .json files found in {SAMPLES_DIR}");
    files
}

fn infer_all() -> (SchemaOverview, Vec<SharedStruct>) {
    let files = load_samples();
    let overview = SchemaOverview::infer(&files, 0.8);
    let all = overview.all_structs();
    (overview, all)
}

#[test]
fn test_no_duplicate_struct_names() {
    let (_overview, all) = infer_all();
    let mut name_counts: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    for s in &all {
        *name_counts.entry(s.name.clone()).or_default() += 1;
    }
    let duplicates: Vec<String> = name_counts
        .iter()
        .filter(|(_, &count)| count > 1)
        .map(|(name, count)| format!("  {} appears {} times", name, count))
        .collect();
    assert!(
        duplicates.is_empty(),
        "Found duplicate struct names:\n{}",
        duplicates.join("\n")
    );
}

#[test]
fn test_no_mixed_types() {
    let (_overview, all) = infer_all();
    let mut violations = Vec::new();
    for s in &all {
        for (field, typ) in &s.fields {
            let short = typ.short_name(&all);
            if short.contains("Mixed") {
                violations.push(format!("  {}.{}: {}", s.name, field, short));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Found Mixed types in struct fields:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_all_objects_resolve_to_named_structs() {
    let (_overview, all) = infer_all();
    let mut violations = Vec::new();
    for s in &all {
        for (field, typ) in &s.fields {
            let short = typ.short_name(&all);
            if short.contains("Object(") {
                violations.push(format!("  {}.{}: {}", s.name, field, short));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "Found unresolved Object(N) types:\n{}",
        violations.join("\n")
    );
}

#[test]
fn test_no_duplicate_field_sets() {
    let files = load_samples();
    let overview = SchemaOverview::infer(&files, 0.8);

    let all_structs: Vec<&SharedStruct> = overview
        .structs
        .iter()
        .chain(overview.unique_structs.iter())
        .collect();

    let mut by_fields: std::collections::BTreeMap<Vec<String>, Vec<&SharedStruct>> = std::collections::BTreeMap::new();
    for s in &all_structs {
        let key: Vec<String> = s.fields.keys().cloned().collect();
        by_fields.entry(key).or_default().push(s);
    }

    let mut duplicates = Vec::new();
    for (fields, structs) in &by_fields {
        if structs.len() > 1 {
            let names: Vec<&str> = structs.iter().map(|s| s.name.as_str()).collect();
            duplicates.push(format!(
                "  {:?} all have fields: {:?}",
                names, fields
            ));
        }
    }

    assert!(
        duplicates.is_empty(),
        "Found {} duplicate field sets:\n{}",
        duplicates.len(),
        duplicates.join("\n")
    );
}

#[test]
fn test_no_high_jaccard_pairs() {
    let files = load_samples();
    let overview = SchemaOverview::infer(&files, 0.8);

    let all_structs: Vec<&SharedStruct> = overview
        .structs
        .iter()
        .chain(overview.unique_structs.iter())
        .collect();

    let pair_sets: Vec<std::collections::BTreeSet<String>> = all_structs
        .iter()
        .map(|s| s.fields.keys().cloned().collect())
        .collect();

    let mut violations = Vec::new();
    for i in 0..all_structs.len() {
        for j in (i + 1)..all_structs.len() {
            let intersection = pair_sets[i].intersection(&pair_sets[j]).count();
            let union = pair_sets[i].union(&pair_sets[j]).count();
            if union == 0 {
                continue;
            }
            let sim = intersection as f64 / union as f64;
            if sim >= 0.8 {
                violations.push(format!(
                    "  {} ({} fields) vs {} ({} fields): Jaccard={:.3}",
                    all_structs[i].name,
                    pair_sets[i].len(),
                    all_structs[j].name,
                    pair_sets[j].len(),
                    sim,
                ));
            }
        }
    }

    assert!(
        violations.is_empty(),
        "Found {} struct pairs with Jaccard >= 0.8 that should have been merged:\n{}",
        violations.len(),
        violations.join("\n")
    );
}

#[test]
fn test_shared_structs_detected() {
    let (overview, _all) = infer_all();
    assert!(
        !overview.structs.is_empty(),
        "Expected shared structs across user/event/product files"
    );
}

#[test]
fn test_edge_cases_no_panic() {
    let content = std::fs::read_to_string(
        std::path::Path::new(SAMPLES_DIR).join("edge_cases.json")
    ).expect("read edge_cases.json");
    let value: serde_json::Value = serde_json::from_str(&content).expect("parse");
    let files = vec![("edge_cases.json".to_string(), value)];
    let overview = SchemaOverview::infer(&files, 0.8);
    let _all = overview.all_structs();
}

#[test]
fn test_temporal_detection() {
    let files = load_samples();
    let overview = SchemaOverview::infer(&files, 0.8);
    let all = overview.all_structs();

    let mut temporal_fields = Vec::new();
    for s in &all {
        for (field, typ) in &s.fields {
            let short = typ.short_name(&all);
            if field.contains("date") || field.contains("time") || field.contains("_at")
                || field.contains("login") || field.contains("created") || field.contains("modified")
            {
                temporal_fields.push((s.name.clone(), field.clone(), short));
            }
        }
    }

    assert!(
        !temporal_fields.is_empty(),
        "Expected temporal fields to be detected in sample data"
    );
}
