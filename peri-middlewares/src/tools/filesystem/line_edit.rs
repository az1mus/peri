use peri_agent::tools::BaseTool;
use serde::Deserialize;
use serde_json::Value;

use super::resolve_path;

const LINE_EDIT_DESCRIPTION: &str = r#"Performs precise line-based edits in files.

Line numbers are 1-based (from Read output). Multiple edits are applied bottom-to-top.
All edits in one call are atomic — if any edit fails, no changes are written.

Actions (set "action" field):
- "replace" (default): Replace lines start_line..end_line with new_string.
- "insert": Insert new_string BEFORE start_line. No existing lines are removed.
- "delete": Remove lines start_line..end_line. new_string is ignored, can be "".

Verification with expected_lines (recommended):
- Set to the content you expect at start_line..end_line from your last Read.
- If actual content differs, a warning is returned but the edit still proceeds.
- This catches stale line numbers after concurrent changes.

Rules:
- new_string replaces the ENTIRE target range — do not duplicate adjacent lines.
- For single-line edits, omit end_line (defaults to start_line) to avoid accidentally removing adjacent lines.
- To add attributes/derives above a line, prefer action:"insert" over replace — it cannot accidentally drop existing lines.
- expected_lines must cover the FULL range (start_line through end_line), not just the first line.
- Multiple edits to the same file must not overlap.

Common patterns:
- Replace lines: {start_line: 42, end_line: 44, expected_lines: "...", new_string: "..."}
- Insert before line: {start_line: 42, action: "insert", new_string: "new line"}
- Delete lines: {start_line: 42, end_line: 44, action: "delete", new_string: ""}
- Single line: {start_line: 42, new_string: "replacement content"}"#;

/// 编辑动作
#[derive(Debug, Clone, PartialEq)]
enum EditAction {
    Replace,
    Insert,
    Delete,
}

/// 单个编辑操作
#[derive(Debug, Deserialize)]
pub struct EditEntry {
    pub file_path: String,
    pub start_line: usize,
    #[serde(default)]
    pub end_line: Option<usize>,
    #[serde(default)]
    pub action: Option<String>,
    pub new_string: String,
    #[serde(default)]
    pub expected_lines: Option<String>,
}

impl EditEntry {
    fn resolve_action(&self) -> EditAction {
        if let Some(ref action) = self.action {
            match action.as_str() {
                "insert" => EditAction::Insert,
                "delete" => EditAction::Delete,
                _ => EditAction::Replace,
            }
        } else if self.new_string.is_empty() {
            EditAction::Delete
        } else {
            EditAction::Replace
        }
    }

    fn effective_end_line(&self) -> usize {
        self.end_line.unwrap_or(self.start_line)
    }
}

/// expected_lines 验证结果
enum ExpectedLinesResult {
    Match,
    Mismatch { expected: String, actual: String },
}

#[derive(Clone)]
enum ValidateOutcome {
    Ok,
    Warn { expected: String, actual: String },
}

struct ValidateError {
    edit_index: usize,
    message: String,
}

/// 编辑应用结果
struct EditResult {
    file_path: String,
    start_line: usize,
    end_line: usize,
    action: EditAction,
    old_line_count: usize,
    new_line_count: usize,
    validation: ValidateOutcome,
    context_before: Vec<String>,
    context_after: Vec<String>,
    old_lines: Vec<String>,
    new_lines: Vec<String>,
}

/// LineEdit 工具 — 基于行号的精确编辑
pub struct LineEditTool {
    pub cwd: String,
}

impl LineEditTool {
    pub fn new(cwd: impl Into<String>) -> Self {
        Self { cwd: cwd.into() }
    }
}

fn verify_expected_lines(
    lines: &[String],
    start_idx: usize,
    end_idx: usize,
    expected: &str,
) -> ExpectedLinesResult {
    let actual: String = lines[start_idx..=end_idx]
        .iter()
        .map(|l| l.trim_end().to_string())
        .collect::<Vec<_>>()
        .join("\n");
    let expected_norm: String = expected
        .lines()
        .map(|l| l.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    if actual == expected_norm {
        ExpectedLinesResult::Match
    } else {
        ExpectedLinesResult::Mismatch {
            expected: expected_norm,
            actual,
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

fn format_results(results: &[EditResult]) -> String {
    let mut output = Vec::new();

    for r in results {
        let action_str = match r.action {
            EditAction::Replace => "replace",
            EditAction::Insert => "insert",
            EditAction::Delete => "delete",
        };

        let status = match &r.validation {
            ValidateOutcome::Ok => "✓",
            ValidateOutcome::Warn { .. } => "⚠",
        };

        let line_range = if r.start_line == r.end_line {
            format!("{}", r.start_line)
        } else {
            format!("{}-{}", r.start_line, r.end_line)
        };

        if matches!(&r.validation, ValidateOutcome::Warn { .. }) {
            if let ValidateOutcome::Warn { expected, actual } = &r.validation {
                output.push(format!(
                    "{} {}:{} {} ({}→{} lines) expected_lines 不匹配",
                    status, r.file_path, line_range, action_str, r.old_line_count, r.new_line_count
                ));
                output.push(format!("  预期: {}", truncate_str(expected, 80)));
                output.push(format!("  实际: {}", truncate_str(actual, 80)));
                output.push("  编辑已执行，建议 Re-read 确认结果".to_string());
            }
        } else {
            output.push(format!(
                "{} {}:{} {} ({}→{} lines)",
                status, r.file_path, line_range, action_str, r.old_line_count, r.new_line_count
            ));
        }

        // 上下文 diff
        let total_lines = r.context_before.len()
            + r.old_lines.len().max(r.new_lines.len())
            + r.context_after.len();
        if total_lines <= 30 {
            let before_start = r.start_line.saturating_sub(r.context_before.len());
            for (i, line) in r.context_before.iter().enumerate() {
                output.push(format!("{:>4} | {}", before_start + i, line));
            }
            for (i, line) in r.old_lines.iter().enumerate() {
                output.push(format!("{:>4} |-{}", r.start_line + i, line));
            }
            for line in r.new_lines.iter() {
                output.push(format!("{:>4} |+{}", r.start_line, line));
            }
            let after_start = r.start_line + r.new_lines.len();
            for (i, line) in r.context_after.iter().enumerate() {
                output.push(format!("{:>4} | {}", after_start + i, line));
            }
        } else {
            output.push(format!(
                "  ... ({} 行变更，已省略) ...",
                r.old_lines.len().max(r.new_lines.len())
            ));
        }
    }

    output.join("\n")
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
                "edits": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "properties": {
                            "file_path": {
                                "type": "string",
                                "description": "Absolute path to the file to modify"
                            },
                            "start_line": {
                                "type": "integer",
                                "description": "1-based line number to start editing at (from Read output)"
                            },
                            "end_line": {
                                "type": "integer",
                                "description": "Line number to end editing at. Defaults to start_line. Ignored when action=insert."
                            },
                            "action": {
                                "type": "string",
                                "enum": ["replace", "insert", "delete"],
                                "description": "Edit action. 'replace' (default): replace lines start_line..end_line with new_string. 'insert': insert new_string before start_line, no lines removed. 'delete': remove lines start_line..end_line, new_string ignored."
                            },
                            "new_string": {
                                "type": "string",
                                "description": "Replacement text. For replace: the new content. For insert: content to insert. For delete: ignored, can be empty string."
                            },
                            "expected_lines": {
                                "type": "string",
                                "description": "Optional but recommended: content you expect at start_line..end_line from your last Read. If actual content differs, a warning is returned but edit still proceeds."
                            }
                        },
                        "required": ["file_path", "start_line", "new_string"]
                    }
                }
            },
            "required": ["edits"]
        })
    }

    async fn invoke(
        &self,
        input: Value,
    ) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
        let edits: Vec<EditEntry> = serde_json::from_value(input["edits"].clone())
            .map_err(|e| format!("edits 参数解析失败: {}", e))?;

        if edits.is_empty() {
            return Err("edits 不能为空".into());
        }

        // 按文件分组
        let mut groups: std::collections::BTreeMap<String, Vec<usize>> =
            std::collections::BTreeMap::new();
        for (i, edit) in edits.iter().enumerate() {
            let resolved = resolve_path(&self.cwd, &edit.file_path);
            let key = resolved.to_string_lossy().to_string();
            groups.entry(key).or_default().push(i);
        }

        // 读取所有文件内容
        let mut file_contents: std::collections::HashMap<String, (Vec<String>, &str, bool)> =
            std::collections::HashMap::new();
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
            file_contents.insert(file_key.clone(), (lines, line_ending, trailing_newline));
        }

        // 阶段 1: 验证所有编辑
        let mut validation_errors: Vec<ValidateError> = Vec::new();
        let mut validation_warnings: Vec<Option<ValidateOutcome>> =
            (0..edits.len()).map(|_| None).collect();

        for (file_key, edit_indices) in &groups {
            let (lines, _, _) = file_contents.get(file_key).unwrap();

            // 检查重叠
            let mut sorted_by_line: Vec<usize> = edit_indices.clone();
            sorted_by_line.sort_by_key(|&i| std::cmp::Reverse(edits[i].start_line));
            for window in sorted_by_line.windows(2) {
                let a_idx = window[0];
                let b_idx = window[1];
                let a = &edits[a_idx];
                let b = &edits[b_idx];
                let a_action = a.resolve_action();
                let b_action = b.resolve_action();
                if a_action != EditAction::Insert && b_action != EditAction::Insert {
                    let a_start = a.start_line;
                    let a_end = a.effective_end_line();
                    let b_start = b.start_line;
                    let b_end = b.effective_end_line();
                    if a_start <= b_end {
                        validation_errors.push(ValidateError {
                            edit_index: a_idx,
                            message: format!(
                                "第 {}-{} 行与第 {}-{} 行重叠，请调整范围",
                                a_start, a_end, b_start, b_end
                            ),
                        });
                    }
                }
            }

            // 验证每个编辑
            for &i in edit_indices {
                let edit = &edits[i];
                let action = edit.resolve_action();
                let end_line = edit.effective_end_line();

                if edit.start_line == 0 {
                    validation_errors.push(ValidateError {
                        edit_index: i,
                        message: "start_line 必须 >= 1".to_string(),
                    });
                    continue;
                }

                match action {
                    EditAction::Insert => {
                        if edit.start_line > lines.len() + 1 {
                            validation_errors.push(ValidateError {
                                edit_index: i,
                                message: format!(
                                    "insert 位置 {} 超出范围 (文件共 {} 行，最大可插入到 {})",
                                    edit.start_line,
                                    lines.len(),
                                    lines.len() + 1
                                ),
                            });
                        }
                    }
                    EditAction::Replace | EditAction::Delete => {
                        if end_line < edit.start_line {
                            validation_errors.push(ValidateError {
                                edit_index: i,
                                message: format!(
                                    "end_line ({}) 不能小于 start_line ({})",
                                    end_line, edit.start_line
                                ),
                            });
                            continue;
                        }
                        if edit.start_line > lines.len() {
                            validation_errors.push(ValidateError {
                                edit_index: i,
                                message: format!(
                                    "start_line {} 超出文件行数 (共 {} 行)",
                                    edit.start_line,
                                    lines.len()
                                ),
                            });
                            continue;
                        }
                        if end_line > lines.len() {
                            validation_errors.push(ValidateError {
                                edit_index: i,
                                message: format!(
                                    "end_line {} 超出文件行数 (共 {} 行)",
                                    end_line,
                                    lines.len()
                                ),
                            });
                            continue;
                        }

                        // expected_lines 验证（警告但继续）
                        if let Some(ref expected) = edit.expected_lines {
                            let start_idx = edit.start_line - 1;
                            let end_idx = end_line - 1;
                            match verify_expected_lines(lines, start_idx, end_idx, expected) {
                                ExpectedLinesResult::Match => {
                                    validation_warnings[i] = Some(ValidateOutcome::Ok);
                                }
                                ExpectedLinesResult::Mismatch {
                                    expected: exp,
                                    actual,
                                } => {
                                    validation_warnings[i] = Some(ValidateOutcome::Warn {
                                        expected: exp,
                                        actual,
                                    });
                                }
                            }
                        }
                    }
                }
            }
        }

        // 如果有验证错误 → 全部拒绝
        if !validation_errors.is_empty() {
            let mut msgs: Vec<String> = Vec::new();
            for err in &validation_errors {
                msgs.push(format!("✗ Edit {}: {}", err.edit_index + 1, err.message));
            }
            msgs.push("未执行任何编辑。修正后重试。".to_string());
            return Ok(msgs.join("\n"));
        }

        // 阶段 2: 应用所有编辑
        let mut results: Vec<EditResult> = Vec::new();

        for (file_key, edit_indices) in &groups {
            let (lines, line_ending, trailing_newline) = file_contents.get(file_key).unwrap();
            let mut lines = lines.clone();
            let line_ending = *line_ending;
            let trailing_newline = *trailing_newline;

            // 按行号降序排列
            let mut sorted: Vec<usize> = edit_indices.clone();
            sorted.sort_by_key(|&i| std::cmp::Reverse(edits[i].start_line));

            for i in sorted {
                let edit = &edits[i];
                let action = edit.resolve_action();
                let start_line = edit.start_line;
                let end_line = edit.effective_end_line();
                let start_idx = start_line - 1;
                let end_idx = end_line - 1;

                // 保存上下文用于反馈（前后各 2 行）
                let ctx_before: Vec<String> =
                    lines[start_idx.saturating_sub(2)..start_idx].to_vec();

                let old_lines: Vec<String> = match action {
                    EditAction::Insert => vec![],
                    _ => lines[start_idx..=end_idx].to_vec(),
                };
                let old_line_count = old_lines.len();

                // 应用编辑
                let new_lines_vec: Vec<String>;
                match action {
                    EditAction::Replace => {
                        new_lines_vec = if edit.new_string.is_empty() {
                            vec![]
                        } else {
                            edit.new_string.lines().map(|s| s.to_string()).collect()
                        };
                        lines.splice(start_idx..=end_idx, new_lines_vec.clone());
                    }
                    EditAction::Insert => {
                        new_lines_vec = if edit.new_string.is_empty() {
                            vec![]
                        } else {
                            edit.new_string.lines().map(|s| s.to_string()).collect()
                        };
                        for (j, line) in new_lines_vec.iter().enumerate() {
                            lines.insert(start_idx + j, line.clone());
                        }
                    }
                    EditAction::Delete => {
                        new_lines_vec = vec![];
                        lines.splice(start_idx..=end_idx, std::iter::empty());
                    }
                }

                // 保存编辑后上下文
                let new_end_idx = start_idx + new_lines_vec.len().saturating_sub(1);
                let ctx_after_end = (new_end_idx + 3).min(lines.len());
                let ctx_after: Vec<String> = if ctx_after_end > new_end_idx + 1 {
                    lines[new_end_idx + 1..ctx_after_end].to_vec()
                } else {
                    vec![]
                };

                results.push(EditResult {
                    file_path: edit.file_path.clone(),
                    start_line,
                    end_line,
                    action,
                    old_line_count,
                    new_line_count: new_lines_vec.len(),
                    validation: validation_warnings[i]
                        .clone()
                        .unwrap_or(ValidateOutcome::Ok),
                    context_before: ctx_before,
                    context_after: ctx_after,
                    old_lines,
                    new_lines: new_lines_vec,
                });
            }

            // 写入文件
            let new_content = if lines.is_empty() {
                String::new()
            } else {
                let mut s = lines.join(line_ending);
                if trailing_newline {
                    s.push_str(line_ending);
                }
                s
            };
            atomic_write(std::path::Path::new(file_key), &new_content)?;
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

#[cfg(test)]
mod tests {
    use super::*;
    include!("line_edit_test.rs");
}
