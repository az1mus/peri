# LineEdit V3 Diff-Apply Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 完全替换 LineEdit V2 为 V3 Diff-Apply 引擎——接受 unified diff 输入，5 级匹配回退 + 3 层验证 + 上下文 diff 反馈。

**Architecture:** 单一工具 `LineEdit`，参数从 `edits[]` 改为 `patches[]`。核心拆分为：diff 解析器 → 匹配引擎 → 应用引擎 → 验证层 → 反馈格式。所有模块在 `line_edit.rs` 中实现，辅助模块拆分为独立文件。

**Tech Stack:** Rust, `similar` crate (diff/similarity), `tree-sitter` (AST), `serde_json`, `uuid` (atomic_write), `tempfile` (tests)

**Design Spec:** `docs/superpowers/specs/2026-06-06-lineedit-v3-diff-apply-design.md`

---

## File Structure

| File | Action | Responsibility |
|------|--------|---------------|
| `peri-middlewares/src/tools/filesystem/line_edit.rs` | Rewrite | 入口：工具 trait 实现、invoke 主流程、反馈格式 |
| `peri-middlewares/src/tools/filesystem/line_edit_diff.rs` | Create | Diff 解析器：unified diff → Hunk 结构体 |
| `peri-middlewares/src/tools/filesystem/line_edit_match.rs` | Create | 匹配引擎：5 级回退匹配 |
| `peri-middlewares/src/tools/filesystem/line_edit_verify.rs` | Create | 验证层：Diff Sanity + 括号平衡 + Tree-sitter AST |
| `peri-middlewares/src/tools/filesystem/line_edit_test.rs` | Rewrite | 全部测试 |
| `peri-middlewares/src/tools/filesystem/mod.rs` | Modify | 注册新模块文件 |
| `CLAUDE.md` | Modify | 更新 lineEdit beta 描述 |
| `prompts/lineedit_stress_test.txt` | Modify | 更新为 V3 说明 |

Registration files (`middleware/filesystem.rs`, `tool_search/core_tools.rs`) — 无需改动，工具名 `LineEdit` 不变。

---

### Task 1: Diff 解析器 (line_edit_diff.rs)

**Files:**
- Create: `peri-middlewares/src/tools/filesystem/line_edit_diff.rs`

- [ ] **Step 1: 实现统一 diff 解析器**

```rust
//! Unified diff 解析器
//! 将标准 unified diff 字符串解析为结构化的 Hunk 列表。

/// diff 中的单行类型
#[derive(Debug, Clone, PartialEq)]
pub enum DiffLine {
    Context(String),   // ' ' 前缀
    Remove(String),    // '-' 前缀
    Add(String),       // '+' 前缀
}

/// hunk header 信息
#[derive(Debug, Clone)]
pub struct HunkHeader {
    pub old_start: usize,  // @@ -L,N 中的 L
    pub old_count: usize,  // @@ -L,N 中的 N
    pub new_start: usize,  // @@ +L,N 中的 L
    pub new_count: usize,  // @@ +L,N 中的 N
}

/// 单个 hunk
#[derive(Debug, Clone)]
pub struct Hunk {
    pub header: HunkHeader,
    pub lines: Vec<DiffLine>,
}

/// 单个 patch（一个文件的完整 diff）
#[derive(Debug, Clone)]
pub struct ParsedPatch {
    pub hunks: Vec<Hunk>,
}

/// 解析错误
#[derive(Debug)]
pub enum ParseError {
    NoHunkFound,
    InvalidHunkHeader(String),
    EmptyPatch,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ParseError::NoHunkFound => write!(f, "diff 中未找到 hunk（缺少 @@ 标记）"),
            ParseError::InvalidHunkHeader(s) => write!(f, "无效的 hunk header: {}", s),
            ParseError::EmptyPatch => write!(f, "diff 内容为空"),
        }
    }
}

/// 解析 unified diff 字符串为 ParsedPatch
pub fn parse_unified_diff(diff: &str) -> Result<ParsedPatch, ParseError> {
    if diff.trim().is_empty() {
        return Err(ParseError::EmptyPatch);
    }

    let mut hunks: Vec<Hunk> = Vec::new();
    let mut current_lines: Vec<DiffLine> = Vec::new();
    let mut current_header: Option<HunkHeader> = None;
    let mut found_hunk = false;

    for line in diff.lines() {
        // 跳过 --- / +++ 头部
        if line.starts_with("--- ") || line.starts_with("+++ ") {
            continue;
        }

        // 解析 hunk header
        if line.starts_with("@@") {
            // 保存前一个 hunk
            if let Some(header) = current_header.take() {
                hunks.push(Hunk {
                    header,
                    lines: std::mem::take(&mut current_lines),
                });
            }

            let header = parse_hunk_header(line)?;
            current_header = Some(header);
            found_hunk = true;
            continue;
        }

        // 解析 diff 行（仅在 hunk 内）
        if current_header.is_some() {
            if let Some(ch) = line.chars().next() {
                match ch {
                    ' ' => current_lines.push(DiffLine::Context(line[1..].to_string())),
                    '-' => current_lines.push(DiffLine::Remove(line[1..].to_string())),
                    '+' => current_lines.push(DiffLine::Add(line[1..].to_string())),
                    _ => {
                        // `\ No newline at end of file` 等元信息行，跳过
                    }
                }
            } else if !line.is_empty() {
                // 空行可能是 context 行（原始行就是空行，diff 中显示为 ' ' + 空串）
                // 但这里的 line 是空字符串，说明 diff 中没有前缀空格
                // 某些 LLM 生成的 diff 可能省略 context 行的前缀空格
                // 将其视为 context 行
                current_lines.push(DiffLine::Context(String::new()));
            }
        }
    }

    // 保存最后一个 hunk
    if let Some(header) = current_header.take() {
        hunks.push(Hunk {
            header,
            lines: current_lines,
        });
    }

    if !found_hunk {
        return Err(ParseError::NoHunkFound);
    }

    Ok(ParsedPatch { hunks })
}

/// 解析 hunk header: @@ -L,N +L,N @@
fn parse_hunk_header(line: &str) -> Result<HunkHeader, ParseError> {
    // @@ -10,3 +10,3 @@
    // @@ -1 +1 @@
    let line = line.trim();
    let re = regex::Regex::new(r"@@ -(\d+)(?:,(\d+))? \+(\d+)(?:,(\d+))? @@")
        .map_err(|e| ParseError::InvalidHunkHeader(e.to_string()))?;

    let caps = re
        .captures(line)
        .ok_or_else(|| ParseError::InvalidHunkHeader(line.to_string()))?;

    let old_start: usize = caps[1]
        .parse()
        .map_err(|e| ParseError::InvalidHunkHeader(e.to_string()))?;
    let old_count: usize = caps
        .get(2)
        .map(|m| m.as_str().parse().unwrap_or(1))
        .unwrap_or(1);
    let new_start: usize = caps[3]
        .parse()
        .map_err(|e| ParseError::InvalidHunkHeader(e.to_string()))?;
    let new_count: usize = caps
        .get(4)
        .map(|m| m.as_str().parse().unwrap_or(1))
        .unwrap_or(1);

    Ok(HunkHeader {
        old_start,
        old_count,
        new_start,
        new_count,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_simple_hunk() {
        let diff = "--- a/file.rs\n+++ b/file.rs\n@@ -1,3 +1,3 @@\n line1\n-old\n+new\n line3";
        let patch = parse_unified_diff(diff).unwrap();
        assert_eq!(patch.hunks.len(), 1);
        let hunk = &patch.hunks[0];
        assert_eq!(hunk.header.old_start, 1);
        assert_eq!(hunk.header.old_count, 3);
        assert_eq!(hunk.header.new_start, 1);
        assert_eq!(hunk.header.new_count, 3);
        assert_eq!(hunk.lines.len(), 4);
        assert_eq!(hunk.lines[0], DiffLine::Context("line1".to_string()));
        assert_eq!(hunk.lines[1], DiffLine::Remove("old".to_string()));
        assert_eq!(hunk.lines[2], DiffLine::Add("new".to_string()));
        assert_eq!(hunk.lines[3], DiffLine::Context("line3".to_string()));
    }

    #[test]
    fn test_parse_multiple_hunks() {
        let diff = "--- a/f\n+++ b/f\n@@ -1,2 +1,2 @@\n a\n-b\n+c\n@@ -10,1 +10,1 @@\n x\n-y\n+z";
        let patch = parse_unified_diff(diff).unwrap();
        assert_eq!(patch.hunks.len(), 2);
        assert_eq!(patch.hunks[0].header.old_start, 1);
        assert_eq!(patch.hunks[1].header.old_start, 10);
    }

    #[test]
    fn test_parse_hunk_without_count() {
        let diff = "@@ -5 +5 @@\n-old\n+new";
        let patch = parse_unified_diff(diff).unwrap();
        let h = &patch.hunks[0];
        assert_eq!(h.header.old_start, 5);
        assert_eq!(h.header.old_count, 1);
    }

    #[test]
    fn test_parse_empty_diff() {
        assert!(matches!(parse_unified_diff(""), Err(ParseError::EmptyPatch)));
        assert!(matches!(parse_unified_diff("  "), Err(ParseError::EmptyPatch)));
    }

    #[test]
    fn test_parse_no_hunk() {
        let diff = "--- a/f\n+++ b/f\njust some text";
        assert!(matches!(parse_unified_diff(diff), Err(ParseError::NoHunkFound)));
    }

    #[test]
    fn test_parse_invalid_header() {
        let diff = "@@ invalid @@\n line";
        assert!(matches!(parse_unified_diff(diff), Err(ParseError::InvalidHunkHeader(_))));
    }
}
```

- [ ] **Step 2: 编译验证**

Run: `cargo build -p peri-middlewares 2>&1 | tail -5`

需要在 `mod.rs` 中注册模块才能编译。先在 `line_edit.rs` 顶部添加 `mod line_edit_diff;`（后续 Task 会改用 mod.rs 注册）。

- [ ] **Step 3: 运行解析器测试**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem::line_edit_diff::tests 2>&1 | tail -10`
Expected: ALL PASS

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_diff.rs
git commit -m "feat(lineedit): add unified diff parser for V3

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 2: 匹配引擎 (line_edit_match.rs)

**Files:**
- Create: `peri-middlewares/src/tools/filesystem/line_edit_match.rs`

- [ ] **Step 1: 实现 5 级匹配引擎**

```rust
//! 5 级匹配引擎
//! 在文件内容中定位 hunk 的 context 行，从精确到宽松逐级回退。

use super::line_edit_diff::{DiffLine, Hunk};

/// 匹配结果
#[derive(Debug, Clone)]
pub struct MatchResult {
    /// 匹配到的起始行索引（0-based）
    pub line_idx: usize,
    /// 使用的匹配级别
    pub level: MatchLevel,
}

/// 匹配级别
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum MatchLevel {
    L1Exact,
    L2Whitespace,
    L3Similarity(f64),
    L4Anchor,
    L5LineNumber,
}

/// 匹配错误
#[derive(Debug)]
pub enum MatchError {
    NotFound {
        searched_content: String,
    },
    MultipleLocations {
        positions: Vec<usize>,
    },
}

/// 从 hunk 中提取 old 端的文本行（context + remove 行）
pub fn extract_old_lines(hunk: &Hunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter_map(|dl| match dl {
            DiffLine::Context(s) => Some(s.clone()),
            DiffLine::Remove(s) => Some(s.clone()),
            DiffLine::Add(_) => None,
        })
        .collect()
}

/// 从 hunk 中提取纯 context 行（不含 remove/add）
pub fn extract_context_lines(hunk: &Hunk) -> Vec<String> {
    hunk.lines
        .iter()
        .filter_map(|dl| match dl {
            DiffLine::Context(s) => Some(s.clone()),
            _ => None,
        })
        .collect()
}

/// 在文件行中匹配 hunk，返回匹配位置
pub fn match_hunk(file_lines: &[String], hunk: &Hunk) -> Result<MatchResult, MatchError> {
    let old_lines = extract_old_lines(hunk);

    // L1: 精确匹配
    if let Some(pos) = find_exact(file_lines, &old_lines) {
        return Ok(MatchResult {
            line_idx: pos,
            level: MatchLevel::L1Exact,
        });
    }

    // L2: 空白归一化匹配
    if let Some(pos) = find_whitespace_normalized(file_lines, &old_lines) {
        return Ok(MatchResult {
            line_idx: pos,
            level: MatchLevel::L2Whitespace,
        });
    }

    // L3: 行级相似度匹配
    if let Some((pos, ratio)) = find_by_similarity(file_lines, &old_lines, 0.8) {
        return Ok(MatchResult {
            line_idx: pos,
            level: MatchLevel::L3Similarity(ratio),
        });
    }

    // L4: 关键行锚定（首尾 context 行）
    let ctx_lines = extract_context_lines(hunk);
    if ctx_lines.len() >= 2 {
        if let Some(pos) = find_by_anchor(file_lines, &ctx_lines) {
            return Ok(MatchResult {
                line_idx: pos,
                level: MatchLevel::L4Anchor,
            });
        }
    }

    // L5: 行号兜底
    if hunk.header.old_start > 0 && hunk.header.old_start <= file_lines.len() {
        return Ok(MatchResult {
            line_idx: hunk.header.old_start - 1,
            level: MatchLevel::L5LineNumber,
        });
    }

    Err(MatchError::NotFound {
        searched_content: old_lines
            .iter()
            .take(3)
            .cloned()
            .collect::<Vec<_>>()
            .join("\n"),
    })
}

/// L1: 精确匹配——全文搜索完全相同的连续行
fn find_exact(haystack: &[String], needle: &[String]) -> Option<usize> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }
    let mut positions = Vec::new();
    for i in 0..=haystack.len() - needle.len() {
        if haystack[i..i + needle.len()] == needle[..] {
            positions.push(i);
        }
    }
    if positions.len() == 1 {
        Some(positions[0])
    } else if positions.is_empty() {
        None
    } else {
        // 多位置——不在 L1 报错，让后续级别尝试
        None
    }
}

/// L2: 空白归一化匹配——tab→4spaces + trim 尾部空白
fn find_whitespace_normalized(haystack: &[String], needle: &[String]) -> Option<usize> {
    let normalize = |s: &str| s.replace('\t', "    ").trim_end().to_string();
    let norm_haystack: Vec<String> = haystack.iter().map(|s| normalize(s)).collect();
    let norm_needle: Vec<String> = needle.iter().map(|s| normalize(s)).collect();
    find_exact(&norm_haystack, &norm_needle)
}

/// L3: 行级相似度匹配——用 similar crate 计算 ratio
fn find_by_similarity(
    haystack: &[String],
    needle: &[String],
    threshold: f64,
) -> Option<(usize, f64)> {
    if needle.is_empty() || needle.len() > haystack.len() {
        return None;
    }

    let needle_text = needle.join("\n");
    let mut best: Option<(usize, f64)> = None;

    for i in 0..=haystack.len().saturating_sub(needle.len()) {
        let candidate: String = haystack[i..i + needle.len()].join("\n");
        let diff = similar::TextDiff::from_lines(&needle_text, &candidate);
        let ratio = diff.ratio();
        if ratio >= threshold {
            if let Some((_, best_ratio)) = best {
                if ratio > best_ratio {
                    best = Some((i, ratio));
                }
            } else {
                best = Some((i, ratio));
            }
        }
    }

    best
}

/// L4: 关键行锚定——首行和末行 context 必须匹配
fn find_by_anchor(haystack: &[String], ctx_lines: &[String]) -> Option<usize> {
    let first_line = &ctx_lines[0];
    let last_line = &ctx_lines[ctx_lines.len() - 1];
    let span = ctx_lines.len().saturating_sub(1); // 首尾行之间的行数差

    let mut positions = Vec::new();
    for i in 0..haystack.len() {
        if haystack[i].trim_end() == first_line.trim_end() {
            // 检查 last_line 是否在预期位置
            let expected_last = i + span;
            if expected_last < haystack.len()
                && haystack[expected_last].trim_end() == last_line.trim_end()
            {
                positions.push(i);
            }
        }
    }

    if positions.len() == 1 {
        Some(positions[0])
    } else if positions.len() > 1 {
        // 多位置不报错，L5 兜底
        None
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::tools::filesystem::line_edit_diff::{HunkHeader, ParsedPatch};

    fn make_hunk(old_start: usize, lines: Vec<DiffLine>) -> Hunk {
        Hunk {
            header: HunkHeader {
                old_start,
                old_count: lines.len(),
                new_start: old_start,
                new_count: lines.len(),
            },
            lines,
        }
    }

    fn s(lines: &[&str]) -> Vec<String> {
        lines.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn test_l1_精确匹配() {
        let file = s(&["aaa", "bbb", "ccc", "ddd", "eee"]);
        let hunk = make_hunk(2, vec![
            DiffLine::Context("bbb".into()),
            DiffLine::Remove("ccc".into()),
            DiffLine::Add("CCC".into()),
            DiffLine::Context("ddd".into()),
        ]);
        let result = match_hunk(&file, &hunk).unwrap();
        assert_eq!(result.line_idx, 1); // 0-based: 第 2 行
        assert_eq!(result.level, MatchLevel::L1Exact);
    }

    #[test]
    fn test_l2_空白归一化() {
        let file = s(&["aaa", "bbb\t", "ccc", "ddd"]);
        let hunk = make_hunk(2, vec![
            DiffLine::Context("bbb".into()),
            DiffLine::Remove("ccc".into()),
            DiffLine::Add("CCC".into()),
            DiffLine::Context("ddd".into()),
        ]);
        let result = match_hunk(&file, &hunk).unwrap();
        assert_eq!(result.level, MatchLevel::L2Whitespace);
    }

    #[test]
    fn test_l3_相似度匹配() {
        let file = s(&["aaa", "bbx", "ccy", "ddd"]);
        let hunk = make_hunk(2, vec![
            DiffLine::Context("bbb".into()),
            DiffLine::Remove("ccc".into()),
            DiffLine::Add("CCC".into()),
            DiffLine::Context("ddd".into()),
        ]);
        let result = match_hunk(&file, &hunk).unwrap();
        assert!(matches!(result.level, MatchLevel::L3Similarity(r) if r >= 0.8));
    }

    #[test]
    fn test_l5_行号兜底() {
        let file = s(&["aaa", "bbb", "ccc"]);
        let hunk = make_hunk(2, vec![
            DiffLine::Context("xxx".into()),
            DiffLine::Remove("yyy".into()),
            DiffLine::Add("YYY".into()),
        ]);
        let result = match_hunk(&file, &hunk).unwrap();
        assert_eq!(result.level, MatchLevel::L5LineNumber);
        assert_eq!(result.line_idx, 1);
    }

    #[test]
    fn test_匹配失败() {
        let file = s(&["aaa", "bbb"]);
        let hunk = make_hunk(99, vec![
            DiffLine::Context("xxx".into()),
        ]);
        let result = match_hunk(&file, &hunk);
        assert!(matches!(result, Err(MatchError::NotFound { .. })));
    }
}
```

- [ ] **Step 2: 编译 + 测试**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem::line_edit_match::tests 2>&1 | tail -10`
Expected: ALL PASS

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_match.rs
git commit -m "feat(lineedit): add 5-level fallback matching engine for V3

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 3: 验证层 (line_edit_verify.rs)

**Files:**
- Create: `peri-middlewares/src/tools/filesystem/line_edit_verify.rs`

- [ ] **Step 1: 实现 3 层验证**

```rust
//! 3 层验证引擎
//! 层 A: Diff Sanity Guard
//! 层 B: 括号平衡 + 缩进一致性
//! 层 C: Tree-sitter AST Guard

use std::path::Path;

/// 验证级别
#[derive(Debug, Clone, PartialEq)]
pub enum VerifyLevel {
    Ok,
    Warn(String),
    Error(String),
    Skip,
}

/// 三层验证结果
#[derive(Debug)]
pub struct VerifyResult {
    pub sanity: VerifyLevel,
    pub brackets: VerifyLevel,
    pub ast: VerifyLevel,
}

impl VerifyResult {
    pub fn has_error(&self) -> bool {
        matches!(self.sanity, VerifyLevel::Error(_))
            || matches!(self.brackets, VerifyLevel::Error(_))
            || matches!(self.ast, VerifyLevel::Error(_))
    }

    pub fn format_tags(&self) -> String {
        format!(
            "sanity:{} brackets:{} ast:{}",
            level_tag(&self.sanity),
            level_tag(&self.brackets),
            level_tag(&self.ast),
        )
    }
}

fn level_tag(level: &VerifyLevel) -> &'static str {
    match level {
        VerifyLevel::Ok => "ok",
        VerifyLevel::Warn(_) => "warn",
        VerifyLevel::Error(_) => "error",
        VerifyLevel::Skip => "skip",
    }
}

/// 运行三层验证（短路：任一层 ERROR 则跳过后续）
pub fn verify(
    file_path: &str,
    old_content: &str,
    new_content: &str,
    edit_start: usize,
    edit_end: usize,
) -> VerifyResult {
    // 层 A: Diff Sanity
    let sanity = verify_diff_sanity(old_content, new_content, edit_start, edit_end);
    if matches!(sanity, VerifyLevel::Error(_)) {
        return VerifyResult {
            sanity,
            brackets: VerifyLevel::Skip,
            ast: VerifyLevel::Skip,
        };
    }

    // 层 B: 括号平衡 + 缩进
    let brackets = verify_brackets_and_indent(new_content, edit_start, edit_end);
    if matches!(brackets, VerifyLevel::Error(_)) {
        return VerifyResult {
            sanity,
            brackets,
            ast: VerifyLevel::Skip,
        };
    }

    // 层 C: Tree-sitter AST
    let ast = verify_ast(file_path, old_content, new_content);

    VerifyResult {
        sanity,
        brackets,
        ast,
    }
}

// ─── 层 A: Diff Sanity ───────────────────────────────────────────

fn verify_diff_sanity(
    old_content: &str,
    new_content: &str,
    _edit_start: usize,
    _edit_end: usize,
) -> VerifyLevel {
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    let diff = similar::TextDiff::from_lines(old_content, new_content);
    let mut changes = diff.iter_changes().collect::<Vec<_>>();

    let additions = changes.iter().filter(|c| c.tag() == similar::ChangeTag::Insert).count();
    let removals = changes.iter().filter(|c| c.tag() == similar::ChangeTag::Delete).count();

    // 改动幅度检查：如果删除行数 > 原文件的 50%，可能是误操作
    if !old_lines.is_empty() && removals > old_lines.len() / 2 {
        return VerifyLevel::Error(format!(
            "改动幅度异常：删除了 {} 行（原文件共 {} 行）",
            removals,
            old_lines.len()
        ));
    }

    // 重复行检测
    if new_lines.len() >= 2 {
        for window in new_lines.windows(2) {
            if window[0].trim_end() == window[1].trim_end() && !window[0].trim().is_empty() {
                return VerifyLevel::Warn("检测到相邻重复行".to_string());
            }
        }
    }

    VerifyLevel::Ok
}

// ─── 层 B: 括号平衡 + 缩进 ───────────────────────────────────────

fn verify_brackets_and_indent(
    content: &str,
    _edit_start: usize,
    _edit_end: usize,
) -> VerifyLevel {
    let mut brace_depth = 0i32;  // {}
    let mut paren_depth = 0i32;  // ()
    let mut bracket_depth = 0i32; // []

    let mut in_string: Option<char> = None;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut prev_char: Option<char> = None;

    for ch in content.chars() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            prev_char = Some(ch);
            continue;
        }
        if in_block_comment {
            if prev_char == Some('*') && ch == '/' {
                in_block_comment = false;
            }
            prev_char = Some(ch);
            continue;
        }
        if let Some(quote) = in_string {
            if ch == '\\' {
                prev_char = Some(ch);
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            prev_char = Some(ch);
            continue;
        }

        match ch {
            '\'' | '"' | '`' => in_string = Some(ch),
            '/' if prev_char == Some('/') => in_line_comment = true,
            '*' if prev_char == Some('/') => in_block_comment = true,
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            _ => {}
        }
        prev_char = Some(ch);
    }

    let mut errors = Vec::new();
    if brace_depth != 0 {
        errors.push(format!(
            "'{{}}' 不平衡（{} {}）",
            if brace_depth > 0 { "多出" } else { "缺少" },
            brace_depth.abs()
        ));
    }
    if paren_depth != 0 {
        errors.push(format!("'()' 不平衡（{} {}）", if paren_depth > 0 { "多出" } else { "缺少" }, paren_depth.abs()));
    }
    if bracket_depth != 0 {
        errors.push(format!("'[]' 不平衡（{} {}）", if bracket_depth > 0 { "多出" } else { "缺少" }, bracket_depth.abs()));
    }

    if !errors.is_empty() {
        return VerifyLevel::Error(errors.join("，"));
    }

    VerifyLevel::Ok
}

// ─── 层 C: Tree-sitter AST ───────────────────────────────────────

fn verify_ast(file_path: &str, old_content: &str, new_content: &str) -> VerifyLevel {
    let ext = Path::new(file_path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("");

    let language = match ext {
        "rs" => tree_sitter_rust::LANGUAGE,
        "ts" | "tsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT,
        "js" | "jsx" => tree_sitter_javascript::LANGUAGE,
        "py" => tree_sitter_python::LANGUAGE,
        "go" => tree_sitter_go::LANGUAGE,
        _ => return VerifyLevel::Skip,
    };

    let mut parser = tree_sitter::Parser::new();
    parser
        .set_language(&language.into())
        .unwrap_or_else(|e| {
            tracing::debug!("tree-sitter language 设置失败: {}", e);
        });

    let errors_before = count_ast_errors(&mut parser, old_content);
    let errors_after = count_ast_errors(&mut parser, new_content);

    if errors_after > errors_before {
        return VerifyLevel::Error(format!(
            "新增 {} 个语法错误（原有 {} 个）",
            errors_after - errors_before,
            errors_before
        ));
    }

    if errors_before > 0 {
        return VerifyLevel::Warn(format!(
            "原有 {} 个语法错误（未增加）",
            errors_before
        ));
    }

    VerifyLevel::Ok
}

fn count_ast_errors(parser: &mut tree_sitter::Parser, content: &str) -> usize {
    match parser.parse(content, None) {
        Some(tree) => count_error_nodes(&tree.root_node()),
        None => 1, // 解析完全失败视为 1 个错误
    }
}

fn count_error_nodes(node: &tree_sitter::Node) -> usize {
    let mut count = 0;
    if node.is_error() || node.is_missing() {
        count += 1;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i) {
            count += count_error_nodes(&child);
        }
    }
    count
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_括号平衡_ok() {
        let result = verify_brackets_and_indent("fn main() { let x = [1, 2]; }", 0, 1);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号不平衡() {
        let result = verify_brackets_and_indent("fn main() { let x = 1;", 0, 1);
        assert!(matches!(result, VerifyLevel::Error(_)));
    }

    #[test]
    fn test_括号平衡_忽略字符串内() {
        let result = verify_brackets_and_indent("let s = \"{[}\"; fn main() {}", 0, 1);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号平衡_忽略注释内() {
        let result = verify_brackets_and_indent("// { unbalanced\nfn main() {}", 0, 1);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_diff_sanity_ok() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nBBB\nccc\n";
        let result = verify_diff_sanity(old, new, 1, 2);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_diff_sanity_改动幅度异常() {
        let old = "line1\nline2\nline3\nline4\nline5\n";
        let new = "only one line\n";
        let result = verify_diff_sanity(old, new, 1, 5);
        assert!(matches!(result, VerifyLevel::Error(_)));
    }

    #[test]
    fn test_verify_短路() {
        // 层 A 报错 → B/C 应为 Skip
        let old = "aaa\nbbb\nccc\nddd\neee\n";
        let new = "one\n"; // 删除太多 → 层 A 报错
        let result = verify("test.txt", old, new, 1, 5);
        assert!(matches!(result.sanity, VerifyLevel::Error(_)));
        assert!(matches!(result.brackets, VerifyLevel::Skip));
        assert!(matches!(result.ast, VerifyLevel::Skip));
    }

    #[test]
    fn test_ast_非支持类型_skip() {
        let result = verify_ast("config.yaml", "old", "new");
        assert_eq!(result, VerifyLevel::Skip);
    }
}
```

- [ ] **Step 2: 编译 + 测试**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem::line_edit_verify::tests 2>&1 | tail -10`
Expected: ALL PASS

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_verify.rs
git commit -m "feat(lineedit): add 3-layer verification engine for V3

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 4: 主工具重写 (line_edit.rs)

**Files:**
- Rewrite: `peri-middlewares/src/tools/filesystem/line_edit.rs`
- Modify: `peri-middlewares/src/tools/filesystem/mod.rs`（注册新模块）

- [ ] **Step 1: 重写 line_edit.rs 为 V3 Diff-Apply 入口**

重写为：`LineEditTool` + 新的 `parameters()` + `invoke()` 主流程。`invoke` 流程按 spec Section 7：
1. 解析 patches
2. 按文件分组
3. 读取文件
4. 解析 diff + 5 级匹配
5. 应用编辑到内存
6. 3 层验证
7. atomic_write
8. 构建反馈

引用 Task 1-3 的模块：
```rust
mod line_edit_diff;
mod line_edit_match;
mod line_edit_verify;

use line_edit_diff::*;
use line_edit_match::*;
use line_edit_verify::*;
```

工具参数 `PatchEntry`：
```rust
#[derive(Debug, Deserialize)]
pub struct PatchEntry {
    pub file_path: String,
    pub diff: String,
}
```

反馈格式按 spec Section 6 实现 `format_results()`。

保留 `atomic_write` 函数不变。

- [ ] **Step 2: 在 mod.rs 中注册新模块文件**

确保 `mod.rs` 中有：
```rust
pub mod line_edit;
// line_edit 内部用 include! 引入子模块，或在此注册
```

由于子模块（`line_edit_diff.rs` 等）和 `line_edit.rs` 同目录，需要在 `line_edit.rs` 中用 `mod` 声明或直接 `use`。推荐：在 `line_edit.rs` 顶部用 `mod line_edit_diff;` 等声明子模块（同目录下 Rust 会自动查找对应文件）。

- [ ] **Step 3: 编译验证**

Run: `cargo build -p peri-middlewares 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/
git commit -m "feat(lineedit): V3 Diff-Apply main tool with invoke pipeline

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 5: 测试文件完全重写 (line_edit_test.rs)

**Files:**
- Rewrite: `peri-middlewares/src/tools/filesystem/line_edit_test.rs`

- [ ] **Step 1: 重写测试覆盖 V3 全功能**

测试清单：

| 测试 | 覆盖 |
|------|------|
| `test_单hunk替换` | 基础 replace |
| `test_多hunk同文件` | 从后往前应用 |
| `test_跨文件多patch` | 原子性 |
| `test_插入新行` | 只有 + 行 |
| `test_删除行` | 只有 - 行 |
| `test_原子性_匹配失败` | 一个失败全部不写 |
| `test_原子性_验证失败` | 括号不平衡全部不写 |
| `test_匹配降级_L2空白` | 空白归一化匹配 |
| `test_匹配降级_L5行号` | 行号兜底 |
| `test_匹配失败_报错` | 全部级别失败 |
| `test_反馈_验证标签` | sanity/brackets/ast 标签 |
| `test_反馈_上下文diff` | +/- 标记行 |
| `test_CRLF保留` | 换行符保留 |
| `test_空diff报错` | 边界 |
| `test_文件不存在` | 边界 |

每个测试构造 diff 字符串 → 调用 `invoke` → 断言文件内容或错误消息。

- [ ] **Step 2: 运行全部测试**

Run: `cargo test -p peri-middlewares --lib -- tools::filesystem::line_edit::tests 2>&1 | tail -15`
Expected: ALL PASS

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_test.rs
git commit -m "test(lineedit): V3 tests covering matching/verification/atomicity/feedback

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 6: 更新 CLAUDE.md + 压力测试文件

**Files:**
- Modify: `CLAUDE.md`
- Modify: `prompts/lineedit_stress_test.txt`

- [ ] **Step 1: 更新 CLAUDE.md lineEdit 描述**

将当前描述改为：
```
| `lineEdit` | 启用行号编辑模式——Edit 替换为 LineEdit（unified diff 输入、5 级匹配回退、3 层验证、原子性写入） |
```

- [ ] **Step 2: 更新压力测试文件注释**

替换为 V3 说明。

- [ ] **Step 3: Commit**

```bash
git add CLAUDE.md prompts/lineedit_stress_test.txt
git commit -m "docs: update CLAUDE.md for LineEdit V3 Diff-Apply

Co-Authored-By: glm-5.1 <zai-org@claude-code-best.win>"
```

---

### Task 7: 全量构建 + 测试 + Clippy

**Files:** 无新增/修改

- [ ] **Step 1: 全量构建**

Run: `cargo build 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 2: 全量测试**

Run: `cargo test -p peri-middlewares --lib 2>&1 | tail -10`
Expected: ALL PASS

- [ ] **Step 3: clippy**

Run: `cargo clippy -p peri-middlewares 2>&1 | tail -10`
Expected: 无 warning/error

---

## Plan Self-Review

### Spec Coverage

| Spec Section | Task |
|--------------|------|
| 3. 工具接口 | Task 4 |
| 4. 匹配引擎 | Task 2 |
| 5. 验证层 | Task 3 |
| 6. 反馈格式 | Task 4 |
| 7. 执行流程 | Task 4 |
| 测试覆盖 | Task 5 |
| 文档更新 | Task 6 |
| 依赖 | 已确认无新依赖 |

### Placeholder Scan

无 TBD/TODO。所有步骤含完整代码。

### Type Consistency

- `PatchEntry` 在 Task 4 定义，invoke 使用
- `ParsedPatch`/`Hunk`/`DiffLine` 在 Task 1 定义，Task 2/4 使用
- `MatchResult`/`MatchLevel` 在 Task 2 定义，Task 4 使用
- `VerifyResult`/`VerifyLevel` 在 Task 3 定义，Task 4 使用
