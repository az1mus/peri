# Metrics 追踪系统实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 peri 全链路中实现轻量异常指标追踪系统，JSONL 文件存储，供事后分析。

**Architecture:** 全局 mpsc channel 单例（与 `persist_tx` 模式一致），emit() 同步 send 到 channel，单 writer task 异步消费写入 JSONL。定义在 `peri-agent` crate，所有上层 crate 直接调用。

**Tech Stack:** tokio mpsc channel, serde_json, chrono, sysinfo（已有）

**Design Spec:** `docs/superpowers/specs/2026-06-06-metrics-tracking-design.md`

---

## File Structure

| 操作 | 文件 | 职责 |
|------|------|------|
| Create | `peri-agent/src/metrics/mod.rs` | Metrics 单例 + mpsc channel + writer task |
| Create | `peri-agent/src/metrics/mod_test.rs` | Metrics 单元测试 |
| Modify | `peri-agent/src/lib.rs:6-16` | 添加 `pub mod metrics;` |
| Modify | `peri-agent/src/llm/retry.rs:121-126` | 添加 `llm.retry` emit |
| Modify | `peri-agent/src/agent/executor/llm_step.rs:49-65` | 添加 `llm.error` emit |
| Modify | `peri-agent/src/agent/executor/tool_dispatch.rs:366-380` | 添加 `tool.error` emit |
| Modify | `peri-middlewares/src/mcp/client.rs:168-189` | 添加 `mcp.error` emit |
| Modify | `peri-tui/src/app/agent_ops/lifecycle.rs:148` | 添加 `trap.cancel_interrupt` emit |
| Modify | `peri-middlewares/src/compact_middleware.rs:286-311` | 添加 `trap.compact_trigger` emit |
| Modify | `peri-tui/src/app/agent_ops/subagent.rs:25-54` | 添加 `trap.cache_anomaly` emit |
| Modify | `peri-agent/src/agent/executor/mod.rs:342-368` | 添加 `threshold.llm_calls_exceeded` emit |
| Modify | `peri-agent/src/agent/executor/llm_step.rs:80-82` | 添加 `threshold.token_spike` emit |
| Modify | `peri-agent/src/agent/executor/final_answer.rs:137` | 添加 `threshold.memory` + `sample.agent_turn_end` emit |

---

### Task 1: Metrics 核心模块

**Files:**
- Create: `peri-agent/src/metrics/mod.rs`
- Create: `peri-agent/src/metrics/mod_test.rs`
- Modify: `peri-agent/src/lib.rs`

- [ ] **Step 1: 创建 metrics 模块文件**

创建 `peri-agent/src/metrics/mod.rs`：

```rust
//! 轻量异常指标追踪系统
//!
//! JSONL 文件存储，mpsc channel 解耦，fire-and-forget 写入。

use chrono::Utc;
use serde::Serialize;
use std::sync::LazyLock;
use tokio::sync::mpsc;

/// 字符串截断上限（字符级，CJK 安全）
const TRUNCATE_LIMIT: usize = 500;

/// 指标事件
#[derive(Debug, Serialize)]
struct MetricEvent {
    /// ISO 8601 毫秒时间戳
    ts: String,
    /// session_id
    #[serde(skip_serializing_if = "Option::is_none")]
    sid: Option<String>,
    /// run_id（当前 ReAct 循环标识）
    #[serde(skip_serializing_if = "Option::is_none")]
    rid: Option<String>,
    /// 事件名（点分层级）
    event: String,
    /// 事件附加数据
    data: serde_json::Value,
}

/// 全局 channel sender
static METRICS_TX: LazyLock<mpsc::UnboundedSender<MetricEvent>> = LazyLock::new(|| {
    let (tx, rx) = mpsc::unbounded_channel();
    tokio::spawn(metrics_writer(rx));
    tx
});

/// 发射一个指标事件。fire-and-forget，不阻塞调用方。
///
/// `data` 中所有字符串值会被截断到 500 字符。
pub fn emit(
    event: &str,
    data: serde_json::Value,
    sid: Option<&str>,
    rid: Option<&str>,
) {
    let data = truncate_json_strings(data);
    let evt = MetricEvent {
        ts: Utc::now().to_rfc3339_opts(chrono::SecondsFormat::Millis, true),
        sid: sid.map(|s| s.to_owned()),
        rid: rid.map(|s| s.to_owned()),
        event: event.to_owned(),
        data,
    };
    if METRICS_TX.send(evt).is_err() {
        tracing::warn!(event, "metrics channel send failed (writer dropped)");
    }
}

/// 单 writer task：消费 channel，追加写入 JSONL 文件
async fn metrics_writer(mut rx: mpsc::UnboundedReceiver<MetricEvent>) {
    let base_dir = dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".peri")
        .join("metrics");

    if let Err(e) = tokio::fs::create_dir_all(&base_dir).await {
        tracing::warn!(path = %base_dir.display(), error = %e, "无法创建 metrics 目录");
        return;
    }

    let mut current_date = today();
    let path = base_dir.join(format!("{current_date}.jsonl"));
    let file = match tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .await
    {
        Ok(f) => f,
        Err(e) => {
            tracing::warn!(path = %path.display(), error = %e, "无法打开 metrics 文件");
            return;
        }
    };
    use tokio::io::AsyncWriteExt;
    let mut writer = tokio::io::BufWriter::new(file);

    while let Some(evt) = rx.recv().await {
        // 跨午夜切换文件
        let date = today();
        if date != current_date {
            let _ = writer.flush().await;
            let path = base_dir.join(format!("{date}.jsonl"));
            match tokio::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&path)
                .await
            {
                Ok(f) => {
                    writer = tokio::io::BufWriter::new(f);
                    current_date = date;
                }
                Err(e) => {
                    tracing::warn!(path = %path.display(), error = %e, "无法切换 metrics 文件");
                    return;
                }
            }
        }

        match serde_json::to_string(&evt) {
            Ok(line) => {
                if let Err(e) = writer.write_all(line.as_bytes()).await {
                    tracing::warn!(error = %e, "metrics write failed");
                }
                if let Err(e) = writer.write_all(b"\n").await {
                    tracing::warn!(error = %e, "metrics newline write failed");
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "metrics serialize failed");
            }
        }
    }

    // channel 关闭时 flush
    let _ = writer.flush().await;
}

fn today() -> String {
    chrono::Local::now().format("%Y-%m-%d").to_string()
}

/// 递归截断 JSON 中所有字符串值到 TRUNCATE_LIMIT 字符
fn truncate_json_strings(val: serde_json::Value) -> serde_json::Value {
    match val {
        serde_json::Value::String(s) => {
            if s.chars().count() > TRUNCATE_LIMIT {
                serde_json::Value::String(s.chars().take(TRUNCATE_LIMIT).collect())
            } else {
                serde_json::Value::String(s)
            }
        }
        serde_json::Value::Object(map) => {
            let new_map: serde_json::Map<String, serde_json::Value> = map
                .into_iter()
                .map(|(k, v)| (k, truncate_json_strings(v)))
                .collect();
            serde_json::Value::Object(new_map)
        }
        serde_json::Value::Array(arr) => {
            serde_json::Value::Array(arr.into_iter().map(truncate_json_strings).collect())
        }
        other => other,
    }
}

#[cfg(test)]
#[path = "mod_test.rs"]
mod tests;
```

- [ ] **Step 2: 创建测试文件**

创建 `peri-agent/src/metrics/mod_test.rs`：

```rust
use super::*;

#[test]
fn test_truncate_short_string_unchanged() {
    let val = serde_json::json!({"error": "short"});
    let result = truncate_json_strings(val);
    assert_eq!(result["error"], "short");
}

#[test]
fn test_truncate_long_string() {
    let long: String = "x".repeat(600);
    let val = serde_json::json!({"error": long});
    let result = truncate_json_strings(val);
    assert_eq!(result["error"].as_str().unwrap().chars().count(), 500);
}

#[test]
fn test_truncate_cjk_string() {
    // CJK 字符，每个占 3 bytes UTF-8，必须用字符级截断
    let long: String = "你".repeat(600);
    let val = serde_json::json!({"error": long});
    let result = truncate_json_strings(val);
    assert_eq!(result["error"].as_str().unwrap().chars().count(), 500);
}

#[test]
fn test_truncate_nested_object() {
    let long: String = "a".repeat(600);
    let val = serde_json::json!({"data": {"nested": long, "ok": "short"}, "arr": [long]});
    let result = truncate_json_strings(val);
    assert_eq!(result["data"]["nested"].as_str().unwrap().chars().count(), 500);
    assert_eq!(result["data"]["ok"], "short");
    assert_eq!(result["arr"][0].as_str().unwrap().chars().count(), 500);
}

#[test]
fn test_truncate_non_string_unchanged() {
    let val = serde_json::json!({"count": 42, "flag": true, "null": null});
    let result = truncate_json_strings(val);
    assert_eq!(result["count"], 42);
    assert_eq!(result["flag"], true);
    assert!(result["null"].is_null());
}

#[test]
fn test_today_format() {
    let date = today();
    // 格式 YYYY-MM-DD
    assert_eq!(date.len(), 10);
    assert!(date.contains('-'));
}
```

- [ ] **Step 3: 注册模块**

在 `peri-agent/src/lib.rs` 的模块声明中（约第 6-16 行），在 `pub mod middleware;` 之后添加：

```rust
pub mod metrics;
```

- [ ] **Step 4: 验证编译和测试**

Run: `cargo build -p peri-agent 2>&1 | tail -5`
Expected: 编译成功

Run: `cargo test -p peri-agent --lib -- metrics::tests 2>&1 | tail -10`
Expected: 6 个测试全部 PASS

- [ ] **Step 5: Commit**

```bash
git add peri-agent/src/metrics/ peri-agent/src/lib.rs
git commit -m "feat(metrics): add metrics core module with mpsc channel writer"
```

---

### Task 2: 错误类指标接入（4 个）

**Files:**
- Modify: `peri-agent/src/llm/retry.rs:121-126`
- Modify: `peri-agent/src/agent/executor/llm_step.rs:49-65`
- Modify: `peri-agent/src/agent/executor/tool_dispatch.rs:366-380`
- Modify: `peri-middlewares/src/mcp/client.rs:168-189`

- [ ] **Step 1: `llm.retry` — 在 `peri-agent/src/llm/retry.rs:121` 处 emit**

在 `self.emit(AgentEvent::LlmRetrying { ... });` 之后添加：

```rust
crate::metrics::emit(
    "llm.retry",
    serde_json::json!({
        "attempt": attempt + 1,
        "max_attempts": self.config.max_retries,
        "model": self.inner.model_name(),
        "error": e.to_string(),
        "delay_ms": delay,
    }),
    None, // sid: retry 层无 state 访问
    None, // rid
);
```

- [ ] **Step 2: `llm.error` — 在 `peri-agent/src/agent/executor/llm_step.rs:49` 的 `Err(e)` 分支内 emit**

在 `agent.chain.run_on_error(state, &e).await?;` 之前添加：

```rust
let (http_status, request_id) = match &e {
    AgentError::LlmHttpError { status, .. } => (Some(*status), None),
    _ => (None, None),
};
let rid = state.get_context("run_id").map(|s| s.to_owned());
crate::metrics::emit(
    "llm.error",
    serde_json::json!({
        "model": agent.llm.model_name(),
        "provider": agent.llm.provider_name(),
        "error": e.to_string(),
        "step": step,
        "http_status": http_status,
        "request_id": request_id,
    }),
    state.get_context("session_id"),
    rid.as_deref(),
);
```

- [ ] **Step 3: `tool.error` — 在 `peri-agent/src/agent/executor/tool_dispatch.rs:366` 的 `if result.is_error` 块内 emit**

在 `tracing::warn!(... "tool call failed");` 之后添加：

```rust
let rid = state.get_context("run_id").map(|s| s.to_owned());
let input_summary: String = modified_call
    .input
    .as_str()
    .unwrap_or("")
    .chars()
    .take(200)
    .collect();
crate::metrics::emit(
    "tool.error",
    serde_json::json!({
        "name": result.tool_name,
        "tool_call_id": modified_call.id,
        "error": result.output,
        "input_summary": input_summary,
        "duration_ms": result.duration_ms,
        "step": state.current_step(),
    }),
    state.get_context("session_id"),
    rid.as_deref(),
);
```

**注意**：需要确认 `ToolResult` 是否有 `duration_ms` 字段。如果没有，先记录为 `null`，后续补上。

- [ ] **Step 4: `mcp.error` — 在 `peri-middlewares/src/mcp/client.rs:168` 的 `insert_failed` 方法内 emit**

在 `pool.clients.write().insert(...)` 之后添加：

```rust
crate::metrics::emit(
    "mcp.error",
    serde_json::json!({
        "server": name,
        "tool": "connect",
        "error": reason,
    }),
    None,
    None,
);
```

- [ ] **Step 5: 验证编译**

Run: `cargo build -p peri-agent -p peri-middlewares 2>&1 | tail -5`
Expected: 编译成功（可能有 unused variable 警告，正常）

- [ ] **Step 6: Commit**

```bash
git add peri-agent/src/llm/retry.rs peri-agent/src/agent/executor/llm_step.rs peri-agent/src/agent/executor/tool_dispatch.rs peri-middlewares/src/mcp/client.rs
git commit -m "feat(metrics): wire llm.retry, llm.error, tool.error, mcp.error emitters"
```

---

### Task 3: TRAP 类指标接入（3 个）

**Files:**
- Modify: `peri-tui/src/app/agent_ops/lifecycle.rs:148`
- Modify: `peri-middlewares/src/compact_middleware.rs:286-311`
- Modify: `peri-tui/src/app/agent_ops/subagent.rs:25-54`

- [ ] **Step 1: `trap.cancel_interrupt` — 在 `peri-tui/src/app/agent_ops/lifecycle.rs:148` 的 `handle_interrupted` 方法内 emit**

在方法入口处（`self.session_mgr.current_mut().agent.cancel_sent_at = None;` 之后），获取上下文信息，然后在函数主要分支返回前添加 emit：

```rust
let iteration = self.session_mgr.current().agent.subagent_depth;
let messages_in_state = self.session_mgr.current().messages.view.len();
let had_progress = /* 检查是否有工具调用 */ has_tool_calls;

peri_agent::metrics::emit(
    "trap.cancel_interrupt",
    serde_json::json!({
        "iteration": iteration,
        "messages_in_state": messages_in_state,
        "had_progress": had_progress,
    }),
    Some(&self.session_mgr.current().session_id),
    None,
);
```

**注意**：`handle_interrupted` 中已有 `has_tool_calls` 变量和 messages 数量信息。emit 位置应在 Pipeline 处理之后、return 之前，确保数据准确。

- [ ] **Step 2: `trap.compact_trigger` — 在 `peri-middlewares/src/compact_middleware.rs:294` 的 `before_model` 中 emit**

在 `if should_full { ... }` 之前、`(full, micro)` 计算之后添加：

```rust
if should_full || should_micro {
    let tracker = state.token_tracker();
    let budget = &self.budget;
    let percentage = tracker
        .context_usage_percent(budget.context_window)
        .unwrap_or(0.0);
    let sid = state.get_context("session_id");
    let rid = state.get_context("run_id");
    crate::metrics::emit(
        "trap.compact_trigger",
        serde_json::json!({
            "trigger": if should_full { "full" } else { "micro" },
            "tokens_used": tracker.estimated_context_tokens().unwrap_or(0),
            "tokens_total": budget.context_window as u64,
            "percentage": percentage,
        }),
        sid,
        rid,
    );
}
```

- [ ] **Step 3: `trap.cache_anomaly` — 在 `peri-tui/src/app/agent_ops/subagent.rs:32` 的 `if rate < 0.8` 块内 emit**

在 `tracing::warn!(... "prompt cache hit rate below threshold");` 之后添加：

```rust
peri_agent::metrics::emit(
    "trap.cache_anomaly",
    serde_json::json!({
        "rate": rate,
        "threshold": 0.80,
        "request_id": tracker.last_request_id.as_deref().unwrap_or("-"),
        "total_input_tokens": tracker.total_input_tokens,
        "total_cache_read_tokens": tracker.total_cache_read_tokens,
    }),
    Some(&self.session_mgr.current().session_id),
    None,
);
```

- [ ] **Step 4: 验证编译**

Run: `cargo build -p peri-middlewares -p peri-tui 2>&1 | tail -5`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/agent_ops/lifecycle.rs peri-middlewares/src/compact_middleware.rs peri-tui/src/app/agent_ops/subagent.rs
git commit -m "feat(metrics): wire trap.cancel_interrupt, trap.compact_trigger, trap.cache_anomaly"
```

---

### Task 4: 阈值类 + 采样指标接入（4 个）

**Files:**
- Modify: `peri-agent/src/agent/executor/mod.rs:342-368`
- Modify: `peri-agent/src/agent/executor/llm_step.rs:80-82`
- Modify: `peri-agent/src/agent/executor/final_answer.rs:137`

- [ ] **Step 1: `threshold.llm_calls_exceeded` — 在 `peri-agent/src/agent/executor/mod.rs:368` 处 emit**

在 `return Err(AgentError::MaxIterationsExceeded(self.max_iterations));` 之前添加：

```rust
crate::metrics::emit(
    "threshold.llm_calls_exceeded",
    serde_json::json!({
        "count": self.max_iterations,
        "limit": self.max_iterations,
    }),
    state.get_context("session_id"),
    state.get_context("run_id"),
);
```

- [ ] **Step 2: `threshold.token_spike` — 在 `peri-agent/src/agent/executor/llm_step.rs:80` 的 LLM 成功返回后 emit**

在 `agent.emit(AgentEvent::LlmCallEnd { ... });` 之后、`if let Some(ref usage) = reasoning.usage` 块内添加：

```rust
if let Some(ref usage) = reasoning.usage {
    if usage.output_tokens > 4000 {
        crate::metrics::emit(
            "threshold.token_spike",
            serde_json::json!({
                "input_tokens": usage.input_tokens,
                "output_tokens": usage.output_tokens,
                "model": agent.llm.model_name(),
            }),
            state.get_context("session_id"),
            state.get_context("run_id"),
        );
    }
}
```

**注意**：需要确认 emit 位置不会与已有的 `if let Some(ref usage) = reasoning.usage` 块重复嵌套。实际代码中这个 if-let 在第 84 行，token_spike 检查应放在其内部的开头。

- [ ] **Step 3: `threshold.memory` + `sample.agent_turn_end` — 在 `peri-agent/src/agent/executor/final_answer.rs:137` 的 `after_agent` 调用前 emit**

需要跨平台获取 RSS。由于 `alloc_config.rs` 在 `peri-tui` 中，`peri-agent` 不能直接依赖。方案：在 emit 点用 `std::process::Command` 调用或使用条件编译。

**实际方案**：创建一个简单的内联函数获取 RSS（仅在 macOS/Linux 上）：

在 `peri-agent/src/metrics/mod.rs` 中添加：

```rust
/// 获取当前进程 RSS（MB），跨平台
pub fn current_rss_mb() -> Option<u64> {
    #[cfg(unix)]
    {
        // 读取 /proc/self/statm（Linux）或使用 sysctl（macOS）
        // 简单方案：通过 libc::getrusage
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        if ret == 0 {
            // ru_maxrss: Linux 上是 KB，macOS 上是 bytes
            #[cfg(target_os = "macos")]
            let rss_kb = usage.ru_maxrss / 1024;
            #[cfg(not(target_os = "macos"))]
            let rss_kb = usage.ru_maxrss;
            return Some(rss_kb / 1024);
        }
        None
    }
    #[cfg(not(unix))]
    {
        None
    }
}
```

然后在 `Cargo.toml` 中确认已有 `libc` 依赖（如果没有则添加）。

在 `final_answer.rs` 的 `match agent.chain.run_after_agent(...)` 之前添加：

```rust
let sid = state.get_context("session_id");
let rid = state.get_context("run_id");
let rss_mb = crate::metrics::current_rss_mb();

// threshold.memory：每个阈值只报一次（通过 state.context 标记）
if let Some(rss) = rss_mb {
    let reported_100 = state.get_context("mem_reported_100").is_some();
    let reported_200 = state.get_context("mem_reported_200").is_some();
    if rss >= 100 && !reported_100 {
        crate::metrics::emit(
            "threshold.memory",
            serde_json::json!({"rss_mb": rss, "level": 100}),
            sid,
            rid,
        );
        state.set_context("mem_reported_100", "1");
    }
    if rss >= 200 && !reported_200 {
        crate::metrics::emit(
            "threshold.memory",
            serde_json::json!({"rss_mb": rss, "level": 200}),
            sid,
            rid,
        );
        state.set_context("mem_reported_200", "1");
    }
}

// sample.agent_turn_end
crate::metrics::emit(
    "sample.agent_turn_end",
    serde_json::json!({
        "rss_mb": rss_mb,
        "iterations": state.current_step(),
        "total_input_tokens": state.token_tracker().total_input_tokens,
        "total_output_tokens": state.token_tracker().total_output_tokens,
        "duration_secs": 0, // TODO: 从 start_time 计算
    }),
    sid,
    rid,
);
```

- [ ] **Step 4: 验证编译和测试**

Run: `cargo build -p peri-agent 2>&1 | tail -5`
Expected: 编译成功

Run: `cargo test -p peri-agent --lib 2>&1 | tail -10`
Expected: 所有测试 PASS

- [ ] **Step 5: Commit**

```bash
git add peri-agent/src/metrics/ peri-agent/src/agent/executor/mod.rs peri-agent/src/agent/executor/llm_step.rs peri-agent/src/agent/executor/final_answer.rs peri-agent/Cargo.toml
git commit -m "feat(metrics): wire threshold and sampling emitters"
```

---

### Task 5: sid/rid 注入 + 全量编译验证

**Files:**
- Modify: `peri-acp/src/session/executor.rs`（`execute_prompt` 入口）

- [ ] **Step 1: 在 `execute_prompt` 入口注入 sid/rid**

在 `execute_prompt` 函数中构建 `AgentState` 之后，添加：

```rust
state.set_context("session_id", &session_id);
state.set_context("run_id", &uuid::Uuid::now_v7().to_string());
```

**注意**：需要确认 `execute_prompt` 的具体签名和 `session_id` 变量名。

- [ ] **Step 2: 全量编译验证**

Run: `cargo build 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 3: 全量测试**

Run: `cargo test 2>&1 | tail -15`
Expected: 所有测试 PASS

- [ ] **Step 4: Commit**

```bash
git add peri-acp/src/session/executor.rs
git commit -m "feat(metrics): inject sid/rid into agent state for metrics propagation"
```

---

### Task 6: 集成验证

- [ ] **Step 1: 启动 TUI，触发一个简短对话**

Run: `cargo run -p peri-tui`
操作：输入简单问题如 "hi"

- [ ] **Step 2: 检查 metrics 文件是否生成**

Run: `ls -la ~/.peri/metrics/`
Expected: 存在当天日期的 `.jsonl` 文件

- [ ] **Step 3: 检查文件内容**

Run: `cat ~/.peri/metrics/$(date +%Y-%m-%d).jsonl`
Expected: 如果有异常事件发生，能看到 JSONL 记录。正常对话可能只有 `sample.agent_turn_end`

- [ ] **Step 4: 最终 Commit**

```bash
git add -A
git commit -m "feat(metrics): complete metrics tracking system integration"
```
