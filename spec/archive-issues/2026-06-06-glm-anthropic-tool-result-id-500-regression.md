> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-glm-anthropic-tool-result-id-500-regression.md

# GLM Anthropic 兼容端口 500 回归: tool_result block 缺少 id 属性多轮工具调用再触发

**状态**：Closed
**优先级**：高
**创建日期**：2026-06-06

## 问题描述

使用 GLM 模型的 Anthropic 兼容端口进行多轮工具调用时，API 返回 500 Internal Server Error：`'ClaudeContentBlockToolResult' object has no attribute 'id'`。与已归档的 `2026-05-15-glm-anthropic-tool-result-id-attribute-error` 症状完全一致：首轮工具调用正常，多轮后触发 500。

该问题在 5 月 15 日修复后已正常工作，今天（6 月 6 日）的 commit 导致回归。

## 症状详情

### 错误日志

```
LLM HTTP 错误 (500): API 错误 500 Internal Server Error:
'ClaudeContentBlockToolResult' object has no attribute 'id'
```

### 触发模式

- **首轮工具调用**：正常完成
- **多轮工具调用后**：某轮触发 500，所有重试均失败
- **错误类型**：Python `AttributeError`——GLM 网关内部 `ClaudeContentBlockToolResult` 对象缺少 `id` 属性

### 回归时间线

| 时间 | 状态 | 说明 |
|------|------|------|
| 2026-05-15 | 首次出现并修复 | 为 `tool_result` block 添加 `id` 字段（commit `8f928a8e`） |
| 2026-05-15 ~ 2026-06-05 | 正常工作 | GLM Anthropic 端口多轮工具调用无问题 |
| 2026-06-06 | 回归 | 今天的 commit 导致问题复现 |

## 复现条件

- **复现频率**：多轮工具调用后触发（首轮正常），必现
- **触发步骤**：
  1. 配置 GLM 模型的 Anthropic 兼容端口
  2. 进行首轮工具调用 → 正常
  3. 继续多轮工具调用 → 某轮触发 500
- **环境**：GLM，Anthropic 兼容端口，多轮工具调用

## 涉及文件

- `peri-agent/src/llm/anthropic/invoke.rs` — `block_to_anthropic`（ToolResult 序列化，已有 `id` 字段）和 `messages_to_anthropic`（Tool 消息序列化，已有 `id` 字段）
- `peri-agent/src/llm/anthropic/cache.rs` — `ensure_thinking_blocks`（重构后改为无条件调用）
- `peri-agent/src/messages/content.rs` — `ContentBlock::ToolResult` 结构体（已有 `id: Option<String>` 字段）

## 可能的回归原因

**注意**：以下是现象层面的可疑变更点，不是根因分析。

1. **`ensure_thinking_blocks` 无条件调用**（`2e469b9c refactor`）：原来仅在 `extended_thinking` 时调用，重构后改为无条件调用。GLM 网关在处理含 thinking block 的消息时可能走不同的验证路径，重新暴露 `.id` 问题
2. **`1c8ac5f0 fix: all tool errors return Err()`**：更多工具错误路径改为 `Err()` 返回，导致 `tool_error`（`is_error: true`）消息增多。序列化路径相同，但 GLM 网关可能对 error tool_result 的验证逻辑不同

## 关联 Issue

- `spec/archive-issues/2026-05-15-glm-anthropic-tool-result-id-attribute-error.md`（Fixed → 需 Reopen）— 同一问题的首次出现和修复
- `spec/global/domains/agent.md#issue_2026-05-15-glm-anthropic-tool-result-id-attribute-error` — 领域知识中的问题记录

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建（回归报告，原 issue 已归档） |

## 修复记录

### 调查 #1（2026-06-06）

- **操作人**：agent
- **调查结果**：代码层面 `tool_result` 的 `id` 字段完整存在，今天所有 commit 均未修改 Anthropic 序列化代码。端到端测试（含多轮 tool_error）确认所有 tool_result block 都有非空 `id`。无法从代码层面复现回归。
- **验证状态**：搁置——需要实际运行环境中捕获 500 错误时的请求体（`invoke.rs:452` 会记录 `request_messages`）来确认 GLM 端实际收到的 JSON
