use jv::har::{extract_har_files, extract_path_segment, is_hex_like};
use serde_json::json;
use serde_json::Value;

fn make_entry(method: &str, url: &str, status: u64, mime: &str, body: &str) -> Value {
    make_entry_with_opts(method, url, status, mime, body, None, None)
}

fn make_entry_with_opts(
    method: &str,
    url: &str,
    status: u64,
    mime: &str,
    body: &str,
    encoding: Option<&str>,
    post_data: Option<Value>,
) -> Value {
    let mut entry = json!({
        "request": {
            "method": method,
            "url": url,
        },
        "response": {
            "status": status,
            "content": {
                "mimeType": mime,
                "text": body,
            }
        }
    });
    if let Some(enc) = encoding {
        entry["response"]["content"]["encoding"] = json!(enc);
    }
    if let Some(pd) = post_data {
        entry["request"]["postData"] = pd;
    }
    entry
}

fn make_har(entries: Vec<Value>) -> Value {
    json!({ "log": { "entries": entries } })
}

#[test]
fn test_extract_har_files_basic() {
    let entry = make_entry(
        "GET",
        "https://api.example.com/api/v2/flights?origin=LAX&date=2026-03-01",
        200,
        "application/json",
        r#"{"flights":[]}"#,
    );
    let har = make_har(vec![entry]);
    let results = extract_har_files(&har);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "flights_LAX_2026-03-01.json");
}

#[test]
fn test_extract_har_files_post() {
    let entry = make_entry_with_opts(
        "POST",
        "https://example.com/auth/login",
        200,
        "application/json",
        r#"{"token":"abc"}"#,
        None,
        Some(json!({ "text": r#"{"username":"alice"}"# })),
    );
    let har = make_har(vec![entry]);
    let results = extract_har_files(&har);
    assert_eq!(results.len(), 1);
    assert_eq!(results[0].0, "login_alice.json");
}

#[test]
fn test_extract_har_skips_non_json() {
    let entry = make_entry("GET", "https://example.com/image", 200, "image/png", "");
    let har = make_har(vec![entry]);
    let results = extract_har_files(&har);
    assert!(results.is_empty());
}

#[test]
fn test_extract_har_skips_non_2xx() {
    let entry = make_entry(
        "GET",
        "https://example.com/missing",
        404,
        "application/json",
        r#"{"error":"not found"}"#,
    );
    let har = make_har(vec![entry]);
    let results = extract_har_files(&har);
    assert!(results.is_empty());
}

#[test]
fn test_extract_har_skips_base64() {
    let entry = make_entry_with_opts(
        "GET",
        "https://example.com/data",
        200,
        "application/json",
        "eyJrZXkiOiJ2YWx1ZSJ9",
        Some("base64"),
        None,
    );
    let har = make_har(vec![entry]);
    let results = extract_har_files(&har);
    assert!(results.is_empty());
}

#[test]
fn test_extract_path_segment() {
    assert_eq!(
        extract_path_segment("https://api.example.com/api/v2/flights?origin=LAX"),
        "flights"
    );
    assert_eq!(
        extract_path_segment("https://example.com/users/42/bookings?status=confirmed"),
        "bookings"
    );
    assert_eq!(
        extract_path_segment("https://example.com/health"),
        "health"
    );
    assert_eq!(
        extract_path_segment("https://example.com/rest/v1/items"),
        "items"
    );
    assert_eq!(
        extract_path_segment("https://example.com/auth/login"),
        "login"
    );
}

#[test]
fn test_is_hex_like() {
    // UUID
    assert!(is_hex_like("550e8400-e29b-41d4-a716-446655440000"));
    // Long hex string
    assert!(is_hex_like("abcdef1234567890abcdef"));
    // Short string — not hex-like
    assert!(!is_hex_like("LAX"));
    assert!(!is_hex_like("hello"));
    // Short hex but under length threshold
    assert!(!is_hex_like("abcdef"));
}

#[test]
fn test_deduplication() {
    let entry1 = make_entry(
        "GET",
        "https://example.com/api/v2/flights?origin=LAX",
        200,
        "application/json",
        r#"{"id":1}"#,
    );
    let entry2 = make_entry(
        "GET",
        "https://example.com/api/v2/flights?origin=LAX",
        200,
        "application/json",
        r#"{"id":2}"#,
    );
    let har = make_har(vec![entry1, entry2]);
    let results = extract_har_files(&har);
    assert_eq!(results.len(), 2);
    assert_eq!(results[0].0, "flights_LAX.json");
    assert_eq!(results[1].0, "flights_LAX_2.json");
}

#[test]
fn test_excludes_long_tokens() {
    let entry = make_entry(
        "GET",
        "https://example.com/api/data?token=abcdefghijklmnopqrstuvwxyz&lang=en",
        200,
        "application/json",
        r#"{"ok":true}"#,
    );
    let har = make_har(vec![entry]);
    let results = extract_har_files(&har);
    assert_eq!(results.len(), 1);
    // Long token excluded, short "en" kept
    assert_eq!(results[0].0, "data_en.json");
}
