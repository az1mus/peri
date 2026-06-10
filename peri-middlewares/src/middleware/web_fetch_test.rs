use super::*;
use std::fs;

#[test]
fn test_truncate_content_超限时触发落盘() {
    // 生成 MAX_CONTENT_LINES + 1 行内容
    let lines: Vec<String> = (0..=MAX_CONTENT_LINES)
        .map(|i| format!("line {i}"))
        .collect();
    let full_content = lines.join("\n");
    let result = truncate_content(&full_content, MAX_CONTENT_LINES);
    // 截断提示存在
    assert!(result.contains("内容已截断"), "应包含截断提示: {result}");
    // 落盘提示存在
    assert!(result.contains("Read"), "应包含 Read 工具提示: {result}");
    // 从提示提取路径并验证文件内容
    let prefix = "saved to ";
    let suffix = " — use Read";
    let path_start = result.find(prefix).expect("应包含 'saved to'") + prefix.len();
    let path_end = result[path_start..]
        .find(suffix)
        .map(|i| path_start + i)
        .unwrap_or(result.len());
    let path = &result[path_start..path_end];
    let saved = fs::read_to_string(path).expect("落盘文件应存在");
    assert_eq!(saved, full_content, "落盘内容应与原始内容完全一致");
    fs::remove_file(path).ok();
}

#[test]
fn test_truncate_content_未超限时不落盘() {
    let content = "line1\nline2\nline3";
    let result = truncate_content(content, MAX_CONTENT_LINES);
    assert_eq!(result, content, "未超限时应原样返回");
    assert!(!result.contains("Read"), "未超限时不应有落盘提示: {result}");
}
