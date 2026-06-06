//! 三层验证引擎
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
    let _removals = old_lines.len().saturating_sub(new_lines.len());

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
    let mut prev_prev_char: Option<char> = None;
    let mut prev_char: Option<char> = None;
    let mut escape_next = false;

    let chars: Vec<char> = content.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let ch = chars[i];

        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            prev_prev_char = prev_char;
            prev_char = Some(ch);
            i += 1;
            continue;
        }
        if in_block_comment {
            if prev_char == Some('*') && ch == '/' {
                in_block_comment = false;
            }
            prev_prev_char = prev_char;
            prev_char = Some(ch);
            i += 1;
            continue;
        }
        if let Some(quote) = in_string {
            if escape_next {
                escape_next = false;
                prev_prev_char = prev_char;
                prev_char = Some(ch);
                i += 1;
                continue;
            }
            if ch == '\\' {
                escape_next = true;
                prev_prev_char = prev_char;
                prev_char = Some(ch);
                i += 1;
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            prev_prev_char = prev_char;
            prev_char = Some(ch);
            i += 1;
            continue;
        }

        match ch {
            '"' | '`' => in_string = Some(ch),
            '\'' => {
                // 区分 Rust lifetime（'ident）与 char literal（'x'）
                // lifetime: ' 后跟标识符字符（a-z/A-Z/_），且后面没有闭合 '
                // char literal: ' 后跟字符再跟闭合 '
                let next_is_ident = chars
                    .get(i + 1)
                    .is_some_and(|c| c.is_ascii_alphabetic() || *c == '_');
                let prev_is_delim =
                    prev_char.is_none_or(|c| !c.is_ascii_alphanumeric() && c != '_' && c != '\'');

                if next_is_ident && prev_is_delim {
                    // peek 过标识符看后面是否有 ' 闭合（char literal: 'm'）
                    let mut peek = i + 1;
                    while peek < chars.len()
                        && (chars[peek].is_ascii_alphabetic() || chars[peek] == '_')
                    {
                        peek += 1;
                    }
                    if peek < chars.len() && chars[peek] == '\'' {
                        // char literal: 'm', 'x' 等
                        in_string = Some(ch);
                    } else {
                        // Rust lifetime: 'static, 'a, 'b 等
                        let before_lifetime = prev_char;
                        i = peek; // skip past the entire lifetime identifier
                        prev_prev_char = before_lifetime;
                        prev_char = chars.get(i.wrapping_sub(1)).copied();
                        continue; // i already points past lifetime, skip i += 1
                    }
                } else {
                    in_string = Some(ch);
                }
            }
            '/' if prev_char == Some('/') && prev_prev_char != Some(':') => {
                in_line_comment = true;
            }
            '*' if prev_char == Some('/') => in_block_comment = true,
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            _ => {}
        }
        prev_prev_char = prev_char;
        prev_char = Some(ch);
        i += 1;
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
    let _ = parser.set_language(&language);

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

    // ═══════════════════════════════════════════════════════════════
    // 基础测试（原有）
    // ═══════════════════════════════════════════════════════════════

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
    fn test_括号平衡_url不触发行注释() {
        let result = verify_brackets(
            "链接 [text](https://example.com/path) 和 [more](https://another.com/x/y)",
        );
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号平衡_真正注释仍触发() {
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
        let new = "fn main( {}\n";
        let result = verify_ast("test.rs", old, new);
        assert!(matches!(result, VerifyLevel::Error(_)));
    }

    #[test]
    fn test_ast_rust_原有错误未增() {
        let old = "fn main( {}\n";
        let new = "fn main( {}\n";
        let result = verify_ast("test.rs", old, new);
        assert!(matches!(result, VerifyLevel::Warn(_)));
    }

    // ═══════════════════════════════════════════════════════════════
    // P0 — 字符串转义类（修复 escape_next 后的回归测试）
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn p0_转义引号不闭合字符串_不再误报() {
        let content = "let s = \"他说：\\\"你好\\\"\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "转义引号 \\\" 不应提前关闭字符串");
    }

    #[test]
    fn p0_双反斜杠后引号正常关闭() {
        let content = "let p = \"C:\\\\Users\\\\\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "\\\\ 后的引号应正常关闭字符串");
    }

    #[test]
    fn p0_转义引号内括号不计入平衡() {
        let content = "let s = \"\\\"()\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "转义引号后的括号在字符串内，不应计入"
        );
    }

    #[test]
    fn p0_转义反斜杠加引号不误关闭() {
        let content = "let s = \"\\\\\\\"\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "三重转义链 \\\\\\\" 应正确处理");
    }

    #[test]
    fn p0_转义单引号在char字面量不误关闭() {
        let content = "let c = '\\''; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "char 字面量中的 \\' 不应提前关闭");
    }

    #[test]
    fn p0_backtick内转义不误关闭() {
        let content = "let s = `\\``; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "backtick 内的 \\` 不应提前关闭");
    }

    #[test]
    fn p0_format宏内转义引号不误关() {
        let content = "let s = \"format!(\\\"hello\\\")\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "format 宏内转义引号不应误关闭字符串"
        );
    }

    #[test]
    fn p0_真实不平衡在转义后仍被检出_缺花括号() {
        let content = "let s = \"a\\\"b\"; {";
        let result = verify_brackets(content);
        assert!(
            matches!(result, VerifyLevel::Error(_)),
            "缺失的 }} 应被检出"
        );
    }

    #[test]
    fn p0_真实不平衡在转义后仍被检出_缺括号() {
        let content = "let s = \"a\\\"b\"; (";
        let result = verify_brackets(content);
        assert!(matches!(result, VerifyLevel::Error(_)), "缺失的 ) 应被检出");
    }

    #[test]
    fn p0_常见转义序列不影响括号() {
        let content = "let s = \"\\n\\t\\r\\\\\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn p0_字符串内括号完全忽略() {
        let content = "let s = \"(){}\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    // ═══════════════════════════════════════════════════════════════
    // P1 — 注释类
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn p1_行注释内所有括号忽略() {
        let content = "// { [ ( unbalanced\nfn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn p1_块注释内所有括号忽略() {
        let content = "/* { [ ( */ fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn p1_跨行块注释内括号忽略() {
        let content = "/* \n { [ ( \n */ fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn p1_行注释在块注释内() {
        let content = "/* // */ fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    // ═══════════════════════════════════════════════════════════════
    // P2 — URL / 特殊模式
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn p2_url在字符串内不触发注释() {
        let content = "let url = \"https://example.com/path\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn p2_除法后行注释不误判() {
        let content = "let c = a / b; // {\nfn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    // ═══════════════════════════════════════════════════════════════
    // P3 — 真实不平衡应被正确捕获
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn p3_缺少大括号检测() {
        let content = "fn main() { let x = 1;";
        let result = verify_brackets(content);
        assert!(
            matches!(result, VerifyLevel::Error(_)),
            "缺失的 }} 应被检测到"
        );
    }

    #[test]
    fn p3_多出括号检测() {
        let content = "fn main() { let x = (1 + 2)); }";
        let result = verify_brackets(content);
        assert!(matches!(result, VerifyLevel::Error(_)));
    }

    #[test]
    fn p3_方括号无匹配检测() {
        let content = "fn main() { let x = [1, 2; }";
        let result = verify_brackets(content);
        assert!(matches!(result, VerifyLevel::Error(_)));
    }

    // ═══════════════════════════════════════════════════════════════
    // P4 — Rust lifetime 与 char literal 区分（回归防护）
    // ═══════════════════════════════════════════════════════════════

    #[test]
    fn p4_static_lifetime不误开字符串() {
        let content = "fn label() -> &'static str { \"hello\" }";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "'static 不应触发字符串模式");
    }

    #[test]
    fn p4_static_lifetime后括号正常计数() {
        let content = "fn f() -> &'static str { let x = (1 + 2); x }";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "'static 后面的括号应正常计数");
    }

    #[test]
    fn p4_lifetime_param后括号正常() {
        let content = "fn foo<'a, 'b>(x: &'a str) -> &'a str { x }";
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "泛型 lifetime 参数不应干扰括号计数"
        );
    }

    #[test]
    fn p4_char_literal字母不误判为lifetime() {
        let content = "let c = 'm'; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "'m' 是 char literal，不应被当作 lifetime"
        );
    }

    #[test]
    fn p4_char_literal字母内括号不泄露() {
        let content = "KeyCode::Char('m'), fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "Char('m') 中的括号在 char literal 内"
        );
    }

    #[test]
    fn p4_char_literal_unicode不误判() {
        let content = "let c = 'µ'; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "Unicode char literal 'µ' 应正常处理"
        );
    }

    #[test]
    fn p4_char_literal_中文不误判() {
        let content = "let c = '你'; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "中文 char literal 应正常处理");
    }

    #[test]
    fn p4_混合lifetime和char_literal() {
        let content = "fn get_label() -> &'static str { let k = KeyCode::Char('m'); \"x\" }";
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "混合 lifetime 和 char literal 的代码"
        );
    }

    #[test]
    fn p4_真实rust结构体片段() {
        let content = r#"
pub(super) static BINDING: KeyBinding = KeyBinding {
    label: &'static str,
    macos_char: Some('µ'),
    key: KeyCode::Char('m'),
};
fn main() {}"#;
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "真实 Rust 结构体含 lifetime + char literal"
        );
    }

    #[test]
    fn p4_多个lifetime参数泛型() {
        let content = "struct Foo<'a, 'b, 'c> { x: &'a str, y: &'b str } fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "多个泛型 lifetime 参数");
    }

    #[test]
    fn p4_lifetime在闭包中() {
        let content = "let f: Box<dyn Fn() + 'static> = Box::new(|| { let x = 1; }); fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "闭包中的 'static lifetime");
    }

    #[test]
    fn p4_lifetime在trait_bound中() {
        let content = "fn process<T: Send + 'static>(t: T) { let _ = t; } fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "trait bound 中的 'static");
    }

    #[test]
    fn p4_转义char_literal不提前关闭() {
        let content = "let c = '\\''; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "转义 char literal '\\'' 应正常处理"
        );
    }

    #[test]
    fn p4_impl块含lifetime() {
        let content = "impl<'a> Foo<'a> { fn bar(&self) -> &'a str { \"hi\" } }";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok, "impl 块中的 lifetime 参数");
    }

    #[test]
    fn p4_超高复杂度_模拟真实文件片段() {
        let content = r#"
use std::path::Path;

pub trait Handler: Send + Sync + 'static {
    fn handle(&self, input: &str) -> &'static str;
}

pub struct KeyBinding {
    label: &'static str,
    macos_char: Option<char>,
    modifiers: KeyModifiers,
    key: KeyCode,
}

impl KeyBinding {
    pub fn matches(&self, event: &KeyEvent) -> bool {
        if let Some(ch) = self.macos_char {
            if matches!(event.code, KeyCode::Char(c) if c == ch) {
                return true;
            }
        }
        let mods_ok = event.modifiers.contains(self.modifiers);
        let key_ok = match (&self.key, &event.code) {
            (KeyCode::Char(a), KeyCode::Char(b)) => a.eq_ignore_ascii_case(b),
            (a, b) => a == b,
        };
        mods_ok && key_ok
    }
}

fn process<'a, T: 'static>(items: &'a [T]) -> Vec<&'a T> {
    let mut result = Vec::new();
    for item in items {
        if item.is_valid() {
            result.push(item);
        }
    }
    result
}

fn main() {
    let c = 'x';
    let s = "hello \"world\"";
    let url = "https://example.com/path";
    // { comment with braces
    let t = ('a', 'b', 'c');
    println!("{} {} {}", c, s, url);
}
"#;
        let result = verify_brackets(content);
        assert_eq!(
            result,
            VerifyLevel::Ok,
            "高复杂度真实 Rust 代码片段：lifetime + char literal + 字符串 + URL + 注释"
        );
    }

    #[test]
    fn p4_真实不平衡在复杂代码中仍被检出() {
        let content = r#"
fn process<'a>(x: &'a str) -> &'static str {
    let c = 'x';
    let s = "hello";
    if x.len() > 0 {
        return s;
    // 故意缺失 }
"#;
        let result = verify_brackets(content);
        assert!(
            matches!(result, VerifyLevel::Error(_)),
            "缺失的 }} 应在复杂代码中被检出"
        );
    }

    #[test]
    fn p4_真实不平衡在复杂代码中仍被检出_缺括号() {
        let content = r#"
fn calc<'a>(x: &'a [i32]) -> i32 {
    let c = 'z';
    let result = x.iter().sum(
    // 故意缺失 ) 和 ;
    result
}
"#;
        let result = verify_brackets(content);
        assert!(
            matches!(result, VerifyLevel::Error(_)),
            "缺失的 ) 应在复杂代码中被检出"
        );
    }
}
