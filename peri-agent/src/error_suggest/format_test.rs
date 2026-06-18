use crate::error_suggest::format::{did_you_mean_summary, format_candidates, format_suggestion};
use crate::error_suggest::registry::Suggestion;

#[test]
fn test_format_suggestion_appends_after_separator() {
    let sug = Suggestion::new("建议改用以下路径：\n  • foo.rs");
    let result = format_suggestion("Error: not found", &sug);
    assert!(result.starts_with("Error: not found\n\n---\n"));
    assert!(result.ends_with("\n---"));
    assert!(result.contains("建议改用以下路径"));
}

#[test]
fn test_format_suggestion_with_details() {
    let sug = Suggestion::new("summary").with_details("detail info");
    let result = format_suggestion("err", &sug);
    assert!(result.contains("summary"));
    assert!(result.contains("detail info"));
}

#[test]
fn test_format_candidates_bullet_format() {
    let cands = vec!["a.rs".to_string(), "b.rs".to_string()];
    let result = format_candidates(&cands);
    assert_eq!(result, "  • a.rs\n  • b.rs");
}

#[test]
fn test_did_you_mean_summary_with_candidates() {
    let cands = vec!["a.rs".to_string()];
    let result = did_you_mean_summary("路径", &cands);
    assert!(result.contains("建议改用以下"));
    assert!(result.contains("a.rs"));
}

#[test]
fn test_did_you_mean_summary_empty_candidates() {
    let result = did_you_mean_summary("路径", &[]);
    assert!(result.contains("未找到"));
}
