#[allow(unused_imports)]
fn make_tool(dir: &tempfile::TempDir) -> LineEditTool {
    LineEditTool::new(dir.path().to_str().unwrap())
}

// ─── 1. 基础功能：单 hunk 替换 ──────────────────────────────────────

#[tokio::test]
async fn test_单hunk替换() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n aaa\n-bbb\n+BBB\n ccc"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("✓"), "应标记成功: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\nBBB\nccc\n"
    );
}

// ─── 2. 多 hunk 同文件（从后往前应用）─────────────────────────────────

#[tokio::test]
async fn test_多hunk同文件() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join("f.txt"),
        "aaa\nbbb\nccc\nddd\neee\nfff\nggg\nhhh\n",
    )
    .unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n aaa\n-bbb\n+BBB\n ccc\n@@ -6,3 +6,3 @@\n fff\n-ggg\n+GGG\n hhh"
            }]
        }))
        .await
        .unwrap();
    assert!(!result.contains("✗"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\nBBB\nccc\nddd\neee\nfff\nGGG\nhhh\n"
    );
}

// ─── 3. 跨文件多 patch ──────────────────────────────────────────────

#[tokio::test]
async fn test_跨文件多patch() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "aaa\nbbb\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "xxx\nyyy\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [
                {"file_path": "a.txt", "diff": "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n aaa\n-bbb\n+BBB"},
                {"file_path": "b.txt", "diff": "--- a/b.txt\n+++ b/b.txt\n@@ -1,2 +1,2 @@\n xxx\n-yyy\n+YYY"}
            ]
        }))
        .await
        .unwrap();
    assert!(!result.contains("✗"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "aaa\nBBB\n"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "xxx\nYYY\n"
    );
}

// ─── 4. 插入新行（纯 + 行在中间插入）─────────────────────────────────

#[tokio::test]
async fn test_插入新行() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -2,2 +2,3 @@\n bbb\n+xxx\n ccc"
            }]
        }))
        .await
        .unwrap();
    assert!(!result.contains("✗"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\nbbb\nxxx\nccc\n"
    );
}

// ─── 5. 删除行（纯 - 行）─────────────────────────────────────────────

#[tokio::test]
async fn test_删除行() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\nddd\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,4 +1,2 @@\n aaa\n-bbb\n-ccc\n ddd"
            }]
        }))
        .await
        .unwrap();
    assert!(!result.contains("✗"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\nddd\n"
    );
}

// ─── 6. 原子性：匹配失败 ────────────────────────────────────────────

#[tokio::test]
async fn test_原子性_匹配失败() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("a.txt"), "aaa\nbbb\n").unwrap();
    std::fs::write(dir.path().join("b.txt"), "xxx\nyyy\n").unwrap();
    let tool = make_tool(&dir);
    // 第 2 个 patch 行号 99 远超文件长度，匹配失败
    let result = tool
        .invoke(serde_json::json!({
            "patches": [
                {"file_path": "a.txt", "diff": "--- a/a.txt\n+++ b/a.txt\n@@ -1,2 +1,2 @@\n aaa\n-bbb\n+BBB"},
                {"file_path": "b.txt", "diff": "--- a/b.txt\n+++ b/b.txt\n@@ -99,1 +99,1 @@\n-zzz\n+YYY"}
            ]
        }))
        .await
        .unwrap();
    assert!(result.contains("✗"), "匹配失败应含错误标记: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("a.txt")).unwrap(),
        "aaa\nbbb\n",
        "原子性：a.txt 不应被修改"
    );
    assert_eq!(
        std::fs::read_to_string(dir.path().join("b.txt")).unwrap(),
        "xxx\nyyy\n",
        "原子性：b.txt 不应被修改"
    );
}

// ─── 7. 原子性：验证失败（括号不平衡）─────────────────────────────────

#[tokio::test]
async fn test_原子性_验证失败() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "line1\nline2\nline3\n").unwrap();
    let tool = make_tool(&dir);
    // 替换 line2 → { 导致花括号不平衡
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n line1\n-line2\n+{\n line3"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("✗") || result.contains("验证失败"), "验证失败应含错误标记: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "line1\nline2\nline3\n",
        "验证失败时文件不应被修改"
    );
}

// ─── 8. 匹配降级：L2 空白归一化 ────────────────────────────────────

#[tokio::test]
async fn test_匹配降级_空白() {
    let dir = tempfile::tempdir().unwrap();
    // 文件中 bbb 后有 tab，diff 中没有 → L1 精确匹配失败，L2 空白归一化成功
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\t\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n aaa\n-bbb\n+BBB\n ccc"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("✓"), "应成功: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\nBBB\nccc\n"
    );
}

// ─── 9. 匹配降级：L5 行号兜底 ──────────────────────────────────────

#[tokio::test]
async fn test_匹配降级_行号兜底() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    // diff 内容与文件完全不同，L1-L4 全部失败，L5 行号兜底成功
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -2,1 +2,1 @@\n-wrong_content\n+BBB"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("✓"), "应成功: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\nBBB\nccc\n"
    );
}

// ─── 10. 匹配失败：行号超出文件长度 ────────────────────────────────

#[tokio::test]
async fn test_匹配失败_报错() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\n").unwrap();
    let tool = make_tool(&dir);
    // hunk header 行号 99 远超文件 1 行，L1-L5 全部失败
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -99,1 +99,1 @@\n-xxx\n+BBB"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("✗"), "行号超出应含错误标记: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\n",
        "文件不应被修改"
    );
}

// ─── 11. 反馈：含验证标签 ──────────────────────────────────────────

#[tokio::test]
async fn test_反馈_含验证标签() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n aaa\n-bbb\n+BBB\n ccc"
            }]
        }))
        .await
        .unwrap();
    assert!(result.contains("sanity:"), "应包含 sanity 标签: {result}");
    assert!(
        result.contains("brackets:"),
        "应包含 brackets 标签: {result}"
    );
    assert!(result.contains("ast:"), "应包含 ast 标签: {result}");
}

// ─── 12. CRLF 换行符保留 ────────────────────────────────────────────

#[tokio::test]
async fn test_CRLF保留() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\r\nbbb\r\nccc\r\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,3 @@\n aaa\n-bbb\n+BBB\n ccc"
            }]
        }))
        .await
        .unwrap();
    assert!(!result.contains("✗"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\r\nBBB\r\nccc\r\n",
        "CRLF 换行符应被保留"
    );
}

// ─── 13. 空 diff 报错 ──────────────────────────────────────────────

#[tokio::test]
async fn test_空diff报错() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{"file_path": "f.txt", "diff": ""}]
        }))
        .await;
    assert!(result.is_err(), "空 diff 应返回错误");
    let msg = result.unwrap_err().to_string();
    assert!(
        msg.contains("空") || msg.contains("empty") || msg.contains("Empty"),
        "错误信息应提及空内容: {msg}"
    );
}

// ─── 14. 文件不存在 ────────────────────────────────────────────────

#[tokio::test]
async fn test_文件不存在() {
    let dir = tempfile::tempdir().unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "ghost.txt",
                "diff": "--- a/ghost.txt\n+++ b/ghost.txt\n@@ -1,1 +1,1 @@\n-old\n+new"
            }]
        }))
        .await;
    assert!(result.is_err(), "文件不存在应返回错误");
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("不存在"), "应报文件不存在: {msg}");
}

// ─── 15. 替换整个文件 ──────────────────────────────────────────────

#[tokio::test]
async fn test_替换整个文件() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\nccc\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -1,3 +1,1 @@\n-aaa\n-bbb\n-ccc\n+brand new"
            }]
        }))
        .await
        .unwrap();
    assert!(!result.contains("✗"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "brand new\n"
    );
}

// ─── 16. 末尾追加 ──────────────────────────────────────────────────

#[tokio::test]
async fn test_末尾追加() {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("f.txt"), "aaa\nbbb\n").unwrap();
    let tool = make_tool(&dir);
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "f.txt",
                "diff": "--- a/f.txt\n+++ b/f.txt\n@@ -2,1 +2,2 @@\n bbb\n+ccc"
            }]
        }))
        .await
        .unwrap();
    assert!(!result.contains("✗"), "不应有失败: {result}");
    assert_eq!(
        std::fs::read_to_string(dir.path().join("f.txt")).unwrap(),
        "aaa\nbbb\nccc\n"
    );
}
