# LLM 返回 400 时消息区域闪烁清空回到空白页

**状态**：Open
**优先级**：高
**创建日期**：2026-05-20

## 问题描述

使用 DeepSeek 模型时，消息流式输出过程中 LLM 返回 400 错误，TUI 消息区域出现短暂闪烁后完全清空，回到初始空白页面状态。历史对话内容丢失，用户无法看到之前的消息。

## 症状详情

| 维度 | 表现 |
|------|------|
| 触发时机 | 消息流式输出过程中，LLM API 返回错误 |
| 闪烁表现 | 消息区域短暂闪烁一下 |
| 清空表现 | 整个消息区域被清空，回到初始空白/欢迎页面状态 |
| 历史消息 | 之前对话内容在界面上消失 |
| 恢复情况 | 不确定是否自动恢复 |

### 错误日志

```
▶ 2026-05-20T08:47:42.577Z  POST [anthropic]
   UPSTREAM: https://api.deepseek.com/anthropic/v1/messages
◀ 2026-05-20T08:47:42.693Z  [anthropic]  → 400  (114ms)
```

DeepSeek 通过 Anthropic 兼容端点调用，返回 HTTP 400。

## 复现条件

- **复现频率**：目前仅遇到一次，尚未确认稳定复现条件
- **触发步骤**：
  1. 使用 DeepSeek 模型（通过 Anthropic 兼容端点）
  2. 发送 prompt 进行对话
  3. LLM 返回 400 错误时触发
- **环境**：DeepSeek 模型，Anthropic 兼容端点 (`/anthropic/v1/messages`)

## 涉及文件

- `peri-tui/src/app/agent_ops/lifecycle.rs` — Agent 生命周期错误处理（Done/Error/Interrupted 状态下的 UI 更新）
- `peri-tui/src/app/agent_ops/acp_bridge.rs` — ACP 通知桥接，将 AcpNotification 转为 AgentEvent
