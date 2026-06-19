# 错误感知建议层（Error Suggestion Layer）

**日期**：2026-06-18
**状态**：Approved
**作者**：KonghaYao
**相关**：`2026-06-03-edit-error-visibility-and-diagnostics-design.md`（B1 先例）

---

## 1. 问题与动机

当前 Agent 框架中，工具错误是"哑终端"：返回一段纯文本错误消息，LLM 看到后只能猜测下一步——通常是再调用一次 Glob 或 LS 探索文件结构，浪费一轮工具调用与上百 token。

典型案例：LLM 调用 `Read("src/peri_agnt/main.rs")`，得到 `"Error: File not found at src/peri_agnt/main.rs"`。LLM 不知道项目里其实有 `src/peri-agent/main.rs`（一个连字符差异），可能要 Glob 一两次才找到正确路径。

**核心洞察**：错误本身常常携带足够信号让框架推出"正确候选"。如果框架在错误消息里直接附加 3 个模糊匹配候选，LLM 就能在一轮内纠正，省去探索步骤。

**目标**：建一个统一的错误感知建议层，覆盖路径不存在、参数语法错、外部资源名拼错等 11 类高频错误场景，在错误返回前注入结构化建议文本，让 LLM 直接消费。

**非目标**：
- 不做自动重试（建议由 LLM 决定是否使用，保持 Agent 自治）
- 不弹窗给用户（V1 纯 LLM 消费，不触碰 TUI/HITL）
- 不改 ToolResult 数据结构（保持 `is_error + output: String` 形态）
- 不做 i18n 错误返回（V1 用关键词匹配，V2 改结构化错误）

---

## 2. V1 范围（11 个场景）

| # | 场景 | 触发工具 | 建议来源 |
|---|------|---------|---------|
| A1 | 文件不存在 | Read / Edit | 同目录 fuzzy + 全局 glob fallback |
| A2 | 目录不存在 | Glob / Read(dir) | 父级目录 fuzzy |
| A3 | 父目录不存在 | Write / CreateDir | 同级目录候选 |
| A4 | 路径穿越被拒 | 所有 fs 工具 | 提示 base_dir 内等价路径 |
| B1 | old_string 未找到 | Edit | 行级 fuzzy（**已由 `edit.rs::build_not_found_hint` 实现，本期不重复**） |
| B2 | offset/limit 越界 | Read | 提示实际行数范围 |
| B3 | glob pattern 语法错 | Glob | 指出非法字符 + 合法示例 |
| B4 | regex 语法错 | Grep | 错误位置 + 修正示例 |
| B5 | JSON 参数结构错 | 任意工具 | 字段缺失/类型错误提示 |
| C1 | 命令不存在 | Bash | `which -a` + 历史 + 常见纠错 |
| C3 | subagent_type 不存在 | Agent | 已注册 agent 类型 fuzzy |

**明确排除**（E 类不可恢复）：HITL 拒绝、权限不足、Provider 鉴权失败、Cancel、磁盘满。
**V2 候选**：C2（URL 域名）、C4（skill 名）、C5（MCP tool 名）、C6（provider 名）、D 系列（id 类不存在）。

---

## 3. 架构

### 3.1 不是中间件，而是注册表 + 集成点

**决策**：不新增 `ErrorSuggestMiddleware`，而是在 `tool_dispatch.rs::collect_tool_results` 内集成。

**理由**（基于代码侦察发现）：
- `Middleware::after_tool` 的 `result: &ToolResult` 是**不可变引用**，无法原地修改 result.output
- [TRAP] 约束：18 个中间件的 after_tool 不写 `state.messages()`——若新增中间件要写 state，会破坏链上不变量
- `collect_tool_results` 是工具错误的天然卡口：run_after_tool 之后、写入 state 之前，此时 result 仍可变
- 集中注入避免 11 个工具各自改错误返回代码

**对比方案（已否决）**：
- ❌ 工具内嵌：遗漏风险高，错误识别分散
- ❌ BaseTool trait 扩展：破坏 trait 签名，所有工具重编
- ❌ 新增中间件：after_tool 不可变引用 + [TRAP] 双重障碍

### 3.2 模块结构

```
peri-middlewares/src/error_suggest/
├── mod.rs                          # 公开 API + 集成入口 build_default_registry()
├── context.rs                      # ErrorContext / ToolRegistrySnapshot
├── registry.rs                     # ErrorSuggester trait + Registry
├── matcher.rs                      # SkimMatcherV2 泛化包装（复用 at-mention 算法）
├── format.rs                       # 建议格式化
├── budget.rs                       # 超时/数量预算工具
└── suggesters/
    ├── path_suggester.rs           # A1-A4
    ├── edit_content_suggester.rs   # B1（迁移 build_not_found_hint）
    ├── range_suggester.rs          # B2
    ├── glob_pattern_suggester.rs   # B3
    ├── regex_suggester.rs          # B4
    ├── json_schema_suggester.rs    # B5
    ├── bash_command_suggester.rs   # C1
    └── subagent_suggester.rs       # C3
```

### 3.3 核心数据结构

```rust
// context.rs
pub struct ErrorContext<'a> {
    pub tool_name: &'a str,
    pub tool_input: &'a serde_json::Value,
    pub error_message: &'a str,
    pub cwd: &'a Path,
    pub tool_registry: &'a ToolRegistrySnapshot,
}

pub struct ToolRegistrySnapshot {
    pub all_tool_names: HashSet<String>,
    pub subagent_types: HashSet<String>,
    // V2 追加：skill_names, mcp_tool_names
}

// registry.rs
pub trait ErrorSuggester: Send + Sync {
    /// 返回 None 表示"本建议器不处理这种错误"
    fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion>;
}

pub struct Suggestion {
    pub summary: String,           // 一行总结，例如"建议改用以下路径之一：A, B, C"
    pub details: Option<String>,   // 可选详细信息（行号、错误位置、修正示例）
}

pub struct ErrorSuggestRegistry {
    suggesters: Vec<Box<dyn ErrorSuggester>>,
}

impl ErrorSuggestRegistry {
    /// 第一个返回 Some 的 suggester 胜出（短路）
    pub fn suggest(&self, ctx: &ErrorContext) -> Option<Suggestion> {
        for s in &self.suggesters {
            if let Some(sug) = s.suggest(ctx) { return Some(sug); }
        }
        None
    }
}
```

**短路语义**：注册顺序决定优先级。如果 path_suggester 命中，不会调用 edit_content_suggester。V1 注册顺序：

1. `glob_pattern_suggester` / `regex_suggester` / `json_schema_suggester`（参数语法类，最廉价、最确定）
2. `edit_content_suggester`（B1，已有逻辑）
3. `range_suggester`（B2，纯字段读）
4. `path_suggester`（A1-A4，需要 IO）
5. `bash_command_suggester`（C1，需要 IO）
6. `subagent_suggester`（C3，registry 查询）

### 3.4 集成点

**位置**：`peri-middlewares/src/tool_dispatch.rs::collect_tool_results`，run_after_tool 之后、写入 state 之前。

```rust
// 伪代码
let mut result = match tool_result {
    Ok(output) => ToolResult::success(&call.id, &call.name, output),
    Err(ref e) => ToolResult::error(&call.id, &call.name, e.to_string()),
};

// run_after_tool 链（已有逻辑，不动）
run_after_tool(&chain, state, &call, &result).await?;

// 🆕 错误感知注入（新增）
// agent.error_suggest_registry: Option<Arc<ErrorSuggestRegistry>>（ReActAgent 字段，build_agent 时注入）
// agent.tool_registry_snapshot: Arc<ToolRegistrySnapshot>（构造期从 collect_tools 结果 + .claude/agents/ 构建）
if result.is_error {
    if let Some(registry) = &agent.error_suggest_registry {
        let tool_reg = &agent.tool_registry_snapshot;
        let ctx = ErrorContext::new(&call.name, &call.input, &result.output, state.cwd(), tool_reg);
        if let Some(sug) = registry.suggest(&ctx) {
            result.output = format_suggestion(&result.output, &sug);
            // is_error 保持 true，不改变语义
        }
    }
}

// 之后 dispatch_tools 把 result 写入 state（原有逻辑不变）
```

**[TRAP] 合规性**：
- 不在 collect_tool_results 中调 `state.add_message`（保持延迟写入语义）
- 不改变 `result.is_error`（避免影响下游 PostToolUseFailure 事件）
- 不修改 `result.id` / `result.name`（避免影响 message 关联）
- 只追加 `result.output` 文本，下游 BaseMessage::tool_error 收到的就是增强后文本

---

## 4. 关键设计决策

| 决策 | 选择 | 理由 |
|------|------|------|
| 集成点 | `tool_dispatch.rs`（非中间件） | after_tool 不可变引用 + [TRAP] 双重障碍 |
| 错误识别 | V1 用关键词匹配字符串 | 工具错误目前是自由文本；V2 改结构化 `ToolError` 枚举 |
| 调用对象 | LLM 直接消费 | 不碰 TUI/HITL，纯工具层增强 |
| 短路 vs 聚合 | 短路（第一个命中即返回） | 避免 11 个建议器都跑一遍浪费预算；多个建议同时命中概率低 |
| 交付节奏 | 一个大 PR | 用户偏好完整交付，审查集中 |
| 建议格式 | 中文自然语言，无 emoji，无 `[hint]` 前缀 | 与项目编码规范、已有 Edit hint 风格一致 |
| Registry 注入 | 构造期 `with_*` + state.context 缓存 | 避免每轮重建；工具名集合通过 collect_tools 同步 |

---

## 5. 数据流

```
工具 execute()
    ↓ Err("Error: File not found at src/peri_agnt/main.rs")
ToolResult::error() 包装（is_error=true, output=原错误文本）
    ↓
run_after_tool 链（GitAttribution/Hook/Lsp 等，不动 result）
    ↓
🆕 apply_error_suggestion(&mut result, state, registry)
    ├─ 关键词识别 "not found"
    ├─ 从 input.file_path 提取目标
    ├─ fuzzy_matcher 扫描同目录 → 3 个候选
    └─ output += "\n\n建议改用以下路径之一：\n  • src/peri-agent/main.rs\n  ..."
    ↓
dispatch_tools 写入 state（BaseMessage::tool_error，已带建议文本）
    ↓
StateSnapshot + 流式事件 → TUI 显示（无需 TUI 改造）
    ↓
下一轮 LLM 调用，看到 tool_result 中的建议文本，自主决策重试
```

---

## 6. 错误识别策略（V1 关键词匹配）

每个 suggester 用 `tool_name 白名单 + 关键词子串匹配` 判断是否处理。

**关键词表**（基于代码侦察的实际错误文本）：

| Suggester | tool_name 白名单 | 关键词 / 识别信号 |
|-----------|-----------------|--------|
| path_suggester | Read / Edit / Write / Glob / CreateDir / Move / Delete | "not found", "no such file", "does not exist", "not a directory", "Search path does not exist" |
| edit_content_suggester | Edit | "old_string not found", "is not unique" |
| range_suggester | Read | "offset", "exceeds file length" |
| glob_pattern_suggester | Glob | ⚠️ 现状无错误文本（`glob_match` 静默返回 false）。需**先改 Glob 工具**让 `Pattern::new` 失败时返回错误，suggester 才能识别 |
| regex_suggester | Grep | "Error: "（grep 错误统一前缀），需进一步用 `regex::Error` 关键词识别 |
| json_schema_suggester | 任意 | "missing field", "expected", "invalid type", "parameter is required" |
| bash_command_suggester | Bash | stderr 含 "command not found" + 输出含 `[Exit code: 127]` |
| subagent_suggester | Agent | "cannot find agent definition", "please provide subagent_type" |

**脆弱性承认**：关键词匹配依赖错误消息稳定性。V2 改造方向是把工具错误从 `String` 升级为结构化：

```rust
// V2 设想（不在本期）
pub enum ToolError {
    NotFound { kind: NotFoundKind, target: String },
    InvalidSyntax { lang: SyntaxLang, position: usize, message: String },
    SchemaViolation { field: String, expected: String, got: String },
    Unknown { raw: String },
}
```

此时 suggester 不再匹配字符串，直接 pattern match on `ToolError`。

---

## 7. 性能预算

**总预算：100ms**。超时降级为"不附建议"（返回 None，原错误正常返回）。

| Suggester | 候选源 | 上限 | 子预算 |
|-----------|--------|------|------|
| path | 同目录 + 1 层子目录 | 200 entry | 50ms |
| edit_content | 同文件 fuzzy（受 `old_string.len() > 5000` 跳过保护） | 文件本身 | 20ms |
| range | 读 metadata | - | 1ms |
| glob_pattern | 纯解析 | - | 1ms |
| regex | 纯解析 | - | 1ms |
| json_schema | 工具 schema 查表 | - | 5ms |
| bash_command | `which -a` + history | 50 条 | 30ms |
| subagent | registry set | < 100 | 5ms |

**实现约束**：
- 所有 IO（path 扫描、which 调用）用 `tokio::time::timeout` 包装
- fuzzy_matcher 在候选 > 200 时先采样
- 短路设计天然保证不会跑空所有 suggester

**超时处理**：单个 suggester 超时返回 `None`，不阻塞后续 suggester，也不阻塞错误本身返回。

---

## 8. 建议输出格式

**风格**：中文自然语言，不用 emoji，不用 `[hint]` 前缀。与 `2026-06-03-edit-error-visibility-and-diagnostics-design.md` 中 `build_not_found_hint` 的输出风格一致。

**格式化函数**：

```rust
// format.rs
pub fn format_suggestion(original_error: &str, sug: &Suggestion) -> String {
    let mut out = format!("{}\n\n---\n{}", original_error, sug.summary);
    if let Some(d) = &sug.details {
        out.push_str("\n");
        out.push_str(d);
    }
    out.push_str("\n---");
    out
}
```

**示例输出**（A1 场景）：

```
Error: File not found at src/peri_agnt/main.rs

---
建议改用以下路径之一：
  • src/peri-agent/main.rs
  • src/peri-agent/agent.rs
  • src/peri-tui/main.rs
---
```

**示例输出**（C3 场景）：

```
Error: Unknown subagent_type: explore

---
建议改用以下 subagent_type 之一：Explore, General, Plan。
---
```

**字符预算**：建议文本 ≤ 500 字符。超出由 suggester 内部截断或减少候选数量。

---

## 9. Registries 可达性方案

**问题**：Skills / MCP / SubAgent 工具名在中间件层不可达，它们通过 `collect_tools()` 直接合并到 ReActAgent 的 `all_tools`。`state.context()` 只支持 `key: &str → Option<&str>` 字符串键值对，不支持泛型 `get::<T>`，无法存复杂结构。

**方案**：把 Registry 和工具名快照作为 **ReActAgent 字段**，构造期注入。`collect_tool_results` 已经接收 `agent: &ReActAgent<L, S>` 作为参数，可直接访问。

```rust
pub struct ReActAgent<L, S> {
    // ... 现有字段
    pub error_suggest_registry: Option<Arc<ErrorSuggestRegistry>>,
    pub tool_registry_snapshot: Arc<ToolRegistrySnapshot>,
}

pub struct ToolRegistrySnapshot {
    pub all_tool_names: HashSet<String>,      // collect_tools 后提取
    pub subagent_types: HashSet<String>,      // 内置 + .claude/agents/ 扫描
    // V2 扩展：skill_names, mcp_tool_names
}
```

**填充时机**：`build_agent`（`peri-middlewares/src/subagent/tool/build_agent.rs:78`）调用 `collect_tools` 后：
1. 提取所有 `tool.name()` 填入 `all_tool_names`
2. 扫描 `.claude/agents/*.md` + 内置列表（`built_in_agents.rs:29-40`）填入 `subagent_types`
3. 构造 `error_suggest_registry = Some(Arc::new(ErrorSuggestRegistry::default()))`

**为什么不用 state.context**：
- API 只支持 `&str → &str`，复杂结构需自行 serde_json 序列化（反序列化成本 + 类型安全丢失）
- ReActAgent 是天然的字段持有位置，collect_tool_results 已有 agent 引用，零额外 plumbing
- AgentState 的 context 偏向于存储跨中间件共享的"环境变量"（session_id、run_id），不是结构化对象

---

## 10. fuzzy_matcher 复用

**现状**：`peri-tui/src/app/at_mention/file_search.rs` 已用 `SkimMatcherV2`，但函数签名是 `fuzzy_match_entries(entries: &[Entry], query: &str)`，`Entry` 是 TUI 层类型，不能直接跨 crate 复用。`fuzzy-matcher = "0.3"` 当前只在 `peri-tui/Cargo.toml:57` 声明，**peri-middlewares 需新增依赖**。

**方案**：在 `peri-middlewares/Cargo.toml` 的 `[dependencies]` 添加 `fuzzy-matcher = "0.3"`，然后在 `peri-middlewares/src/error_suggest/matcher.rs` 写泛化版本：

```rust
// matcher.rs
use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

/// 通用 fuzzy：候选 + 查询，返回 top-N 候选（按 score 降序）
pub fn fuzzy_top_n<'a>(candidates: &'a [String], query: &str, n: usize) -> Vec<&'a String> {
    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(&String, i64)> = candidates.iter()
        .filter_map(|c| matcher.fuzzy_match(c, query).map(|s| (c, s)))
        .collect();
    scored.sort_by(|a, b| b.1.cmp(&a.1));
    scored.iter().take(n).map(|(c, _)| *c).collect()
}
```

**未来 TUI 复用**：V2 可以让 at-mention 也调用这个泛化版本，移除重复代码。本期不强制。

---

## 11. 测试策略

### 11.1 单元测试（每个 suggester 独立）

每个 suggester 文件配套 `_test.rs`，构造 mock `ErrorContext`，断言：
- 命中场景返回 `Some(Suggestion)`，内容正确
- 不命中场景返回 `None`（tool_name 不对、关键词不匹配、候选为空）
- 边界：超时、候选超量、空 input

测试命名遵循项目规范：`test_<被测对象>_<场景>`，注释中文，Arrange-Act-Assert。

### 11.2 集成测试（`tool_dispatch`）

新增 `tool_dispatch_test.rs` 测试用例：
- `test_error_suggest_path_not_found_injects_candidates`：mock 工具返回 NotFound，断言写入 state 的 ToolResult.output 包含"建议改用以下路径"
- `test_error_suggest_no_match_keeps_original`：mock 工具返回未知错误，断言 result.output 不变
- `test_error_suggest_timeout_returns_none`：mock 超时场景，断言不阻塞错误返回

### 11.3 [TRAP] 回归测试

- `test_error_suggest_does_not_mutate_messages`：在 collect_tool_results 调用前后用 mock state 断言 `state.messages().len()` 未变（仅 result.output 被改写，不调 add_message）
- `test_error_suggest_preserves_is_error_flag`：断言 is_error 在注入前后保持 true
- `test_error_suggest_preserves_deferred_error_semantics`：构造多工具并发错误场景，断言 P3/P4 错误仍然走 deferred_error 收集路径

### 11.4 性能测试

- `test_path_suggester_p99_under_50ms`：在 1000 文件目录下跑 100 次 fuzzy，p99 < 50ms
- `test_registry_total_budget_under_100ms`：构造复杂错误场景，断言总耗时 < 100ms

---

## 12. 交付

**一个 PR**，覆盖 11 个场景 + 基础设施 + 集成 + 测试。

PR 内部 commit 建议（便于 review，但仍合为一个 PR）：

1. `feat(error_suggest): registry + trait + ErrorContext + matcher + format`
2. `feat(error_suggest): path_suggester (A1-A4) + tests`
3. `feat(error_suggest): range + glob_pattern + regex suggesters (B2-B4)`
4. `refactor(error_suggest): migrate build_not_found_hint to edit_content_suggester (B1)`
5. `feat(error_suggest): json_schema_suggester (B5)`
6. `feat(error_suggest): bash_command_suggester (C1)`
7. `feat(error_suggest): subagent_suggester + registry snapshot sync (C3)`
8. `feat(tool_dispatch): integrate error suggestion into collect_tool_results`
9. `test(error_suggest): integration + TRAP regression + perf`

---

## 13. 风险与缓解

| 风险 | 缓解 |
|------|------|
| 关键词匹配脆弱，工具错误改 wording 就失效 | V1 接受；V2 改结构化 ToolError；监控触发率 |
| 集成点改 tool_dispatch.rs 影响 18 个中间件链 | 只在 run_after_tool 之后追加，不改原有调用顺序；TRAP 回归测试覆盖 |
| 性能：1000+ 文件目录 fuzzy 慢 | 子预算 50ms + 候选采样 200；超时降级 |
| B5 JSON schema 各工具自定义，覆盖不全 | V1 只覆盖有明确 schema 校验的工具；其他工具 json_schema_suggester 返回 None |
| C1 bash 命令建议触发副作用（比如 PATH 修改后 which 慢） | 30ms 超时；which 失败不抛错；缓存 PATH |
| 建议文本过长污染 context | 每条建议 ≤ 500 字符；候选数量 ≤ 3 |
| LLM 不接受建议仍然重试 Glob | 接受——这是 LLM 自主权；监控建议采纳率作为指标 |

---

## 14. V2 与未来扩展

- **结构化 ToolError**：升级错误类型，suggester 不再匹配字符串
- **更多场景**：C2（URL 域名）/ C4（skill 名）/ C5（MCP tool 名）/ C6（provider 名）/ D 系列
- **TUI 弹窗消费**：V2 增加用户介入路径，候选弹窗选择后系统代替 LLM 重试
- **建议采纳率监控**：Langfuse 上报"建议是否被 LLM 在下一轮采纳"，用于评估效果
- **fuzzy 算法统一**：让 at-mention 和 error_suggest 共用 `matcher.rs`，移除 TUI 层重复

---

## 15. 相关陷阱引用

实现时必须遵守的不变量（详见 CLAUDE.md）：

- **[TRAP] `tool_dispatch.rs` 延迟写入**：错误感知注入不得调 `state.add_message`，dispatch_tools 最后统一写入
- **[TRAP] P3/P4 错误路径 deferred_error**：错误感知不得破坏多工具并发错误收集；所有 tool_result 必须写入 state
- **[TRAP] `prepend_message` 不变量**：错误感知只用 `result.output` 追加文本，不涉及消息插入
- **[TRAP] 系统提示词稳定性**：错误感知建议文本进入 ToolResult 而非 System 消息，不会污染 frozen system prompt
