use crate::error_suggest::context::ErrorContext;
use crate::error_suggest::registry::{ErrorSuggester, Suggestion};
use regex::Regex;
use std::sync::OnceLock;

/// B5：JSON 参数结构错误建议（参数缺失 / 类型错误）
pub struct JsonSchemaSuggester;

impl ErrorSuggester for JsonSchemaSuggester {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        let msg = ctx.error_message;

        // 模式 1：参数缺失 "'X' parameter is required" 或 "parameter 'X' is required"
        //          或 serde 风格 "missing field `X`" / "missing field X"
        static RE_REQUIRED: OnceLock<Regex> = OnceLock::new();
        let re_required = RE_REQUIRED.get_or_init(|| {
            Regex::new(r"'(\w+)' parameter is required|parameter '(\w+)' is required|missing field\s*[`'](\w+)[`']|missing field\s+(\w+)")
                .unwrap()
        });
        if let Some(caps) = re_required.captures(msg) {
            let field = caps
                .get(1)
                .or_else(|| caps.get(2))
                .or_else(|| caps.get(3))
                .or_else(|| caps.get(4))?
                .as_str();
            return Some(Suggestion::new(format!(
                "缺少必需参数 {field:?}。请检查工具 schema，补全该字段后重试。"
            )));
        }

        // 模式 2：类型错误。优先匹配 "expected X"（期望类型），fallback 取首个单词（实际类型）
        static RE_EXPECTED: OnceLock<Regex> = OnceLock::new();
        let re_expected = RE_EXPECTED
            .get_or_init(|| Regex::new(r"invalid (?:type|value).*expected\s+(\w+)").unwrap());
        static RE_INVALID: OnceLock<Regex> = OnceLock::new();
        let re_invalid =
            RE_INVALID.get_or_init(|| Regex::new(r"invalid (?:type|value).*?(\w+)").unwrap());

        let hint = re_expected
            .captures(msg)
            .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            .or_else(|| {
                re_invalid
                    .captures(msg)
                    .and_then(|c| c.get(1).map(|m| m.as_str().to_string()))
            });
        if let Some(hint) = hint {
            return Some(Suggestion::new(format!(
                "参数类型错误。期望类型：{hint}。请检查对应字段应该是字符串还是数字。"
            )));
        }

        None
    }
}
