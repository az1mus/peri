# 并发 SubAgent 工具调用路由错误 + 背景色移除

**状态**：Open（背景色已修复，并发路由待解决）
**优先级**：中
**创建日期**：2026-05-16
**最新更新**：2026-05-17

## 问题描述

两个问题：

1. ~~SubAgent 展开后内部工具调用渲染时使用了 `SUB_AGENT_BG` 背景色，用户希望去掉背景色，与父 Agent 工具调用的视觉效果一致。~~ **已修复**（commit `70a904b`）
2. 当父 Agent 在同一轮中并发调用多个普通 SubAgent 时，前面的 SubAgent 展开后看不到工具调用记录，只有最后一个 SubAgent 的 `recent_messages` 中保留了什么。

## 症状详情

| 现象 | 详情 |
|------|------|
| 背景色问题 | ~~SubAgent 内部工具调用（ToolBlock）有背景色~~ **已修复**（移除 `message_render.rs` 中 3 处 `SUB_AGENT_BG`） |
| 并发路由问题 | 并发 2+ 个普通 SubAgent 时，仅最后一个 SubAgentGroup 的 `recent_messages` 中有工具调用记录，其余为空 |
| 影响范围 | 普通 SubAgent（非 background），fork/dispatching 版本暂未确认 |

## 复现条件

- **复现频率**：必现（并发时）
- **触发步骤**：
  1. 启动 TUI
  2. 让父 Agent 在同一轮中并发调用 2 个 Agent 工具（不同的 subagent_type）
  3. SubAgent 全部完成后，展开各 SubAgentGroup
  4. 观察：只有最后一个完成的 SubAgent 内部有工具调用记录
- **环境**：任意模型

## 涉及文件

- `peri-tui/src/ui/message_render.rs:511` —— SubAgentGroup 内部消息渲染时的 `bg(theme::SUB_AGENT_BG)` 逻辑
- `peri-tui/src/app/message_pipeline.rs:475-496` —— `subagent_tool_start` 通过 `subagent_stack.last_mut()` 路由工具调用
- `peri-tui/src/app/message_pipeline.rs:250-268` —— `ToolEnd` 事件同样通过 `last_mut()` 更新 `recent_messages`
- `peri-tui/src/app/message_pipeline.rs:595-600` —— `in_subagent()` 仅检查栈顶 SubAgent 是否运行

## 根因分析（并发路由问题）

`MessagePipeline::subagent_stack` 存储活跃 SubAgent 状态，所有路由均使用 `subagent_stack.last_mut()`——在并发场景下永远返回最后入栈的 SubAgent，导致内部 ToolStart/ToolEnd 事件全部路由到错误的 SubAgentState。

## 已尝试方案（均导致运行时 hang）

1. **`parking_lot::Mutex<HashMap>` 传递 tool_call_id → agent_id 映射**：在 tokio multi-thread runtime 中 `parking_lot` 的自旋行为导致死锁。
2. **移除 Mutex，将 agent_id 编码在 `format_subagent_result` 的字符串后缀中**：3 个并发 SubAgent 时完全卡死，Ctrl+C 无效。
3. **在 `map_executor_event` 中提取后缀并填充 `SubAgentEnd.tool_data` 字段**：仍 hang。

这些方案均涉及改动 `format_subagent_result` 或移除 `with_event_handler` 事件转发。根本原因可能是事件转发路径上某些竞态条件，而不仅仅是锁类型选择。

## 背景色移除修复（commit `70a904b`）

`peri-tui/src/ui/message_render.rs` 中 3 处 `SUB_AGENT_BG` 全部移除：
- 批量 agent 展开视图（task_preview + final_result）
- 嵌套工具调用渲染（缩进前缀 + patch_style）
- 单 agent final_result 行

## 待解决

并发 SubAgent 内部工具调用的流式路由问题。需要在不引入 hang 的前提下传递 agent_id。——可能是 `peri_agent::AgentEvent` 层面上需要添加 `subagent_id` 字段，或考虑将并发 SubAgent 改为顺序执行。
