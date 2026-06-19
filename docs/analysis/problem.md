# orphan tool_use_id 问题陈述

**关联**: [orphan-tool-use-id-专题.md](./orphan-tool-use-id-专题.md) | [主报告](./metrics-analysis-2026-06.md#41-llm-error469-次)

---

## 现象

LLM 400 错误：`tool_result` 中的 `tool_use_id` 在前一条 assistant 消息中找不到对应 `tool_use` block。

- **431 次**（06-06 ~ 06-19），占 LLM 错误 **91.9%**
- **仅 deepseek-v4-pro**（OpenAI 兼容路径），Anthropic 原生模型无此问题
- 大部分不可重试（仅 2 次触发重试）
