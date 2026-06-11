> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-bg-agent-bar-tool-count-always-zero.md

# BG Agent Bar 始终显示 0 calls

**状态**：Fixed
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

BG Agent Bar（`ui/main_ui/bg_agent_bar.rs`）中每个后台 agent 行显示的工具调用次数始终为 `0 calls`，无论 bg agent 实际执行了多少工具调用。问题必现，所有 bg agent 均受影响。用户期望在 bar 中实时看到每个 bg agent 的工具调用进度。

## 症状详情

| 场景 | 期望显示 | 实际显示 |
|------|---------|---------|
| 单个 bg agent 执行中 | 递增的工具调用数 | `0 calls` |
| 并发多个 bg agent | 每个 agent 各自的工具调用数 | 全部 `0 calls` |
| bg agent 完成后 | 最终工具调用总数 | `0 calls` |

显示格式示例（实际）：`● general-purpose       0 calls 45s`

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 在对话中让 LLM 调用 Agent 工具并设置 `run_in_background: true`
  2. 观察 TUI 底部 BG Agent Bar
  3. 即使 bg agent 在持续执行工具调用，`calls` 计数始终为 0
- **环境**：所有模型、所有 OS

## 根因分析

bg agent 在 `tokio::spawn` 中独立运行，**不共享 parent 的 event_handler**。TUI 只收到 `SubagentStarted` 和 `BackgroundTaskCompleted` 两个事件，中间的 ToolStart/ToolEnd 事件完全不会到达 TUI pipeline。

原 `find_total_steps()` 从 `view_messages` 查找 `SubAgentGroup` 的 `total_steps`，但该字段仅在 ToolStart 事件到达 pipeline 时递增——bg agent 的事件根本不会到达 pipeline，所以 `total_steps` 始终为 0。

## 涉及文件

| 文件 | 改动 |
|------|------|
| `peri-agent/src/agent/events.rs` | 新增 `BgToolStep { child_thread_id }` 事件变体 |
| `peri-middlewares/src/subagent/tool/execute_bg.rs` | bg agent builder 添加轻量级 event_handler，转发 ToolStart 为 BgToolStep |
| `peri-tui/src/app/events.rs` | 新增 TUI `AgentEvent::BgToolStep` 变体 |
| `peri-tui/src/app/agent.rs` | 映射 `ExecutorEvent::BgToolStep` → TUI `AgentEvent::BgToolStep` |
| `peri-tui/src/app/agent_ops/mod.rs` | 路由 `BgToolStep` 事件到 `handle_bg_tool_step` |
| `peri-tui/src/app/agent_events_bg.rs` | 新增 `handle_bg_tool_step()` 递增 `RunningBgAgent.tool_count` |
| `peri-tui/src/app/chat_session.rs` | `RunningBgAgent` 新增 `tool_count: usize` 字段 |
| `peri-tui/src/app/agent_ops/subagent.rs` | 初始化 `tool_count: 0` |
| `peri-tui/src/ui/main_ui/bg_agent_bar.rs` | 使用 `agent.tool_count` 替代 `find_total_steps()`，移除已删除函数 |
| `peri-acp/src/event/mapper.rs` | 将 `BgToolStep` 归类为 `tui_only` |

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |
| 2026-06-06 | Open | Fixed | agent | 修复：事件转发方案，1049 tests pass |

## 修复记录

方案 A（事件转发）：bg agent 的 event_handler 将每个 ToolStart 转换为轻量级 `BgToolStep { child_thread_id }` 事件，通过 `bg_event_sender` 发送到 TUI。TUI 收到后递增对应 `RunningBgAgent.tool_count`，bg_agent_bar 直接读取 `agent.tool_count` 显示。
