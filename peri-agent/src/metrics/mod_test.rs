use super::*;

#[test]
fn test_truncate_short_string_unchanged() {
    let val = serde_json::json!({"error": "short"});
    let result = truncate_json_strings(val);
    assert_eq!(result["error"], "short");
}

#[test]
fn test_truncate_long_string() {
    let long: String = "x".repeat(600);
    let val = serde_json::json!({"error": long});
    let result = truncate_json_strings(val);
    assert_eq!(result["error"].as_str().unwrap().chars().count(), 500);
}

#[test]
fn test_truncate_cjk_string() {
    let long: String = "你".repeat(600);
    let val = serde_json::json!({"error": long});
    let result = truncate_json_strings(val);
    assert_eq!(result["error"].as_str().unwrap().chars().count(), 500);
}

#[test]
fn test_truncate_nested_object() {
    let long: String = "a".repeat(600);
    let val = serde_json::json!({"data": {"nested": long, "ok": "short"}, "arr": [long]});
    let result = truncate_json_strings(val);
    assert_eq!(
        result["data"]["nested"].as_str().unwrap().chars().count(),
        500
    );
    assert_eq!(result["data"]["ok"], "short");
    assert_eq!(result["arr"][0].as_str().unwrap().chars().count(), 500);
}

#[test]
fn test_truncate_non_string_unchanged() {
    let val = serde_json::json!({"count": 42, "flag": true, "null": null});
    let result = truncate_json_strings(val);
    assert_eq!(result["count"], 42);
    assert_eq!(result["flag"], true);
    assert!(result["null"].is_null());
}

#[test]
fn test_today_format() {
    let date = today();
    assert_eq!(date.len(), 10);
    assert!(date.contains('-'));
}
