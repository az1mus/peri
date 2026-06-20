# tracer/tool_handler.rs 存在 is_agent 重复检测

**状态**：Fixed
**优先级**：中
**创建日期**：2026-06-19

## 问题描述

commit `b6c55550` 将 tracer.rs（761 行）拆分为 tracer/ 目录后，`tool_handler.rs` 中 `on_tool_start` 和 `on_tool_end` 各自实现了 `is_agent` 检测逻辑，两处逻辑重复但实现方式不同——违反了 DRY 原则。在拆分前它们是同一文件内的相邻方法，重复不显著；拆分后跨文件依赖 `pending_tools` + `subagent_stack` 的内部结构，存在维护时两处逻辑漂移的风险。

## 症状详情

### 重复位置

`peri-acp/src/langfuse/tracer/tool_handler.rs`：

| 方法 | 行号 | 实现 |
|------|------|------|
| `on_tool_start` | line 19 | `let is_agent = name == "Agent";` |
| `on_tool_end` | line 64-74 | 遍历 `pending_tools` + `subagent_stack` 双层查找 `tool_call_id` 的 `name == "Agent"` |

### 为何不是简单重复

`on_tool_start` 可以直接根据参数 `name` 判断（此时工具名已传入），但 `on_tool_end` 中 PendingTool 可能已被 pop 到子 agent 栈，需要遍历两层查找。两者语义相同（"是否 Agent 工具调用"），但访问路径不同。

### 风险

如果未来 `is_agent` 判断条件发生变化（例如增加对 `fork` 模式的支持），两处代码需要同步修改，容易遗漏。

## 期望改进方向

提取公共方法 `fn is_agent_tool(&self, tool_call_id: &str) -> bool`，放在 `tracer/subagent_stack.rs` 或 `tracer/context.rs` 中，供 `on_tool_start` 和 `on_tool_end` 共享。

## 涉及文件

- `peri-acp/src/langfuse/tracer/tool_handler.rs`（140 行）—— 重复逻辑所在
- `peri-acp/src/langfuse/tracer/subagent_stack.rs`（199 行）—— 建议安置公共方法的位置
- `peri-acp/src/langfuse/tracer/context.rs`（50 行）—— 备选安置位置

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-19 | — | Open | agent | 创建 |

## 修复记录

| 日期 | 操作人 | 说明 |
|------|--------|------|
| 2026-06-20 | agent | 在 subagent_stack.rs 提取 pub(crate) fn is_agent_tool()，tool_handler.rs 的 on_tool_start 和 on_tool_end 统一委托此方法。on_tool_start 调整为 insert 后再调用（插入 pending_tools 后搜索自身） |
