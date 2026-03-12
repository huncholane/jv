use serde_json::Value;
use std::collections::HashMap;

/// Extracts JSON response bodies from a HAR file, returning (filename, parsed_json) pairs.
pub fn extract_har_files(har: &Value) -> Vec<(String, Value)> {
    let entries = match har.pointer("/log/entries") {
        Some(Value::Array(arr)) => arr,
        _ => return vec![],
    };

    let mut name_counts: HashMap<String, usize> = HashMap::new();
    let mut results = Vec::new();

    for entry in entries {
        // Filter: response status must be 2xx
        let status = entry
            .pointer("/response/status")
            .and_then(|v| v.as_u64())
            .unwrap_or(0);
        if status < 200 || status >= 300 {
            continue;
        }

        // Filter: mimeType must contain "json"
        let mime = entry
            .pointer("/response/content/mimeType")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if !mime.contains("json") {
            continue;
        }

        // Filter: must not be base64 encoded
        let encoding = entry
            .pointer("/response/content/encoding")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if encoding == "base64" {
            continue;
        }

        // Parse response body
        let text = match entry.pointer("/response/content/text").and_then(|v| v.as_str()) {
            Some(t) => t,
            None => continue,
        };
        let parsed: Value = match serde_json::from_str(text) {
            Ok(v) => v,
            Err(_) => continue,
        };

        // Build filename
        let url = entry
            .pointer("/request/url")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let method = entry
            .pointer("/request/method")
            .and_then(|v| v.as_str())
            .unwrap_or("GET");

        let path_seg = extract_path_segment(url);
        let values = collect_distinguishing_values(entry, method, url);

        let base_name = if values.is_empty() {
            path_seg.clone()
        } else {
            format!("{}_{}", path_seg, values.join("_"))
        };

        let count = name_counts.entry(base_name.clone()).or_insert(0);
        *count += 1;
        let filename = if *count == 1 {
            format!("{}.json", base_name)
        } else {
            format!("{}_{}.json", base_name, count)
        };

        results.push((filename, parsed));
    }

    results
}

/// Extracts the last meaningful URL path segment, stripping common API prefixes
/// and numeric-only segments.
fn extract_path_segment(url: &str) -> String {
    // Strip query string and fragment
    let path_part = url.split('?').next().unwrap_or(url);
    let path_part = path_part.split('#').next().unwrap_or(path_part);

    // Extract path from full URL (strip scheme + host)
    let path = if let Some(idx) = path_part.find("://") {
        let after_scheme = &path_part[idx + 3..];
        after_scheme.find('/').map_or("", |i| &after_scheme[i..])
    } else {
        path_part
    };

    let segments: Vec<&str> = path
        .split('/')
        .filter(|s| !s.is_empty())
        .filter(|s| {
            let lower = s.to_lowercase();
            lower != "api" && lower != "rest" && !is_version_segment(s)
        })
        .collect();

    // Walk backwards to find a non-numeric segment
    for seg in segments.iter().rev() {
        if !seg.chars().all(|c| c.is_ascii_digit()) {
            return seg.to_string();
        }
    }

    // Fallback: use last segment even if numeric, or "response"
    segments
        .last()
        .map(|s| s.to_string())
        .unwrap_or_else(|| "response".to_string())
}

/// Collects short, useful param values from the request for naming purposes.
fn collect_distinguishing_values(entry: &Value, method: &str, url: &str) -> Vec<String> {
    let mut values = Vec::new();

    // Extract query string values
    if let Some(query) = url.split('?').nth(1) {
        for pair in query.split('&') {
            let val = pair.split('=').nth(1).unwrap_or("");
            if is_useful_value(val) {
                values.push(val.to_string());
            }
        }
    }

    // For POST, also look at body fields
    if method.eq_ignore_ascii_case("POST") {
        if let Some(text) = entry
            .pointer("/request/postData/text")
            .and_then(|v| v.as_str())
        {
            if let Ok(Value::Object(map)) = serde_json::from_str::<Value>(text) {
                for (_key, val) in &map {
                    if let Some(s) = val.as_str() {
                        if is_useful_value(s) {
                            values.push(s.to_string());
                        }
                    }
                }
            }
        }
    }

    values
}

/// Returns true for version path segments like v1, v2, v10, etc.
fn is_version_segment(s: &str) -> bool {
    let lower = s.to_lowercase();
    if let Some(rest) = lower.strip_prefix('v') {
        !rest.is_empty() && rest.chars().all(|c| c.is_ascii_digit())
    } else {
        false
    }
}

/// Returns true if the value is short, not all digits, and not hex-like.
fn is_useful_value(s: &str) -> bool {
    if s.is_empty() || s.len() > 15 {
        return false;
    }
    if s.chars().all(|c| c.is_ascii_digit()) {
        return false;
    }
    if is_hex_like(s) {
        return false;
    }
    true
}

/// Returns true if the string looks like a UUID or is predominantly hex digits.
fn is_hex_like(s: &str) -> bool {
    // UUID pattern: 8-4-4-4-12 hex digits
    let stripped = s.replace('-', "");
    if stripped.len() == 32 && stripped.chars().all(|c| c.is_ascii_hexdigit()) {
        return true;
    }
    // Long strings with >50% hex digits
    if s.len() > 15 {
        let hex_count = s.chars().filter(|c| c.is_ascii_hexdigit()).count();
        if hex_count as f64 / s.len() as f64 > 0.5 {
            return true;
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

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
}
