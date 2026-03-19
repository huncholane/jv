use jv::widgets::jq_bar::{extract_current_segment, split_at_current_segment, JqBar};

#[test]
fn test_extract_segment_simple() {
    assert_eq!(extract_current_segment(".users"), ".users");
    assert_eq!(extract_current_segment(".users.na"), ".users.na");
}

#[test]
fn test_extract_segment_pipe() {
    assert_eq!(extract_current_segment(".users[] | .na"), ".na");
}

#[test]
fn test_split_at_segment() {
    let (before, seg) = split_at_current_segment(".users[] | .na");
    assert_eq!(before, ".users[] | ");
    assert_eq!(seg, ".na");
}

#[test]
fn test_split_at_segment_simple() {
    let (before, seg) = split_at_current_segment(".users");
    assert_eq!(before, "");
    assert_eq!(seg, ".users");
}

#[test]
fn test_completions_from_root_dot() {
    let root: serde_json::Value = serde_json::json!({"users": [], "metadata": {}});
    let mut view = JqBar::new();
    view.query = ".".to_string();
    view.rebuild_completions(&root);
    assert!(!view.completions.is_empty(), "rebuild: completions should not be empty for '.'");
    assert!(view.completions.iter().any(|c| c.contains("users")), "should contain .users");
}

#[test]
fn test_completions_from_empty() {
    let root: serde_json::Value = serde_json::json!({"foo": 1, "bar": 2});
    let mut view = JqBar::new();
    view.query = String::new();
    view.rebuild_completions(&root);
    assert!(!view.completions.is_empty(), "rebuild: completions should not be empty for empty query");
}

#[test]
fn test_apply_completion_from_dot() {
    let root: serde_json::Value = serde_json::json!({"users": [{"id": 1}]});
    let mut view = JqBar::new();
    view.query = ".".to_string();
    view.rebuild_completions(&root);
    assert!(!view.completions.is_empty());
    let comp = view.completions[0].clone();
    view.apply_completion(&comp);
    assert!(view.query.starts_with('.'), "query should start with dot, got: {:?}", view.query);
}

#[test]
fn test_completions_all_start_with_dot() {
    let root: serde_json::Value = serde_json::json!({"users": [{"id": 1}], "metadata": {"version": "1.0"}});
    let mut view = JqBar::new();
    view.query = ".".to_string();
    view.rebuild_completions(&root);
    assert!(!view.completions.is_empty());
    for c in &view.completions {
        assert!(c.starts_with('.'), "completion should start with dot: {:?}", c);
    }
}

#[test]
fn test_completions_array_root() {
    let root: serde_json::Value = serde_json::json!([{"alternate2": "val", "name": "test"}]);
    let mut view = JqBar::new();
    view.query = ".".to_string();
    view.rebuild_completions(&root);
    assert!(!view.completions.is_empty());
    assert!(view.completions.iter().any(|c| c == ".[]"), "should contain .[], got: {:?}", view.completions);
    assert!(view.completions.iter().any(|c| c == ".[].alternate2"), "should contain .[].alternate2, got: {:?}", view.completions);
    for c in &view.completions {
        assert!(c.starts_with('.'), "completion should start with dot: {:?}", c);
    }
}

#[test]
fn test_fuzzy_search() {
    let root: serde_json::Value = serde_json::json!({"users": [{"id": 1, "username": "test"}], "metadata": {}});
    let mut view = JqBar::new();
    view.query = ".usrn".to_string(); // fuzzy for "username"
    view.rebuild_completions(&root);
    assert!(view.completions.iter().any(|c| c.contains("username")),
        "fuzzy should match username, got: {:?}", view.completions);
}
