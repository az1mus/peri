# Peri 异常指标追踪系统设计

## 目标

在 peri 全链路中捕获异常/特殊事件，持久化为 JSONL 文件，供事后分析发现系统问题。仅记录"不该发生但发生了"的情况，不记录常规流程事件。

## 存储格式

### 文件布局

```
~/.peri/metrics/
  2026-06-06.jsonl
  2026-06-07.jsonl
  ...
```

按日期分文件，无自动清理策略（手动管理）。

### 单行格式

```json
{"ts":"2026-06-06T12:00:00.123Z","sid":"abc123","rid":"turn_003","event":"llm.retry","data":{"attempt":2,"max_attempts":3,"model":"claude-sonnet","error":"HTTP 429: Rate limit exceeded","delay_ms":1500}}
```

五个顶层字段：

| 字段 | 类型 | 必填 | 说明 |
|------|------|------|------|
| `ts` | ISO 8601 毫秒时间戳 | 是 | 事件发生时间 |
| `sid` | string | 是 | session_id，按会话过滤用 |
| `rid` | string | 否 | run_id（当前 ReAct 循环的 turn 标识），不可获取时省略 |
| `event` | string | 是 | 事件名，点分层级命名 |
| `data` | object | 是 | 事件附加数据，结构因事件类型而异 |

## 接入方式

### 全局静态单例

```rust
// peri-agent 中定义
pub struct Metrics { /* 内部持有 append-only 文件 handle */ }

impl Metrics {
    pub fn emit(&self, event: &str, data: serde_json::Value) { ... }
}
```

全局单例，任何位置直接调用 `metrics.emit()`。写入失败仅 `tracing::warn!`，不影响主流程。

### 实现要点

- 内部用 mpsc channel 解耦：emit() 同步 send 到 channel，单 writer task 异步消费写入文件（与项目 `persist_tx` 模式一致）
- `emit()` 为 fire-and-forget，不阻塞调用方
- 进程启动时按当天日期打开文件，跨午夜时 writer task 自动切换到新文件
- 目录 `~/.peri/metrics/` 不存在时自动创建
- `sid` 和 `rid` 由调用方传入，通过 `state.context` 注入传播

### 数据保护

- `Metrics::emit()` 内统一截断所有字符串字段到 500 字符（`chars().take(500)`），防止 Bash 长输出撑爆 JSONL + 泄露文件路径
- 截断在 emit 层单一拦截，各采集点无需单独处理

## 采集点（11 个）

### 1. 错误和重试（4 个）

| event | data 字段 | 触发条件 | 触发位置 |
|-------|----------|---------|---------|
| `llm.retry` | `attempt, max_attempts, model, error(str), delay_ms` | LLM 调用失败但可重试 | `peri-agent/src/llm/retry.rs` |
| `llm.error` | `model, provider, error(str), step, http_status, request_id` | LLM 调用最终失败（重试耗尽或不可重试） | `peri-agent/src/agent/executor/llm_step.rs` |
| `tool.error` | `name, tool_call_id, error(str), input_summary(200字截断), duration_ms, step` | 工具执行返回 is_error=true（含业务错误） | `peri-agent/src/agent/executor/tool_dispatch.rs` |
| `mcp.error` | `server, tool, error(str)` | MCP 连接/调用失败（连接层面，非工具业务错误） | `peri-middlewares/src/mcp/client.rs` |

### 2. 已知 TRAP 触发（3 个）

| event | data 字段 | 触发条件 | 触发位置 |
|-------|----------|---------|---------|
| `trap.cancel_interrupt` | `iteration, messages_in_state, had_progress(bool)` | 用户 Ctrl+C 中断 | `peri-tui/src/app/agent_ops/lifecycle.rs` |
| `trap.compact_trigger` | `trigger(micro/full), tokens_used, tokens_total, percentage` | compact 被触发 | `peri-middlewares/src/compact_middleware.rs` |
| `trap.cache_anomaly` | `rate, threshold, request_id, total_input_tokens, total_cache_read_tokens` | prompt cache 命中率低于阈值（默认 80%） | `peri-tui/src/app/agent_ops/subagent.rs` |

### 3. 异常阈值突破（3 个）

| event | data 字段 | 触发条件 | 触发位置 |
|-------|----------|---------|---------|
| `threshold.llm_calls_exceeded` | `count, limit` | 单轮 LLM 调用次数达到上限 | ReAct 循环上限检查 |
| `threshold.token_spike` | `input_tokens, output_tokens, model` | 单次 output_tokens > 4000 | LLM 调用返回后 |
| `threshold.memory` | `rss_mb, level(100/200)` | agent loop 结束时 RSS 超过阈值（每个阈值每轮只报一次） | ReAct 循环 `after_agent` |

### 4. 周期采样（1 个）

| event | data 字段 | 触发条件 | 触发位置 |
|-------|----------|---------|---------|
| `sample.agent_turn_end` | `rss_mb, iterations, total_input_tokens, total_output_tokens, duration_secs` | 每次 agent loop 正常结束时 | `after_agent` 钩子 |

### 示例记录

```jsonl
{"ts":"2026-06-06T12:00:00.123Z","sid":"sess_abc","rid":"turn_003","event":"llm.retry","data":{"attempt":2,"max_attempts":3,"model":"claude-sonnet","error":"HTTP 429: Rate limit exceeded","delay_ms":1500}}
{"ts":"2026-06-06T12:00:05.456Z","sid":"sess_abc","rid":"turn_003","event":"llm.error","data":{"model":"claude-sonnet","provider":"anthropic","error":"Connection timeout after 3 retries","step":5,"http_status":429,"request_id":"req_abc123"}}
{"ts":"2026-06-06T12:00:10.789Z","sid":"sess_abc","rid":"turn_003","event":"tool.error","data":{"name":"Bash","tool_call_id":"call_x1","error":"exit code 1: command not found","input_summary":"bash -c 'cargo build 2>&1'","duration_ms":3200,"step":3}}
{"ts":"2026-06-06T12:00:15.012Z","sid":"sess_abc","rid":"turn_003","event":"mcp.error","data":{"server":"filesystem","tool":"read_file","error":"Connection refused"}}
{"ts":"2026-06-06T12:00:20.345Z","sid":"sess_abc","rid":"turn_003","event":"trap.cancel_interrupt","data":{"iteration":5,"messages_in_state":12,"had_progress":true}}
{"ts":"2026-06-06T12:00:25.678Z","sid":"sess_abc","rid":"turn_003","event":"trap.compact_trigger","data":{"trigger":"full","tokens_used":85000,"tokens_total":100000,"percentage":0.85}}
{"ts":"2026-06-06T12:00:28.000Z","sid":"sess_abc","rid":"turn_004","event":"trap.cache_anomaly","data":{"rate":0.65,"threshold":0.80,"request_id":"req_xyz789","total_input_tokens":12000,"total_cache_read_tokens":7800}}
{"ts":"2026-06-06T12:00:30.901Z","sid":"sess_abc","rid":"turn_005","event":"threshold.llm_calls_exceeded","data":{"count":500,"limit":500}}
{"ts":"2026-06-06T12:00:35.234Z","sid":"sess_abc","rid":"turn_005","event":"threshold.token_spike","data":{"input_tokens":1200,"output_tokens":5200,"model":"claude-sonnet"}}
{"ts":"2026-06-06T12:00:36.000Z","sid":"sess_abc","rid":"turn_005","event":"threshold.memory","data":{"rss_mb":130,"level":100}}
{"ts":"2026-06-06T12:00:36.100Z","sid":"sess_abc","rid":"turn_005","event":"sample.agent_turn_end","data":{"rss_mb":135,"iterations":23,"total_input_tokens":45000,"total_output_tokens":8000,"duration_secs":42}}
```

## 数据流

```
调用方（任意 crate，任意上下文）
       ↓  metrics.emit(event, data)  // 同步 send，non-blocking
全局 mpsc channel（LazyLock<UnboundedSender>）
       ↓  单 writer task 异步消费
~/.peri/metrics/YYYY-MM-DD.jsonl
       ↓  事后分析
用户 / Langfuse 对接
```

## 约束

- **非核心路径**：Metrics 写入失败不影响任何主流程，最多 warn 日志
- **不存敏感数据**：data 中禁止记录 API key、用户消息原文等敏感信息
- **数据保护**：emit() 统一截断所有字符串字段到 500 字符，单一拦截点
- **轻量**：每个采集点只增加 1-2 行 `metrics.emit()` 调用，不引入新依赖（仅用 serde_json）
- **不膨胀**：只在异常场景触发，正常运行时 JSONL 文件几乎不增长
