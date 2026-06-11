# 拆分 Top 3 大文件 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 slop-cleaner 报告的 3 个最大文件（message_pipeline/mod.rs 1187 行，acp_stdio.rs 988 行，message_view/mod.rs 917 行）各拆分至 ≤400 行，消除膨胀。

**Architecture:** 每个文件沿现有拆分骨架（已有子文件的模块）补充缺失的子模块，不改变外部 API（re-export 从 mod.rs 保持兼容）。三个文件位于三个独立子系统（pipeline、stdio transport、view model），互不依赖，可按任意顺序独立实施。

**Tech Stack:** Rust 2021, tokio async, ratatui TUI

---

## Part A：message_pipeline/mod.rs（1187 → 约 350）

### 目标结构

```
app/message_pipeline/
├── mod.rs             # ~350 行：struct 定义 + handle_event() 薄路由 + re-exports
├── reconcile.rs       # 已有（不变）
├── transform.rs       # 已有（不变）
├── throttle.rs        # NEW：AdaptiveChunkingPolicy + check_throttle*()
├── streaming.rs       # NEW：StreamingMode + push_chunk*() + Block 缓冲区
├── tools.rs           # NEW：PendingTool/CompletedTool + tool_start/end_internal
├── subagent.rs        # NEW：SubAgentState/BatchInfo + bg 路由 + notify_bg_completed
├── lifecycle.rs       # NEW：done/interrupt/begin_round/clear + 查询方法
└── message_pipeline_test.rs  # 已有（不变）
```

### 外部消费者（需保持兼容的 pub API）

| 符号 | 消费者（生产代码） |
|------|------------------|
| `MessagePipeline` | headless_test.rs |
| `PipelineAction` | headless_test.rs, agent_render.rs, message_state.rs, agent_compact.rs, thread_ops.rs 等 7 处 |
| `StreamingMode` | agent_submit.rs |
| `aggregate_batch_groups` | agent_render.rs (re-export via mod.rs) |

所有符号在 mod.rs 中 `pub use` 重导出，消费者无需修改。

---

### Task A1：提取 throttle.rs（零耦合，最低风险）

**Files:**
- Create: `peri-tui/src/app/message_pipeline/throttle.rs`
- Modify: `peri-tui/src/app/message_pipeline/mod.rs`（删除行 55-199 + 行 1096-1146）

**Step 1：创建 throttle.rs**

```rust
//! 自适应节流策略：控制 LLM 流式输出的渲染帧率。

use std::time::{Duration, Instant};

use super::PipelineAction;

/// 排空计划：控制每次 check_throttle 的消费量
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum DrainPlan {
    /// 正常模式：提交一行（单次 RebuildAll）
    Single,
    /// 积压模式：一次性排空所有积压行
    Batch,
}

/// 分块模式（内部状态）
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ChunkingMode {
    Smooth,
    CatchUp,
}

/// 自适应分块策略：根据队列压力在 Smooth/CatchUp 模式间动态切换。
pub(crate) struct AdaptiveChunkingPolicy {
    pub(crate) mode: ChunkingMode,
    pub(crate) pending_lines: usize,
    pub(crate) oldest_chunk_at: Option<Instant>,
    queue_depth_threshold: usize,
    oldest_age_threshold: Duration,
    exit_depth: usize,
    exit_age: Duration,
}

impl AdaptiveChunkingPolicy {
    pub(crate) fn new() -> Self {
        Self {
            mode: ChunkingMode::Smooth,
            pending_lines: 0,
            oldest_chunk_at: None,
            queue_depth_threshold: 8,
            oldest_age_threshold: Duration::from_millis(120),
            exit_depth: 2,
            exit_age: Duration::from_millis(40),
        }
    }

    pub(crate) fn reset(&mut self) {
        self.mode = ChunkingMode::Smooth;
        self.pending_lines = 0;
        self.oldest_chunk_at = None;
    }

    pub(crate) fn on_chunk(&mut self, chunk: &str) {
        self.pending_lines += chunk.chars().filter(|&c| c == '\n').count();
        if self.oldest_chunk_at.is_none() {
            self.oldest_chunk_at = Some(Instant::now());
        }
    }

    pub(crate) fn on_reasoning_chunk(&mut self) {
        self.pending_lines += 1;
        if self.oldest_chunk_at.is_none() {
            self.oldest_chunk_at = Some(Instant::now());
        }
    }

    pub(crate) fn check(&mut self) -> Option<DrainPlan> {
        if self.pending_lines == 0 {
            return None;
        }
        let oldest_ms = Instant::now()
            .duration_since(self.oldest_chunk_at.unwrap_or_else(Instant::now))
            .as_millis() as u64;
        match self.mode {
            ChunkingMode::Smooth => {
                if self.pending_lines >= self.queue_depth_threshold
                    || oldest_ms >= self.oldest_age_threshold.as_millis() as u64
                {
                    self.mode = ChunkingMode::CatchUp;
                    Some(DrainPlan::Batch)
                } else {
                    Some(DrainPlan::Single)
                }
            }
            ChunkingMode::CatchUp => {
                if self.pending_lines <= self.exit_depth
                    && oldest_ms <= self.exit_age.as_millis() as u64
                {
                    self.mode = ChunkingMode::Smooth;
                }
                Some(DrainPlan::Batch)
            }
        }
    }

    pub(crate) fn drain(&mut self) {
        self.pending_lines = 0;
        self.oldest_chunk_at = None;
    }
}

// ── 节流方法（从 MessagePipeline 移入）──────────────────────────────────

impl super::MessagePipeline {
    /// 检查自适应节流策略。
    pub(crate) fn check_throttle(&mut self, prefix_len: usize) -> Option<PipelineAction> {
        match self.streaming_mode {
            super::streaming::StreamingMode::Streaming => self.check_throttle_streaming(prefix_len),
            super::streaming::StreamingMode::Block => self.check_throttle_block(prefix_len),
            super::streaming::StreamingMode::None => None,
        }
    }

    fn check_throttle_streaming(&mut self, prefix_len: usize) -> Option<PipelineAction> {
        let plan = self.adaptive_policy.check()?;
        match plan {
            DrainPlan::Single => {
                let now = Instant::now();
                let min_interval = Duration::from_millis(16);
                let should_fire = match self.throttle_last_fire {
                    None => true,
                    Some(last) => now.duration_since(last) >= min_interval,
                };
                if !should_fire {
                    return None;
                }
                self.throttle_last_fire = Some(now);
                self.adaptive_policy.drain();
                Some(self.build_rebuild_all(prefix_len))
            }
            DrainPlan::Batch => {
                self.throttle_last_fire = Some(Instant::now());
                self.adaptive_policy.drain();
                Some(self.build_rebuild_all(prefix_len))
            }
        }
    }

    fn check_throttle_block(&mut self, prefix_len: usize) -> Option<PipelineAction> {
        if self.block_pending_flush {
            self.block_pending_flush = false;
            Some(self.build_rebuild_all(prefix_len))
        } else {
            None
        }
    }
}
```

**Step 2：在 mod.rs 中替换节流代码**

在 `mod.rs` 行 15 的文档注释后，将所有节流相关代码（行 41-199 的 `StreamingMode`、`DrainPlan`、`ChunkingMode`、`AdaptiveChunkingPolicy`、行 1096-1146 的 `check_throttle*` 方法）替换为两行：

```rust
mod throttle;
mod streaming;

pub(crate) use throttle::DrainPlan;
pub(crate) use streaming::StreamingMode;
```

**注意**：`StreamingMode` 需要放在 `streaming.rs` 中（见 Task A2），但在此步骤中它已被节流代码引用。为了保持每个 task 独立编译，Task A1 暂时将 `StreamingMode` 保留在 mod.rs 行 41-53，只在 `throttle.rs` 中通过 `use super::StreamingMode` 引用。Task A2 再将其移到 streaming.rs。

**Step 3：构建验证**

```bash
cargo check -p peri-tui 2>&1
```

Expected: 编译通过，无 warning。

**Step 4：提交**

```bash
git add peri-tui/src/app/message_pipeline/throttle.rs peri-tui/src/app/message_pipeline/mod.rs
git commit -m "refactor(message_pipeline): extract throttle logic to throttle.rs (-145 lines)

Move AdaptiveChunkingPolicy, DrainPlan, ChunkingMode, and check_throttle*
methods out of mod.rs into separate throttle module.

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task A2：提取 streaming.rs

**Files:**
- Create: `peri-tui/src/app/message_pipeline/streaming.rs`
- Modify: `peri-tui/src/app/message_pipeline/mod.rs`

**Step 1：创建 streaming.rs**

```rust
//! 流式渲染模式及 Block 模式缓冲区管理。

use super::MessagePipeline;

/// 流式渲染模式：控制 LLM 输出时的渲染粒度。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum StreamingMode {
    /// 逐 token 实时渲染 + 自适应帧率（默认）
    #[default]
    Streaming,
    /// 按 Markdown block 粒度整块渲染
    Block,
    /// 不渲染流式内容，LLM 完成后一次性显示
    None,
}

impl MessagePipeline {
    /// 追加流式文本 chunk
    pub(crate) fn push_chunk(&mut self, chunk: &str) {
        match self.streaming_mode {
            StreamingMode::Streaming => {
                self.current_ai_text.push_str(chunk);
                self.adaptive_policy.on_chunk(chunk);
            }
            StreamingMode::Block => {
                if self.push_chunk_block(chunk) {
                    self.flush_block_buffer();
                }
            }
            StreamingMode::None => {
                self.current_ai_text.push_str(chunk);
            }
        }
    }

    /// 追加推理 chunk
    pub(crate) fn push_reasoning(&mut self, text: &str) {
        self.current_ai_reasoning.push_str(text);
        self.adaptive_policy.on_reasoning_chunk();
    }

    // ─── Block 模式缓冲区管理 ───────────────────────────────────────

    fn push_chunk_block(&mut self, chunk: &str) -> bool {
        self.block_buffer.push_str(chunk);
        if self.inside_code_fence {
            if self.detect_code_fence_close() {
                self.inside_code_fence = false;
                return true;
            }
        } else {
            if self.block_buffer.contains("\n\n") {
                return true;
            }
            if self.detect_code_fence_open() {
                self.inside_code_fence = true;
            }
        }
        false
    }

    fn detect_code_fence_open(&self) -> bool {
        self.block_buffer
            .lines()
            .last()
            .is_some_and(|line| line.trim_start().starts_with("```"))
    }

    fn detect_code_fence_close(&self) -> bool {
        self.block_buffer
            .lines()
            .last()
            .is_some_and(|line| line.trim() == "```")
    }

    fn flush_block_buffer(&mut self) {
        if !self.block_buffer.is_empty() {
            self.current_ai_text.push_str(&self.block_buffer);
            self.block_buffer.clear();
            self.block_pending_flush = true;
        }
    }

    pub(crate) fn force_flush_block(&mut self) {
        self.flush_block_buffer();
    }

    pub(crate) fn has_pending_block_flush(&self) -> bool {
        self.block_pending_flush
    }

    pub(crate) fn init_streaming_mode_from_config(&mut self) {
        // 从 config 读取 streaming_mode 偏好（当前默认 Streaming）
        self.streaming_mode = StreamingMode::Streaming;
    }

    pub(crate) fn streaming_mode(&self) -> StreamingMode {
        self.streaming_mode
    }

    pub(crate) fn set_streaming_mode(&mut self, mode: StreamingMode) {
        self.streaming_mode = mode;
    }
}
```

**Step 2：修改 mod.rs**

- 删除行 41-53 的 `StreamingMode` enum 定义
- 删除行 564-637 的 `push_chunk/push_reasoning` 及 Block 缓冲区方法
- 删除行 326-354 的 `streaming_mode()/set_streaming_mode()/init_streaming_mode_from_config()/has_pending_block_flush()`
- 删除行 634-635 的 `force_flush_block()`（已在 streaming.rs 中）
- 确保 mod.rs 中 `mod streaming;` 已声明（由 Task A1 添加）

**Step 3：构建验证**

```bash
cargo check -p peri-tui 2>&1
```

Expected: 编译通过。

**Step 4：提交**

```bash
git add peri-tui/src/app/message_pipeline/streaming.rs peri-tui/src/app/message_pipeline/mod.rs
git commit -m "refactor(message_pipeline): extract streaming mode + block buffer to streaming.rs (-110 lines)

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task A3：提取 tools.rs

**Files:**
- Create: `peri-tui/src/app/message_pipeline/tools.rs`
- Modify: `peri-tui/src/app/message_pipeline/mod.rs`

**Step 1：创建 tools.rs**

```rust
//! 工具调用状态追踪：PendingTool/CompletedTool + tool_start/tool_end 处理。

use std::collections::HashMap;

use peri_agent::messages::ToolCallRequest;

use crate::app::{events::AgentEvent, tool_display};
use crate::ui::message_view::{instance_hash, MessageViewModel};

use super::{CompletedTool, PendingTool, SubAgentState};

impl super::MessagePipeline {
    /// 处理 ToolStart 事件
    pub(crate) fn tool_start_internal(
        &mut self,
        tool_call_id: String,
        name: String,
        input: serde_json::Value,
        source_agent_id: Option<String>,
    ) {
        // ... 现有实现（行 641-703，保持不变）
    }

    /// 处理 ToolEnd 事件
    pub(crate) fn tool_end_internal(
        &mut self,
        tool_call_id: String,
        output: String,
        is_error: bool,
        signal: Option<&str>,
        source_agent_id: Option<String>,
    ) {
        // ... 现有实现（行 705-756，保持不变）
    }
}
```

**实际实现**：将 mod.rs 行 641-756 完整移动到 tools.rs 的 `impl super::MessagePipeline` 块中。

**Step 2：修改 mod.rs**

- 删除行 639-756 的 `tool_start_internal()` 和 `tool_end_internal()` 方法
- 在 mod.rs 顶部添加 `mod tools;`
- 将 `PendingTool`、`CompletedTool` 定义（行 201-218）保留在 mod.rs 中（它们被 reconcile.rs 读取），或者通过 `pub(crate) use` 重导出

**Step 3：构建验证**

```bash
cargo check -p peri-tui 2>&1
```

Expected: 编译通过。

**Step 4：提交**

```bash
git add peri-tui/src/app/message_pipeline/tools.rs peri-tui/src/app/message_pipeline/mod.rs
git commit -m "refactor(message_pipeline): extract tool tracking to tools.rs (-115 lines)

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task A4：提取 subagent.rs + lifecycle.rs

**Files:**
- Create: `peri-tui/src/app/message_pipeline/subagent.rs`
- Create: `peri-tui/src/app/message_pipeline/lifecycle.rs`
- Modify: `peri-tui/src/app/message_pipeline/mod.rs`

**Step 1：创建 subagent.rs（~280 行）**

包含：`SubAgentState` struct、`BatchInfo` struct、所有 subagent routing 静态方法、`find_running_subagent_mut()`、`drain_subagent_stack()`、`notify_bg_completed()`。

注意 `notify_bg_completed()` 调用了 `self.build_tail_vms()`（定义在 reconcile.rs），需要 `use super::reconcile::build_tail_vms` 或在 impl 块内部 `use`。

`SubAgentState` 字段中的 `pub(crate)` 字段在移动到 subagent.rs 后改为 `pub(super)` 或通过 re-export 保持：

```rust
// subagent.rs
use crate::ui::message_view::MessageViewModel;

/// 活跃 SubAgent 执行状态
pub(super) struct SubAgentState {
    pub(super) agent_id: String,
    pub(super) instance_id: String,
    pub(super) task_preview: String,
    pub(super) total_steps: usize,
    pub(super) recent_messages: Vec<MessageViewModel>,
    pub(super) is_running: bool,
    pub(super) finalized_vm: Option<MessageViewModel>,
    pub(super) is_background: bool,
    pub(super) bg_hash: Option<String>,
}

pub(super) struct BatchInfo {
    pub(super) started: usize,
    pub(super) completed: usize,
}

// impl super::MessagePipeline { ... }
```

**Step 2：创建 lifecycle.rs（~120 行）**

包含：`done()`、`interrupt()`、`begin_round()`、`clear()`、`shrink_to_fit()`、`set_completed()`、`restore_completed()`、`completed_messages()`、`completed_stats()` 及所有查询方法（`has_streaming_content`、`has_pending_tool_calls`、`in_subagent`、`has_snapshot_this_round`、`frozen_subagent_vms_count`、`frozen_subagent_vms_mut`）。

**Step 3：修改 mod.rs**

- 删除对应的代码块
- 添加 `mod subagent;` 和 `mod lifecycle;`
- 对 `SubAgentState` 和 `BatchInfo` 添加 `pub(crate) use subagent::{SubAgentState, BatchInfo};` 以保持 reconcile.rs 访问
- mod.rs 现在仅保留：`MessagePipeline` struct 定义、`new()`、`cwd()`、`handle_event()`、`build_rebuild_all()`（保持 inline 或移入 transform.rs）、以及所有 `mod` 声明 + re-exports

**Step 4：构建验证**

```bash
cargo check -p peri-tui 2>&1
cargo test -p peri-tui --lib -- message_pipeline 2>&1
```

Expected: 编译通过，已有测试全部 pass。

**Step 5：提交**

```bash
git add peri-tui/src/app/message_pipeline/
git commit -m "refactor(message_pipeline): extract subagent + lifecycle modules (-400 lines)

Split subagent routing, notify_bg_completed, and lifecycle methods into
dedicated modules. mod.rs reduced from 1187 to ~350 lines.

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

## Part B：acp_stdio.rs（988 → 约 200）

### 目标结构

```
acp_stdio/
├── mod.rs             # ~200 行：run_acp_stdio() 骨架 + Agent.builder() 链 + re-exports
├── context.rs         # ~80 行：SessionInfo + StdioContext + StdioBroker
├── init.rs            # ~140 行：init_stdio_context() 初始化函数
└── handlers.rs        # ~750 行：16 个 handler 提取为命名函数（可后续进一步拆分）
```

### 外部消费者

| 调用点 | 使用 |
|--------|------|
| `main.rs:30` | `mod acp_stdio;` |
| `main.rs:364` | `acp_stdio::run_acp_stdio(cwd)` |

仅一处调用，改动范围极小。

---

### Task B1：创建目录 + 提取 context.rs

**Files:**
- Create: `peri-tui/src/acp_stdio/mod.rs`
- Create: `peri-tui/src/acp_stdio/context.rs`
- Delete: `peri-tui/src/acp_stdio.rs`

**Step 1：移动现有文件到 mod.rs**

```bash
mkdir -p peri-tui/src/acp_stdio
mv peri-tui/src/acp_stdio.rs peri-tui/src/acp_stdio/mod.rs
```

**Step 2：创建 context.rs**

```rust
//! ACP Stdio 传输的共享上下文。

use std::sync::Arc;

use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

use crate::app::service_registry::ServiceRegistry;
use crate::app::ui_state::UiState;

/// 每个 stdio session 的运行时状态
pub(super) struct SessionInfo {
    pub(super) session_id: String,
    pub(super) cwd: String,
    pub(super) history: Vec<peri_agent::messages::BaseMessage>,
    pub(super) cancel_token: CancellationToken,
    pub(super) frozen: Option<FrozenSessionData>,
    pub(super) agent_pool: Option<Arc<peri_acp::session::agent_pool::AgentPool>>,
    // ... 其余字段
}

/// Stdio 传输环境的共享上下文（跨 session）
pub(super) struct StdioContext {
    pub(super) provider: Arc<parking_lot::RwLock<ProviderSnapshot>>,
    pub(super) config: Arc<parking_lot::RwLock<peri_config::PeriConfig>>,
    // ... MCP、plugin、hooks、langfuse 等字段
    pub(super) service_registry: Arc<ServiceRegistry>,
    pub(super) ui_state: Arc<RwLock<UiState>>,
}

/// Stdio 交互执行器（始终批准）
pub(super) struct StdioBroker;

impl Broker for StdioBroker { /* ... */ }
```

**Step 3：修改 mod.rs**

- 在 mod.rs 顶部添加 `mod context;`
- 用 `use context::{SessionInfo, StdioContext, StdioBroker};` 替换原有的 struct 定义
- 删除 `SessionInfo`、`StdioContext`、`StdioBroker` 的 inline 定义

**Step 4：构建验证**

```bash
cargo check -p peri-tui 2>&1
```

Expected: 编译通过。

**Step 5：提交**

```bash
git add peri-tui/src/acp_stdio/
git rm peri-tui/src/acp_stdio.rs 2>/dev/null
git commit -m "refactor(acp_stdio): split into directory module, extract context.rs

Move StdioContext, SessionInfo, StdioBroker to separate context module.

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task B2：提取 init.rs

**Files:**
- Create: `peri-tui/src/acp_stdio/init.rs`
- Modify: `peri-tui/src/acp_stdio/mod.rs`

**Step 1：创建 init.rs**

将 mod.rs 中 `run_acp_stdio()` 的 86-213 行初始化逻辑提取为独立函数：

```rust
//! ACP Stdio 环境的初始化逻辑。

use std::sync::Arc;

use crate::app::service_registry::ServiceRegistry;
// ... 其他 imports

use super::context::StdioContext;

/// 初始化 ACP Stdio 运行环境，返回共享上下文。
pub(super) async fn init_stdio_context(cwd: String) -> anyhow::Result<Arc<StdioContext>> {
    // 1. 加载 peri_config + provider
    let peri_config = crate::config::load_config_with_defaults()?;
    let provider = crate::app::provider::create_provider_from_config(&peri_config)?;

    tracing::info!("provider={:?}, model={}, cwd={}", ...);

    // 2. cron scheduler
    let cron = crate::app::cron_ops::create_cron_scheduler()?;

    // 3. MCP 连接池（后台初始化）
    let mcp_pool = ...;

    // 4. 插件数据 + hook groups
    let plugin_data = ...;
    let hook_groups = ...;

    // 5. permission_mode, tool_search_index, shared_tools
    let permission_mode = ...;

    // 6. thread store + langfuse session
    let thread_store = ...;
    let langfuse_session = ...;

    let ctx = StdioContext {
        provider: Arc::new(parking_lot::RwLock::new(provider)),
        config: Arc::new(parking_lot::RwLock::new(peri_config)),
        // ... 全部字段
    };

    Ok(Arc::new(ctx))
}
```

**Step 2：修改 mod.rs**

- 添加 `mod init;`
- 在 `run_acp_stdio()` 中，将行 86-213 替换为：

```rust
let ctx = init::init_stdio_context(cwd).await?;
```

**Step 3：构建验证**

```bash
cargo check -p peri-tui 2>&1
```

Expected: 编译通过。

**Step 4：提交**

```bash
git add peri-tui/src/acp_stdio/
git commit -m "refactor(acp_stdio): extract initialization to init.rs (-130 lines)

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task B3：提取 handlers.rs（可选，高风险）

此任务为可选——提取 handler 闭包为命名函数。由于 `agent_client_protocol` SDK 的 `on_receive_request()` 要求闭包形式，此步骤不影响 mod.rs 的行数（handler 闭包仍需在 builder 链中注册），但能将 handler 的实现逻辑移到独立文件。

**策略**：每个 handler 提取为返回闭包的工厂函数：

```rust
// handlers.rs
use super::context::StdioContext;
use agent_client_protocol::types::*;

pub(super) fn handle_session_prompt(
    ctx: Arc<StdioContext>,
) -> impl Fn(PromptRequest, Responder, Cx) -> impl Future<Output = ()> + Send + 'static {
    move |req, responder, cx| {
        let ctx = ctx.clone();
        async move {
            // ... 171 行实现
        }
    }
}
```

**风险**：SDK 的闭包类型约束可能导致编译失败。如果失败，跳过此 Task 不影响前两个 Task 的效果（mod.rs 已从 988 → ~800）。

---

## Part C：message_view/mod.rs（917 → 约 450）

### 目标结构

```
ui/message_view/
├── mod.rs             # ~200 行：mod 声明 + re-exports + MessageViewModel enum 定义 + trait impls
├── build.rs           # NEW：from_base_message_with_cwd 转换逻辑（~250 行）
├── builders.rs        # NEW：工厂函数 user()/assistant()/tool_block()/system()/etc.（~110 行）
├── aggregate.rs       # 已有（不变）
├── tools.rs           # 已有（不变）
├── utils.rs           # 已有（不变）
└── message_view_test.rs  # 已有（不变）
```

### 外部消费者（需保持兼容）

大量文件通过 `use crate::ui::message_view::*` 导入，因此所有 pub 类型必须从 mod.rs 重导出：

- `MessageViewModel` enum（7 变体全部 pub）
- `ContentBlockView` enum（3 变体全部 pub）
- `tool_color`、`AgentSummary`、`ToolCategory`、`ToolEntry`
- `aggregate_batch_groups`、`aggregate_tool_groups`、`aggregate_tail_tool_groups`
- `instance_hash`、`parse_bg_hash`

---

### Task C1：提取 build.rs（from_base_message_with_cwd）

**Files:**
- Create: `peri-tui/src/ui/message_view/build.rs`
- Modify: `peri-tui/src/ui/message_view/mod.rs`

**Step 1：创建 build.rs**

将 `from_base_message` 和 `from_base_message_with_cwd`（行 460-701，约 242 行）完整移动到 build.rs：

```rust
//! 从 BaseMessage 到 MessageViewModel 的转换逻辑。

use peri_agent::messages::{BaseMessage, ContentBlock};

use super::MessageViewModel;
use super::tools::{tool_color, parse_subagent_tool_count};
use crate::ui::markdown::parse_markdown_default;

impl MessageViewModel {
    /// 从 BaseMessage 转换为视图模型（向后兼容，cwd 为 None）
    pub fn from_base_message(
        msg: &BaseMessage,
        prev_ai_tool_calls: &[(String, String, serde_json::Value)],
    ) -> Self {
        Self::from_base_message_with_cwd(msg, prev_ai_tool_calls, None)
    }

    /// 从 BaseMessage 转换为视图模型（带 cwd 上下文）
    pub fn from_base_message_with_cwd(
        msg: &BaseMessage,
        prev_ai_tool_calls: &[(String, String, serde_json::Value)],
        cwd: Option<&str>,
    ) -> Self {
        // ... 现有 242 行实现，完整拷贝、不修改
    }
}
```

**Step 2：修改 mod.rs**

- 添加 `mod build;`
- 删除行 460-701 的 `from_base_message*` 实现
- 添加 `pub use build::*;`（或显式 re-export `from_base_message*`）

注意：`from_base_message_with_cwd` 在 Ai 分支中调用了 `ContentBlockView::from_block()`、`MessageViewModel::SubAgentGroup { .. }` 等，这些都在 mod.rs 的类型定义中，build.rs 通过 `use super::*` 访问。

**Step 3：同时移动 build_diff_lines**

`build_diff_lines`（行 22-75，53 行）属于渲染层，移到 `build.rs` 作为 `pub(super)` 函数（如果它被 mod.rs 的其他方法调用），或等待后续完全移到 `message_render.rs`。

如果 `build_diff_lines` 被 `from_base_message_with_cwd` 内部调用（Tool 分支），它应该随同移动。

检查方式：

```bash
grep -n "build_diff_lines" peri-tui/src/ui/message_view/mod.rs
```

如果调用点在移动的代码块内，直接移动到 build.rs。如果被外部模块调用，添加 pub(super) re-export。

**Step 4：构建验证**

```bash
cargo check -p peri-tui 2>&1
```

Expected: 编译通过。

**Step 5：提交**

```bash
git add peri-tui/src/ui/message_view/
git commit -m "refactor(message_view): extract from_base_message conversion to build.rs (-245 lines)

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task C2：提取 builders.rs（工厂函数）

**Files:**
- Create: `peri-tui/src/ui/message_view/builders.rs`
- Modify: `peri-tui/src/ui/message_view/mod.rs`

**Step 1：创建 builders.rs**

将工厂函数（行 770-873，约 104 行）移动到 builders.rs：

```rust
//! MessageViewModel 工厂函数。

use ratatui::style::Color;
use ratatui::text::{Line, Text};

use super::MessageViewModel;

impl MessageViewModel {
    pub fn user(text: String) -> Self { /* ... 行 771-779 */ }
    pub fn assistant(rendered: Text<'static>, raw: String) -> Self { /* ... 行 782-800 */ }
    pub fn tool_block(/* ... */) -> Self { /* ... */ }
    pub fn tool_block_with_id(/* ... */) -> Self { /* ... */ }
    pub fn system(text: String) -> Self { /* ... */ }
    pub fn cache_warning(text: String) -> Self { /* ... */ }
    pub fn subagent_group(/* ... */) -> Self { /* ... 行 835-872 */ }
}
```

注意 `subagent_group` 构造函数（约 38 行，18 个字段）是最大的工厂函数。

**Step 2：修改 mod.rs**

- 添加 `mod builders;`
- 删除行 770-873
- 添加 `pub use builders::*;`

**Step 3：构建验证**

```bash
cargo check -p peri-tui 2>&1
```

Expected: 编译通过。

**Step 4：提交**

```bash
git add peri-tui/src/ui/message_view/
git commit -m "refactor(message_view): extract factory functions to builders.rs (-105 lines)

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task C3（可选）：宏驱动消除 4×7 模式重复

`PartialEq`（113 行）、`Hash`（97 行）、`content_hash()`（42 行）、`recompute_hash()`（27 行）在 7 个 `MessageViewModel` 变体上重复匹配模式。可以用宏消除。

```rust
// mod.rs 中定义宏
macro_rules! for_each_vm_variant {
    ($self:ident, $pattern:pat => $body:expr) => {
        match $self {
            MessageViewModel::UserBubble { .. } $pattern => $body,
            MessageViewModel::AssistantBubble { .. } $pattern => $body,
            MessageViewModel::ToolBlock { .. } $pattern => $body,
            MessageViewModel::SystemNote { .. } $pattern => $body,
            MessageViewModel::CacheWarning { .. } $pattern => $body,
            MessageViewModel::ToolCallGroup { .. } $pattern => $body,
            MessageViewModel::SubAgentGroup { .. } $pattern => $body,
        }
    };
}
```

然后用宏简化 `content_hash()`：

```rust
pub fn content_hash(&self) -> u64 {
    for_each_vm_variant!(self, { content_hash, .. } => *content_hash)
}
```

**此 task 可选**，因为宏改造虽然减少行数但不改变语义。优先完成 Task C1 + C2。

---

## 验证清单

全部完成后的检查：

```bash
# 编译检查
cargo build -p peri-tui 2>&1

# 测试
cargo test -p peri-tui --lib 2>&1

# Clippy
cargo clippy -p peri-tui 2>&1 | grep -E 'warning|error'

# 确认 line count
wc -l peri-tui/src/app/message_pipeline/mod.rs
wc -l peri-tui/src/acp_stdio/mod.rs
wc -l peri-tui/src/ui/message_view/mod.rs
```

Expected: 三个 mod.rs 各 ≤400 行，所有测试 pass，clippy 零 warning。

---

## 风险评估

| 文件 | 风险 | 缓解 |
|------|------|------|
| message_pipeline | 中：throttle.rs 的 `impl MessagePipeline` 分散在多个文件，编译器需全部找到 | 每个子模块用 `impl super::MessagePipeline` 而非 `impl MessagePipeline` |
| acp_stdio | 低：仅 main.rs 一处调用，目录化后 import 路径不变 | `pub use acp_stdio::run_acp_stdio` 在 crate root |
| message_view | 低：已有子模块骨架（aggregate/tools/utils），外部 API 完全通过 mod.rs 重导出 | 所有 pub fn/enum 从 mod.rs pub use |

## 实施顺序建议

Part A（message_pipeline）和 Part C（message_view）是两个独立子系统，可并行实施。Part B（acp_stdio）也可独立。

推荐顺序（按风险递增）：
1. **Part C**（message_view）— 风险最低，已有拆分骨架
2. **Part B Task B1+B2**（acp_stdio）— 仅 main.rs 一处调用
3. **Part A**（message_pipeline）— 最多消费者，需仔细验证 re-exports
