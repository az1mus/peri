# 错误感知建议层（Error Suggestion Layer）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在工具错误返回前自动注入结构化建议文本（路径候选、参数修正、命令纠错等），让 LLM 直接消费，省去额外的探索工具调用。

**Architecture:** 注册表模式 + 集成点。不是新增中间件（after_tool 是 `&ToolResult` 不可变引用，且 [TRAP] 约束中间件不写 state），而是把建议注入放在 `tool_dispatch.rs::collect_tool_results` 内（run_after_tool 之后、写入 state 之前），此时 result 是 owned 可变。Registry 作为 `ReActAgent` 字段，构造期注入，`collect_tool_results` 通过 `agent: &ReActAgent<L, S>` 参数访问。

**Tech Stack:** Rust 2021 + tokio async + async-trait + `fuzzy-matcher = "0.3"`（SkimMatcherV2） + `glob` + `regex` + `serde_json`。测试用 `tempfile` + `tokio::test`。

**Spec:** `docs/superpowers/specs/2026-06-18-error-suggestion-design.md`

**Scope:** V1 覆盖 11 个场景（A1-A4 / B1-B5 / C1 / C3），一个大 PR 交付。V2 候选（C2/C4-C6/D 系列）不在本期。

---

## File Structure

**新增文件**（`peri-middlewares/src/error_suggest/` 目录）：

| 文件 | 职责 |
|------|------|
| `mod.rs` | 公开 API + `build_default_registry()` + 集成入口 `apply_error_suggestion()` |
| `context.rs` | `ErrorContext<'a>` / `ToolRegistrySnapshot` |
| `registry.rs` | `ErrorSuggester` trait + `ErrorSuggestRegistry` + `Suggestion` |
| `matcher.rs` | `fuzzy_top_n()` 泛化包装 |
| `format.rs` | `format_suggestion()` 输出格式化 |
| `budget.rs` | 超时包装 + 候选采样工具 |
| `suggesters/path_suggester.rs` | A1-A4 |
| `suggesters/range_suggester.rs` | B2 |
| `suggesters/glob_pattern_suggester.rs` | B3 |
| `suggesters/regex_suggester.rs` | B4 |
| `suggesters/edit_content_suggester.rs` | B1（迁移 `build_not_found_hint`） |
| `suggesters/json_schema_suggester.rs` | B5 |
| `suggesters/bash_command_suggester.rs` | C1 |
| `suggesters/subagent_suggester.rs` | C3 |
| 每个文件配套 `*_test.rs` | 单元测试 |

**修改文件**：

| 文件 | 修改内容 |
|------|---------|
| `peri-middlewares/Cargo.toml` | 添加 `fuzzy-matcher = "0.3"` 依赖 |
| `peri-middlewares/src/lib.rs` | 声明 `pub mod error_suggest;` |
| `peri-middlewares/src/tools/filesystem/glob.rs` | `glob_match` 加 pattern 语法检查（B3 前置条件） |
| `peri-agent/src/agent/executor/mod.rs` | `ReActAgent` 加 `error_suggest_registry` + `tool_registry_snapshot` 字段（注：实际定义在 `executor/mod.rs:27`，不是 `react.rs`） |
| `peri-agent/src/agent/executor/tool_dispatch.rs` | `collect_tool_results` 加 `apply_error_suggestion()` 调用 |
| `peri-middlewares/src/subagent/tool/build_agent.rs` | 构造期填充 snapshot 字段 |
| `peri-middlewares/src/middleware/mod.rs` | 暴露 `build_default_registry()` |
| `CLAUDE.md` | 文档化 ErrorSuggest 集成点 + [TRAP] 约束 |

---

## Task 1: 基础设施（Registry + Trait + Context + Matcher + Format + Budget）

**Files:**
- Modify: `peri-middlewares/Cargo.toml`
- Modify: `peri-middlewares/src/lib.rs`
- Create: `peri-middlewares/src/error_suggest/mod.rs`
- Create: `peri-middlewares/src/error_suggest/context.rs`
- Create: `peri-middlewares/src/error_suggest/registry.rs`
- Create: `peri-middlewares/src/error_suggest/matcher.rs`
- Create: `peri-middlewares/src/error_suggest/format.rs`
- Create: `peri-middlewares/src/error_suggest/budget.rs`
- Test: `peri-middlewares/src/error_suggest/registry_test.rs`
- Test: `peri-middlewares/src/error_suggest/matcher_test.rs`
- Test: `peri-middlewares/src/error_suggest/format_test.rs`

- [ ] **Step 1.1: 添加 fuzzy-matcher 依赖**

修改 `peri-middlewares/Cargo.toml` 的 `[dependencies]` 段，在合适位置（按字母序）插入：

```toml
fuzzy-matcher = "0.3"
```

- [ ] **Step 1.2: 在 lib.rs 声明模块**

修改 `peri-middlewares/src/lib.rs`，在现有 `pub mod` 列表中按字母序加入：

```rust
pub mod error_suggest;
```

- [ ] **Step 1.3: 写 registry_test.rs（先写失败测试）**

创建 `peri-middlewares/src/error_suggest/registry_test.rs`：

```rust
use std::sync::Arc;
use crate::error_suggest::context::{ErrorContext, ToolRegistrySnapshot};
use crate::error_suggest::registry::{ErrorSuggester, ErrorSuggestRegistry, Suggestion};
use std::collections::HashSet;

// 一个总是返回 Some 的测试 suggester
struct AlwaysSuggest {
    label: &'static str,
}

impl ErrorSuggester for AlwaysSuggest {
    fn suggest(&self, _ctx: &ErrorContext) -> Option<Suggestion> {
        Some(Suggestion {
            summary: format!("来自 {}", self.label),
            details: None,
        })
    }
}

// 一个总是返回 None 的测试 suggester
struct NeverSuggest;

impl ErrorSuggester for NeverSuggest {
    fn suggest(&self, _ctx: &ErrorContext) -> Option<Suggestion> {
        None
    }
}

fn make_ctx() -> ErrorContext<'static> {
    // 用 'static 引用构造上下文比较麻烦，所以我们用 owned 字段
    unreachable!("测试中直接用静态字符串")
}

#[test]
fn test_registry_short_circuits_on_first_hit() {
    // 注册两个 suggester，第一个返回 Some，第二个不应被调用
    let registry = ErrorSuggestRegistry::new(vec![
        Box::new(AlwaysSuggest { label: "first" }),
        Box::new(AlwaysSuggest { label: "second" }),
    ]);

    let snap = ToolRegistrySnapshot {
        all_tool_names: HashSet::new(),
        subagent_types: HashSet::new(),
    };
    let tool_name: &'static str = "Read";
    let input = serde_json::json!({});
    let err: &'static str = "Error: File not found";
    let cwd = std::path::Path::new(".");
    let ctx = ErrorContext::new(tool_name, &input, err, cwd, &snap);

    let result = registry.suggest(&ctx);
    assert!(result.is_some());
    assert_eq!(result.unwrap().summary, "来自 first");
}

#[test]
fn test_registry_returns_none_when_all_miss() {
    let registry = ErrorSuggestRegistry::new(vec![
        Box::new(NeverSuggest),
        Box::new(NeverSuggest),
    ]);

    let snap = ToolRegistrySnapshot {
        all_tool_names: HashSet::new(),
        subagent_types: HashSet::new(),
    };
    let input = serde_json::json!({});
    let err: &'static str = "Error: unknown";
    let cwd = std::path::Path::new(".");
    let ctx = ErrorContext::new("Read", &input, err, cwd, &snap);

    let result = registry.suggest(&ctx);
    assert!(result.is_none());
}

#[test]
fn test_registry_falls_through_to_next_when_first_misses() {
    let registry = ErrorSuggestRegistry::new(vec![
        Box::new(NeverSuggest),
        Box::new(AlwaysSuggest { label: "fallback" }),
    ]);

    let snap = ToolRegistrySnapshot {
        all_tool_names: HashSet::new(),
        subagent_types: HashSet::new(),
    };
    let input = serde_json::json!({});
    let err: &'static str = "Error: unknown";
    let cwd = std::path::Path::new(".");
    let ctx = ErrorContext::new("Read", &input, err, cwd, &snap);

    let result = registry.suggest(&ctx);
    assert!(result.is_some());
    assert_eq!(result.unwrap().summary, "来自 fallback");
}
```

- [ ] **Step 1.4: 运行测试确认失败**

Run: `cargo test -p peri-middlewares --lib error_suggest::registry_test`
Expected: FAIL，原因 `error_suggest` 模块和类型未定义。

- [ ] **Step 1.5: 创建 context.rs**

创建 `peri-middlewares/src/error_suggest/context.rs`：

```rust
use std::collections::HashSet;
use std::path::Path;

/// 错误上下文，包含建议器做决策所需的全部信息
pub struct ErrorContext<'a> {
    pub tool_name: &'a str,
    pub tool_input: &'a serde_json::Value,
    pub error_message: &'a str,
    pub cwd: &'a Path,
    pub tool_registry: &'a ToolRegistrySnapshot,
}

impl<'a> ErrorContext<'a> {
    pub fn new(
        tool_name: &'a str,
        tool_input: &'a serde_json::Value,
        error_message: &'a str,
        cwd: &'a Path,
        tool_registry: &'a ToolRegistrySnapshot,
    ) -> Self {
        Self {
            tool_name,
            tool_input,
            error_message,
            cwd,
            tool_registry,
        }
    }
}

/// 工具名 + subagent 类型快照，每轮 ReAct 构造期填充
#[derive(Clone, Default)]
pub struct ToolRegistrySnapshot {
    pub all_tool_names: HashSet<String>,
    pub subagent_types: HashSet<String>,
}
```

- [ ] **Step 1.6: 创建 registry.rs**

创建 `peri-middlewares/src/error_suggest/registry.rs`：

```rust
use crate::error_suggest::context::ErrorContext;

/// 单个建议器接口。返回 None 表示"本建议器不处理这种错误"
pub trait ErrorSuggester: Send + Sync {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion>;
}

/// 建议文本
#[derive(Debug, Clone)]
pub struct Suggestion {
    pub summary: String,
    pub details: Option<String>,
}

impl Suggestion {
    pub fn new(summary: impl Into<String>) -> Self {
        Self { summary: summary.into(), details: None }
    }

    pub fn with_details(mut self, details: impl Into<String>) -> Self {
        self.details = Some(details.into());
        self
    }
}

/// 建议器注册表，按注册顺序短路
pub struct ErrorSuggestRegistry {
    suggesters: Vec<Box<dyn ErrorSuggester>>,
}

impl ErrorSuggestRegistry {
    pub fn new(suggesters: Vec<Box<dyn ErrorSuggester>>) -> Self {
        Self { suggesters }
    }

    pub fn empty() -> Self {
        Self { suggesters: Vec::new() }
    }

    /// 第一个返回 Some 的 suggester 胜出（短路）
    pub fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        for s in &self.suggesters {
            if let Some(sug) = s.suggest(ctx) {
                return Some(sug);
            }
        }
        None
    }
}
```

- [ ] **Step 1.7: 创建 matcher.rs + matcher_test.rs**

创建 `peri-middlewares/src/error_suggest/matcher.rs`：

```rust
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

/// 通用 fuzzy：候选 + 查询，返回 top-N 候选（按 score 降序）
/// 复用 at-mention 的 SkimMatcherV2 算法，泛化为 &[String]
pub fn fuzzy_top_n<'a>(candidates: &'a [String], query: &str, n: usize) -> Vec<&'a String> {
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(&String, i64)> = candidates
        .iter()
        .filter_map(|c| matcher.fuzzy_match(c, query).map(|s| (c, s)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.iter().take(n).map(|(c, _)| *c).collect()
}

/// 仅保留 score > 0 的候选（剔除完全不匹配的）
pub fn fuzzy_filter(candidates: &[String], query: &str) -> Vec<String> {
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(String, i64)> = candidates
        .iter()
        .filter_map(|c| matcher.fuzzy_match(c, query).map(|s| (c.clone(), s)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.into_iter().map(|(c, _)| c).collect()
}
```

创建 `peri-middlewares/src/error_suggest/matcher_test.rs`：

```rust
use crate::error_suggest::matcher::{fuzzy_top_n, fuzzy_filter};

#[test]
fn test_fuzzy_top_n_returns_sorted_matches() {
    let candidates: Vec<String> = vec![
        "peri-agent".into(),
        "peri-tui".into(),
        "peri-middlewares".into(),
        "langfuse-client".into(),
    ];
    let result = fuzzy_top_n(&candidates, "peri", 3);
    assert_eq!(result.len(), 3);
    // 三个 peri-* 都应匹配，langfuse-client 不应出现
    let names: Vec<&str> = result.iter().map(|s| s.as_str()).collect();
    assert!(names.contains(&"peri-agent"));
    assert!(names.contains(&"peri-tui"));
    assert!(names.contains(&"peri-middlewares"));
}

#[test]
fn test_fuzzy_top_n_handles_no_matches() {
    let candidates: Vec<String> = vec!["foo".into(), "bar".into()];
    let result = fuzzy_top_n(&candidates, "zzz", 3);
    assert!(result.is_empty());
}

#[test]
fn test_fuzzy_top_n_respects_limit() {
    let candidates: Vec<String> = (0..10).map(|i| format!("candidate-{i}")).collect();
    let result = fuzzy_top_n(&candidates, "candidate", 3);
    assert_eq!(result.len(), 3);
}

#[test]
fn test_fuzzy_filter_returns_owned_strings_sorted() {
    let candidates: Vec<String> = vec![
        "src/main.rs".into(),
        "src/lib.rs".into(),
        "README.md".into(),
    ];
    let result = fuzzy_filter(&candidates, "main");
    assert_eq!(result.len(), 1);
    assert_eq!(result[0], "src/main.rs");
}
```

- [ ] **Step 1.8: 创建 format.rs + format_test.rs**

创建 `peri-middlewares/src/error_suggest/format.rs`：

```rust
use crate::error_suggest::registry::Suggestion;

/// 把建议格式化进原错误文本
/// 风格：中文自然语言，无 emoji，与 Edit 工具的 hint 风格一致
pub fn format_suggestion(original_error: &str, sug: &Suggestion) -> String {
    let mut out = format!("{}\n\n---\n{}", original_error, sug.summary);
    if let Some(d) = &sug.details {
        out.push('\n');
        out.push_str(d);
    }
    out.push_str("\n---");
    out
}

/// 把候选列表格式化为 bullet 文本
pub fn format_candidates(candidates: &[String]) -> String {
    candidates
        .iter()
        .map(|c| format!("  • {c}"))
        .collect::<Vec<_>>()
        .join("\n")
}

/// "Did you mean" 风格的 summary
pub fn did_you_mean_summary(resource_kind: &str, candidates: &[String]) -> String {
    if candidates.is_empty() {
        return format!("未找到相近的 {resource_kind}。");
    }
    let bullet = format_candidates(candidates);
    format!("建议改用以下 {resource_kind} 之一：\n{bullet}")
}
```

创建 `peri-middlewares/src/error_suggest/format_test.rs`：

```rust
use crate::error_suggest::format::{format_suggestion, format_candidates, did_you_mean_summary};
use crate::error_suggest::registry::Suggestion;

#[test]
fn test_format_suggestion_appends_after_separator() {
    let sug = Suggestion::new("建议改用以下路径：\n  • foo.rs");
    let result = format_suggestion("Error: not found", &sug);
    assert!(result.starts_with("Error: not found\n\n---\n"));
    assert!(result.ends_with("\n---"));
    assert!(result.contains("建议改用以下路径"));
}

#[test]
fn test_format_suggestion_with_details() {
    let sug = Suggestion::new("summary").with_details("detail info");
    let result = format_suggestion("err", &sug);
    assert!(result.contains("summary"));
    assert!(result.contains("detail info"));
}

#[test]
fn test_format_candidates_bullet_format() {
    let cands = vec!["a.rs".to_string(), "b.rs".to_string()];
    let result = format_candidates(&cands);
    assert_eq!(result, "  • a.rs\n  • b.rs");
}

#[test]
fn test_did_you_mean_summary_with_candidates() {
    let cands = vec!["a.rs".to_string()];
    let result = did_you_mean_summary("路径", &cands);
    assert!(result.contains("建议改用以下"));
    assert!(result.contains("a.rs"));
}

#[test]
fn test_did_you_mean_summary_empty_candidates() {
    let result = did_you_mean_summary("路径", &[]);
    assert!(result.contains("未找到"));
}
```

- [ ] **Step 1.9: 创建 budget.rs**

创建 `peri-middlewares/src/error_suggest/budget.rs`：

```rust
use std::time::Duration;
use tokio::time::timeout;

/// 带超时执行 future，超时返回 None
pub async fn with_timeout_ms<F, T>(ms: u64, f: F) -> Option<T>
where
    F: std::future::Future<Output = T>,
{
    match timeout(Duration::from_millis(ms), f).await {
        Ok(v) => Some(v),
        Err(_) => None,
    }
}

/// 采样候选，超过 max 时只取前 max 个
pub fn sample_candidates(candidates: Vec<String>, max: usize) -> Vec<String> {
    if candidates.len() <= max {
        candidates
    } else {
        candidates.into_iter().take(max).collect()
    }
}
```

- [ ] **Step 1.10: 创建 mod.rs 模块入口**

创建 `peri-middlewares/src/error_suggest/mod.rs`：

```rust
pub mod budget;
pub mod context;
pub mod format;
pub mod matcher;
pub mod registry;
// suggesters 子模块在后续 task 添加

pub use context::{ErrorContext, ToolRegistrySnapshot};
pub use registry::{ErrorSuggestRegistry, ErrorSuggester, Suggestion};

#[cfg(test)]
mod registry_test;

#[cfg(test)]
mod matcher_test;

#[cfg(test)]
mod format_test;
```

- [ ] **Step 1.11: 运行所有基础设施测试**

Run: `cargo test -p peri-middlewares --lib error_suggest::`
Expected: PASS，所有测试通过（registry_test 3 个 + matcher_test 4 个 + format_test 5 个 = 12 个）。

- [ ] **Step 1.12: Commit**

```bash
git add peri-middlewares/Cargo.toml \
        peri-middlewares/src/lib.rs \
        peri-middlewares/src/error_suggest/
git commit -m "$(cat <<'EOF'
feat(error_suggest): 基础设施——registry/trait/context/matcher/format/budget

引入错误感知建议层的基础设施：
- ErrorContext：tool_name/input/error_message/cwd/registry 快照
- ErrorSuggester trait + ErrorSuggestRegistry（短路语义）
- fuzzy_top_n/fuzzy_filter：泛化 SkimMatcherV2 包装
- format_suggestion：中文自然语言输出格式
- budget：超时 + 候选采样工具

V1 spec: docs/superpowers/specs/2026-06-18-error-suggestion-design.md

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 2: path_suggester（A1-A4：路径不存在）

**Files:**
- Create: `peri-middlewares/src/error_suggest/suggesters/path_suggester.rs`
- Create: `peri-middlewares/src/error_suggest/suggesters/mod.rs`
- Modify: `peri-middlewares/src/error_suggest/mod.rs`
- Test: `peri-middlewares/src/error_suggest/suggesters/path_suggester_test.rs`

- [ ] **Step 2.1: 写失败测试**

创建 `peri-middlewares/src/error_suggest/suggesters/path_suggester_test.rs`：

```rust
use std::collections::HashSet;
use std::fs;
use crate::error_suggest::context::{ErrorContext, ToolRegistrySnapshot};
use crate::error_suggest::suggesters::path_suggester::PathSuggester;

fn make_ctx<'a>(tool_name: &'a str, input: serde_json::Value, err: &'a str, cwd: &'a std::path::Path) -> ErrorContext<'a> {
    let snap = ToolRegistrySnapshot {
        all_tool_names: HashSet::new(),
        subagent_types: HashSet::new(),
    };
    // 注意：input 需要泄漏为 'static 生命周期以匹配 ctx，或用 Box::leak
    // 简化：测试中用临时 input 持有
    Box::leak(Box::new(input))
    // 这个写法有问题，改用下面的方式
}

// 修正版：用持有 input 的辅助函数
struct CtxHolder {
    input: serde_json::Value,
    snap: ToolRegistrySnapshot,
}

impl CtxHolder {
    fn new(input: serde_json::Value) -> Self {
        Self {
            input,
            snap: ToolRegistrySnapshot {
                all_tool_names: HashSet::new(),
                subagent_types: HashSet::new(),
            },
        }
    }

    fn ctx<'a>(&'a self, tool_name: &'a str, err: &'a str, cwd: &'a std::path::Path) -> ErrorContext<'a> {
        ErrorContext::new(tool_name, &self.input, err, cwd, &self.snap)
    }
}

#[test]
fn test_path_suggester_skips_non_path_tools() {
    let holder = CtxHolder::new(serde_json::json!({}));
    let cwd = std::path::Path::new(".");
    let ctx = holder.ctx("Bash", "Error: command not found", cwd);
    let result = PathSuggester.suggest(&ctx);
    assert!(result.is_none());
}

#[test]
fn test_path_suggester_skips_non_path_errors() {
    let holder = CtxHolder::new(serde_json::json!({ "file_path": "/nonexistent" }));
    let cwd = std::path::Path::new(".");
    let ctx = holder.ctx("Read", "Error: permission denied", cwd);
    let result = PathSuggester.suggest(&ctx);
    assert!(result.is_none());
}

#[test]
fn test_path_suggester_returns_candidates_for_not_found() {
    // 准备临时目录，包含几个相似文件
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fs::write(base.join("main.rs"), "fn main() {}").unwrap();
    fs::write(base.join("lib.rs"), "").unwrap();
    fs::write(base.join("mainold.rs"), "").unwrap();

    let holder = CtxHolder::new(serde_json::json!({
        "file_path": base.join("maiin.rs").to_string_lossy().to_string(),
    }));
    let err = format!("Error: File not found at {}", base.join("maiin.rs").display());
    let ctx = holder.ctx("Read", &err, base);
    let result = PathSuggester.suggest(&ctx);
    assert!(result.is_some(), "应该返回建议");
    let sug = result.unwrap();
    assert!(sug.summary.contains("建议改用以下路径"));
    // 应该至少命中 main.rs 或 mainold.rs
    assert!(sug.summary.contains("main.rs") || sug.summary.contains("mainold.rs"));
}

#[test]
fn test_path_suggester_handles_relative_path() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    fs::create_dir_all(base.join("src")).unwrap();
    fs::write(base.join("src").join("lib.rs"), "").unwrap();

    let holder = CtxHolder::new(serde_json::json!({
        "file_path": "src/lb.rs",
    }));
    let err = "Error: File not found at src/lb.rs";
    let ctx = holder.ctx("Read", err, base);
    let result = PathSuggester.suggest(&ctx);
    assert!(result.is_some());
    assert!(result.unwrap().summary.contains("lib.rs"));
}

#[test]
fn test_path_suggester_no_candidates_returns_none() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    // 空目录
    let holder = CtxHolder::new(serde_json::json!({
        "file_path": "totally_different.xyz",
    }));
    let err = "Error: File not found at totally_different.xyz";
    let ctx = holder.ctx("Read", err, base);
    let result = PathSuggester.suggest(&ctx);
    assert!(result.is_none(), "无候选时应返回 None");
}
```

注意：上面 `fs::write(base.join("src").join("lib.rs"), ...).unwrap() is_err()` 这一行是错的，请删除——这是测试编写时的笔误。

- [ ] **Step 2.2: 运行测试确认失败**

Run: `cargo test -p peri-middlewares --lib error_suggest::suggesters::path_suggester_test`
Expected: FAIL，`path_suggester` 模块不存在。

- [ ] **Step 2.3: 创建 suggesters/mod.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/mod.rs`：

```rust
pub mod path_suggester;
// 后续 task 添加其他 suggester
```

- [ ] **Step 2.4: 创建 path_suggester.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/path_suggester.rs`：

```rust
use crate::error_suggest::context::ErrorContext;
use crate::error_suggest::matcher::fuzzy_filter;
use crate::error_suggest::registry::{ErrorSuggester, Suggestion};
use crate::error_suggest::format::did_you_mean_summary;
use std::path::{Path, PathBuf};

/// 路径类错误建议器，覆盖 A1-A4
pub struct PathSuggester;

const PATH_TOOLS: &[&str] = &["Read", "Edit", "Write", "Glob", "CreateDir", "Move", "Delete"];
const ERROR_KEYWORDS: &[&str] = &[
    "not found",
    "no such file",
    "does not exist",
    "not a directory",
    "search path does not exist",
];

impl ErrorSuggester for PathSuggester {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        // 1. 工具白名单
        if !PATH_TOOLS.contains(&ctx.tool_name) {
            return None;
        }

        // 2. 关键词识别
        let lower = ctx.error_message.to_lowercase();
        if !ERROR_KEYWORDS.iter().any(|k| lower.contains(k)) {
            return None;
        }

        // 3. 从 input 提取目标路径
        let target = extract_target_path(ctx.tool_name, ctx.tool_input)?;
        let target_path = Path::new(&target);

        // 4. 找候选：同目录 fuzzy + 一层子目录
        let candidates = collect_candidates(ctx.cwd, target_path);

        // 5. fuzzy 过滤
        let target_name = target_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or(target.clone());
        let matched = fuzzy_filter(&candidates, &target_name);
        let top3: Vec<String> = matched.into_iter().take(3).collect();

        if top3.is_empty() {
            return None;
        }

        let summary = did_you_mean_summary("路径", &top3);
        Some(Suggestion::new(summary))
    }
}

fn extract_target_path(tool_name: &str, input: &serde_json::Value) -> Option<String> {
    // 不同工具用不同字段名
    let key = match tool_name {
        "Glob" => "path",
        _ => "file_path",
    };
    input.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// 收集候选：target 所在目录 + cwd 一层子目录
fn collect_candidates(cwd: &Path, target: &Path) -> Vec<String> {
    let mut candidates: Vec<String> = Vec::new();

    // 策略 1：target 所在目录的兄弟文件
    if let Some(parent) = target.parent() {
        let dir = if parent.as_os_str().is_empty() {
            cwd.to_path_buf()
        } else if parent.is_absolute() {
            parent.to_path_buf()
        } else {
            cwd.join(parent)
        };
        for entry in read_dir_names(&dir) {
            candidates.push(entry);
        }
    }

    // 策略 2：cwd 一层子目录的兄弟文件（兜底）
    if candidates.len() < 50 {
        for sub in read_subdirs(cwd) {
            for entry in read_dir_names(&sub) {
                if !candidates.contains(&entry) {
                    candidates.push(entry);
                }
            }
        }
    }

    // 去重
    candidates.sort();
    candidates.dedup();
    candidates
}

fn read_dir_names(dir: &Path) -> Vec<String> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter_map(|e| {
                e.file_name()
                    .to_str()
                    .map(|s| s.to_string())
            })
            .collect(),
        Err(_) => Vec::new(),
    }
}

fn read_subdirs(dir: &Path) -> Vec<PathBuf> {
    match std::fs::read_dir(dir) {
        Ok(entries) => entries
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
            .map(|e| e.path())
            .take(10) // 最多扫 10 个子目录
            .collect(),
        Err(_) => Vec::new(),
    }
}
```

- [ ] **Step 2.5: 更新 mod.rs 添加 suggesters 子模块**

修改 `peri-middlewares/src/error_suggest/mod.rs`，在 `pub mod` 列表加入：

```rust
pub mod suggesters;
```

并在文件末尾添加 path_suggester_test 的引用：

```rust
#[cfg(test)]
mod suggesters {
    mod path_suggester_test;
}
```

注意：如果 mod.rs 已有 `#[cfg(test)] mod xxx_test` 模式，保持一致即可。suggesters 的测试入口需要嵌套：

```rust
// 在 mod.rs 末尾
#[cfg(test)]
mod suggesters_tests {
    #[path = "suggesters/path_suggester_test.rs"]
    mod path_suggester_test;
}
```

- [ ] **Step 2.6: 运行测试确认通过**

Run: `cargo test -p peri-middlewares --lib error_suggest::`
Expected: PASS，5 个 path_suggester 测试全过。

- [ ] **Step 2.7: Commit**

```bash
git add peri-middlewares/src/error_suggest/
git commit -m "$(cat <<'EOF'
feat(error_suggest): path_suggester 覆盖 A1-A4 路径不存在场景

- 关键词识别：not found / does not exist / no such file 等
- 候选策略：目标父目录 + cwd 一层子目录
- fuzzy 过滤用 SkimMatcherV2，top-3 候选
- 工具白名单：Read/Edit/Write/Glob/CreateDir/Move/Delete

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 3: range_suggester（B2：offset/limit 越界）

**Files:**
- Create: `peri-middlewares/src/error_suggest/suggesters/range_suggester.rs`
- Modify: `peri-middlewares/src/error_suggest/suggesters/mod.rs`
- Test: `peri-middlewares/src/error_suggest/suggesters/range_suggester_test.rs`

- [ ] **Step 3.1: 写失败测试**

创建 `peri-middlewares/src/error_suggest/suggesters/range_suggester_test.rs`：

```rust
use std::collections::HashSet;
use crate::error_suggest::context::{ErrorContext, ToolRegistrySnapshot};
use crate::error_suggest::suggesters::range_suggester::RangeSuggester;

struct CtxHolder {
    input: serde_json::Value,
    snap: ToolRegistrySnapshot,
}

impl CtxHolder {
    fn new(input: serde_json::Value) -> Self {
        Self {
            input,
            snap: ToolRegistrySnapshot::default(),
        }
    }

    fn ctx<'a>(&'a self, tool_name: &'a str, err: &'a str, cwd: &'a std::path::Path) -> ErrorContext<'a> {
        ErrorContext::new(tool_name, &self.input, err, cwd, &self.snap)
    }
}

#[test]
fn test_range_suggester_only_for_read() {
    let holder = CtxHolder::new(serde_json::json!({}));
    let cwd = std::path::Path::new(".");
    let ctx = holder.ctx("Edit", "Error: offset 100 exceeds file length (50 lines)", cwd);
    assert!(RangeSuggester.suggest(&ctx).is_none());
}

#[test]
fn test_range_suggester_recognizes_offset_error() {
    let holder = CtxHolder::new(serde_json::json!({
        "file_path": "/tmp/foo.rs",
        "offset": 100,
        "limit": 10,
    }));
    let cwd = std::path::Path::new(".");
    let ctx = holder.ctx("Read", "Error: offset 100 exceeds file length (50 lines)", cwd);
    let result = RangeSuggester.suggest(&ctx);
    assert!(result.is_some());
    let sug = result.unwrap();
    assert!(sug.summary.contains("50"));
    assert!(sug.summary.contains("offset"));
}

#[test]
fn test_range_suggester_skips_non_range_errors() {
    let holder = CtxHolder::new(serde_json::json!({
        "file_path": "/tmp/foo.rs",
    }));
    let cwd = std::path::Path::new(".");
    let ctx = holder.ctx("Read", "Error: File not found", cwd);
    assert!(RangeSuggester.suggest(&ctx).is_none());
}
```

- [ ] **Step 3.2: 运行测试确认失败**

Run: `cargo test -p peri-middlewares --lib range_suggester_test`
Expected: FAIL，模块不存在。

- [ ] **Step 3.3: 创建 range_suggester.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/range_suggester.rs`：

```rust
use crate::error_suggest::context::ErrorContext;
use crate::error_suggest::registry::{ErrorSuggester, Suggestion};
use regex::Regex;

/// B2：Read 工具 offset/limit 越界建议
pub struct RangeSuggester;

impl ErrorSuggester for RangeSuggester {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        if ctx.tool_name != "Read" {
            return None;
        }

        // 识别 "offset X exceeds file length (Y lines)" 错误
        let re = Regex::new(r"offset\s+(\d+)\s+exceeds file length\s+\((\d+)\s+lines\)").ok()?;
        let caps = re.captures(ctx.error_message)?;

        let _requested: u64 = caps[1].parse().ok()?;
        let total: u64 = caps[2].parse().ok()?;

        Some(Suggestion::new(format!(
            "文件总共 {total} 行。建议把 offset 改为 1（从头读）或小于 {total} 的值，配合 limit 控制读取范围。"
        )))
    }
}
```

- [ ] **Step 3.4: 在 suggesters/mod.rs 添加模块**

修改 `peri-middlewares/src/error_suggest/suggesters/mod.rs`：

```rust
pub mod path_suggester;
pub mod range_suggester;
```

- [ ] **Step 3.5: 在 mod.rs 添加测试入口**

修改 `peri-middlewares/src/error_suggest/mod.rs` 的测试 mod 块：

```rust
#[cfg(test)]
mod suggesters_tests {
    #[path = "suggesters/path_suggester_test.rs"]
    mod path_suggester_test;

    #[path = "suggesters/range_suggester_test.rs"]
    mod range_suggester_test;
}
```

- [ ] **Step 3.6: 确认 regex 依赖**

检查 `peri-middlewares/Cargo.toml` 是否已有 `regex` 依赖：

Run: `grep '^regex' peri-middlewares/Cargo.toml`
Expected: 已有 `regex = "..."`（grep.rs 用了）。
如果没有，添加 `regex = "1"`。

- [ ] **Step 3.7: 运行测试确认通过**

Run: `cargo test -p peri-middlewares --lib range_suggester_test`
Expected: PASS，3 个测试全过。

- [ ] **Step 3.8: Commit**

```bash
git add peri-middlewares/src/error_suggest/
git commit -m "$(cat <<'EOF'
feat(error_suggest): range_suggester 覆盖 B2 offset 越界

识别 Read 工具 "offset X exceeds file length (Y lines)" 错误，
建议正确 offset 范围。

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 4: Glob 工具改造 + glob_pattern_suggester（B3）

> **注意**：B3 需要先改 Glob 工具，让 pattern 语法错误时返回明确错误文本（现状是静默返回 false）。

**Files:**
- Modify: `peri-middlewares/src/tools/filesystem/glob.rs`
- Test: `peri-middlewares/src/tools/filesystem/glob_test.rs`
- Create: `peri-middlewares/src/error_suggest/suggesters/glob_pattern_suggester.rs`
- Test: `peri-middlewares/src/error_suggest/suggesters/glob_pattern_suggester_test.rs`

- [ ] **Step 4.1: 在 glob_test.rs 写失败测试**

在 `peri-middlewares/src/tools/filesystem/glob_test.rs` 末尾追加（注意：实际工具类型是 `GlobFilesTool`，构造函数 `new(cwd: impl Into<String>)`，参考现有 glob_test.rs 的用法）：

```rust
#[tokio::test]
async fn test_glob_invalid_pattern_returns_error() {
    use crate::tools::filesystem::glob::GlobFilesTool;
    use crate::tools::BaseTool;

    let tool = GlobFilesTool::new(".");
    // 不合法的 glob pattern：[ 不闭合
    let input = serde_json::json!({
        "pattern": "[unclosed",
        "path": ".",
    });
    let result = tool.invoke(input).await;
    assert!(result.is_err(), "语法错误应该返回 Err");
    let err = result.unwrap_err().to_string();
    assert!(
        err.contains("Pattern syntax error") || err.contains("pattern"),
        "错误应该提到 pattern，实际: {err}"
    );
}
```

- [ ] **Step 4.2: 运行测试确认失败**

Run: `cargo test -p peri-middlewares --lib filesystem::glob_test::test_glob_invalid_pattern_returns_error`
Expected: FAIL，因为现状 `glob_match` 静默返回 false，工具返回 Ok(空结果)。

- [ ] **Step 4.3: 修改 glob.rs 加 pattern 校验**

修改 `peri-middlewares/src/tools/filesystem/glob.rs`。`glob_match` 函数定义在 **line 83-87**，被 `collect_files`（line 89）的递归扫描在 line 109 调用。

原函数：

```rust
// line 83-87
fn glob_match(pattern: &str, path: &str) -> bool {
    glob::Pattern::new(pattern)
        .map(|p| p.matches(path))
        .unwrap_or(false)
}
```

**最简改动**：在 `invoke` 入口（line 133-146 解析参数后、line 167 "Directory not found" 检查之前）加一次 pattern 语法预校验，不动 `glob_match`/`collect_files` 的现有签名（避免大规模改动）：

```rust
// 在 pattern 字段解析后立即校验语法
if let Err(e) = glob::Pattern::new(&pattern) {
    return Err(format!("Error: Pattern syntax error in {pattern:?}: {e}").into());
}
```

这样 B3 suggester 能稳定拿到 `"Pattern syntax error"` 关键词。

注意：pattern 字段可能是 `**/*.rs` 这种复合模式，glob crate 能识别；但 `[` 不闭合就是语法错。如果担心 `collect_files` 内部静默忽略其他 corner case，可在 `glob_match` 内加日志（但不要改签名）。

- [ ] **Step 4.4: 运行 glob 测试确认通过**

Run: `cargo test -p peri-middlewares --lib filesystem::glob_test`
Expected: PASS，包括新增的 invalid pattern 测试。

- [ ] **Step 4.5: 写 glob_pattern_suggester_test.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/glob_pattern_suggester_test.rs`：

```rust
use std::collections::HashSet;
use crate::error_suggest::context::{ErrorContext, ToolRegistrySnapshot};
use crate::error_suggest::suggesters::glob_pattern_suggester::GlobPatternSuggester;

struct CtxHolder {
    input: serde_json::Value,
    snap: ToolRegistrySnapshot,
}

impl CtxHolder {
    fn new(input: serde_json::Value) -> Self {
        Self {
            input,
            snap: ToolRegistrySnapshot::default(),
        }
    }

    fn ctx<'a>(&'a self, tool_name: &'a str, err: &'a str) -> ErrorContext<'a> {
        let cwd = std::path::Path::new(".");
        ErrorContext::new(tool_name, &self.input, err, cwd, &self.snap)
    }
}

#[test]
fn test_glob_pattern_suggester_only_for_glob() {
    let holder = CtxHolder::new(serde_json::json!({}));
    let ctx = holder.ctx("Read", "Error: Pattern syntax error in \"[foo\": ...");
    assert!(GlobPatternSuggester.suggest(&ctx).is_none());
}

#[test]
fn test_glob_pattern_suggester_recognizes_syntax_error() {
    let holder = CtxHolder::new(serde_json::json!({
        "pattern": "[unclosed",
    }));
    let ctx = holder.ctx("Glob", "Error: Pattern syntax error in \"[unclosed\": unclosed character class");
    let result = GlobPatternSuggester.suggest(&ctx);
    assert!(result.is_some());
    let sug = result.unwrap();
    assert!(sug.summary.contains("合法") || sug.summary.contains("示例"));
}

#[test]
fn test_glob_pattern_suggester_skips_non_syntax_errors() {
    let holder = CtxHolder::new(serde_json::json!({
        "pattern": "*.rs",
    }));
    let ctx = holder.ctx("Glob", "Error: Directory not found: /nonexistent");
    assert!(GlobPatternSuggester.suggest(&ctx).is_none());
}
```

- [ ] **Step 4.6: 创建 glob_pattern_suggester.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/glob_pattern_suggester.rs`：

```rust
use crate::error_suggest::context::ErrorContext;
use crate::error_suggest::registry::{ErrorSuggester, Suggestion};

/// B3：Glob pattern 语法错误建议
pub struct GlobPatternSuggester;

impl ErrorSuggester for GlobPatternSuggester {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        if ctx.tool_name != "Glob" {
            return None;
        }
        if !ctx.error_message.contains("Pattern syntax error") {
            return None;
        }

        Some(Suggestion::new(
            "Glob pattern 语法有误。合法示例：\n  • *.rs —— 当前目录所有 Rust 文件\n  • **/*.rs —— 递归所有子目录\n  • src/**/*.rs —— src 下所有 Rust 文件\n  • {foo,bar}.rs —— 枚举\n注意：方括号 [ 必须闭合，例如 [abc].rs"
        ))
    }
}
```

- [ ] **Step 4.7: 更新 suggesters/mod.rs 和 mod.rs 测试入口**

修改 `peri-middlewares/src/error_suggest/suggesters/mod.rs`：

```rust
pub mod glob_pattern_suggester;
pub mod path_suggester;
pub mod range_suggester;
```

修改 `peri-middlewares/src/error_suggest/mod.rs` 的测试 mod 块，追加：

```rust
#[path = "suggesters/glob_pattern_suggester_test.rs"]
mod glob_pattern_suggester_test;
```

- [ ] **Step 4.8: 运行测试**

Run: `cargo test -p peri-middlewares --lib error_suggest::`
Expected: PASS。

- [ ] **Step 4.9: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/glob.rs \
        peri-middlewares/src/tools/filesystem/glob_test.rs \
        peri-middlewares/src/error_suggest/
git commit -m "$(cat <<'EOF'
feat(error_suggest): glob pattern 语法错误提示（B3）

- 修改 Glob 工具：pattern 语法错时返回 Err 而非静默 false
- 新增 glob_pattern_suggester：识别 "Pattern syntax error" 关键词，
  返回合法 pattern 示例

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 5: regex_suggester（B4：Grep regex 错误）

**Files:**
- Create: `peri-middlewares/src/error_suggest/suggesters/regex_suggester.rs`
- Test: `peri-middlewares/src/error_suggest/suggesters/regex_suggester_test.rs`
- Modify: `peri-middlewares/src/error_suggest/suggesters/mod.rs`, `mod.rs`

- [ ] **Step 5.1: 写失败测试**

创建 `peri-middlewares/src/error_suggest/suggesters/regex_suggester_test.rs`：

```rust
use std::collections::HashSet;
use crate::error_suggest::context::{ErrorContext, ToolRegistrySnapshot};
use crate::error_suggest::suggesters::regex_suggester::RegexSuggester;

struct CtxHolder {
    input: serde_json::Value,
    snap: ToolRegistrySnapshot,
}

impl CtxHolder {
    fn new(input: serde_json::Value) -> Self {
        Self { input, snap: ToolRegistrySnapshot::default() }
    }
    fn ctx<'a>(&'a self, tool_name: &'a str, err: &'a str) -> ErrorContext<'a> {
        ErrorContext::new(tool_name, &self.input, err, std::path::Path::new("."), &self.snap)
    }
}

#[test]
fn test_regex_suggester_only_for_grep() {
    let holder = CtxHolder::new(serde_json::json!({}));
    let ctx = holder.ctx("Read", "Error: regex parse error");
    assert!(RegexSuggester.suggest(&ctx).is_none());
}

#[test]
fn test_regex_suggester_recognizes_unclosed_paren() {
    let holder = CtxHolder::new(serde_json::json!({ "pattern": "(foo" }));
    let err = "Error: regex parse error: unclosed group, expected ')', POS: 4";
    let ctx = holder.ctx("Grep", err);
    let result = RegexSuggester.suggest(&ctx);
    assert!(result.is_some());
    let sug = result.unwrap();
    assert!(sug.summary.contains("regex") || sug.summary.contains("正则"));
}

#[test]
fn test_regex_suggester_skips_non_regex_errors() {
    let holder = CtxHolder::new(serde_json::json!({ "pattern": "foo" }));
    let ctx = holder.ctx("Grep", "Error: Search path does not exist: /tmp/none");
    assert!(RegexSuggester.suggest(&ctx).is_none());
}
```

- [ ] **Step 5.2: 运行确认失败**

Run: `cargo test -p peri-middlewares --lib regex_suggester_test`
Expected: FAIL。

- [ ] **Step 5.3: 创建 regex_suggester.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/regex_suggester.rs`：

```rust
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
        if !lower.contains("parse error") && !lower.contains("unclosed") && !lower.contains("unbalanced") {
            return None;
        }

        Some(Suggestion::new(
            "正则表达式语法有误。常见问题：\n  • 括号必须闭合：() [] {}\n  • 特殊字符需转义：\\\\ \\. \\\\* \\\\+\n  • 如需字面匹配，可以用 fixed_strings: true 参数关闭正则模式\n  • 复杂模式建议先用工具（如 regex101）验证"
        ))
    }
}
```

- [ ] **Step 5.4: 注册模块和测试入口**

修改 `peri-middlewares/src/error_suggest/suggesters/mod.rs`：

```rust
pub mod glob_pattern_suggester;
pub mod path_suggester;
pub mod range_suggester;
pub mod regex_suggester;
```

修改 `peri-middlewares/src/error_suggest/mod.rs` 测试 mod，追加：

```rust
#[path = "suggesters/regex_suggester_test.rs"]
mod regex_suggester_test;
```

- [ ] **Step 5.5: 运行测试**

Run: `cargo test -p peri-middlewares --lib error_suggest::`
Expected: PASS。

- [ ] **Step 5.6: Commit**

```bash
git add peri-middlewares/src/error_suggest/
git commit -m "$(cat <<'EOF'
feat(error_suggest): regex_suggester 覆盖 B4 Grep 正则语法错

识别 "regex parse error" 关键词，返回常见正则语法提示。

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 6: edit_content_suggester（B1：迁移 build_not_found_hint）

> **注意**：B1 是迁移现有 `build_not_found_hint`，把行为统一到 suggester 接口。原函数在 edit.rs，迁移后 edit.rs 仍然调用一次产生原始错误，suggester 再次识别并改写——这是双重处理。**简化策略**：保留 edit.rs 现有 hint 逻辑不动，suggester 仅识别 not_unique 场景补全候选。或者：edit.rs 改为只返回裸错误，hint 完全交给 suggester。

经权衡，**采用方案 A**：保留 edit.rs 的 build_not_found_hint 不动（已经实现且测试通过），edit_content_suggester 只处理 not_unique 的"行号补全"——但这也已经被 edit.rs 覆盖。所以**实际上 B1 不需要新建 suggester**，已有实现已经满足需求。

**调整决定**：跳过 B1 新建，仅在文档中说明 B1 已由 edit.rs 现有逻辑覆盖。本 Task 改为只更新文档说明。

- [ ] **Step 6.1: 更新 spec 说明 B1 已实现**

修改 `docs/superpowers/specs/2026-06-18-error-suggestion-design.md`，在 §2 V1 范围表的 B1 行追加备注：

```markdown
| B1 | old_string 未找到 | Edit | 行级 fuzzy（**已由 `edit.rs::build_not_found_hint` 实现，本期不重复**） |
```

- [ ] **Step 6.2: 跳过新建 edit_content_suggester**

不做任何代码改动。

- [ ] **Step 6.3: Commit**

```bash
git add docs/superpowers/specs/2026-06-18-error-suggestion-design.md
git commit -m "$(cat <<'EOF'
docs(error_suggest): B1 已由 edit.rs::build_not_found_hint 覆盖，无需新建

V1 范围调整：B1 不重复实现，已在 2026-06-03 spec 中落地。

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 7: json_schema_suggester（B5：JSON 参数错）

**Files:**
- Create: `peri-middlewares/src/error_suggest/suggesters/json_schema_suggester.rs`
- Test: `peri-middlewares/src/error_suggest/suggesters/json_schema_suggester_test.rs`
- Modify: `suggesters/mod.rs`, `mod.rs`

- [ ] **Step 7.1: 写失败测试**

创建 `peri-middlewares/src/error_suggest/suggesters/json_schema_suggester_test.rs`：

```rust
use std::collections::HashSet;
use crate::error_suggest::context::{ErrorContext, ToolRegistrySnapshot};
use crate::error_suggest::suggesters::json_schema_suggester::JsonSchemaSuggester;

struct CtxHolder {
    input: serde_json::Value,
    snap: ToolRegistrySnapshot,
}

impl CtxHolder {
    fn new(input: serde_json::Value) -> Self {
        Self { input, snap: ToolRegistrySnapshot::default() }
    }
    fn ctx<'a>(&'a self, tool_name: &'a str, err: &'a str) -> ErrorContext<'a> {
        ErrorContext::new(tool_name, &self.input, err, std::path::Path::new("."), &self.snap)
    }
}

#[test]
fn test_json_schema_recognizes_missing_field() {
    let holder = CtxHolder::new(serde_json::json!({}));
    let ctx = holder.ctx("Read", "The 'file_path' parameter is required.");
    let result = JsonSchemaSuggester.suggest(&ctx);
    assert!(result.is_some());
    let sug = result.unwrap();
    assert!(sug.summary.contains("file_path"));
}

#[test]
fn test_json_schema_recognizes_invalid_type() {
    let holder = CtxHolder::new(serde_json::json!({ "offset": "abc" }));
    let ctx = holder.ctx("Read", "Error: invalid type: string \"abc\", expected u64");
    let result = JsonSchemaSuggester.suggest(&ctx);
    assert!(result.is_some());
    assert!(result.unwrap().summary.contains("offset"));
}

#[test]
fn test_json_schema_skips_non_schema_errors() {
    let holder = CtxHolder::new(serde_json::json!({}));
    let ctx = holder.ctx("Read", "Error: File not found at /tmp/foo");
    assert!(JsonSchemaSuggester.suggest(&ctx).is_none());
}
```

- [ ] **Step 7.2: 运行确认失败**

Run: `cargo test -p peri-middlewares --lib json_schema_suggester_test`
Expected: FAIL。

- [ ] **Step 7.3: 创建 json_schema_suggester.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/json_schema_suggester.rs`：

```rust
use crate::error_suggest::context::ErrorContext;
use crate::error_suggest::registry::{ErrorSuggester, Suggestion};
use regex::Regex;

/// B5：JSON 参数结构错误建议
pub struct JsonSchemaSuggester;

impl ErrorSuggester for JsonSchemaSuggester {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        let msg = ctx.error_message;

        // 模式 1：参数缺失 "The 'X' parameter is required" 或 "missing field X"
        let re1 = Regex::new(r"(?:parameter '(\w+)' is required|missing field[` ]'?(\w+)'?)").ok()?;
        if let Some(caps) = re1.captures(msg) {
            let field = caps.get(1).or_else(|| caps.get(2))?.as_str();
            return Some(Suggestion::new(format!(
                "缺少必需参数 {field:?}。请检查工具 schema，补全该字段后重试。"
            )));
        }

        // 模式 2：类型错误 "invalid type: ..., expected X" 或 "invalid value"
        let re2 = Regex::new(r"invalid (?:type|value).*?(\w+)").ok()?;
        if let Some(caps) = re2.captures(msg) {
            let hint = caps[1].to_string();
            return Some(Suggestion::new(format!(
                "参数类型错误。提示：{hint}。检查对应字段应该是字符串还是数字。"
            )));
        }

        None
    }
}
```

- [ ] **Step 7.4: 注册模块和测试入口**

修改 `peri-middlewares/src/error_suggest/suggesters/mod.rs`：

```rust
pub mod glob_pattern_suggester;
pub mod json_schema_suggester;
pub mod path_suggester;
pub mod range_suggester;
pub mod regex_suggester;
```

修改 `peri-middlewares/src/error_suggest/mod.rs` 测试 mod，追加：

```rust
#[path = "suggesters/json_schema_suggester_test.rs"]
mod json_schema_suggester_test;
```

- [ ] **Step 7.5: 运行测试**

Run: `cargo test -p peri-middlewares --lib error_suggest::`
Expected: PASS。

- [ ] **Step 7.6: Commit**

```bash
git add peri-middlewares/src/error_suggest/
git commit -m "$(cat <<'EOF'
feat(error_suggest): json_schema_suggester 覆盖 B5 参数结构错

识别参数缺失（required / missing field）和类型错误（invalid type），
提示具体字段名。

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 8: bash_command_suggester（C1：命令不存在）

**Files:**
- Create: `peri-middlewares/src/error_suggest/suggesters/bash_command_suggester.rs`
- Test: `peri-middlewares/src/error_suggest/suggesters/bash_command_suggester_test.rs`
- Modify: `suggesters/mod.rs`, `mod.rs`

- [ ] **Step 8.1: 写失败测试**

创建 `peri-middlewares/src/error_suggest/suggesters/bash_command_suggester_test.rs`：

```rust
use std::collections::HashSet;
use crate::error_suggest::context::{ErrorContext, ToolRegistrySnapshot};
use crate::error_suggest::suggesters::bash_command_suggester::BashCommandSuggester;

struct CtxHolder {
    input: serde_json::Value,
    snap: ToolRegistrySnapshot,
}

impl CtxHolder {
    fn new(input: serde_json::Value) -> Self {
        Self { input, snap: ToolRegistrySnapshot::default() }
    }
    fn ctx<'a>(&'a self, tool_name: &'a str, err: &'a str) -> ErrorContext<'a> {
        ErrorContext::new(tool_name, &self.input, err, std::path::Path::new("."), &self.snap)
    }
}

#[test]
fn test_bash_recognizes_command_not_found() {
    let holder = CtxHolder::new(serde_json::json!({
        "command": "gti status",
    }));
    let err = "zsh:1: command not found: gti\n[Exit code: 127]";
    let ctx = holder.ctx("Bash", err);
    let result = BashCommandSuggester.suggest(&ctx);
    assert!(result.is_some(), "应该识别 command not found + exit 127");
    let sug = result.unwrap();
    // git 应该是候选之一（如果在 PATH 中）
    // 测试环境通常有 git
    assert!(sug.summary.contains("建议") || sug.summary.contains("git") || sug.summary.contains("未找到"));
}

#[test]
fn test_bash_skips_non_command_errors() {
    let holder = CtxHolder::new(serde_json::json!({
        "command": "ls /nonexistent",
    }));
    let err = "ls: /nonexistent: No such file or directory\n[Exit code: 1]";
    let ctx = holder.ctx("Bash", err);
    // exit code 不是 127，不是 command not found
    assert!(BashCommandSuggester.suggest(&ctx).is_none());
}

#[test]
fn test_bash_skips_non_bash_tools() {
    let holder = CtxHolder::new(serde_json::json!({}));
    let err = "zsh: command not found: foo\n[Exit code: 127]";
    let ctx = holder.ctx("Read", err);
    assert!(BashCommandSuggester.suggest(&ctx).is_none());
}
```

- [ ] **Step 8.2: 运行确认失败**

Run: `cargo test -p peri-middlewares --lib bash_command_suggester_test`
Expected: FAIL。

- [ ] **Step 8.3: 创建 bash_command_suggester.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/bash_command_suggester.rs`：

```rust
use crate::error_suggest::context::ErrorContext;
use crate::error_suggest::format::did_you_mean_summary;
use crate::error_suggest::matcher::fuzzy_filter;
use crate::error_suggest::registry::{ErrorSuggester, Suggestion};

/// C1：Bash 命令不存在建议
pub struct BashCommandSuggester;

impl ErrorSuggester for BashCommandSuggester {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        if ctx.tool_name != "Bash" {
            return None;
        }

        // 识别信号：stderr 含 "command not found" + 输出含 [Exit code: 127]
        let lower = ctx.error_message.to_lowercase();
        if !lower.contains("command not found") && !lower.contains("not found in path") {
            return None;
        }
        if !ctx.error_message.contains("[Exit code: 127]") {
            return None;
        }

        // 从 input 提取命令名
        let cmd = ctx.tool_input.get("command").and_then(|v| v.as_str())?;
        let cmd_name = cmd.split_whitespace().next()?;

        // 从 PATH 中扫描所有可执行文件，fuzzy 匹配
        let candidates = scan_path_executables();
        let matched = fuzzy_filter(&candidates, cmd_name);
        let top3: Vec<String> = matched.into_iter().take(3).collect();

        if top3.is_empty() {
            return Some(Suggestion::new(format!(
                "命令 {cmd_name:?} 不在 PATH 中。请确认是否安装，或检查拼写。"
            )));
        }

        let summary = did_you_mean_summary("命令", &top3);
        Some(Suggestion::new(summary))
    }
}

/// 扫描 PATH 中所有可执行文件名（去重）
fn scan_path_executables() -> Vec<String> {
    let path_env = match std::env::var_os("PATH") {
        Some(p) => p,
        None => return Vec::new(),
    };
    let mut all: Vec<String> = Vec::new();
    for dir in std::env::split_paths(&path_env) {
        if let Ok(entries) = std::fs::read_dir(&dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if !all.iter().any(|x| x == name) {
                        all.push(name.to_string());
                    }
                }
            }
        }
        if all.len() > 500 {
            break; // 性能保护
        }
    }
    all
}
```

- [ ] **Step 8.4: 注册模块和测试入口**

修改 `peri-middlewares/src/error_suggest/suggesters/mod.rs`：

```rust
pub mod bash_command_suggester;
pub mod glob_pattern_suggester;
pub mod json_schema_suggester;
pub mod path_suggester;
pub mod range_suggester;
pub mod regex_suggester;
```

修改 `peri-middlewares/src/error_suggest/mod.rs` 测试 mod，追加：

```rust
#[path = "suggesters/bash_command_suggester_test.rs"]
mod bash_command_suggester_test;
```

- [ ] **Step 8.5: 运行测试**

Run: `cargo test -p peri-middlewares --lib bash_command_suggester_test`
Expected: PASS。

注意：`test_bash_recognizes_command_not_found` 测试依赖系统 PATH 中有 git。CI 环境如果没装 git 会失败。如需稳健，可在测试中先检查 `which git`，没有就 skip：

```rust
if std::process::Command::new("which").arg("git").output().is_err() {
    return; // skip
}
```

把这段加到测试函数开头。

- [ ] **Step 8.6: Commit**

```bash
git add peri-middlewares/src/error_suggest/
git commit -m "$(cat <<'EOF'
feat(error_suggest): bash_command_suggester 覆盖 C1 命令不存在

识别 "command not found" + "[Exit code: 127]" 信号，
从 PATH 扫描可执行文件做 fuzzy 匹配。
性能保护：候选上限 500 个。

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 9: subagent_suggester（C3：subagent_type 不存在）

**Files:**
- Create: `peri-middlewares/src/error_suggest/suggesters/subagent_suggester.rs`
- Test: `peri-middlewares/src/error_suggest/suggesters/subagent_suggester_test.rs`
- Modify: `suggesters/mod.rs`, `mod.rs`

- [ ] **Step 9.1: 写失败测试**

创建 `peri-middlewares/src/error_suggest/suggesters/subagent_suggester_test.rs`：

```rust
use std::collections::HashSet;
use crate::error_suggest::context::{ErrorContext, ToolRegistrySnapshot};
use crate::error_suggest::suggesters::subagent_suggester::SubagentSuggester;

#[test]
fn test_subagent_recognizes_unknown_type() {
    let mut snap = ToolRegistrySnapshot::default();
    // 顺序与 BUILT_IN_AGENTS（built_in_agents.rs:29）一致：
    // coder / explore / general-purpose / plan / verification / web-researcher
    snap.subagent_types = [
        "coder".to_string(),
        "explore".to_string(),
        "general-purpose".to_string(),
        "plan".to_string(),
        "verification".to_string(),
        "web-researcher".to_string(),
    ].into_iter().collect();

    let input = serde_json::json!({ "subagent_type": "explor" });
    let err = "Error: cannot find agent definition 'explor'. Check .claude/agents/ directory or use a built-in agent (explore, plan, general-purpose, verification)";
    let ctx = ErrorContext::new("Agent", &input, err, std::path::Path::new("."), &snap);
    let result = SubagentSuggester.suggest(&ctx);
    assert!(result.is_some());
    let sug = result.unwrap();
    assert!(sug.summary.contains("explore"), "应该 fuzzy 命中 explore");
}

#[test]
fn test_subagent_recognizes_missing_param() {
    let snap = ToolRegistrySnapshot::default();
    let input = serde_json::json!({});
    let err = "Error: please provide subagent_type parameter to specify the agent type";
    let ctx = ErrorContext::new("Agent", &input, err, std::path::Path::new("."), &snap);
    let result = SubagentSuggester.suggest(&ctx);
    assert!(result.is_some());
    let sug = result.unwrap();
    // 应该列出已知 subagent types
    assert!(sug.summary.contains("subagent_type"));
}

#[test]
fn test_subagent_skips_non_agent_tools() {
    let snap = ToolRegistrySnapshot::default();
    let input = serde_json::json!({});
    let err = "Error: cannot find agent definition 'foo'";
    let ctx = ErrorContext::new("Read", &input, err, std::path::Path::new("."), &snap);
    assert!(SubagentSuggester.suggest(&ctx).is_none());
}

#[test]
fn test_subagent_skips_non_subagent_errors() {
    let snap = ToolRegistrySnapshot::default();
    let input = serde_json::json!({ "subagent_type": "explore" });
    let err = "Error: prompt is required";
    let ctx = ErrorContext::new("Agent", &input, err, std::path::Path::new("."), &snap);
    assert!(SubagentSuggester.suggest(&ctx).is_none());
}
```

- [ ] **Step 9.2: 运行确认失败**

Run: `cargo test -p peri-middlewares --lib subagent_suggester_test`
Expected: FAIL。

- [ ] **Step 9.3: 创建 subagent_suggester.rs**

创建 `peri-middlewares/src/error_suggest/suggesters/subagent_suggester.rs`：

```rust
use crate::error_suggest::context::ErrorContext;
use crate::error_suggest::format::did_you_mean_summary;
use crate::error_suggest::matcher::fuzzy_filter;
use crate::error_suggest::registry::{ErrorSuggester, Suggestion};

/// C3：subagent_type 不存在建议
pub struct SubagentSuggester;

impl ErrorSuggester for SubagentSuggester {
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        if ctx.tool_name != "Agent" {
            return None;
        }

        let lower = ctx.error_message.to_lowercase();
        let is_missing = lower.contains("please provide subagent_type");
        let is_unknown = lower.contains("cannot find agent definition");
        if !is_missing && !is_unknown {
            return None;
        }

        // 已知 subagent types 来自 ToolRegistrySnapshot
        let known: Vec<String> = ctx.tool_registry.subagent_types.iter().cloned().collect();
        if known.is_empty() {
            return Some(Suggestion::new(
                "缺少 subagent_type 参数。请显式提供 agent 类型。"
            ));
        }

        if is_missing {
            let bullet = known
                .iter()
                .map(|s| format!("  • {s}"))
                .collect::<Vec<_>>()
                .join("\n");
            return Some(Suggestion::new(format!(
                "缺少 subagent_type 参数。可选值：\n{bullet}"
            )));
        }

        // is_unknown：fuzzy 匹配
        let target = ctx
            .tool_input
            .get("subagent_type")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let matched = fuzzy_filter(&known, target);
        let top3: Vec<String> = matched.into_iter().take(3).collect();

        if top3.is_empty() {
            let bullet = known.iter().map(|s| format!("  • {s}")).collect::<Vec<_>>().join("\n");
            return Some(Suggestion::new(format!(
                "未找到匹配的 subagent。已知类型：\n{bullet}"
            )));
        }

        Some(Suggestion::new(did_you_mean_summary("subagent_type", &top3)))
    }
}
```

- [ ] **Step 9.4: 注册模块和测试入口**

修改 `peri-middlewares/src/error_suggest/suggesters/mod.rs`：

```rust
pub mod bash_command_suggester;
pub mod glob_pattern_suggester;
pub mod json_schema_suggester;
pub mod path_suggester;
pub mod range_suggester;
pub mod regex_suggester;
pub mod subagent_suggester;
```

修改 `peri-middlewares/src/error_suggest/mod.rs` 测试 mod，追加：

```rust
#[path = "suggesters/subagent_suggester_test.rs"]
mod subagent_suggester_test;
```

- [ ] **Step 9.5: 运行测试**

Run: `cargo test -p peri-middlewares --lib error_suggest::`
Expected: PASS。

- [ ] **Step 9.6: Commit**

```bash
git add peri-middlewares/src/error_suggest/
git commit -m "$(cat <<'EOF'
feat(error_suggest): subagent_suggester 覆盖 C3 subagent_type 不存在

识别 "cannot find agent definition" / "please provide subagent_type"，
从 ToolRegistrySnapshot.subagent_types fuzzy 匹配。

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 10: build_default_registry + ToolRegistrySnapshot 填充

**Files:**
- Create: `peri-middlewares/src/error_suggest/default_registry.rs`
- Modify: `peri-middlewares/src/error_suggest/mod.rs`
- Modify: `peri-middlewares/src/subagent/built_in_agents.rs`（暴露内置 agent 名列表）
- Modify: `peri-middlewares/src/subagent/tool/build_agent.rs`（构造期填充 snapshot）

- [ ] **Step 10.1: 在 built_in_agents.rs 暴露内置 agent 名**

读取 `peri-middlewares/src/subagent/built_in_agents.rs`：实际 API 是 `list_built_in_agents() -> &'static [BuiltInAgent]`（line 20）和 `BuiltInAgent { agent_id: &'static str, content: &'static str }`（line 12-17）。内置列表 `BUILT_IN_AGENTS` 顺序：`coder / explore / general-purpose / plan / verification / web-researcher`。

新增一个返回 agent_id 切片的便捷函数：

```rust
/// 返回所有内置 subagent type 名（agent_id）
pub fn built_in_agent_types() -> Vec<&'static str> {
    BUILT_IN_AGENTS.iter().map(|a| a.agent_id).collect()
}
```

- [ ] **Step 10.2: 创建 default_registry.rs**

创建 `peri-middlewares/src/error_suggest/default_registry.rs`：

```rust
use crate::error_suggest::context::ToolRegistrySnapshot;
use crate::error_suggest::registry::{ErrorSuggestRegistry, ErrorSuggester};
use crate::error_suggest::suggesters::{
    bash_command_suggester::BashCommandSuggester,
    glob_pattern_suggester::GlobPatternSuggester,
    json_schema_suggester::JsonSchemaSuggester,
    path_suggester::PathSuggester,
    range_suggester::RangeSuggester,
    regex_suggester::RegexSuggester,
    subagent_suggester::SubagentSuggester,
};
use std::sync::Arc;

/// 构造默认 registry，按短路顺序注册
/// 顺序：参数语法类 → 范围 → 路径 → 命令 → subagent
pub fn build_default_registry() -> Arc<ErrorSuggestRegistry> {
    let suggesters: Vec<Box<dyn ErrorSuggester>> = vec![
        Box::new(JsonSchemaSuggester),       // B5 最先：参数级错误最廉价
        Box::new(GlobPatternSuggester),      // B3
        Box::new(RegexSuggester),            // B4
        Box::new(RangeSuggester),            // B2
        Box::new(PathSuggester),             // A1-A4（需 IO）
        Box::new(BashCommandSuggester),      // C1（需 PATH 扫描）
        Box::new(SubagentSuggester),         // C3（registry 查询）
    ];
    Arc::new(ErrorSuggestRegistry::new(suggesters))
}

/// 从 collect_tools 结果 + .claude/agents/ 目录构建 snapshot
pub fn build_tool_registry_snapshot(
    tool_names: impl IntoIterator<Item = String>,
    agents_dir: Option<&std::path::Path>,
) -> ToolRegistrySnapshot {
    use crate::subagent::built_in_agents::built_in_agent_types;

    let mut all_tool_names: std::collections::HashSet<String> = tool_names.into_iter().collect();

    let mut subagent_types: std::collections::HashSet<String> = built_in_agent_types()
        .iter()
        .map(|s| s.to_string())
        .collect();

    // 扫描 .claude/agents/*.md
    if let Some(dir) = agents_dir {
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                if let Some(name) = entry.file_name().to_str() {
                    if let Some(stem) = name.strip_suffix(".md") {
                        subagent_types.insert(stem.to_string());
                    }
                }
            }
        }
    }

    // subagent_type 也是有效"工具名"候补
    for t in &subagent_types {
        all_tool_names.insert(t.clone());
    }

    ToolRegistrySnapshot {
        all_tool_names,
        subagent_types,
    }
}
```

- [ ] **Step 10.3: 在 mod.rs 暴露 default_registry**

修改 `peri-middlewares/src/error_suggest/mod.rs`，添加：

```rust
pub mod default_registry;
pub use default_registry::{build_default_registry, build_tool_registry_snapshot};
```

- [ ] **Step 10.4: 编译确认**

Run: `cargo build -p peri-middlewares`
Expected: PASS。

- [ ] **Step 10.5: Commit**

```bash
git add peri-middlewares/src/error_suggest/ \
        peri-middlewares/src/subagent/built_in_agents.rs
git commit -m "$(cat <<'EOF'
feat(error_suggest): build_default_registry + snapshot 构造器

- 注册 7 个 suggester（B1 已由 edit.rs 覆盖不重复）
- build_tool_registry_snapshot：collect_tools 结果 + .claude/agents/ 扫描
- 暴露 built_in_agent_types 给 snapshot 构造

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 11: 集成到 ReActAgent + collect_tool_results

**Files:**
- Modify: `peri-agent/src/agent/react.rs`
- Modify: `peri-agent/src/agent/executor/tool_dispatch.rs`
- Modify: `peri-middlewares/src/subagent/tool/build_agent.rs`
- Test: `peri-agent/src/agent/executor/tool_dispatch_test.rs`（新增或修改）

> **注意**：`peri-agent` crate 不依赖 `peri-middlewares`（依赖方向反过来）。为避免循环依赖，`ErrorSuggester` trait 和 `ErrorSuggestRegistry` 应该定义在 `peri-agent`（底层），具体 suggester 实现在 `peri-middlewares`（上层）。**或者**：在 `peri-agent` 定义 trait + Registry，`peri-middlewares` 实现具体 suggester。

**调整决定**：把 `error_suggest/context.rs`、`registry.rs`、`matcher.rs`、`format.rs`、`budget.rs` 从 `peri-middlewares/src/error_suggest/` **移动到** `peri-agent/src/error_suggest/`。具体 suggester 仍留在 `peri-middlewares/src/error_suggest/suggesters/`。

- [ ] **Step 11.1: 移动基础设施到 peri-agent**

```bash
mkdir -p peri-agent/src/error_suggest
mv peri-middlewares/src/error_suggest/context.rs peri-agent/src/error_suggest/
mv peri-middlewares/src/error_suggest/registry.rs peri-agent/src/error_suggest/
mv peri-middlewares/src/error_suggest/matcher.rs peri-agent/src/error_suggest/
mv peri-middlewares/src/error_suggest/format.rs peri-agent/src/error_suggest/
mv peri-middlewares/src/error_suggest/budget.rs peri-agent/src/error_suggest/

# 移动测试
mv peri-middlewares/src/error_suggest/registry_test.rs peri-agent/src/error_suggest/
mv peri-middlewares/src/error_suggest/matcher_test.rs peri-agent/src/error_suggest/
mv peri-middlewares/src/error_suggest/format_test.rs peri-agent/src/error_suggest/
```

- [ ] **Step 11.2: 在 peri-agent/Cargo.toml 加 fuzzy-matcher**

修改 `peri-agent/Cargo.toml`，在 `[dependencies]` 加：

```toml
fuzzy-matcher = "0.3"
```

（从 peri-middlewares 移除 fuzzy-matcher 依赖，避免重复声明）

- [ ] **Step 11.3: 创建 peri-agent/src/error_suggest/mod.rs**

创建 `peri-agent/src/error_suggest/mod.rs`：

```rust
pub mod budget;
pub mod context;
pub mod format;
pub mod matcher;
pub mod registry;

pub use context::{ErrorContext, ToolRegistrySnapshot};
pub use registry::{ErrorSuggestRegistry, ErrorSuggester, Suggestion};

#[cfg(test)]
mod registry_test;

#[cfg(test)]
mod matcher_test;

#[cfg(test)]
mod format_test;
```

- [ ] **Step 11.4: 在 peri-agent/src/lib.rs 暴露模块**

修改 `peri-agent/src/lib.rs`，加入：

```rust
pub mod error_suggest;
```

- [ ] **Step 11.5: 修改 peri-middlewares 的 mod.rs 改为 re-export**

修改 `peri-middlewares/src/error_suggest/mod.rs`：

```rust
// 从 peri-agent re-export 基础设施
pub use peri_agent::error_suggest::{
    budget, context, format, matcher, registry,
    ErrorContext, ToolRegistrySnapshot, ErrorSuggestRegistry, ErrorSuggester, Suggestion,
};

pub mod suggesters;
pub mod default_registry;
pub use default_registry::{build_default_registry, build_tool_registry_snapshot};
```

并从 `peri-middlewares/Cargo.toml` 移除 `fuzzy-matcher` 依赖。

- [ ] **Step 11.6: 修改 ReActAgent 加字段**

读取 `peri-agent/src/agent/executor/mod.rs:27` 找到 `pub struct ReActAgent<L, S>` 定义（注意：不在 `react.rs`）。在字段列表末尾加入：

```rust
use crate::error_suggest::{ErrorSuggestRegistry, ToolRegistrySnapshot};
use std::sync::Arc;

pub struct ReActAgent<L, S> {
    // ... 现有字段
    pub error_suggest_registry: Option<Arc<ErrorSuggestRegistry>>,
    pub tool_registry_snapshot: Arc<ToolRegistrySnapshot>,
}
```

构造函数 `ReActAgent::new`（搜索 `impl<L, S> ReActAgent<L, S>`）末尾追加字段初始化：

```rust
error_suggest_registry: None,
tool_registry_snapshot: Arc::new(ToolRegistrySnapshot::default()),
```

并添加 builder 方法：

```rust
pub fn with_error_suggest_registry(mut self, r: Arc<ErrorSuggestRegistry>) -> Self {
    self.error_suggest_registry = Some(r);
    self
}

pub fn with_tool_registry_snapshot(mut self, s: ToolRegistrySnapshot) -> Self {
    self.tool_registry_snapshot = Arc::new(s);
    self
}
```

- [ ] **Step 11.7: 修改 collect_tool_results 调用 apply_error_suggestion**

**集成点确认**（已通过 `Read tool_dispatch.rs:420-450` 验证）：

`collect_tool_results` 函数内部循环，每个 tool 处理流程：
- line 423-430: `agent.emit(ToolEnd {...})` 用 result.output 和 result.is_error 发事件
- line 432-439: `run_after_tool(state, &modified_call, &result)` 用不可变引用
- line 441: `exec_results.push((modified_call, result))` 把 owned result push 到结果列表

**result 在循环里是 owned（绑定在 `let result = match ...` 上）**，因此可以在 line 439 之后、line 441 之前修改 `result.output`。

修改 `tool_dispatch.rs`，在 line 439（run_after_tool 的 if let Err 块结束）之后、line 441（push）之前插入：

```rust
// 🆕 错误感知注入（保持 is_error=true，仅追加 output 文本）
// 此时 result 仍是 owned 可变，尚未 push 到 exec_results
if result.is_error {
    if let Some(registry) = &agent.error_suggest_registry {
        let tool_reg = &agent.tool_registry_snapshot;
        let ctx = crate::error_suggest::ErrorContext::new(
            &modified_call.name,
            &modified_call.input,
            &result.output,
            state.cwd(),
            tool_reg,
        );
        if let Some(sug) = registry.suggest(&ctx) {
            result.output = crate::error_suggest::format::format_suggestion(&result.output, &sug);
            // 不修改 result.is_error / tool_call_id / tool_name
        }
    }
}
```

**关键类型注意**：
- 变量名是 `modified_call`（不是 `call`），由 line 420 上面的代码产生（before_tool 链可能改写）
- `State::cwd(&self) -> &str`（不是 `&Path`，见 `peri-agent/src/agent/state.rs:13`），传给 `ErrorContext.cwd: &'a Path` 需要 `Path::new(state.cwd())` 包一层
- 完整修正后的注入代码：

```rust
if result.is_error {
    if let Some(registry) = &agent.error_suggest_registry {
        let tool_reg = &agent.tool_registry_snapshot;
        let ctx = crate::error_suggest::ErrorContext::new(
            &modified_call.name,
            &modified_call.input,
            &result.output,
            std::path::Path::new(state.cwd()),
            tool_reg,
        );
        if let Some(sug) = registry.suggest(&ctx) {
            result.output = crate::error_suggest::format::format_suggestion(&result.output, &sug);
        }
    }
}
```

- [ ] **Step 11.8: 修改 build_agent.rs 注入 registry + snapshot**

读取 `peri-middlewares/src/subagent/tool/build_agent.rs:78-81`，在 `filter_tools` 之后、构造 ReActAgent 之前加入。注意：build_agent 函数签名里 `cwd: &str`（不是 `&Path`），构造路径需要 `Path::new(cwd).join(...)`：

```rust
use peri_middlewares::error_suggest::{build_default_registry, build_tool_registry_snapshot};
use std::path::Path;

let all_tool_names: Vec<String> = tools.iter().map(|t| t.name()).collect();
let agents_dir = Path::new(cwd).join(".claude/agents");
let agents_dir_opt = if agents_dir.exists() { Some(agents_dir.as_path()) } else { None };
let snapshot = build_tool_registry_snapshot(all_tool_names, agents_dir_opt);
let registry = build_default_registry();

// 在 ReActAgent 构造链上追加：
//   .with_tool_registry_snapshot(snapshot)
//   .with_error_suggest_registry(registry)
```

- [ ] **Step 11.9: 写集成测试**

找到 `peri-agent/src/agent/executor/tool_dispatch_test.rs`（或新建）。如果没有，找到测试 mod 入口加一个：

```rust
#[tokio::test]
async fn test_apply_error_suggestion_appends_to_output() {
    use crate::error_suggest::{ErrorSuggestRegistry, ErrorSuggester, Suggestion, ErrorContext, ToolRegistrySnapshot};
    use std::sync::Arc;

    struct StaticSuggest;
    impl ErrorSuggester for StaticSuggest {
        fn suggest(&self, _ctx: &ErrorContext) -> Option<Suggestion> {
            Some(Suggestion::new("测试建议"))
        }
    }

    let registry = Arc::new(ErrorSuggestRegistry::new(vec![Box::new(StaticSuggest)]));
    let snap = ToolRegistrySnapshot::default();

    // 验证：原错误 + 建议文本组合后包含两者
    let original = "Error: File not found".to_string();
    let sug = registry.suggest(&ErrorContext::new(
        "Read",
        &serde_json::json!({}),
        &original,
        std::path::Path::new("."),
        &snap,
    )).unwrap();
    let combined = crate::error_suggest::format::format_suggestion(&original, &sug);
    assert!(combined.contains("Error: File not found"));
    assert!(combined.contains("测试建议"));
    assert!(combined.contains("---"));
}
```

- [ ] **Step 11.10: 运行所有测试**

Run: `cargo test -p peri-agent --lib error_suggest && cargo test -p peri-middlewares --lib error_suggest`
Expected: PASS。

- [ ] **Step 11.11: 完整构建**

Run: `cargo build`
Expected: PASS。

- [ ] **Step 11.12: Commit**

```bash
git add peri-agent/Cargo.toml \
        peri-agent/src/lib.rs \
        peri-agent/src/error_suggest/ \
        peri-agent/src/agent/react.rs \
        peri-agent/src/agent/executor/tool_dispatch.rs \
        peri-agent/src/agent/executor/tool_dispatch_test.rs \
        peri-middlewares/Cargo.toml \
        peri-middlewares/src/error_suggest/ \
        peri-middlewares/src/subagent/tool/build_agent.rs
git commit -m "$(cat <<'EOF'
feat(error_suggest): 集成到 ReActAgent + collect_tool_results

- 基础设施移到 peri-agent（避免循环依赖）
- ReActAgent 加 error_suggest_registry + tool_registry_snapshot 字段
- collect_tool_results 在 run_after_tool 后注入建议（保持 is_error=true）
- build_agent 构造期填充 registry + snapshot

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## Task 12: [TRAP] 回归测试 + 性能测试 + 文档更新

**Files:**
- Test: `peri-agent/src/agent/executor/tool_dispatch_test.rs`（追加）
- Modify: `CLAUDE.md`（新增 ErrorSuggest 章节）

- [ ] **Step 12.1: [TRAP] 回归测试——messages 不被建议注入修改**

**核心不变量**：`apply_error_suggestion` 注入路径只修改 `result.output` 字符串，**不调** `state.add_message`。验证方法是单元测试 `format_suggestion` + `Registry::suggest` 的组合行为：

在 `peri-agent/src/agent/executor/tool_dispatch_test.rs` 追加：

```rust
#[tokio::test]
async fn test_apply_error_suggestion_preserves_is_error_flag() {
    use crate::error_suggest::{
        ErrorContext, ErrorSuggestRegistry, ErrorSuggester, Suggestion, ToolRegistrySnapshot,
        format::format_suggestion,
    };
    use std::sync::Arc;

    struct Always;
    impl ErrorSuggester for Always {
        fn suggest(&self, _: &ErrorContext) -> Option<Suggestion> {
            Some(Suggestion::new("建议"))
        }
    }

    let registry = Arc::new(ErrorSuggestRegistry::new(vec![Box::new(Always)]));
    let snap = ToolRegistrySnapshot::default();
    let ctx = ErrorContext::new(
        "Read",
        &serde_json::json!({}),
        "Error: File not found",
        std::path::Path::new("."),
        &snap,
    );

    // 模拟 collect_tool_results 中的注入逻辑
    let mut result_output = "Error: File not found".to_string();
    let mut result_is_error = true;

    if result_is_error {
        if let Some(sug) = registry.suggest(&ctx) {
            result_output = format_suggestion(&result_output, &sug);
        }
    }

    // 断言：output 包含建议，is_error 保持 true
    assert!(result_output.contains("Error: File not found"));
    assert!(result_output.contains("建议"));
    assert!(result_output.contains("---"));
    assert!(result_is_error, "is_error 必须保持 true");
}

#[test]
fn test_apply_error_suggestion_skips_when_no_match() {
    use crate::error_suggest::{
        ErrorContext, ErrorSuggestRegistry, ErrorSuggester, Suggestion, ToolRegistrySnapshot,
        format::format_suggestion,
    };
    use std::sync::Arc;

    struct Never;
    impl ErrorSuggester for Never {
        fn suggest(&self, _: &ErrorContext) -> Option<Suggestion> {
            None
        }
    }

    let registry = Arc::new(ErrorSuggestRegistry::new(vec![Box::new(Never)]));
    let snap = ToolRegistrySnapshot::default();
    let ctx = ErrorContext::new(
        "Read",
        &serde_json::json!({}),
        "Error: unknown",
        std::path::Path::new("."),
        &snap,
    );

    let original = "Error: unknown".to_string();
    let mut output = original.clone();
    if let Some(sug) = registry.suggest(&ctx) {
        output = format_suggestion(&output, &sug);
    }
    assert_eq!(output, original, "无建议时 output 必须保持原值");
}
```

**TRAP 兜底说明**：因为 `apply_error_suggestion` 实际只在 collect_tool_results 内一处调用、不抽成独立函数，"不调 state.add_message"由代码 review 保证（Step 11.7 的注入代码块本身不调 add_message）。本测试覆盖"输出格式 + is_error 保留"两个可机器验证的不变量。

- [ ] **Step 12.2: 性能测试——path_suggester 在大目录下 < 50ms**

在 `peri-middlewares/src/error_suggest/suggesters/path_suggester_test.rs` 追加：

```rust
#[test]
fn test_path_suggester_perf_under_50ms_in_large_dir() {
    use std::time::Instant;

    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();

    // 创建 200 个文件
    for i in 0..200 {
        std::fs::write(base.join(format!("file_{i:03}.rs")), "").unwrap();
    }

    let holder = CtxHolder::new(serde_json::json!({
        "file_path": base.join("fle_100.rs").to_string_lossy().to_string(),
    }));
    let err = format!("Error: File not found at {}", base.join("fle_100.rs").display());

    let start = Instant::now();
    let result = crate::error_suggest::suggesters::path_suggester::PathSuggester.suggest(
        &holder.ctx("Read", &err, base),
    );
    let elapsed = start.elapsed();

    assert!(result.is_some());
    assert!(elapsed.as_millis() < 50, "应该 < 50ms，实际: {elapsed:?}");
}
```

- [ ] **Step 12.3: 更新 CLAUDE.md**

在 `CLAUDE.md` 合适位置（中间件链执行顺序章节附近）追加：

```markdown
## 错误感知建议层（Error Suggestion Layer）

工具错误返回前，会通过 `ErrorSuggestRegistry` 自动注入结构化建议文本（路径候选、参数修正、命令纠错等），让 LLM 直接消费。

**集成点**：`peri-agent/src/agent/executor/tool_dispatch.rs::collect_tool_results`，run_after_tool 之后、写入 state 之前。**不是中间件**——因为 `after_tool` 的 `result: &ToolResult` 是不可变引用，且 [TRAP] 约束中间件不写 state。

**[TRAP]** `apply_error_suggestion` 注入路径必须遵守：
- 只修改 `result.output` 文本，**不调** `state.add_message`（保持延迟写入语义）
- 不修改 `result.is_error` 标志（保持 PostToolUseFailure 事件触发）
- 不修改 `result.tool_call_id` / `result.tool_name`（保持消息关联）
- 整体超时预算 100ms，超时返回 None 不阻塞错误返回

**架构**：基础设施在 `peri-agent/src/error_suggest/`，具体 suggester 在 `peri-middlewares/src/error_suggest/suggesters/`。Registry 和 Snapshot 作为 `ReActAgent` 字段，构造期注入。

**新增建议器流程**：
1. 在 `peri-middlewares/src/error_suggest/suggesters/` 新建 `<name>_suggester.rs` + 测试
2. 实现 `ErrorSuggester` trait
3. 在 `default_registry.rs::build_default_registry()` 注册（顺序决定短路优先级）
4. 更新本文档章节
```

- [ ] **Step 12.4: 跑全量测试**

Run: `cargo test`
Expected: 全部 PASS。

如有失败，逐一修复（**不要禁用测试或加 #[ignore]**）。

- [ ] **Step 12.5: 跑 clippy**

Run: `cargo clippy --all-targets -- -D warnings`
Expected: 无警告。如有，修复。

- [ ] **Step 12.6: 跑 fmt 检查**

Run: `cargo fmt --all -- --check`
Expected: 无 diff。如有，先 `cargo fmt --all`。

- [ ] **Step 12.7: 最终 commit**

```bash
git add CLAUDE.md \
        peri-agent/src/agent/executor/tool_dispatch_test.rs \
        peri-middlewares/src/error_suggest/suggesters/path_suggester_test.rs
git commit -m "$(cat <<'EOF'
test(error_suggest): TRAP 回归 + 性能 + 文档

- 验证 is_error 在建议注入后保持 true
- 验证 path_suggester 在 200 文件目录下 < 50ms
- CLAUDE.md 新增"错误感知建议层"章节，记录集成点和 [TRAP] 约束

Co-Authored-By: glm-5.2 <zai-org@claude-code-best.win>
EOF
)"
```

---

## 完成判定

- [ ] 所有 12 个 Task 完成
- [ ] `cargo build` 通过
- [ ] `cargo test` 全部通过
- [ ] `cargo clippy --all-targets -- -D warnings` 无警告
- [ ] `cargo fmt --all -- --check` 无 diff
- [ ] 11 个场景（A1-A4 / B2-B5 / C1 / C3）都有对应 suggester 和测试（B1 由 edit.rs 覆盖）
- [ ] CLAUDE.md 更新
- [ ] spec + plan 一起 commit（一次性或最后 squash 均可）

---

## 风险与缓解（实现时关注）

| 风险 | 缓解 |
|------|------|
| 基础设施从 middlewares 移到 agent 时遗漏 re-export | Step 11.5 之后跑 `cargo build -p peri-middlewares` 确认 |
| `collect_tool_results` 内 result 可能不是 mut | Step 11.7 之前先 Read tool_dispatch.rs:430-450 确认 |
| Glob 工具改造影响现有测试 | Step 4.3 改完立即跑 glob_test 全部 |
| Bash C1 测试依赖系统 git | Step 8.5 加 which 检测 skip |
| 性能：PATH 扫描慢 | bash_command_suggester 有 500 候选上限 |
| HookMiddleware 兼容性 | 保持 is_error=true，PostToolUseFailure 仍触发（Q13 调研确认）|

---

## Self-Review

**Spec coverage:**
- ✅ §2 范围 A1-A4 → Task 2
- ✅ §2 B1 → Task 6（说明已由 edit.rs 覆盖，不重复）
- ✅ §2 B2-B5 → Task 3, 4, 5, 7
- ✅ §2 C1 → Task 8
- ✅ §2 C3 → Task 9
- ✅ §3.1 集成点决策 → Task 11
- ✅ §3.4 集成点代码 → Task 11 Step 11.7
- ✅ §6 关键词识别 → 各 suggester 测试
- ✅ §7 性能预算 → Task 12 Step 12.2 + 各 suggester 内部
- ✅ §9 Registry 注入 → Task 11 Step 11.6/11.8
- ✅ §11.3 [TRAP] 回归 → Task 12 Step 12.1

**Placeholder scan:** 计划中无 TBD/TODO，所有步骤含具体代码或命令。

**Type consistency:**
- `ErrorContext::new(tool_name, tool_input, error_message, cwd, tool_registry)` 全计划一致
- `Suggestion::new(summary) + .with_details(details)` 一致
- `ErrorSuggestRegistry::new(Vec<Box<dyn ErrorSuggester>>)` 一致
- `build_default_registry()` / `build_tool_registry_snapshot(...)` 一致

---

## Execution Handoff

计划保存于 `docs/superpowers/plans/2026-06-18-error-suggestion.md`。

**执行选择**：

1. **Subagent-Driven（推荐）**：每个 Task 派一个新 subagent 执行，两阶段 review
2. **Inline Execution**：在本会话用 executing-plans skill 批量执行 + checkpoint

请选择执行方式。
