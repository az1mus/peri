use super::line_edit_diff::*;
use super::line_edit_match::*;
use super::line_edit_verify::*;

use peri_agent::tools::BaseTool;
use serde::Deserialize;
use serde_json::Value;

use super::resolve_path;

const LINE_EDIT_DESCRIPTION: &str = r#"Applies unified diff patches to files with 5-level fuzzy matching and 3-layer verification.

Provide patches as an array of {file_path, diff} objects. The diff format follows standard unified diff:

```
--- a/file
+++ b/file
@@ -L,N +L,N @@
 context
-old
+new
 context
```

Features:
- **5-level fuzzy matching**: L1 exact → L2 whitespace-normalized → L3 similarity → L4 anchor → L5 line-number fallback
- **3-layer verification**: sanity check → bracket balance → tree-sitter AST guard
- **Atomic writes**: all patches to a file are applied in-memory first, verified, then written atomically
- **Multiple hunks**: multiple hunks per file are applied bottom-to-top to preserve line numbers
- **CRLF preservation**: detects and preserves original line endings

The tool is designed for LLM-generated edits. Matching is fuzzy by default — exact match is preferred but not required."#;

/// 单个 patch 条目
#[derive(Debug, Deserialize)]
pub struct PatchEntry {
    pub file_path: String,
    pub diff: String,
}

/// 匹配好的 hunk（用于后续应用）
struct MatchedHunk {
    hunk: Hunk,
    match_result: MatchResult,
}

/// 文件应用结果
struct FileResult {
    file_path: String,
    hunk_count: usize,
    additions: usize,
    deletions: usize,
    verify_result: VerifyResult,
}

/// LineEdit 工具 — 基于 unified diff 的精确编辑
pub struct LineEditTool {
    pub cwd: String,
}

impl LineEditTool {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self { cwd: cwd.into() }
    }
}

#[async_trait::async_trait]
impl BaseTool for LineEditTool {
    fn name(&self) -> &str {
        "LineEdit"
    }

    fn description(&self) -> &str {
        LINE_EDIT_DESCRIPTION
    }

    fn parameters(&self) -> Value {
        serde_json::json!({
            "type": "object",
            "properties": {
                "patches": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file_path": {
                                "type": "string",
                                "description": "Absolute path to the file to patch"
                            },
                            "diff": {
                                "type": "string",
                                "description": "Unified diff string for this file. Format: --- a/file\\n+++ b/file\\n@@ -L,N +L,N @@\\n context\\n-old\\n+new\\n context"
                            }
                        },
                        "required": ["file_path", "diff"]
                    }
                }
            },
            "required": ["patches"]
        })
    }

    async fn invoke(
        &self,
        input: Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let patches: Vec<PatchEntry> = serde_json::from_value(input["patches"].clone())
            .map_err(|e| format!("patches 参数解析失败: {}", e))?;

        if patches.is_empty() {
            return Err("patches 不能为空".into());
        }

        // 按文件分组：file_key → Vec<(patch_index, ParsedPatch)>
        let mut groups: std::collections::BTreeMap<String, Vec<(usize, ParsedPatch)>> =
            std::collections::BTreeMap::new();

        // 解析所有 diff
        for (i, patch) in patches.iter().enumerate() {
            let resolved = resolve_path(&self.cwd, &patch.file_path);
            let file_key = resolved.to_string_lossy().to_string();
            let parsed = parse_unified_diff(&patch.diff)
                .map_err(|e| format!("Patch {} 解析失败: {}", i + 1, e))?;
            groups.entry(file_key).or_default().push((i, parsed));
        }

        // 读取所有文件内容
        let mut file_contents: std::collections::HashMap<
            String,
            (Vec<String>, &str, bool, String),
        > = std::collections::HashMap::new();
        for file_key in groups.keys() {
            let content = match std::fs::read_to_string(file_key) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(format!("文件不存在: {}", file_key).into());
                }
                Err(e) => return Err(e.into()),
            };
            let trailing_newline = content.ends_with('\n');
            let line_ending = if content.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };
            let lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            let original_content = content.clone();
            file_contents.insert(
                file_key.clone(),
                (lines, line_ending, trailing_newline, original_content),
            );
        }

        // 阶段 1: 匹配所有 hunk
        let mut all_matched: std::collections::HashMap<String, Vec<MatchedHunk>> =
            std::collections::HashMap::new();
        let mut match_errors: Vec<String> = Vec::new();

        for (file_key, patch_list) in &groups {
            let (lines, _, _, _) = file_contents.get(file_key).unwrap();
            let mut file_matched = Vec::new();

            for (patch_idx, parsed) in patch_list {
                for (hunk_idx, hunk) in parsed.hunks.iter().enumerate() {
                    match match_hunk(lines, hunk) {
                        Ok(mr) => file_matched.push(MatchedHunk {
                            hunk: hunk.clone(),
                            match_result: mr,
                        }),
                        Err(MatchError::NotFound { searched_content }) => {
                            match_errors.push(format!(
                                "✗ Patch {} hunk {}: 未找到匹配位置\n  搜索内容: {}",
                                patch_idx + 1,
                                hunk_idx + 1,
                                truncate_str(&searched_content, 120)
                            ));
                        }
                        Err(MatchError::MultipleLocations { positions }) => {
                            match_errors.push(format!(
                                "✗ Patch {} hunk {}: 多个匹配位置 ({:?})",
                                patch_idx + 1,
                                hunk_idx + 1,
                                positions.iter().map(|p| p + 1).collect::<Vec<_>>()
                            ));
                        }
                    }
                }
            }

            all_matched.insert(file_key.clone(), file_matched);
        }

        // 匹配失败 → 全部拒绝
        if !match_errors.is_empty() {
            let mut msgs = match_errors;
            msgs.push("未执行任何编辑。修正后重试。".to_string());
            return Ok(msgs.join("\n"));
        }

        // 阶段 2: 应用编辑到内存
        let mut results: Vec<FileResult> = Vec::new();

        for file_key in groups.keys() {
            let (lines, line_ending, trailing_newline, original_content) =
                file_contents.get(file_key).unwrap();
            let mut lines = lines.clone();
            let line_ending = *line_ending;
            let trailing_newline = *trailing_newline;
            let original = original_content.clone();

            let matched = all_matched.get(file_key).unwrap();

            // 统计
            let mut total_additions = 0usize;
            let mut total_deletions = 0usize;

            // 按匹配位置从后往前排序（稳定排序保持同位置 hunk 的原始顺序）
            let mut sorted_matched: Vec<&MatchedHunk> = matched.iter().collect();
            sorted_matched.sort_by_key(|b| std::cmp::Reverse(b.match_result.line_idx));

            for mh in &sorted_matched {
                let line_idx = mh.match_result.line_idx;

                // old_count = context + remove 行数（文件中需要被替换的行数）
                let old_count: usize = mh
                    .hunk
                    .lines
                    .iter()
                    .filter(|dl| matches!(dl, DiffLine::Context(_) | DiffLine::Remove(_)))
                    .count();

                // replacement_lines = context + add（移除 remove 行）
                // Context 行使用文件当前行内容，避免 bottom-to-top 应用时
                // 被后续（更高 line_idx）hunk 修改的内容被还原为原值
                let mut old_pos = 0usize; // Context+Remove 在原文件中的偏移
                let replacement_lines: Vec<String> = mh
                    .hunk
                    .lines
                    .iter()
                    .filter_map(|dl| match dl {
                        DiffLine::Context(_) => {
                            let pos = old_pos;
                            old_pos += 1;
                            Some(lines.get(line_idx + pos).cloned().unwrap_or_default())
                        }
                        DiffLine::Add(s) => Some(s.clone()),
                        DiffLine::Remove(_) => {
                            old_pos += 1;
                            None
                        }
                    })
                    .collect();

                let removes = mh
                    .hunk
                    .lines
                    .iter()
                    .filter(|dl| matches!(dl, DiffLine::Remove(_)))
                    .count();
                total_deletions += removes;
                total_additions += replacement_lines
                    .len()
                    .saturating_sub(old_count.saturating_sub(removes));

                // 边界检查
                let end_idx = line_idx + old_count;
                if end_idx > lines.len() {
                    return Err(format!(
                        "Hunk 应用越界: 文件 {} 共 {} 行，需要 {}-{}",
                        file_key,
                        lines.len(),
                        line_idx + 1,
                        end_idx
                    )
                    .into());
                }

                // splice 替换
                lines.splice(line_idx..end_idx, replacement_lines);
            }

            // 验证
            let new_content = if lines.is_empty() {
                String::new()
            } else {
                let mut s = lines.join(line_ending);
                if trailing_newline {
                    s.push_str(line_ending);
                }
                s
            };

            let verify_result = verify(file_key, &original, &new_content);

            if verify_result.has_error() {
                return Ok(format!(
                    "✗ {} 验证失败 [{}]\n  编辑已取消，文件未被修改。",
                    file_key,
                    verify_result.format_tags()
                ));
            }

            // 写入文件
            atomic_write(std::path::Path::new(file_key), &new_content)?;

            results.push(FileResult {
                file_path: patches
                    .iter()
                    .find(|p| {
                        resolve_path(&self.cwd, &p.file_path)
                            .to_string_lossy()
                            .as_ref()
                            == file_key.as_str()
                    })
                    .map(|p| p.file_path.clone())
                    .unwrap_or_else(|| file_key.clone()),
                hunk_count: matched.len(),
                additions: total_additions,
                deletions: total_deletions,
                verify_result,
            });
        }

        // 构建反馈
        Ok(format_results(&results))
    }
}

/// 原子写入：临时文件 + rename
fn atomic_write(path: &std::path::Path, content: &str) -> Result<(), std::io::Error> {
    let tmp_ext = format!("tmp.{}", uuid::Uuid::now_v7());
    let tmp_path = path.with_extension(tmp_ext);
    std::fs::write(&tmp_path, content)?;
    match std::fs::rename(&tmp_path, path) {
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp_path);
            Err(e)
        }
    }
}

fn truncate_str(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        s.to_string()
    } else {
        s.chars().take(max_chars).collect::<String>() + "..."
    }
}

fn format_results(results: &[FileResult]) -> String {
    let mut output = Vec::new();
    let mut total_hunks = 0usize;
    let mut total_additions = 0usize;
    let mut total_deletions = 0usize;

    for r in results {
        let icon = if r.verify_result.has_error() {
            "✗"
        } else {
            match (&r.verify_result.brackets, &r.verify_result.ast) {
                (VerifyLevel::Warn(_), _) | (_, VerifyLevel::Warn(_)) => "⚠",
                _ => "✓",
            }
        };

        output.push(format!(
            "{} {} ({})",
            icon,
            r.file_path,
            r.verify_result.format_tags()
        ));
        output.push(format!(
            "  {} hunks applied ({} additions, {} deletions)",
            r.hunk_count, r.additions, r.deletions
        ));

        total_hunks += r.hunk_count;
        total_additions += r.additions;
        total_deletions += r.deletions;
    }

    // 汇总行
    output.push(format!(
        "\n{} files, {} hunks ({}+, {}-)",
        results.len(),
        total_hunks,
        total_additions,
        total_deletions
    ));

    output.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("line_edit_test.rs");
}
