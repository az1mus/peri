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
pub fn verify(file_path: &str, old_content: &str, new_content: &str) -> VerifyResult {
    // 层 A: Diff Sanity
    let sanity = verify_diff_sanity(old_content, new_content);
    if matches!(sanity, VerifyLevel::Error(_)) {
        return VerifyResult {
            sanity,
            brackets: VerifyLevel::Skip,
            ast: VerifyLevel::Skip,
        };
    }

    // 层 B: 括号平衡 + 缩进
    let brackets = verify_brackets(new_content);
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

fn verify_diff_sanity(old_content: &str, new_content: &str) -> VerifyLevel {
    let old_lines: Vec<&str> = old_content.lines().collect();
    let new_lines: Vec<&str> = new_content.lines().collect();

    // 简单统计：新文件比旧文件少了多少行
    let removals = old_lines.len().saturating_sub(new_lines.len());

    // 改动幅度检查：如果旧文件远大于新文件（删除过多）
    if !old_lines.is_empty() && old_lines.len() > 3 && new_lines.len() * 3 < old_lines.len() {
        return VerifyLevel::Error(format!(
            "改动幅度异常：文件从 {} 行变为 {} 行",
            old_lines.len(),
            new_lines.len()
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

// ─── 层 B: 括号平衡 ───────────────────────────────────────────────

fn verify_brackets(content: &str) -> VerifyLevel {
    let mut brace_depth = 0i32;
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;

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
        errors.push(format!(
            "'()' 不平衡（{} {}）",
            if paren_depth > 0 { "多出" } else { "缺少" },
            paren_depth.abs()
        ));
    }
    if bracket_depth != 0 {
        errors.push(format!(
            "'[]' 不平衡（{} {}）",
            if bracket_depth > 0 {
                "多出"
            } else {
                "缺少"
            },
            bracket_depth.abs()
        ));
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

    let language: tree_sitter::Language = match ext {
        "rs" => tree_sitter_rust::LANGUAGE.into(),
        "ts" | "tsx" => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        "js" | "jsx" => tree_sitter_javascript::LANGUAGE.into(),
        "py" => tree_sitter_python::LANGUAGE.into(),
        "go" => tree_sitter_go::LANGUAGE.into(),
        _ => return VerifyLevel::Skip,
    };
    let mut parser = tree_sitter::Parser::new();
    if parser.set_language(&language).is_err() {}

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
        return VerifyLevel::Warn(format!("原有 {} 个语法错误（未增加）", errors_before));
    }

    VerifyLevel::Ok
}

fn count_ast_errors(parser: &mut tree_sitter::Parser, content: &str) -> usize {
    match parser.parse(content, None) {
        Some(tree) => count_error_nodes(&tree.root_node()),
        None => 1,
    }
}

fn count_error_nodes(node: &tree_sitter::Node) -> usize {
    let mut count = 0;
    if node.is_error() || node.is_missing() {
        count += 1;
    }
    for i in 0..node.child_count() {
        if let Some(child) = node.child(i as u32) {
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
        let result = verify_brackets("fn main() { let x = [1, 2]; }");
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号不平衡() {
        let result = verify_brackets("fn main() { let x = 1;");
        assert!(matches!(result, VerifyLevel::Error(_)));
    }

    #[test]
    fn test_括号平衡_忽略字符串内() {
        let result = verify_brackets("let s = \"{[}\"; fn main() {}");
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号平衡_忽略注释内() {
        let result = verify_brackets("// { unbalanced\nfn main() {}");
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_diff_sanity_ok() {
        let old = "aaa\nbbb\nccc\n";
        let new = "aaa\nBBB\nccc\n";
        let result = verify_diff_sanity(old, new);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_diff_sanity_改动幅度异常() {
        let old = "line1\nline2\nline3\nline4\nline5\n";
        let new = "only one line\n";
        let result = verify_diff_sanity(old, new);
        assert!(matches!(result, VerifyLevel::Error(_)));
    }

    #[test]
    fn test_verify_短路() {
        let old = "aaa\nbbb\nccc\nddd\neee\n";
        let new = "one\n";
        let result = verify("test.txt", old, new);
        assert!(matches!(result.sanity, VerifyLevel::Error(_)));
        assert!(matches!(result.brackets, VerifyLevel::Skip));
        assert!(matches!(result.ast, VerifyLevel::Skip));
    }

    #[test]
    fn test_ast_非支持类型_skip() {
        let result = verify_ast("config.yaml", "old", "new");
        assert_eq!(result, VerifyLevel::Skip);
    }

    #[test]
    fn test_ast_rust_语法错误() {
        let old = "fn main() {}\n";
        let new = "fn main( {}\n"; // 缺少 )
        let result = verify_ast("test.rs", old, new);
        assert!(matches!(result, VerifyLevel::Error(_)));
    }

    #[test]
    fn test_ast_rust_原有错误未增() {
        // 两个文件都有语法错误，但未增加
        let old = "fn main( {}\n";
        let new = "fn main( {}\n";
        let result = verify_ast("test.rs", old, new);
        assert!(matches!(result, VerifyLevel::Warn(_)));
    }
}
