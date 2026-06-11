# acp_stdio 职责拆解 — 实施计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 acp_stdio/mod.rs 的 787 行 handler 按领域职责拆解为 11 个小文件，最大单文件 ≤130 行，消除 7 处重复代码。

**Architecture:** 两步走——先消除重复（freeze / notification / model / commands 四个辅助模块），再按业务域拆分 handler（session/ 子目录：create / prompt + prompt_exec / config / control）。mod.rs 退化为 ~60 行的薄注册层。

**Tech Stack:** Rust 2021, tokio async, agent-client-protocol SDK

---

## 现状分析

### 6 类重复模式

| 模式 | 出现次数 | 相同代码行数 | 位置 |
|------|---------|------------|------|
| frozen_data 构建 | **4×** | 9 行 | new:73-83, resume:502-510, load:545-553, fork:665-673 |
| config_options 通知 | **4×** | 10 行 | set_mode:341-350, set_model:378-387, set_config_option:442-451, update_config:750-758 |
| model 切换 + pool 失效 | **2×** | 17 行 | set_model:361-377, set_config_option model 分支:408-424 |
| AvailableCommands 通知 | **2×** | 9 行 | new:86-131(含 scan), load:602-612 |
| SessionInfo 构造 | **4×** | 9 行 | new:95-105, resume:513-524, load:569-580, fork:676-687 |
| modes + models + config_options 响应组装 | **3×** | 10 行 | new:108-124, load:584-598, 以及内部多处 |

### 目标文件结构

```
acp_stdio/
├── mod.rs              # ~60 行：Agent.builder() 薄注册
├── context.rs          # 已有（99 行）
├── init.rs             # 已有（147 行）
├── freeze.rs           # NEW：~35 行，消除 frozen_data 4× 重复
├── notification.rs     # NEW：~25 行，消除 config_options 通知 4× 重复
├── model.rs            # NEW：~30 行，消除 model 切换 2× 重复
├── commands.rs         # NEW：~25 行，消除 AvailableCommands 通知 2× 重复
├── session/
│   ├── mod.rs          # ~10 行：re-exports
│   ├── create.rs       # ~130 行：new / load / resume / fork 共享入口
│   ├── prompt.rs       # ~50 行：prompt 薄入口 → spawn prompt_exec
│   ├── prompt_exec.rs  # ~80 行：executor::execute_prompt + 持久化 + 响应发送
│   ├── config.rs       # ~110 行：set_mode / set_model / set_config_option / update_config
│   └── control.rs      # ~50 行：list / cancel / close
└── transport.rs        # ~40 行：initialize 响应 + type:cancel 钩子
```

---

## Task 1：freeze.rs — 消除 frozen_data 重复（4 处）

**Files:**
- Create: `peri-tui/src/acp_stdio/freeze.rs`
- Modify: `peri-tui/src/acp_stdio/mod.rs`（new / resume / load / fork 四处替换）

### 重复的 4 处位置

| 行号 | handler | 
|------|---------|
| 73-83 | session/new |
| 502-510 | session/resume |
| 545-553 | session/load |
| 665-673 | session/fork |

全部是：
```rust
let frozen_date = chrono::Local::now().format("%Y-%m-%d").to_string();
let frozen_language = ctx.peri_config.read().config.language.clone();
let frozen_data = peri_acp::session::frozen::build_frozen_session_data(
    &cwd_str,
    frozen_language.as_deref(),
    &ctx.plugin_skill_dirs,
    &ctx.plugin_agent_dirs,
    &frozen_date,
);
```

- [ ] **Step 1：创建 freeze.rs**

```rust
//! 构建 frozen session data，供 session/new、load、resume、fork 复用。

use std::sync::Arc;

use super::context::StdioContext;

/// 构建会话级别的冻结数据（system prompt / skills / CLAUDE.md 等）。
pub(super) fn build(ctx: &StdioContext, cwd: &str) -> peri_acp::session::frozen::FrozenSessionData {
    let frozen_date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let frozen_language = ctx.peri_config.read().config.language.clone();
    peri_acp::session::frozen::build_frozen_session_data(
        cwd,
        frozen_language.as_deref(),
        &ctx.plugin_skill_dirs,
        &ctx.plugin_agent_dirs,
        &frozen_date,
    )
}
```

- [ ] **Step 2：替换 mod.rs 中 4 处调用**

在 mod.rs 顶部添加 `mod freeze;`，然后将每处 9 行替换为：
```rust
let frozen_data = freeze::build(&ctx, &cwd_str);
```
cwd 变量名在不同 handler 中可能有 `cwd_str`、`cwd` 等差异，确保传入一致。

- [ ] **Step 3：构建验证**

```bash
cd /Users/konghayao/code/ai/perihelion && cargo check -p peri-tui 2>&1
```
Expected: 通过。

- [ ] **Step 4：运行测试**

```bash
cargo test -p peri-tui --lib 2>&1 | tail -5
```

- [ ] **Step 5：提交**

```bash
git add peri-tui/src/acp_stdio/ && git commit -m "refactor(acp_stdio): extract freeze.rs — eliminate 4× frozen_data duplication

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

## Task 2：notification.rs — 消除 config_options 通知重复（4 处）

**Files:**
- Create: `peri-tui/src/acp_stdio/notification.rs`
- Modify: `peri-tui/src/acp_stdio/mod.rs`

### 重复的 4 处位置

| 行号 | handler |
|------|---------|
| 341-350 | set_mode |
| 378-387 | set_model |
| 442-451 | set_config_option |
| 750-758 | update_config |

全部是：
```rust
let config_options = {
    let c = ctx.peri_config.read();
    let p = ctx.provider.read();
    dispatch::config_update::make_config_options(&c, &p, ctx.permission_mode.load())
};
let notif = SessionNotification::new(
    req.session_id.clone(),
    SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(config_options)),
);
let _ = cx.send_notification(notif);
```

其中 `set_config_option` 变体在响应体中附加 `config_options`：
```rust
responder.respond(SetSessionConfigOptionResponse::new(config_options))
```

- [ ] **Step 1：创建 notification.rs**

```rust
//! 发送 config_options 更新通知，供 set_mode / set_model / set_config_option / update_config 复用。

use std::sync::Arc;

use agent_client_protocol::{
    schema::{
        ConfigOptionUpdate, SessionId, SessionNotification, SessionUpdate,
    },
    Client, ConnectionTo,
};

use super::context::StdioContext;

/// 构建并发送 ConfigOptionUpdate 通知。返回 config_options 列表供响应体使用。
pub(super) fn send_config_update(
    ctx: &StdioContext,
    session_id: &SessionId,
    cx: &ConnectionTo<Client>,
) -> Vec<agent_client_protocol::schema::SessionConfigOption> {
    let c = ctx.peri_config.read();
    let p = ctx.provider.read();
    let options = peri_acp::dispatch::config_update::make_config_options(&c, &p, ctx.permission_mode.load());
    let notif = SessionNotification::new(
        session_id.clone(),
        SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(options.clone())),
    );
    let _ = cx.send_notification(notif);
    options
}
```

- [ ] **Step 2：替换 mod.rs 中 4 处调用**

每处替换为：
```rust
let _config_options = notification::send_config_update(&ctx, &req.session_id, &cx);
```
`set_config_option` handler 中进一步使用返回的 options：
```rust
responder.respond(SetSessionConfigOptionResponse::new(_config_options))
```

- [ ] **Step 3：构建 + 测试 + 提交**

```bash
cargo check -p peri-tui && cargo test -p peri-tui --lib
git add peri-tui/src/acp_stdio/ && git commit -m "refactor(acp_stdio): extract notification.rs — eliminate 4× config_options duplication

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

## Task 3：model.rs + commands.rs — 消除剩余重复

**Files:**
- Create: `peri-tui/src/acp_stdio/model.rs`
- Create: `peri-tui/src/acp_stdio/commands.rs`
- Modify: `peri-tui/src/acp_stdio/mod.rs`

### model.rs — model 切换（2 处重复，set_model:361-377 + set_config_option 的 model 分支:408-424）

```rust
//! 模型切换辅助，供 set_model / set_config_option 的 model 分支复用。

use super::context::StdioContext;

/// 切换模型并失效缓存。返回切换后的模型名（或 None 表示切换失败）。
pub(super) fn switch_model(ctx: &StdioContext, sid: &str, model_id: &str) -> Option<String> {
    let new_provider = {
        let cfg = ctx.peri_config.read();
        peri_tui::app::agent::LlmProvider::from_config_for_alias(&cfg, model_id)
    };
    let name = new_provider.as_ref().map(|p| p.model_name().to_string());
    if let Some(p) = new_provider {
        tracing::info!(model_id = %model_id, model = %p.model_name(), "Model changed");
        *ctx.provider.write() = p;
    }
    // Invalidate cached LLM instances
    let mut sessions = ctx.sessions.write();
    if let Some(s) = sessions.get_mut(sid) {
        s.agent_pool.invalidate();
    }
    name
}
```

替换 set_model（行 361-377）和 set_config_option model 分支（行 408-424）为：
```rust
let _ = model::switch_model(&ctx, &sid, model_id);
```

### commands.rs — AvailableCommands 通知（2 处重复，new:86-131 + load:602-612）

```rust
//! AvailableCommands 通知辅助，供 session/new 和 session/load 复用。

use std::sync::Arc;
use agent_client_protocol::{
    schema::{
        AvailableCommandsUpdate, SessionId, SessionNotification, SessionUpdate,
    },
    Client, ConnectionTo,
};

use super::context::StdioContext;

/// 扫描 skill 目录并发送 AvailableCommandsUpdate 通知。
pub(super) fn send_available_commands(
    cwd: &str,
    plugin_skill_dirs: &[std::path::PathBuf],
    session_id: &SessionId,
    cx: &ConnectionTo<Client>,
) {
    let skill_dirs = peri_middlewares::SkillsMiddleware::resolve_dirs_static(cwd, plugin_skill_dirs);
    let skills = peri_middlewares::skills::list_skills(&skill_dirs);
    let cmds = peri_acp::dispatch::build_available_commands(&skills);
    let notif = SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(cmds)),
    );
    let _ = cx.send_notification(notif);
}
```

替换 new（行 85-131 的 skill scan + notification 部分）和 load（行 602-612）为：
```rust
commands::send_available_commands(&cwd_str, &ctx.plugin_skill_dirs, &session_id, &cx);
```

- [ ] **Step 1：创建 model.rs 和 commands.rs**
- [ ] **Step 2：替换 mod.rs 中的各处调用**
- [ ] **Step 3：构建 + 测试 + 提交**

```bash
cargo check -p peri-tui && cargo test -p peri-tui --lib
git add peri-tui/src/acp_stdio/ && git commit -m "refactor(acp_stdio): extract model.rs + commands.rs — eliminate 2× model switch + 2× commands duplication

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

## Task 4：transport.rs — 提取 initialize + type:cancel 钩子

**Files:**
- Create: `peri-tui/src/acp_stdio/transport.rs`
- Modify: `peri-tui/src/acp_stdio/mod.rs`

**提取内容**：
1. `initialize` handler（行 50-55）
2. `type:cancel` 钩子（行 772-783，`Stdio::new().with_debug(...)` 回调）

```rust
//! 传输层事件：initialize 响应 + type:cancel 中断钩子。

use std::sync::Arc;
use super::context::StdioContext;

/// 构建 initialize 响应的 handler 闭包。
pub(super) fn initialize_handler() -> impl FnOnce(
    agent_client_protocol::schema::InitializeRequest, 
    agent_client_protocol::Responder, 
    agent_client_protocol::ConnectionTo<agent_client_protocol::Client>,
) -> Result<(), agent_client_protocol::Error> + Send + 'static {
    |_req, responder, _cx| {
        tracing::info!("ACP initialize");
        responder.respond(peri_acp::dispatch::build_initialize_response());
        Ok(())
    }
}
// 注意：实际类型参数需从 mod.rs 中精确复制，以上为示意
```

```rust
/// 构建 type:cancel 中断钩子。
pub(super) fn cancel_debug_hook(
    ctx: Arc<StdioContext>,
) -> impl FnMut(&str, agent_client_protocol_tokio::LineDirection) {
    move |line: &str, _direction| {
        if line.trim() == r#"{"type":"cancel"}"# {
            let guard = ctx.sessions.read();
            for (sid, s) in guard.iter() {
                if let Some(ref token) = s.cancel_token {
                    token.cancel();
                    tracing::info!(session_id = %sid, "Cancelled via type:cancel");
                }
            }
        }
    }
}
```

- [ ] **Step 1：创建 transport.rs**
- [ ] **Step 2：mod.rs 顶部加 `mod transport;`，替换 initialize handler 和 cancel 钩子**
- [ ] **Step 3：验证 `.connect_to(Stdio::new().with_debug(transport::cancel_debug_hook(ctx.clone())))` 编译通过**
- [ ] **Step 4：构建 + 测试 + 提交**

---

## Task 5：session/ 目录 — 创建目录骨架 + control.rs

**Files:**
- Create: `peri-tui/src/acp_stdio/session/mod.rs`
- Create: `peri-tui/src/acp_stdio/session/control.rs`
- Modify: `peri-tui/src/acp_stdio/mod.rs`

**Step 1：创建 session/mod.rs**

```rust
//! Session 级 handler：create / prompt / config / control。

pub mod control;
// 后续 task 逐步添加：pub mod create; pub mod prompt; pub mod prompt_exec; pub mod config;
```

**Step 2：提取 control.rs — session/list + cancel + close**

这三个 handler 职责简单（查询 / 取消 / 删除），合计 ~60 行。每个提取为 pub(super) 函数，接受 `Arc<StdioContext>`：

```rust
//! 会话控制：list / cancel / close。

use std::sync::Arc;
use super::super::context::StdioContext;

/// 处理 session/list 请求
pub(super) fn list_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    move |req: ListSessionsRequest, responder, _cx: ConnectionTo<Client>| {
        let ctx = ctx.clone();
        async move {
            let cwd_filter = req.cwd.as_ref().map(|p| p.to_string_lossy().to_string());
            let entries = peri_acp::dispatch::list_sessions_as_info(
                ctx.thread_store.as_ref(), cwd_filter.as_deref(),
            ).await.unwrap_or_else(|e| {
                tracing::warn!(error = %e, "session/list: failed");
                Vec::new()
            });
            let _ = responder.respond(ListSessionsResponse::new(entries));
            Ok(())
        }
    }
}

/// 处理 session/cancel 通知
pub(super) fn cancel_handler(
    ctx: Arc<StdioContext>,
) -> impl FnMut(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 458-474 复制
}

/// 处理 session/close 请求
pub(super) fn close_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 476-493 复制
}
```

> **注意**：闭包返回类型需精确匹配 SDK 签名。从 mod.rs 中复制完整的类型标注。

- [ ] **Step 3：mod.rs 顶部加 `mod session;`，将 list/cancel/close 的闭包替换为 control 函数调用**
- [ ] **Step 4：验证编译**
- [ ] **Step 5：提交**

```bash
git add peri-tui/src/acp_stdio/ && git commit -m "refactor(acp_stdio): create session/ dir + extract control.rs (list/cancel/close)

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

## Task 6：session/config.rs — 提取 4 个配置 handler

**Files:**
- Create: `peri-tui/src/acp_stdio/session/config.rs`
- Modify: `peri-tui/src/acp_stdio/session/mod.rs`（加 `pub mod config;`）
- Modify: `peri-tui/src/acp_stdio/mod.rs`

**配置 handler 清单**：set_mode (~25 行)、set_model (~35 行)、set_config_option (~60 行)、update_config (~70 行)

```rust
//! 会话配置：set_mode / set_model / set_config_option / update_config。

use std::sync::Arc;
use super::super::{context::StdioContext, model, notification};

pub(super) fn set_mode_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 333-355 提取，内部调用 notification::send_config_update
}

pub(super) fn set_model_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 357-392 提取，内部调用 model::switch_model + notification::send_config_update
}

pub(super) fn set_config_option_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 394-456 提取，match config_id { "mode" | "model" | "thinking_effort" | _ }
}

pub(super) fn update_config_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 698-768 提取
}
```

- [ ] **Step 1：创建 session/config.rs，逐个提取 4 个 handler 工厂函数**
- [ ] **Step 2：mod.rs 替换：`.on_receive_request(session::config::set_mode_handler(ctx.clone()), ...)` 等**
- [ ] **Step 3：构建 + 测试 + 提交**

---

## Task 7：session/create.rs + session/prompt.rs + session/prompt_exec.rs

**Files:**
- Create: `peri-tui/src/acp_stdio/session/create.rs`（~130 行）
- Create: `peri-tui/src/acp_stdio/session/prompt.rs`（~50 行）
- Create: `peri-tui/src/acp_stdio/session/prompt_exec.rs`（~80 行）
- Modify: `peri-tui/src/acp_stdio/session/mod.rs`
- Modify: `peri-tui/src/acp_stdio/mod.rs`

### create.rs — new / load / resume / fork 共享入口

四个 handler 共享以下模式：
1. freeze::build() 构建 frozen_data
2. 构造 SessionInfo → sessions.insert()
3. 对于 new / load：构建 modes + models + config_options → 响应
4. 对于 new / load：commands::send_available_commands()

提取为 4 个 handler 工厂函数。因它们的结构相似（都用 freeze / commands / SessionInfo 构造），可进一步提取私有辅助函数 `insert_session(sessions, sid, SessionInfo)`。但不过度抽象——先提取 4 个独立函数。

```rust
//! Session 创建：new / load / resume / fork。

use std::sync::Arc;
use super::super::{context::{SessionInfo, StdioContext}, freeze, commands};

pub(super) fn new_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 57-136 提取，内部调用 freeze::build + commands::send_available_commands
}

pub(super) fn load_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 536-617 提取
}

pub(super) fn resume_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 494-534 提取
}

pub(super) fn fork_handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 618-696 提取
}
```

### prompt.rs — 薄入口（~50 行）

只做三件事：参数转换 → spawn → 返回 Ok(())。

```rust
//! Prompt 入口：参数转换 + tokio::spawn。

use std::sync::Arc;
use super::super::context::StdioContext;
use super::prompt_exec;

pub(super) fn handler(
    ctx: Arc<StdioContext>,
) -> impl FnOnce(...) -> Result<(), ...> + Send + 'static {
    // 从 mod.rs 行 165-331 的薄入口部分提取
    // 保留：content 转换、session 数据捕获、cancel_token 设置、pool 提取
    // 最后的 tokio::spawn(async move { prompt_exec::run(...).await }) 替换原来的 80 行执行体
}
```

### prompt_exec.rs — 执行管线（~80 行）

纯业务逻辑，不碰闭包：`pub(super) async fn run(params: PromptExecParams) { ... }`

```rust
//! Prompt 执行管线：executor::execute_prompt → 持久化 → 响应发送。

use std::sync::Arc;
use super::super::context::StdioContext;

pub(super) struct PromptExecParams {
    pub ctx: Arc<StdioContext>,
    pub sid: String,
    pub thread_id: String,
    pub agent_cwd: String,
    pub content: peri_agent::messages::MessageContent,
    pub frozen: Option<FrozenSessionData>,
    pub history: Vec<BaseMessage>,
    pub is_empty_history: bool,
    pub cancel: AgentCancellationToken,
    pub pool: Arc<parking_lot::Mutex<AgentPool>>,
    pub event_sink: Arc<StdioEventSink>,
    pub broker: Arc<dyn UserInteractionBroker>,
    pub session_id: agent_client_protocol::schema::SessionId,
    pub responder: agent_client_protocol::Responder,
    pub cx: ConnectionTo<Client>,
}

pub(super) async fn run(params: PromptExecParams) {
    // 从 mod.rs 的 tokio::spawn 内部（行 242-323）提取
    // provider_snapshot + config_snapshot → executor::execute_prompt
    // → pool 恢复 → 持久化 → 内存更新 → 响应 → SessionInfoUpdate
}
```

- [ ] **Step 1：创建 session/create.rs、session/prompt.rs、session/prompt_exec.rs**
- [ ] **Step 2：更新 session/mod.rs 添加三个 pub mod 声明**
- [ ] **Step 3：mod.rs 替换对应的 handler 注册**
- [ ] **Step 4：特别注意 prompt_exec.rs 的参数传递——从闭包的捕获变量转为显式 struct 参数**
- [ ] **Step 5：构建 + 测试 + 提交**

```bash
git add peri-tui/src/acp_stdio/ && git commit -m "refactor(acp_stdio): split session handlers into create / prompt / prompt_exec

Split new/load/resume/fork into create.rs, prompt entry into prompt.rs,
executor bridge into prompt_exec.rs. mod.rs reduced to thin registration layer.

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

## Task 8：mod.rs 收尾 — 清理 + 尺寸验证

**Files:**
- Modify: `peri-tui/src/acp_stdio/mod.rs`

- [ ] **Step 1：确认 mod.rs 仅包含**
  - `mod` 声明：context, init, freeze, notification, model, commands, transport, session
  - `run_acp_stdio()` 骨架：`let ctx = init::init_stdio_context(cwd).await?;` → `Agent.builder()` 链式注册 → `.connect_to(...)` → `.await`
  - 每个 handler 注册为一行调用
- [ ] **Step 2：删除所有 handler 内联实现后的残留死导入**
- [ ] **Step 3：运行完整验证**

```bash
cd /Users/konghayao/code/ai/perihelion
cargo build -p peri-tui 2>&1
cargo test -p peri-tui --lib 2>&1 | tail -5
cargo clippy -p peri-tui --lib 2>&1 | grep -E 'warning|error'
wc -l peri-tui/src/acp_stdio/mod.rs
```

Expected: mod.rs ≤ 60 行，tests 642 passed，clippy 零 warning。

- [ ] **Step 4：提交**

```bash
git add peri-tui/src/acp_stdio/ && git commit -m "chore(acp_stdio): cleanup mod.rs — thin registration layer (~60 lines)

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

## 验证清单

全部 task 完成后：

```bash
# 编译
cargo build -p peri-tui 2>&1

# 测试
cargo test -p peri-tui --lib 2>&1

# Clippy
cargo clippy -p peri-tui --lib 2>&1 | grep -E 'warning|error'

# 文件计数
wc -l peri-tui/src/acp_stdio/mod.rs
wc -l peri-tui/src/acp_stdio/freeze.rs
wc -l peri-tui/src/acp_stdio/notification.rs
wc -l peri-tui/src/acp_stdio/model.rs
wc -l peri-tui/src/acp_stdio/commands.rs
wc -l peri-tui/src/acp_stdio/transport.rs
wc -l peri-tui/src/acp_stdio/session/*.rs
```

Expected: 所有单文件 ≤130 行，mod.rs ≤60 行，642 tests pass。

---

## 风险评估

| 风险 | 缓解 |
|------|------|
| prompt_exec.rs 的 `PromptExecParams` 参数过多（~14 个字段） | 当前可接受——这是 prompt 入口和执行的明确契约。如果未来膨胀，再考虑 Builder 或拆分为 prepare / execute / persist 三步 |
| 闭包工厂函数的返回类型依赖 SDK 内部类型 | 从 mod.rs 逐个复制精确的类型签名；如果某个 handler 的类型推导失败，先跳过该 handler，其余照常提取 |
| session/config.rs 的 4 个 handler 依赖 model / notification 辅助模块 | Task 1-3 先完成辅助模块，Task 6 的 config 自然可用 |

## 实施顺序

1. Task 1 (freeze) → Task 2 (notification) → Task 3 (model + commands)：先消除所有重复
2. Task 4 (transport)：提取传输层，独立于 session
3. Task 5 (session/ 骨架 + control)：建立目录结构，控制 handler 最简单先打样
4. Task 6 (session/config)：配置 handler，相对独立
5. Task 7 (session/create + prompt + prompt_exec)：最大的拆分，最后做
6. Task 8 (mod.rs 收尾)：清理验证
