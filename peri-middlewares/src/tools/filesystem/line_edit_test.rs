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
async fn test_crlf保留() {
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

// ═══════════════════════════════════════════════════════════════
// 复现测试：match 块插入新 arm 产生冗余 _ => {} 分支
// ═══════════════════════════════════════════════════════════════

/// 复现场景：match 块插入新 arm 前，多 hunk diff 产生冗余 _ => {}
///
/// Bug 描述：在 match 块的 _ => { 通配分支之前插入新 Esc handler arm，
/// 同时有增加 import 和函数调用的其他 hunks。LineEdit 成功应用后，
/// 结果中多了一个空的 _ => {} 在原始 _ => { 之前。
#[tokio::test]
async fn test_复现_match块插入新arm_多hunk产生冗余通配分支() {
    let dir = tempfile::tempdir().unwrap();
    // 构造一个类似 normal_keys.rs match 块结构的文件
    let original = r#"use crate::app::App;

fn handle_keys(app: &mut App, input: Input) {
    match input {
        Input { key: Key::Char('c'), ctrl: true, .. } => {
            handle_ctrl_c(app);
        }
        Input { key: Key::Char('u'), ctrl: true, .. } => {
            scroll_up(app);
        }
        Input {
            key: Key::Char('d'),
            ctrl: true,
            ..
        } => {
            scroll_down(app);
        }
        // Intercept plain Enter to avoid textarea default newline
        input if input.key != Key::Enter => {
            app.textarea.input(input);
            if !app.loading {
                update_hints(app);
            }
        }
        _ => {
            // Any other key cancels quit-pending state
            app.quit_pending = None;
        }
    }
}

fn update_hints(app: &mut App) {
    // existing hint update
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // 3 个 hunks: (1) 加 import, (2) 在 _ => { 前插入 Esc arm, (3) 加函数调用
    // 精确行号：文件有 31 行 (1-based)
    // Hunk 1: 在 use crate::app::App 之后加 use std::rc::Rc
    // Hunk 2: 在 _ => { 之前插入新的 Esc arm
    // Hunk 3: 在 update_hints 函数中添加调用
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,4 @@
 use crate::app::App;
+use std::rc::Rc;
 
 fn handle_keys(app: &mut App, input: Input) {
@@ -20,6 +25,12 @@
                 update_hints(app);
             }
         }
+        // Esc: 关闭 slash hint 弹窗
+        Input { key: Key::Esc, .. } if app.slash_hint.active => {
+            app.slash_hint.deactivate();
+        }
+
         _ => {
             // Any other key cancels quit-pending state
             app.quit_pending = None;
@@ -30,4 +41,5 @@
 
 fn update_hints(app: &mut App) {
     // existing hint update
+    app.slash_hint.deactivate();
 }
"#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    // 验证结果
    eprintln!("=== LineEdit Result ===");
    eprintln!("{result}");
    let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
    eprintln!("=== Output File ===");
    eprintln!("{output}");

    // 检查是否存在冗余的 _ => {}（额外的通配臂）
    let wildcard_count = output
        .lines()
        .filter(|l| l.trim().starts_with("_ =>"))
        .count();
    assert_eq!(
        wildcard_count, 1,
        "预期只有 1 个 _ => 臂，实际有 {} 个\n文件内容:\n{}",
        wildcard_count, output
    );
}

/// 变体：单 hunk 仅插入新 arm，排除其他 hunks 干扰
#[tokio::test]
async fn test_复现_单hunk插入match臂_无冗余() {
    let dir = tempfile::tempdir().unwrap();
    let original = r#"fn handle_keys(app: &mut App, input: Input) {
    match input {
        Key::Char('a') => { do_a(); }
        Key::Char('b') => { do_b(); }
        _ => { do_default(); }
    }
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // 唯一 hunk：在 _ => { 前插入新 arm
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -2,3 +2,7 @@
         Key::Char('a') => { do_a(); }
         Key::Char('b') => { do_b(); }
+        // New arm
+        Key::Char('c') => { do_c(); }
+
         _ => { do_default(); }
     }
"#;

    let _result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
    eprintln!("=== Single Hunk Output ===");
    eprintln!("{output}");

    // 检查是否有重复的 _ =>
    let wildcard_count = output
        .lines()
        .filter(|l| l.trim().starts_with("_ =>"))
        .count();
    assert_eq!(
        wildcard_count, 1,
        "预期只有 1 个 _ => 臂，实际有 {} 个\n文件内容:\n{}",
        wildcard_count, output
    );
}

/// 变体：尝试用带有 _ => {} 单行臂的文件 + 插入操作
#[tokio::test]
async fn test_复现_单行通配臂_插入新arm() {
    let dir = tempfile::tempdir().unwrap();
    let original = r#"fn handle(input: Input) {
    match input {
        Key::Char('a') => do_a(),
        Key::Char('b') => do_b(),
        _ => {}
    }
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // 在 _ => {} 前插入新 arm
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -2,3 +2,6 @@
         Key::Char('a') => do_a(),
         Key::Char('b') => do_b(),
+        Key::Char('c') => do_c(),
         _ => {}
     }
"#;

    let _result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
    eprintln!("=== Single-Line Wildcard Output ===");
    eprintln!("{output}");

    // 检查 _ => {} 只出现 1 次
    let wildcard_count = output
        .lines()
        .filter(|l| l.trim() == "_ => {}" || l.trim().starts_with("_ => {"))
        .count();
    assert_eq!(
        wildcard_count, 1,
        "预期只有 1 个 _ => {{}}，实际有 {} 个\n文件内容:\n{}",
        wildcard_count, output
    );
}

/// 变体：尝试 hunks 之间有重叠上下文的情况
/// 
/// 这个测试模拟当两个 hunks 共享部分 context 行时，
/// 底层匹配和应用逻辑可能产生意外结果。
#[tokio::test]
async fn test_复现_重叠上下文hunks() {
    let dir = tempfile::tempdir().unwrap();
    // 构造文件：第 9-10 行的 _ => { 和第 7-8 行是重叠区域
    let lines: Vec<&str> = vec![
        "line1",
        "line2",
        "line3",
        "    a => {",
        "        do_a();",
        "    }",
        "    b => {",
        "        do_b();",
        "    }",
        "    _ => {",
        "        fallback();",
        "    }",
        "line13",
    ];
    let original = lines.join("\n") + "\n";
    std::fs::write(dir.path().join("test.txt"), &original).unwrap();

    let tool = make_tool(&dir);

    // Hunk 1: 在 _ => { 之前插入 c arm
    // Hunk 2: 在 _ => { 体内添加函数调用
    // 两个 hunks 都引用 _ => { 作为 context，可能产生歧义
    let diff = r#"--- a/test.txt
+++ b/test.txt
@@ -8,3 +7,7 @@
         do_b();
     }
+    c => {
+        do_c();
+    }
+
     _ => {
@@ -11,2 +15,3 @@
         fallback();
+        extra_call();
     }
"#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.txt",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    eprintln!("=== Overlap Hunks Result ===");
    eprintln!("{result}");
    let output = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
    eprintln!("=== Overlap Hunks Output ===");
    eprintln!("{output}");

    // 检查 _ => { 只出现 1 次
    let wildcard_count = output
        .lines()
        .filter(|l| l.trim_start().starts_with("_ =>"))
        .count();
    assert_eq!(
        wildcard_count, 1,
        "_ => 臂不应重复，当前 {} 个\n文件内容:\n{}",
        wildcard_count, output
    );
}
/// 变体：构造 where 上下文行过长（包含 wildcard body 闭合），可能与文件其他位置匹配
#[tokio::test]
async fn test_复现_长上下文包含通配臂body闭合() {
    let dir = tempfile::tempdir().unwrap();
    // 文件：match 块中有 3 个 arm，wildcard body 以 } 结束
    let original = r#"fn handle(input: Input) {
    match input {
        Key::A => {
            a();
        }
        Key::B => {
            b();
        }
        _ => {
            c();
        }
    }
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // Hunk 上下文包含 Key::B 的 } + _ => { + body + } 整个 wildcard body
    // 这样 old_lines 中有两个 } 行（第一个是 arm 闭合，第二个是 wildcard body 闭合）
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -6,7 +6,13 @@
         Key::B => {
             b();
         }
+        // New arm
+        Key::C => {
+            new_handler();
+        }
+
         _ => {
             c();
         }
"#;

    let _result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
    eprintln!("=== Long Context Output ===");
    eprintln!("{output}");

    let wildcard_count = output
        .lines()
        .filter(|l| l.trim().starts_with("_ =>"))
        .count();
    assert_eq!(
        wildcard_count, 1,
        "预期只有 1 个 _ => 臂，实际有 {} 个\n文件内容:\n{}",
        wildcard_count, output
    );
}

/// 变体：hunk header 行号错误 + 多 hunks 重叠范围
///
/// 当 hunk header @@ 行号与实际匹配位置有偏差时，L1-L4 失败后 L5 兜底，
/// 如果 line_idx 错误，splice 可能删除或保留不应该的代码行
#[tokio::test]
async fn test_复现_hunk行号偏移_多hunk重叠() {
    let dir = tempfile::tempdir().unwrap();
    let original = r#"use std::io;

fn handle(input: Input) {
    match input {
        Key::A => {
            a();
        }
        Key::B => {
            b();
        }
        _ => {
            c();
        }
    }
    finish();
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // Hunk 1: 加 import (行号正确)
    // Hunk 2: 插入 arm — 但 @@ 行号故意错误（header 行号偏大 1）
    // Hunk 3: 函数调用 (行号正确)
    // 
    // 关键：hunk 2 的 context 行在文件中 L1 match 到正确位置（因为内容唯一），
    // 但 header @@ 行号不匹配 — 不过这不影响，因为 L1 先于 L5 执行
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,4 @@
 use std::io;
+use std::rc::Rc;
 
 fn handle(input: Input) {
@@ -8,2 +13,8 @@
         }
+        // New arm
+        Key::C => {
+            c_handler();
+        }
+
         _ => {
@@ -16,2 +22,3 @@
     finish();
+    extra_cleanup();
 }
"#;

    let _result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
    eprintln!("=== Offset Header Output ===");
    eprintln!("{output}");

    let wildcard_count = output
        .lines()
        .filter(|l| l.trim_start().starts_with("_ =>"))
        .count();
    assert_eq!(
        wildcard_count, 1,
        "预期只有 1 个 _ => 臂，实际有 {} 个\n文件内容:\n{}",
        wildcard_count, output
    );
}

/// 变体：hunk 的 old_lines 在文件中精确匹配到唯一位置但 apply 阶段 splice 范围
/// 包含了不应该被替换的行
///
/// 场景：hunk 匹配位置正确，但 old_count 因 diff 格式问题导致多算/少算行数。
/// 例如，当 diff 在 hunk 末尾包含了一个空 Context 行时。
#[tokio::test]
async fn test_复现_空context尾行导致oldcount偏差() {
    let dir = tempfile::tempdir().unwrap();
    // 使用简单文本测试 old_count 计算
    let original = "line1\nline2\nline3\nline4\nline5\nline6\n";
    std::fs::write(dir.path().join("test.txt"), original).unwrap();

    let tool = make_tool(&dir);

    // Hunk context 末尾有空行 — old_count 可能多数一行
    let diff = r#"--- a/test.txt
+++ b/test.txt
@@ -2,2 +2,5 @@
 line2
+inserted_a
+inserted_b
+
 line3
"#;

    let _result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.txt",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    let output = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
    eprintln!("=== Empty Context Line Output ===");
    eprintln!("{output}");
    eprintln!("=== Original ===");
    eprintln!("{original}");

    // 预期: line1, line2, inserted_a, inserted_b, line3, line4, line5, line6
    assert!(
        output.contains("line3") && output.contains("line4"),
        "line3 和 line4 应该都保留\n文件内容:\n{}",
        output
    );
}

/// 变体：当 diff 使用 - 移除 _ => { 再 + 添加回来时，是否会误判
#[tokio::test]
async fn test_复现_remove然后add通配臂() {
    let dir = tempfile::tempdir().unwrap();
    let original = r#"fn f() {
    match x {
        A => a(),
        _ => {
            default();
        }
    }
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // 使用 -_ => { ... } 再 + 的方式插入新 arm
    // 这模拟 LLM 可能会把 _ => { 标记为移除，然后作为新行添加回来
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -2,5 +2,9 @@
         A => a(),
-        _ => {
-            default();
-        }
+        B => b(),
+        _ => {
+            default();
+        }
     }
"#;

    let _result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
    eprintln!("=== Remove-Add Wildcard Output ===");
    eprintln!("{output}");

    let wildcard_count = output
        .lines()
        .filter(|l| l.trim_start() == "_ => {" || l.trim_start() == "_ => {}")
        .count();
    assert!(
        wildcard_count <= 1,
        "预期最多 1 个 _ => {{}}，实际有 {} 个\n文件内容:\n{}",
        wildcard_count, output
    );
}

/// 变体：精确模拟报告中描述的 3-hunk 场景
/// 关键特征：
/// 1. Hunk 1 在顶部加 import  
/// 2. Hunk 2 在 match 块中 _ => { 之前插入 Esc handler
/// 3. Hunk 3 在另一个函数中加调用
/// 4. 所有 hunks 的 context 都精确匹配
///
/// 此测试旨在确认在完全正确的 diff 下，工具是否产生正确结果。
/// 如果此测试失败，说明工具存在 bug；如果通过，说明原始报告的问题
/// 可能源于 diff 内容本身的错误。
#[tokio::test]
async fn test_复现_精确模拟原始报告场景() {
    let dir = tempfile::tempdir().unwrap();
    // 模拟 normal_keys.rs 的简化版
    let original = r#"use crate::app::App;
use super::Action;

pub(super) fn handle_normal_keys(app: &mut App, input: Input) -> anyhow::Result<Option<Action>> {
    match input {
        Input { key: Key::Char('c'), ctrl: true, .. } => {
            handle_ctrl_c(app);
        }
        Input { key: Key::Char('u'), ctrl: true, .. } => {
            scroll_up(app);
        }
        Input { key: Key::Char('d'), ctrl: true, .. } => {
            scroll_down(app);
        }
        Input { key: Key::Delete, .. } if !app.ui.loading => {
            pop_attachment(app);
        }
        input if input.key != Key::Enter => {
            app.ui.textarea.input(input);
            if !app.ui.loading {
                update_at_mention(app);
                update_slash_hint(app);
            }
        }
        _ => {
            app.quit_pending = None;
        }
    }
    Ok(Some(Action::Redraw))
}

fn update_at_mention(app: &mut App) { /* stub */ }
fn update_slash_hint(app: &mut App) { /* stub */ }
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // 3 hunks diff，严格对齐行号
    // Hunk 1: 在 use super::Action 之后加 use std::rc::Rc  
    // Hunk 2: 在 _ => { 之前插入 Esc handler
    // Hunk 3: 在 update_slash_hint 函数中加 deactivate 调用
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -2,3 +2,4 @@
 use super::Action;
+use std::rc::Rc;
 
 pub(super) fn handle_normal_keys(app: &mut App, input: Input) -> anyhow::Result<Option<Action>> {
@@ -22,6 +23,12 @@
                 update_slash_hint(app);
             }
         }
+        // Esc: 关闭 slash hint 弹窗
+        Input { key: Key::Esc, .. } if app.ui.slash_hint.active => {
+            app.ui.slash_hint.deactivate();
+        }
+
         _ => {
             app.quit_pending = None;
         }
@@ -32,3 +39,4 @@
 fn update_slash_hint(app: &mut App) { /* stub */ }
+fn deactivate_slash_hint(app: &mut App) { app.ui.slash_hint.deactivate(); }
 "#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
    eprintln!("=== Exact Simulation Result ===");
    eprintln!("{result}");
    eprintln!("=== Exact Simulation Output ===");
    eprintln!("{output}");

    // 检查是否有 spurious _ => {}
    let wildcard_lines: Vec<(usize, &str)> = output
        .lines()
        .enumerate()
        .filter(|(_, l)| l.trim().starts_with("_ =>"))
        .collect();

    assert!(
        wildcard_lines.len() <= 1,
        "检测到多余的 _ => {{}} 或 _ => {{ 分支！wildcard_lines: {:?}\n文件内容:\n{}",
        wildcard_lines, output
    );
}

/// 变体：多 hunk 共享同一行 context  
/// 
/// 两个 hunks 的 old_lines 都匹配到文件中同一位置（或极近的位置），
/// 一个 hunk 应用后会改变另一个 hunk 的匹配位置。
/// bottom-to-top 排序理论上应处理此场景，但需验证。
#[tokio::test]
async fn test_复现_共享context行两hunk同位置() {
    let dir = tempfile::tempdir().unwrap();
    // 文件：两行简单的数据
    let original = "header\nAAA\nBBB\nCCC\nfooter\n";
    std::fs::write(dir.path().join("test.txt"), original).unwrap();

    let tool = make_tool(&dir);

    // 两个 hunks 都引用 "BBB" 作为 context
    // Hunk A: 在 BBB 前插入 XXX（改变 BBB 的位置）
    // Hunk B: 替换 BBB 为 DDD
    let diff = r#"--- a/test.txt
+++ b/test.txt
@@ -2,2 +2,4 @@
 AAA
+XXX
+
 BBB
@@ -3,1 +5,1 @@
-BBB
+DDD
 "#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.txt",
                "diff": diff
            }]
        }))
        .await
        .unwrap();

    let output = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
    eprintln!("=== Shared Context Output ===");
    eprintln!("{output}");
    eprintln!("=== Shared Context Result ===");
    eprintln!("{result}");

    // 预期: header, AAA, XXX, (blank), DDD, CCC, footer
    // 不应有 BBB 残留，也不应有重复
    assert!(
        output.contains("DDD"),
        "输出应包含 DDD:\n{}",
        output
    );
    let bbb_count = output.lines().filter(|l| *l == "BBB").count();
    assert_eq!(
        bbb_count, 0,
        "BBB 应被完全替换，不应残留\n文件内容:\n{}",
        output
    );
}

/// 复现关键测试：两个重叠 hunks 导致 wildcard 被复制
///
/// 场景：Hunk A 在 _ => {} 前插入新 arm（context 包含 _ => {}），
/// Hunk B 修改 _ => {} 展开为多行体。
/// 由于 bottom-to-top 应用，Hunk B 先应用，展开 _ => {} 为 { body }，
/// 然后 Hunk A 应用时其 replacement_lines 中包含原始的 _ => {}（单行 context），
/// splice 覆盖掉了 Hunk B 的修改结果，重新引入单行 _ => {}。
///
/// 结果：文件中同时存在 _ => {}（Hunk A 的 context）和 Hunk B 的 body 残留。
#[tokio::test]
async fn test_复现_重叠hunk导致通配臂被复制() {
    let dir = tempfile::tempdir().unwrap();
    // 文件：match 块，wildcard 是单行 _ => {}
    let original = r#"fn handle(input: Input) {
    match input {
        Key::A => { a(); }
        _ => {}
    }
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // Hunk A: 在 _ => {} 之前插入新 arm Key::B
    //   context: "    Key::A => { a(); }" + "    _ => {}"
    // Hunk B: 展开 _ => {} 为多行 body
    //   context: "    _ => {}" (与 Hunk A 的尾部 context 重叠！)
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -2,2 +2,5 @@
         Key::A => { a(); }
+        Key::B => { b(); }
+
         _ => {}
@@ -4,1 +7,3 @@
-    _ => {}
+    _ => {
+        default_handler();
+    }
"#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await;

    match result {
        Ok(msg) => {
            let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
            eprintln!("=== Overlap Wildcard Result ===");
            eprintln!("{msg}");
            eprintln!("=== Overlap Wildcard Output ===");
            eprintln!("{output}");

            // 关键检查：是否有重复的 _ =>
            let wildcard_list: Vec<(usize, &str)> = output
                .lines()
                .enumerate()
                .filter(|(_, l)| l.trim_start().starts_with("_ =>"))
                .collect();
            eprintln!("=== _ => lines: {:?}", wildcard_list);

            // 预期：只应有 1 个 _ => {（展开后的，带 body）
            // bug 情况下：可能有 _ => {} 和 _ => { 混合
            if wildcard_list.len() > 1 {
                eprintln!("BUG REPRODUCED! 通配分支被复制！");
            }
            assert_eq!(
                wildcard_list.len(),
                1,
                "通配分支不应被复制，当前 {} 个\nwildcard lines: {:?}\n文件内容:\n{}",
                wildcard_list.len(), wildcard_list, output
            );
        }
        Err(e) => {
            eprintln!("=== Overlap Wildcard Error ===");
            eprintln!("{e}");
            // 匹配失败也可能发生（因为两个 hunks 共享 context）
        }
    }
}

/// 最小复现：两个 hunks 修改同一区域（但不共享 context）
/// 验证 bottom-to-top splice 逻辑在重叠区域的正确性
#[tokio::test]
async fn test_复现_最小重叠两hunk() {
    let dir = tempfile::tempdir().unwrap();
    let original = "line1\nline2\nline3\n";
    std::fs::write(dir.path().join("test.txt"), original).unwrap();

    let tool = make_tool(&dir);

    // Hunk A: 在 line2 后插入 content_a
    // context: line2
    // Hunk B: 在 line2 后插入 content_b  
    // context: line2（与 Hunk A 相同！）
    let diff = r#"--- a/test.txt
+++ b/test.txt
@@ -2,1 +2,2 @@
 line2
+content_a
@@ -2,1 +2,2 @@
 line2
+content_b
"#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.txt",
                "diff": diff
            }]
        }))
        .await;

    match result {
        Ok(msg) => {
            let output = std::fs::read_to_string(dir.path().join("test.txt")).unwrap();
            eprintln!("=== Min Overlap Result ===");
            eprintln!("{msg}");
            eprintln!("=== Min Overlap Output ===");
            eprintln!("{output}");

            // 预期：line1, line2, content_a, content_b, line3 (或 content_b, content_a)
            // 注意：两个 hunk 都匹配位置 1 (line2)
            // 因为 bottom-to-top 中相同 line_idx 保持稳定顺序，后 hunk 先应用
            // Hunk B (第二个，先应用): splice(1..2, [line2, content_b]) → [line1, line2, content_b, line3]
            // Hunk A (第一个，后应用): splice(1..2, [line2, content_a]) → [line1, line2, content_a, content_b, line3]
            // 等等，Hunk A 的 old_count=1，从 line_idx=1 开始 splice
            // 在 [line1, line2, content_b, line3] 中 splice(1..2, [line2, content_a])
            // → [line1, line2, content_a, content_b, line3] ✓ 
            eprintln!("Note: This behavior depends on stable sort for equal line_idx");
        }
        Err(e) => {
            eprintln!("=== Min Overlap Error ===");
            eprintln!("{e}");
        }
    }
}

/// 关键复现：重叠 hunks 产生 spurious line，且通过验证
///
/// 场景：两个 hunks 修改 match 块的不同位置但共享 context 行。
/// 当 bottom-to-top 应用时，先应用的 hunk 修改了文件行，
/// 后应用的 hunk 用其 replacement_lines（含原始 context 副本）
/// 覆盖了前一 hunk 的修改。
/// 
/// 此测试针对问题：在 _ => { 之前插入新 arm 时，另一个 hunk 如果
/// 修改了 wildcard body 内的行，且两个 hunks 共享足够多的 context，
/// 则 Hunk 2 的 splice 可能重新引入原始的 _ => { 行。
#[tokio::test]
async fn test_复现_重叠hunk_通配臂上下文被重插() {
    let dir = tempfile::tempdir().unwrap();
    // 文件：wildcard 是单行 _ => {}，有两个前导 arm
    let original = r#"fn handle(key: Key) {
    match key {
        Key::A => {
            a();
        }
        Key::B => {
            b();
        }
        _ => {}
    }
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // Hunk 1: 在 _ => {} 前插入新 arm Key::C
    //   context 包括 Key::B 的 } + _ => {}
    // Hunk 2: 在 Key::B 的 body 中插入 new_func() 
    //   context: "            b();" + "        }"
    //
    // 两个 hunks 的 context 都包含 Key::B 的 }，但匹配位置不同：
    // Hunk 1 匹配在 Key::B 的 } 和 _ => {} 处
    // Hunk 2 匹配在 b(); 和 Key::B }
    //
    // 应用：bottom-to-top，先应用位置更高的 Hunk 2（修改 b body），
    // 再应用 Hunk 1（修改 Key::B 的 } 到 _ => {} 区域）。
    // Hunk 1 的 replacement_lines 包含原始 "        }"（Key::B 的 }），
    // 如果 overlap 导致 Hunk 2 插入的 new_func() 被覆盖，则丢失修改。
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -6,2 +6,6 @@
         Key::B => {
+            // 新注释
         }
+        Key::C => {
+            c();
+        }
+
         _ => {}
@@ -4,1 +5,2 @@
             a();
+            extra();
         }
"#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await;

    match result {
        Ok(msg) => {
            let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
            eprintln!("=== Result ===");
            eprintln!("{msg}");
            eprintln!("=== Output ===");
            eprintln!("{output}");

            let wildcard_list: Vec<(usize, &str)> = output
                .lines()
                .enumerate()
                .filter(|(_, l)| l.trim_start().starts_with("_ =>"))
                .collect();
            if wildcard_list.len() > 1 {
                eprintln!("!!! BUG REPRODUCED: 通配分支被复制 !!!");
            }
        }
        Err(e) => {
            eprintln!("=== Error ===");
            eprintln!("{e}");
        }
    }
}

/// 变体：3 个 hunks 同时修改同一 match 块，构造可能产生冗余分支的场景
///
/// 模拟原始报告：Hunk1 加 import，Hunk2 插入 Esc arm，Hunk3 加函数调用。
/// 关键：Hunk2 和 Hunk3 的 context 范围有重叠，导致 splice 相互干扰。
#[tokio::test]
async fn test_复现_三hunk重叠产生冗余分支() {
    let dir = tempfile::tempdir().unwrap();
    // 模拟更像 normal_keys.rs 的结构
    let original = r#"use crate::app::App;

fn handle_keys(app: &mut App, input: Input) {
    match input {
        Input { key: Key::Char('c'), ctrl: true, .. } => {
            handle_c(app);
        }
        Input { key: Key::Char('u'), ctrl: true, .. } => {
            scroll_up(app);
        }
        Input { key: Key::Char('d'), ctrl: true, .. } => {
            scroll_down(app);
        }
        input if input.key != Key::Enter => {
            text_input(app, input);
            update_hints(app);
        }
        _ => {
            app.quit_pending = None;
        }
    }
}

fn update_hints(app: &mut App) {
    do_hints(app);
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // 3 hunks，精心构造以让 Hunk 2 和 Hunk 3 的 context 重叠：
    // Hunk 1: 加 import
    // Hunk 2: 在 _ => { 之前插入 Esc arm
    //   context 包含了 _ => { 和前面的 }
    // Hunk 3: 修改 _ => { 体内的一行（添加 deactivate 调用）
    //   注意：Hunk 2 的 replacement_lines 包含原始 _ => { 行，
    //   如果 Hunk 3 先应用（更高行号），展开 _ => { 的 body，
    //   然后 Hunk 2 应用时 replacement_lines 中的原始 _ => { 
    //   会覆盖 Hunk 3 添加的内容
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -1,3 +1,4 @@
 use crate::app::App;
+use std::rc::Rc;
 
 fn handle_keys(app: &mut App, input: Input) {
@@ -18,3 +19,9 @@
             update_hints(app);
         }
+        // Esc: 关闭 slash hint
+        Input { key: Key::Esc, .. } if app.slash_hint.active => {
+            app.slash_hint.deactivate();
+        }
+
         _ => {
             app.quit_pending = None;
@@ -17,2 +17,3 @@
             text_input(app, input);
+            pre_update(app);
             update_hints(app);
"#;

    // 先按原样匹配看看能否 match
    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await;

    match result {
        Ok(msg) => {
            let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
            eprintln!("=== 3-hunk Result ===");
            eprintln!("{msg}");
            eprintln!("=== 3-hunk Output ===");
            eprintln!("{output}");

            let wildcard_list: Vec<(usize, &str)> = output
                .lines()
                .enumerate()
                .filter(|(_, l)| l.trim_start().starts_with("_ =>"))
                .collect();
            eprintln!("=== _ => lines: {:?}", wildcard_list);
            if wildcard_list.len() > 1 {
                eprintln!("!!! BUG REPRODUCED in 3-hunk scenario !!!");
            }
        }
        Err(e) => {
            eprintln!("=== 3-hunk Error ===");
            eprintln!("{e}");
        }
    }
}

/// 最终尝试：构造 bracket-balanced overlapping 场景产生 spurious 行
///
/// 关键思路：
/// 1. wildcard 是单行 _ => {}（self-balanced）
/// 2. Hunk A (先应用，higher line_idx)：展开 _ => {} 为多行 body
///    - _ => {} 与 { body } 具有相同的 bracket 效果 (都 +1{,-1})
/// 3. Hunk B (后应用，lower line_idx)：在 _ => {} 前插入新 arm
///    - replacement_lines 包含原始 _ => {} 作为 context
///    - 由于 overlap，_ => {} 重新插入，但 expand 后的 body 残留
/// 4. 关键：Hunk A 的 body expansion 和 Hunk B 的 context overlap
///    必须使得最终 bracket balance = 0
///
/// 这需要 Hunk B 的 old_count 精确覆盖 expand 后的部分内容
#[tokio::test]
async fn test_复现_平衡重叠_单行通配臂被复制() {
    let dir = tempfile::tempdir().unwrap();
    // 设计：wildcard 单行 _ => {}，后面紧接 match 结束 }
    // Hunk B 在 _ => {} 之前插入 arm，context 包括 _ => {} 和最后的 }
    // Hunk A 展开 _ => {} 为多行 body
    //
    // Bottom-to-top: Hunk A (line 3) 先应用，expand _ => {}
    // Hunk B (line 2) 后应用，用 context 副本覆盖 Hunk A 的修改
    let original = r#"fn go() {
    match x {
        A => a(),
        _ => {}
    }
}
"#;
    // line 0: fn go() {
    // line 1:     match x {
    // line 2:         A => a(),
    // line 3:         _ => {}
    // line 4:     }
    // line 5: }

    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // Hunk A (line 3): expand _ => {} to multi-line
    // Hunk B (line 2-4): insert B before _ => {}, context covers line 2-4
    // 关键：Hunk B 的 old_count 包括 _ => {} + match_closing }，
    // Hunk A 展开 _ => {} 不改变后续行（old_count=1, replacement=3 lines）
    // Hunk B old_count=3 (lines 2-4: "A => a()", "_ => {}", "}")
    // After Hunk A: lines 2, {expand 3 lines}, 4,5
    // Hunk B splice(2, 5 original? No — line_idx still 2, old_count=3):
    //   In modified file: lines[2] = "A => a()", lines[3-5] = expand body
    //   splice(2..5, [A => a(), new B, blank, _ => {}, }])
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -3,1 +3,4 @@
-        _ => {}
+        _ => {
+            d();
+        }
@@ -2,3 +2,7 @@
         A => a(),
+        B => b(),
+
         _ => {}
     }
"#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await;

    match result {
        Ok(msg) => {
            let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
            eprintln!("=== BALANCED OVERLAP Result ===");
            eprintln!("{msg}");
            eprintln!("=== BALANCED OVERLAP Output ===");
            for (i, l) in output.lines().enumerate() {
                eprintln!("{:3}: {}", i, l);
            }

            let wildcard_list: Vec<(usize, &str)> = output
                .lines()
                .enumerate()
                .filter(|(_, l)| l.contains("_ =>"))
                .collect();
            eprintln!("=== _ => lines: {:?}", wildcard_list);
            if wildcard_list.len() > 1 {
                eprintln!("!!! BUG REPRODUCED: 通配分支被复制 !!!");
            }
            assert!(
                wildcard_list.len() <= 1,
                "通配分支不应被复制，当前 {} 个\n{:?}",
                wildcard_list.len(), wildcard_list
            );
        }
        Err(e) => {
            eprintln!("=== BALANCED OVERLAP Error ===");
            eprintln!("{e}");
        }
    }
}

/// 基准对比：两个 hunk 都独立应用，不重叠
/// 验证非重叠场景的正确行为
#[tokio::test]
async fn test_复现_非重叠两hunk_基准正确性() {
    let dir = tempfile::tempdir().unwrap();
    let original = r#"fn go() {
    match x {
        A => a(),
        B => b(),
        _ => {}
    }
}
"#;
    std::fs::write(dir.path().join("test.rs"), original).unwrap();

    let tool = make_tool(&dir);

    // 两个 hunks 修改不同的 arm，不共享 context
    let diff = r#"--- a/test.rs
+++ b/test.rs
@@ -2,2 +2,5 @@
         A => a(),
+        // 插入注释
+
         B => b(),
@@ -4,1 +7,3 @@
-        _ => {}
+        _ => {
+            d();
+        }
"#;

    let result = tool
        .invoke(serde_json::json!({
            "patches": [{
                "file_path": "test.rs",
                "diff": diff
            }]
        }))
        .await;

    match result {
        Ok(msg) => {
            let output = std::fs::read_to_string(dir.path().join("test.rs")).unwrap();
            eprintln!("=== BASELINE Result ===");
            eprintln!("{msg}");
            eprintln!("=== BASELINE Output ===");
            eprintln!("{output}");

            let wildcard_count = output
                .lines()
                .filter(|l| l.contains("_ =>"))
                .count();
            assert_eq!(
                wildcard_count, 1,
                "非重叠场景通配分支不应被复制，当前 {} 个",
                wildcard_count
            );
        }
        Err(e) => {
            eprintln!("=== BASELINE Error ===");
            eprintln!("{e}");
        }
    }
}
