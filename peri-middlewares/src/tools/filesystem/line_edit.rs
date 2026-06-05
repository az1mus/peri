use peri_agent::tools::BaseTool;
use serde::Deserialize;
use serde_json::Value;

use super::resolve_path;

const LINE_EDIT_DESCRIPTION: &str = r#"Performs precise line-based edits in files.

Line numbers are 1-based (from Read output). Multiple edits are applied bottom-to-top.
insert=true inserts before start_line; empty new_string deletes the range.

Caution: new_string replaces the target range entirely — do not duplicate content from adjacent lines outside the edit range.
Caution: start_word/end_word must be unique within the line. If the word matches multiple times (e.g., "foo" in "foo bar foo"), use a longer prefix (e.g., "foo bar") to disambiguate.
Caution: when replacing an entire line, omit start_word/end_word and use only start_line. Using start_word/end_word for full-line replacement risks matching an unexpected position within the line and producing truncated output.
Caution: the replacement range of start_word/end_word is from the START of start_word to the END of end_word — not the text between them. The anchor words themselves will be replaced, so keep them short and avoid including content you want to preserve."#;

/// 单个编辑操作
#[derive(Debug, Deserialize)]
pub struct EditEntry {
    pub file_path: String,
    pub start_line: usize,
    pub end_line: Option<usize>,
    pub start_word: Option<String>,
    pub end_word: Option<String>,
    pub new_string: String,
    #[serde(default)]
    pub insert: bool,
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
                                "description": "Line number to start editing (from Read output)"
                            },
                            "end_line": {
                                "type": "integer",
                                "description": "Line number to end editing. Defaults to start_line if not provided"
                            },
                            "start_word": {
                                "type": "string",
                                "description": "Optional: text within start_line to begin replacement at. Must be unique in that line"
                            },
                            "end_word": {
                                "type": "string",
                                "description": "Optional: text within end_line to end replacement at. Must be unique in that line"
                            },
                            "new_string": {
                                "type": "string",
                                "description": "Replacement text. Empty string deletes the target range"
                            },
                            "insert": {
                                "type": "boolean",
                                "description": "If true, insert new_string before start_line without replacing any lines"
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
        let mut groups: std::collections::BTreeMap<String, Vec<&EditEntry>> =
            std::collections::BTreeMap::new();
        for edit in &edits {
            let resolved = resolve_path(&self.cwd, &edit.file_path);
            let key = resolved.to_string_lossy().to_string();
            groups.entry(key).or_default().push(edit);
        }

        let mut results: Vec<String> = Vec::new();

        for (file_key, file_edits) in &groups {
            // 按行号降序排列（从后往前）
            let mut sorted_edits: Vec<&&EditEntry> = file_edits.iter().collect();
            sorted_edits.sort_by_key(|b| std::cmp::Reverse(b.start_line));

            let content = match std::fs::read_to_string(file_key) {
                Ok(c) => c,
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                    return Err(format!("文件不存在: {}", file_key).into());
                }
                Err(e) => return Err(e.into()),
            };

            let mut lines: Vec<String> = content.lines().map(|l| l.to_string()).collect();
            let trailing_newline = content.ends_with('\n');
            // 检测原始换行符风格：如果内容中包含 \r\n 则保留 CRLF
            let line_ending = if content.contains("\r\n") {
                "\r\n"
            } else {
                "\n"
            };

            for edit in sorted_edits {
                match apply_single_edit(&mut lines, edit) {
                    Ok(msg) => results.push(msg),
                    Err(e) => results.push(format!("失败: {}", e)),
                }
            }

            // 构建新内容，保留原始换行符风格
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

        Ok(results.join("\n"))
    }
}

/// 应用单个编辑到行数组
fn apply_single_edit(
    lines: &mut Vec<String>,
    edit: &EditEntry,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let start_line = edit.start_line;
    let end_line = edit.end_line.unwrap_or(start_line);

    if edit.insert {
        return apply_insert(lines, start_line, &edit.new_string);
    }

    if start_line == 0 {
        return Err("start_line 必须 >= 1".into());
    }
    if end_line < start_line {
        return Err(format!(
            "end_line ({}) 不能小于 start_line ({})",
            end_line, start_line
        )
        .into());
    }
    if start_line > lines.len() {
        return Err(format!(
            "start_line {} 超出文件行数 (共 {} 行)",
            start_line,
            lines.len()
        )
        .into());
    }
    if end_line > lines.len() {
        return Err(format!("end_line {} 超出文件行数 (共 {} 行)", end_line, lines.len()).into());
    }

    // 转为 0-based 索引
    let start_idx = start_line - 1;
    let end_idx = end_line - 1;

    if edit.start_word.is_some() || edit.end_word.is_some() {
        return apply_word_edit(
            lines,
            start_idx,
            end_idx,
            edit.start_word.as_deref(),
            edit.end_word.as_deref(),
            &edit.new_string,
        );
    }

    // 整行替换
    let old_line_count = end_idx - start_idx + 1;
    let new_lines: Vec<&str> = edit.new_string.lines().collect();
    let diff = new_lines.len() as i64 - old_line_count as i64;

    lines.splice(start_idx..=end_idx, new_lines.iter().map(|s| s.to_string()));

    let desc = describe_diff(diff);
    Ok(format!(
        "{} 第 {}-{} 行 → {} 行",
        desc,
        start_line,
        end_line,
        new_lines.len()
    ))
}

/// 插入操作
fn apply_insert(
    lines: &mut Vec<String>,
    before_line: usize,
    new_string: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    if before_line == 0 {
        return Err("insert 的 start_line 必须 >= 1".into());
    }
    if before_line > lines.len() + 1 {
        return Err(format!(
            "insert 位置 {} 超出范围 (文件共 {} 行，最大可插入到 {})",
            before_line,
            lines.len(),
            lines.len() + 1
        )
        .into());
    }

    let new_lines: Vec<&str> = new_string.lines().collect();
    let insert_idx = before_line - 1;

    for (i, line) in new_lines.iter().enumerate() {
        lines.insert(insert_idx + i, line.to_string());
    }

    Ok(format!(
        "插入 {} 行到第 {} 行前",
        new_lines.len(),
        before_line
    ))
}

/// 行内编辑（start_word / end_word）
fn apply_word_edit(
    lines: &mut Vec<String>,
    start_idx: usize,
    end_idx: usize,
    start_word: Option<&str>,
    end_word: Option<&str>,
    new_string: &str,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let start_col = if let Some(word) = start_word {
        find_unique_word(&lines[start_idx], word)?
    } else {
        0
    };

    let end_col = if let Some(word) = end_word {
        let pos = find_unique_word(&lines[end_idx], word)?;
        pos + word.len()
    } else {
        lines[end_idx].len()
    };

    let line_start = &lines[start_idx][..start_col];
    let line_end = &lines[end_idx][end_col..];

    let combined = format!("{}{}{}", line_start, new_string, line_end);

    let new_lines: Vec<String> = combined.lines().map(|s| s.to_string()).collect();
    lines.splice(start_idx..=end_idx, new_lines);

    Ok(format!(
        "行内替换 第 {}-{} 行 (start_word: {}, end_word: {})",
        start_idx + 1,
        end_idx + 1,
        start_word.unwrap_or("(行首)"),
        end_word.unwrap_or("(行尾)")
    ))
}

/// 在行内查找唯一匹配的 word 位置
fn find_unique_word(
    line: &str,
    word: &str,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let matches: Vec<usize> = line.match_indices(word).map(|(i, _)| i).collect();

    match matches.len() {
        0 => Err(format!(
            "word '{}' 未在该行中找到。行内容: '{}'",
            word,
            line.chars().take(80).collect::<String>()
        )
        .into()),
        1 => Ok(matches[0]),
        n => Err(format!(
            "word '{}' 在行内匹配了 {} 处，请提供更长的前缀使其唯一",
            word, n
        )
        .into()),
    }
}

fn describe_diff(diff: i64) -> &'static str {
    match diff.cmp(&0) {
        std::cmp::Ordering::Greater => "新增",
        std::cmp::Ordering::Less => "删除",
        std::cmp::Ordering::Equal => "替换",
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
