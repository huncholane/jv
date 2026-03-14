use jv::jq_engine::JqEngine;

#[test]
fn test_identity() {
    let input: serde_json::Value = serde_json::json!({"a": 1, "b": 2});
    let result = JqEngine::execute(".", &input);
    assert!(result.error.is_none(), "Error: {:?}", result.error);
    assert_eq!(result.output.len(), 1);
}

#[test]
fn test_field_access() {
    let input: serde_json::Value = serde_json::json!({"name": "Alice", "age": 30});
    let result = JqEngine::execute(".name", &input);
    assert!(result.error.is_none(), "Error: {:?}", result.error);
    assert_eq!(result.output, vec!["\"Alice\""]);
}

#[test]
fn test_array_iter() {
    let input: serde_json::Value = serde_json::json!({"items": [1, 2, 3]});
    let result = JqEngine::execute(".items[]", &input);
    assert!(result.error.is_none(), "Error: {:?}", result.error);
    assert_eq!(result.output, vec!["1", "2", "3"]);
}
