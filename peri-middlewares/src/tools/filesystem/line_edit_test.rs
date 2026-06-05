#[allow(unused_imports)]
fn make_tool(dir: &tempfile::TempDir) -> LineEditTool {
    LineEditTool::new(dir.path().to_str().unwrap())
}

fn make_edit(file: &str, start_line: usize, new_string: &str) -> serde_json::Value {
    serde_json::json!({
        "file_path": file,
        "start_line": start_line,
        "new_string": new_string
    })
}

// ─── 基础替换测试 ──────────────────────────────────────────────────────────

#[tokio::test]
async fn test_line_edit_替换单行() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [make_edit("f.txt", 2, "BBB")]
        }))
        .await
        .unwrap();
    assert!(result.contains("替换"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nBBB\nccc\n");
}

#[tokio::test]
async fn test_line_edit_替换多行() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\nddd\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 2,
                "end_line": 3,
                "new_string": "XXX"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("删除"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nXXX\nddd\n");
}

#[tokio::test]
async fn test_line_edit_删除行() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    tool.invoke(serde_json::json!({
        "edits": [make_edit("f.txt", 2, "")]
    }))
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nccc\n");
}

#[tokio::test]
async fn test_line_edit_插入() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 2,
                "new_string": "xxx\nyyy",
                "insert": true
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("插入"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nxxx\nyyy\nbbb\n");
}

#[tokio::test]
async fn test_line_edit_start_line超出范围() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [make_edit("f.txt", 99, "xxx")]
        }))
        .await
        .unwrap();
    assert!(result.contains("失败"), "best-effort 应包含失败: {result}");
    assert!(result.contains("超出"), "应报超出范围: {result}");
}

#[tokio::test]
async fn test_line_edit_文件不存在() {
    let dir = tempfile::tempdir().unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [make_edit("ghost.txt", 1, "xxx")]
        }))
        .await;
    let err = result.unwrap_err();
    assert!(err.to_string().contains("不存在"), "应报文件不存在: {err}");
}

#[tokio::test]
async fn test_line_edit_end_line小于start_line() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 3,
                "end_line": 1,
                "new_string": "xxx"
            }]
        }))
        .await;
    let output = result.unwrap();
    assert!(output.contains("失败"), "应报告失败: {output}");
}

// ─── start_word / end_word 测试 ────────────────────────────────────────────

#[tokio::test]
async fn test_line_edit_start_word行内替换() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("f.txt"),
        "pub async fn handle(&self, req: Request, config: &Config) -> Result {\n",
    )
    .unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "req:",
                "end_word": "Config)",
                "new_string": "input: Input, opts: Options"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("行内替换"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(
        content,
        "pub async fn handle(&self, input: Input, opts: Options -> Result {\n"
    );
}

#[tokio::test]
async fn test_line_edit_start_word不匹配报错() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "hello world\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "missing",
                "new_string": "xxx"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("失败"), "应报告失败: {result}");
    assert!(result.contains("未在"), "应报告未找到: {result}");
}

#[tokio::test]
async fn test_line_edit_start_word多处匹配报错() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "foo and foo and foo\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "foo",
                "new_string": "bar"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("失败"), "应报告失败: {result}");
    assert!(result.contains("3 处"), "应报告匹配次数: {result}");
}

#[tokio::test]
async fn test_line_edit_end_word定位() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa [remove me] bbb\n").unwrap();
    let tool = make_tool(&dir);
    let _result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "[remove",
                "end_word": "me]",
                "new_string": "kept"
            }]
        }))
        .await
        .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa kept bbb\n");
}

#[tokio::test]
async fn test_line_edit_insert忽略start_word() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [{
                "file_path": "f.txt",
                "start_line": 1,
                "start_word": "ignored",
                "new_string": "xxx",
                "insert": true
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("插入"), "unexpected: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "xxx\naaa\nbbb\n");
}

// ─── 多编辑测试 ────────────────────────────────────────────────────────────

#[tokio::test]
async fn test_line_edit_同文件多编辑从后往前() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "a\nb\nc\nd\ne\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [
                {"file_path": "f.txt", "start_line": 2, "new_string": "BBB"},
                {"file_path": "f.txt", "start_line": 4, "new_string": "DDD"}
            ]
        }))
        .await
        .unwrap();
    assert!(!result.contains("失败"), "不应有失败: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "a\nBBB\nc\nDDD\ne\n");
}

#[tokio::test]
async fn test_line_edit_多编辑前增后减行号稳定() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "a\nb\nc\nd\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [
                {"file_path": "f.txt", "start_line": 3, "new_string": "X\nY\nZ"},
                {"file_path": "f.txt", "start_line": 1, "new_string": "AAA"}
            ]
        }))
        .await
        .unwrap();
    assert!(!result.contains("失败"), "不应有失败: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "AAA\nb\nX\nY\nZ\nd\n");
}

#[tokio::test]
async fn test_line_edit_跨文件多编辑() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "aaa\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "bbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [
                {"file_path": "a.txt", "start_line": 1, "new_string": "AAA"},
                {"file_path": "b.txt", "start_line": 1, "new_string": "BBB"}
            ]
        }))
        .await
        .unwrap();
    assert!(!result.contains("失败"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "AAA\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "BBB\n"
    );
}

#[tokio::test]
async fn test_line_edit_best_effort部分失败() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "edits": [
                {"file_path": "f.txt", "start_line": 1, "new_string": "AAA"},
                {"file_path": "f.txt", "start_line": 99, "new_string": "XXX"}
            ]
        }))
        .await
        .unwrap();
    assert!(result.contains("失败"), "应包含失败: {result}");
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "AAA\nbbb\nccc\n");
}
// ─── CRLF 换行符保留测试 ──────────────────────────────────────────────────

#[tokio::test]
async fn test_line_edit_crlf文件保留换行符() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\r\nbbb\r\nccc\r\n").unwrap();
    let tool = make_tool(&dir);
    tool.invoke(serde_json::json!({
        "edits": [make_edit("f.txt", 2, "BBB")]
    }))
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\r\nBBB\r\nccc\r\n", "CRLF 应被保留");
}

#[tokio::test]
async fn test_line_edit_crlf多行替换保留换行符() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\r\nbbb\r\nccc\r\nddd\r\n").unwrap();
    let tool = make_tool(&dir);
    tool.invoke(serde_json::json!({
        "edits": [{
            "file_path": "f.txt",
            "start_line": 2,
            "end_line": 3,
            "new_string": "XXX"
        }]
    }))
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\r\nXXX\r\nddd\r\n", "CRLF 多行替换应保留");
}

#[tokio::test]
async fn test_line_edit_crlf插入保留换行符() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\r\nbbb\r\n").unwrap();
    let tool = make_tool(&dir);
    tool.invoke(serde_json::json!({
        "edits": [{
            "file_path": "f.txt",
            "start_line": 2,
            "new_string": "xxx\nyyy",
            "insert": true
        }]
    }))
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\r\nxxx\r\nyyy\r\nbbb\r\n", "CRLF 插入应保留");
}

#[tokio::test]
async fn test_line_edit_crlf删除保留换行符() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\r\nbbb\r\nccc\r\n").unwrap();
    let tool = make_tool(&dir);
    tool.invoke(serde_json::json!({
        "edits": [make_edit("f.txt", 2, "")]
    }))
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\r\nccc\r\n", "CRLF 删除应保留");
}

#[tokio::test]
async fn test_line_edit_lf文件不受影响() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    tool.invoke(serde_json::json!({
        "edits": [make_edit("f.txt", 2, "BBB")]
    }))
    .await
    .unwrap();
    let content = std::fs::read_to_string(dir.path().join("f.txt")).unwrap();
    assert_eq!(content, "aaa\nBBB\nccc\n", "LF 文件应保持 LF");
}