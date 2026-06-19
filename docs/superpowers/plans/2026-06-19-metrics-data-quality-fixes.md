# 计划：修复 Metrics 数据质量缺陷（5 个字段恒为默认值）

**关联 Issue**：`spec/issues/2026-06-19-metrics-data-quality-defects.md`
**创建日期**：2026-06-19
**状态**：Draft

## 分析结论

5 个字段中，**2 个是代码 bug**，**3 个是分析脚本字段名错误**：

| # | 字段 | 性质 | 根因 |
|---|------|------|------|
| 1 | `duration_secs` | **代码 bug** | `final_answer.rs:174` 硬编码 `0u64`，TODO 未实现 |
| 2 | `rate`（threshold.memory） | **代码缺失** | emit 数据只有 `rss_mb, level`，设计规范和分析均缺 `rate` |
| 3 | `session_id`（agent_turn_end） | 分析脚本字段名错误 | JSONL 顶层字段叫 `sid`（executor.rs:1044 已正确设置），分析脚本查找 `data.session_id` 自然找不到 |
| 4 | `total_input_tokens`（token_spike） | 分析脚本字段名错误 | emit 用 `input_tokens`（设计规范名），分析脚本用 `total_input_tokens`（agent_turn_end 的字段名混淆） |
| 5 | `type`（compact_trigger） | 分析脚本字段名错误 | emit 用 `trigger`（设计规范名），分析脚本用 `type` |

**验证**：`executor.rs:1044-1045` 在每次 execute 前正确设置 `session_id` 和 `run_id` 到 state context，所有后续 emit 调用均传入 `state.get_context("session_id")`，因此主 agent 路径中 `sid` 字段**已正确写入 JSONL**。

## Step 1: 修复 `duration_secs`（最终回答耗时）

**文件**：`peri-agent/src/agent/executor/mod.rs` + `final_answer.rs`
**验证**：`cargo build -p peri-agent && cargo test -p peri-agent --lib -- agent::executor::mod_test`

### 1a：mod.rs — 在每轮迭代开始捕获 start instant

在 `for step in 0..self.max_iterations {` 之后添加：

```rust
let turn_start = std::time::Instant::now();
```

位置：`mod.rs:336`（`for step` 之后的第一条语句）。

### 1b：mod.rs — 传递 turn_start 到 handle_final_answer

修改 `handle_final_answer` 调用（`mod.rs:382-389`），添加 `turn_start` 参数：

```rust
self::final_answer::handle_final_answer(
    self, state, &reasoning, all_tool_calls.clone(),
    &mut snapshot_anchor, step, turn_start,
)
.await,
```

### 1c：final_answer.rs — 接收参数并计算耗时

修改函数签名（`final_answer.rs:61-68`）：

```rust
pub(crate) async fn handle_final_answer<L: ReactLLM, S: State>(
    agent: &ReActAgent<L, S>,
    state: &mut S,
    reasoning: &Reasoning,
    all_tool_calls: Vec<(ToolCall, ToolResult)>,
    snapshot_anchor: &mut MessageId,
    step: usize,
    turn_start: std::time::Instant,  // 新增
) -> AgentResult<AgentOutput> {
```

替换 emit 数据（`final_answer.rs:174`）：

```rust
// Before
"duration_secs": 0u64,
// After  
"duration_secs": turn_start.elapsed().as_secs(),
```

### 1d：MaxIterationsExceeded 路径不涉及

`handle_final_answer` 仅在正常回答路径调用。MaxIterationsExceeded 路径不调此函数，不影响。

---

## Step 2: 添加 `rate` 字段到 `threshold.memory`

**文件**：`peri-agent/src/metrics/mod.rs` + `peri-agent/src/agent/executor/final_answer.rs`
**验证**：`cargo build -p peri-agent`

### 2a：添加 sysinfo 依赖

在 `peri-agent/Cargo.toml` 中添加：

```toml
sysinfo.workspace = true
```

### 2b：metrics/mod.rs — 新增 `total_system_memory_mb()`

在 `current_rss_mb()` 之后添加：

```rust
/// 获取系统总物理内存（MB），跨平台
pub fn total_system_memory_mb() -> Option<u64> {
    use sysinfo::{RefreshKind, System};
    let sys = System::new_with_specifics(RefreshKind::new().with_memory());
    Some(sys.total_memory() / (1024 * 1024))
}
```

注意：`System::new_with_specifics(RefreshKind::new().with_memory())` 只刷新内存信息，不扫描进程列表，避免性能开销。

### 2c：final_answer.rs — emit 时添加 rate 字段

修改 `threshold.memory` 的 emit 数据（`final_answer.rs:149`）：

```rust
// Before
serde_json::json!({"rss_mb": rss, "level": 100}),
// After
serde_json::json!({
    "rss_mb": rss,
    "level": 100,
    "rate": crate::metrics::total_system_memory_mb()
        .map(|total| rss as f64 / total as f64)
        .unwrap_or(0.0),
}),
```

同时对 `level: 200` 路径做同样修改（`final_answer.rs:158`）。

---

## Step 3: 更新设计规范——添加 JSONL 字段查询规范

**文件**：`docs/superpowers/specs/2026-06-06-metrics-tracking-design.md`
**验证**：无编译依赖，人工 review

在示例记录之后、数据流之前（大约 line 111），添加一节：

```markdown
### JSONL 格式与字段查询规范

每行 JSON 的顶层结构：

```json
{"ts":"...","sid":"sess_xxx","rid":"turn_xxx","event":"...","data":{...}}
```

**顶层层字段**（不在 `data` 内）：
- `ts`：ISO 8601 时间戳（含毫秒）
- `sid`：**session_id**——会话标识。NOT `session_id`！
- `rid`：**run_id**——当前 ReAct 循环标识
- `event`：事件名（点分层级，如 `sample.agent_turn_end`）
- `data`：事件附属数据对象

**分析脚本字段名速查**（按设计规范名，非顶层层都位于 `data` 内）：

| 事件 | 字段名 | 顶层层？ | 说明 |
|------|--------|----------|------|
| `sample.agent_turn_end` | `duration_secs` | 否 | 本轮耗时（秒） |
| `sample.agent_turn_end` | `total_input_tokens` | 否 | 累计 input tokens |
| `threshold.memory` | `rate` | 否 | RSS / 系统总内存 |
| `threshold.token_spike` | `input_tokens` | 否 | 单次调用的 input tokens（NOT `total_input_tokens`！） |
| `trap.compact_trigger` | `trigger` | 否 | "micro" / "full"（NOT `type`！） |

**重要**：所有顶层层字段需按上述名称查询，不在 `data` 内。不可用其他名称代替（如 `session_id` ≠ `sid`）。
```

---

## 执行顺序

1. Step 1（duration_secs）→ `cargo build -p peri-agent && cargo test -p peri-agent --lib -- agent::executor::mod_test`
2. Step 2（rate）→ `cargo build -p peri-agent`
3. Step 3（设计规范）→ 人工 review
4. 全量构建 → `cargo build && cargo test`

## 不修复项

- SubAgent 路径可能没有 `session_id`——当前设计规范未要求 SubAgent 向 metrics 发射事件，不在此 issue 范围内
- `current_rss_mb()` 在 Windows 上返回 `None`——`threshold.memory` 本就不在 Windows 触发，不修
- 分析脚本字段名错误——不在本仓库内，通过 Step 3 的文档规范指导修正
