use crate::error_suggest::context::ErrorContext;
use crate::error_suggest::registry::{ErrorSuggester, Suggestion};

/// B4：Grep 工具 regex 语法错误建议
pub struct RegexSuggester;

impl ErrorSuggester for RegexSuggester {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        if ctx.tool_name != "Grep" {
            return None;
        }
        let lower = ctx.error_message.to_lowercase();
        if !lower.contains("regex parse error") && !lower.contains("regex") {
            return None;
        }
        if !lower.contains("parse error")
            && !lower.contains("unclosed")
            && !lower.contains("unbalanced")
        {
            return None;
        }

        Some(Suggestion::new(
            "正则表达式语法有误。常见问题：\n  • 括号必须闭合：() [] {}\n  • 特殊字符需转义：\\ . \\* \\+\n  • 如需字面匹配，可以用 fixed_strings: true 参数关闭正则模式\n  • 复杂模式建议先用工具（如 regex101）验证",
        ))
    }
}
