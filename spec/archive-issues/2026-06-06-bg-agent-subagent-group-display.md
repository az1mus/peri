> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-bg-agent-subagent-group-display.md

# 消息区域 SubAgentGroup 卡片显示异常——完成后残留、未聚合、状态错误

**状态**：Fixed
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

消息区域中后台 SubAgent 的卡片（SubAgentGroup VM）显示逻辑整体异常。运行中的后台 Agent 以黄色（WARNING 色）显示，完成后应转为绿色（SAGE）或聚合为批次汇总，但实际表现为卡片残留不消失、多个卡片未聚合、卡片状态显示错误。

## 症状详情

| 症状 | 表现 |
|------|------|
| 卡片残留 | 后台 Agent 执行完毕后，其黄色卡片仍留在消息区域，不会自动消失或更新 |
| 聚合失败 | 多次调用 SubAgent 后，多个独立卡片堆在消息区，未按预期聚合为批次汇总视图 |
| 状态错误 | 卡片的运行状态（is_running/已完成）显示不正确，可能已完成但显示为运行中（黄色） |
| 颜色异常 | 运行中的后台 Agent 卡片使用 WARNING 色（`#FFC107` 黄色），但完成后未切换到 SAGE 色 |

### 颜色逻辑（`message_render.rs:384-390`）

```rust
let agent_color = if *is_error {
    theme::ERROR          // 红色
} else if *is_running && *is_background {
    theme::WARNING        // 黄色 (#FFC107) — 运行中的后台 Agent
} else {
    theme::SAGE           // 绿色 — 已完成的前台/后台 Agent
};
```

### 聚合逻辑（`aggregate.rs:84-161`）

`aggregate_batch_groups()` 仅聚合 `is_running: false` 且 `batch_agents` 为空的 SubAgentGroup。如果 `is_running` 标志未正确更新，卡片不会被聚合。

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 启动 TUI，让 Agent 用 `run_in_background: true` 启动一个或多个后台 SubAgent
  2. 观察消息区域：运行中后台 Agent 显示为黄色卡片
  3. 等待后台 Agent 执行完毕
  4. 预期：黄色卡片变为绿色（SAGE），或与其他已完成 Agent 聚合为批次汇总
  5. 实际：黄色卡片残留、未聚合

## 涉及文件

- `peri-tui/src/ui/message_render.rs:384-390` —— SubAgentGroup 卡片颜色逻辑（is_running + is_background → WARNING）
- `peri-tui/src/ui/message_view/aggregate.rs:84-161` —— SubAgentGroup 批次聚合（aggregate_batch_groups）
- `peri-tui/src/app/agent_events_bg.rs` —— 后台 Agent 事件处理（handle_subagent_start/end、handle_background_task_completed）
- `peri-tui/src/app/message_pipeline/mod.rs` —— 消息管线（subagent_stack、frozen_subagent_vms、tool_end_internal）
- `peri-tui/src/ui/main_ui/bg_agent_bar.rs` —— BG Agent Bar（8 色循环，第 3 个是 Yellow）

## 关联 Issue

- `spec/issues/2026-05-26-bg-agent-message-flow-broken.md` —— 后台 SubAgent 消息流显示失败（Open）。本 issue 关注 SubAgentGroup 卡片的聚合、状态、残留问题，与前 issue 的消息流内容丢失相关但侧重不同

## 根因分析

`handle_background_task_completed` 只更新 `view_messages`（UI 层），**不更新 pipeline 的 `subagent_stack`/`frozen_subagent_vms`**。当 `Done` 或 `request_rebuild()` 触发 reconcile 时，管线用过期的 frozen VM（`is_running=true`）覆盖已正确更新的 UI 状态。

### 数据流

```
BackgroundTaskCompleted → view_messages (is_running=false ✅)
                       → 未更新 subagent_stack (is_running=true ❌)
                       → 未更新 frozen_subagent_vms (is_running=true ❌)

Done → drain_subagent_stack → frozen VM (is_running=true, 来自过期 SubAgentState)
     → reconcile → merge_frozen_subagents → 用过期 frozen VM 覆盖 view_messages
     → is_running=true 恢复 ❌
```

### 时序影响

| 时序 | 行为 | 结果 |
|------|------|------|
| BG Complete → Done | notify 更新 SubAgentState → Done drain 见 is_running=false → 不创建 frozen VM | ✅ |
| Done → BG Complete | Done drain 创建 frozen VM(is_running=true) → notify 更新 frozen VM(is_running=false) | ✅ |
| Done → reconcile → BG Complete | reconcile 用 frozen VM(is_running=true) → notify 更新 frozen VM → 下一帧 rebuild 纠正 | ✅ |

## 修复

### 变更 1: MessagePipeline::notify_bg_completed（新增 ~100 行）

`peri-tui/src/app/message_pipeline/mod.rs`

- 按 `instance_id`（优先）或 `agent_name`（回退）查找匹配的 SubAgentState
- 更新 SubAgentState（`is_running=false`、push finalized VM 到 `frozen_subagent_vms`、标记 `finalized_vm`）
- 更新 `frozen_subagent_vms` 中已冻结但 `is_running=true` 的 VM（两遍匹配：instance_id 精确 → agent_name 兜底）
- 仿照前台 agent 的 `tool_end_internal` 路径创建 finalized SubAgentGroup VM

### 变更 2: handle_background_task_completed 调用（~12 行）

`peri-tui/src/app/agent_events_bg.rs`

- 无条件调用 `pipeline.notify_bg_completed()`，不依赖 `child_thread_id` 是否有值
- 在更新 `view_messages` 之后、`request_rebuild()` 之前调用

## 测试验证

诊断测试 `test_diagnostic_bg_subagent_group_disappears` 修复前后对比：

| Step | Before | After |
|------|--------|-------|
| 6 (BG Complete) | `running=true, steps=0, has_result=false` ❌ | `running=false, steps=5, has_result=true` ✅ |
| 7 (continuation begin) | `running=true` ❌ | `running=false` ✅ |
| 8-9 (continuation) | 状态回退 ❌ | 稳定保持 ✅ |

全量回归：611/611 passed，零回归。

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 基于用户反馈创建 |
| 2026-06-06 | Open | Fixed | agent | 新增 notify_bg_completed 管线同步方法修复 |

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 基于用户反馈创建 |

## 修复记录

（待修复）
