# Predictive Input Design

**日期**: 2026-06-11
**状态**: Draft

## 概述

Agent 完成响应后，自动发起一次 fork LLM 请求，预测用户下一步可能的输入，以灰色 placeholder 形式显示在输入框中。用户按 Tab 接受建议，或开始输入时自动取消。

## 需求

- **触发时机**: Agent 正常完成（非 error/interrupt）后自动触发
- **建议类型**: 用户自然语言输入建议（如 "帮我添加单元测试"）
- **交互方式**: 单条建议，Tab 接收，任意输入取消
- **模型**: 复用当前会话同一模型
- **可见性**: 对用户透明，不显示在 bg agent bar 或消息流中

## 架构

### 数据流

```
Agent Done
  → ACP Server: spawn prediction tokio task
    → 构建 ReactAgent (max_iterations=1, 无工具)
    → 执行单次 LLM 调用
    → 结果通过 AcpNotification::PredictionReady 发送
  → TUI: 收到 PredictionReady
    → 设置 UiState.prediction = Some(text)
    → 渲染 textarea placeholder (灰色叠加)
  → 用户 Tab → 填入 textarea, 清除 prediction
  → 用户输入 → 清除 prediction
```

### 组件变更

#### 1. 新增 Prediction 指令模板

**位置**: `peri-middlewares/src/subagent/fork.rs`（或新建 `prediction.rs`）

```xml
<prediction_directive>
你是预测输入助手。根据对话上下文，预测用户下一步最可能在输入框中输入什么。

规则：
1. 只输出一句预测文本，不要解释
2. 预测应该是自然的用户语言，像用户自己会打的那样
3. 不要加引号、前缀或格式
4. 长度控制在 5-30 个字
5. 如果无法判断，输出空字符串
</prediction_directive>
```

上下文传入：最近 10 条对话历史（System 消息可省略以减少 token）。

#### 2. ACP 层：Prediction Fork Spawn

**位置**: `peri-acp/src/session/` 下新建 `prediction.rs`（或放在现有模块中）

**职责**:
- 在 agent 正常完成后触发
- `tokio::spawn` 异步任务
- 构建 `ReactAgent::builder()` + `.max_iterations(1)` + 空工具集
- 传入 prediction directive + 最近对话历史
- 结果通过 `AcpTransportSink` 发送 `PredictionReady` 通知

**取消机制**:
- 传入 session 的 `AgentCancellationToken`
- 新 agent 轮次开始时 cancel token 触发，自动取消 prediction fork
- 5 秒超时（`tokio::time::timeout`）

**触发入口**: `executor::execute_prompt()` 正常返回后（`result.ok == true`），或 ACP server `handle_done()` 流程中。

#### 3. ACP 协议：新增 Notification

**位置**: ACP 通知类型定义处

```rust
pub enum AcpNotification {
    // ... existing ...
    PredictionReady {
        session_id: String,
        text: String,
    },
}
```

#### 4. TUI 层：PredictionState

**位置**: `peri-tui/src/app/ui_state.rs`

```rust
pub struct PredictionState {
    pub text: String,
    pub received_at: Instant,
}

pub struct UiState {
    // ... existing ...
    pub prediction: Option<PredictionState>,
}
```

#### 5. TUI 层：渲染 Placeholder

**位置**: textarea 渲染区域

- textarea 内容为空 + `prediction.is_some()` → 叠加渲染灰色文本
- 使用 `Span::styled(text, Style::default().fg(Color::DarkGray))`
- 文本渲染在 textarea 光标位置之后

#### 6. TUI 层：Tab 接收

**位置**: `peri-tui/src/event/keyboard/normal_keys.rs` → `handle_tab()`

优先级：
1. `prediction` 存在 → `textarea.insert_str()` + 清除 prediction → return
2. `@` mention 补全（现有逻辑）
3. `/` hint 导航（现有逻辑）

#### 7. TUI 层：取消逻辑

- **任意输入键**（非 Tab/ShiftTab/方向键/翻页）→ 清除 `prediction`
- **Agent 新轮次**（`set_loading(true)`）→ 清除 `prediction`
- **新 PredictionReady 到达** → 替换旧 prediction
- **Escape** → 清除 `prediction`（不填入）

### 边界情况

| 场景 | 处理 |
|------|------|
| Prediction fork 返回空字符串 | 不设置 prediction，无 placeholder |
| Prediction fork 超时/失败 | 静默忽略，不影响用户体验 |
| 用户在 prediction 到达前已开始输入 | prediction 到达时检查 textarea 是否为空，非空则丢弃 |
| 快速连续完成多轮对话 | 新 agent 轮次开始取消上一个 prediction |
| Compact 后预测质量 | prediction fork 使用 compact 后的消息，LLM 能看到摘要上下文 |
| 会话结束/session 销毁 | cancel token 取消所有 pending prediction |

### Token 成本

- **System prompt**: ~100 tokens（prediction directive）
- **上下文**: 最近 6-10 条消息（~2000-4000 tokens）
- **输出**: 5-30 个字（~20-60 tokens）
- **总计每轮**: ~2200-4200 tokens

### 不做什么

- 不做多候选切换
- 不做流式逐字显示
- 不在 bg agent bar 展示
- 不注入到对话历史
- 不记录到 thread store
- 不做跨会话学习

## 涉及文件

| 文件 | 变更 |
|------|------|
| `peri-middlewares/src/subagent/fork.rs` | 新增 prediction directive 模板 |
| `peri-acp/src/session/prediction.rs` (新建) | prediction fork spawn 逻辑 |
| `peri-acp/src/session/mod.rs` | 模块声明 |
| `peri-acp/src/session/executor.rs` | 完成后触发 prediction |
| `peri-acp/src/event/` (通知类型) | 新增 `PredictionReady` 变体 |
| `peri-tui/src/acp_client/` (通知类型) | 新增 `PredictionReady` 变体 |
| `peri-tui/src/app/ui_state.rs` | 新增 `PredictionState` + `prediction` 字段 |
| `peri-tui/src/app/agent_ops/lifecycle.rs` | 接收 `PredictionReady` 事件 |
| `peri-tui/src/event/keyboard/normal_keys.rs` | Tab 接受 + 输入取消 |
| `peri-tui/src/app/render/` (textarea 渲染) | placeholder 灰色叠加渲染 |
