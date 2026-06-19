# Metrics 采集数据质量缺陷：5 个字段恒为默认值

**状态**：Open
**优先级**：中
**创建日期**：2026-06-19

## 问题描述

当前 `~/.peri/metrics/*.jsonl` 采集的指标事件中，有 5 个关键字段的值恒为默认值/占位符，导致无法按 session 维度分析 Token 消耗趋势、无法评估 compact 策略效果、无法关联内存压力与实际使用率。这些字段实现在 `docs/superpowers/specs/2026-06-06-metrics-tracking-design.md` 中均有设计方案，但实际 emit 代码未正确接入上游数据源。

数据分析来源：`docs/analysis/metrics-analysis-2026-06.md`（5,044 条事件，12 天数据，2026-06-06 ~ 2026-06-18）。

## 症状详情

| 字段 | 所属事件（event） | 当前值 | 期望值 |
|------|-------------------|--------|--------|
| `sid`（顶层） | `sample.agent_turn_end` | 恒为缺失（不存在于 JSON 行中） | 当前会话的实际 session_id |
| `duration_secs`（data 字段） | `sample.agent_turn_end` | 恒为 `0` | 本轮 Agent loop 的实际耗时（秒） |
| `rate`（data 字段） | `threshold.memory` | 恒为 `0.0` | 当前 RSS 占用率（实际值/系统总内存） |
| `total_input_tokens`（data 字段） | `threshold.token_spike` | 恒为 `0` | 该次调用的 input token 量 |
| `type`（data 字段） | `trap.compact_trigger` | 恒为 `?` | compact 类型（`micro` / `full`） |

### 各缺陷详情

**1. `sample.agent_turn_end` — `sid` 缺失**

顶层 `sid` 字段使用 `#[serde(skip_serializing_if = "Option::is_none")]` 序列化，当 `sid` 为 `None` 时整字段不出现在 JSON 行中。调用方通过 `state.get_context("session_id")` 获取 sid，但 `session_id` 未在 emit 之前注入到 state context 中，导致始终为 `None` → JSONL 中无 `sid` 字段。

（设计期望：每行 JSONL 应有 `sid` 字段标识所属会话，按会话过滤时可用。）

**2. `sample.agent_turn_end` — `duration_secs` 恒为 0**

代码中硬编码 `"duration_secs": 0u64`，设计 plan 标记了 `// TODO: 从 start_time 计算` 但未实现。缺少本轮 start time 的跨中间件传播机制。

**3. `threshold.memory` — `rate` 恒为 0.0**

实际 emit 的 data 字段仅有 `{"rss_mb": rss, "level": 100}`，不含 `rate` 字段。但分析时发现 JSON 数据中 `data.rate` 字段存在且值恒为 `0.0`。可能来源：
- （a）`Metrics::emit()` 或 writer task 在某处为缺失字段填充了默认值
- （b）上游分析脚本自行补充了缺失字段并填充默认值

**4. `threshold.token_spike` — `total_input_tokens` 恒为 0**

设计规范用字段名 `input_tokens`，来源为 `usage.input_tokens`。若分析脚本按 `total_input_tokens` 查询则恒为 0（字段名不一致）。

但设计规范中 `sample.agent_turn_end` 用的是 `total_input_tokens`（累计值），`threshold.token_spike` 用的是 `input_tokens`（单次值）。两者语义不同但同属 Token 消耗分析维度，混淆会阻碍分析。

**5. `trap.compact_trigger` — `type` 恒为 `?`**

设计规范和实际 emit 的 data 字段名均为 `trigger`（值 `"micro"` / `"full"`），而非 `type`。若分析脚本按 `type` 查询则恒为 `?`（字段名不一致）。

### 数据样本（来自分析报告）

从 5,044 条事件分析确认：
- 2,028 条 `sample.agent_turn_end` — 全部无有效的 `sid` 且 `duration_secs` 全为 `0`
- 1,104 条 `threshold.memory` — `rate` 全部为 `0.0`
- 251 条 `threshold.token_spike` — `total_input_tokens` 全部为 `0`
- 139 条 `trap.compact_trigger` — `type` 全部为 `?`

## 影响

- **无法按 session 维度分析**：`sid` 缺失导致无法区分不同会话的 Token 消耗模式，无法做 cohort 对比
- **无法评估 compact 策略效果**：不知道每次 compact 的类型（micro/full）、不知道 compact 发生频率与 session 的关系
- **无法关联内存压力与实际使用率**：RSS 绝对值（138 MB P90）无法与系统总内存关联判断压力程度
- **Token 消耗趋势无法按 session 追踪**：仅能从全局或日期维度粗粒度分析，无法做 per-session 优化回放

## 涉及文件

- `peri-agent/src/metrics/mod.rs` — `MetricEvent` 结构、`emit()` 函数、`sid` 序列化逻辑
- `peri-agent/src/agent/executor/final_answer.rs` — `sample.agent_turn_end` + `threshold.memory` emit 位置
- `peri-agent/src/agent/executor/llm_step.rs` — `threshold.token_spike` emit 位置
- `peri-middlewares/src/compact_middleware.rs` — `trap.compact_trigger` emit 位置
- `docs/superpowers/specs/2026-06-06-metrics-tracking-design.md` — 设计规范（字段定义、期望行为）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-19 | — | Open | agent | 创建：基于 12 天数据分析确认 5 个字段缺陷 |

## 修复记录

（由 fix-issue 或 issue-verify skill 追加，创建时留空）
