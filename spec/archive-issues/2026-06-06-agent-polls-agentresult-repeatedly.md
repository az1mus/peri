> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-agent-polls-agentresult-repeatedly.md

# Agent 反复轮询 AgentResult 而非等待后台任务通知

**状态**：Fixed
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

Agent 在派发后台任务（`run_in_background: true`）后，反复调用 `AgentResult` 轮询结果，而不是等待系统通知。这导致产生大量无用的工具调用（单次可多达 20+ 次），浪费 token 和时间。

## 症状详情

### 典型行为序列

```
用户: /ultra-batch  # 派发 3 个后台 agent
Agent: 3 个 agent 已并行启动...
       [后台任务已启动]

Agent: AgentResult() → "No completed results yet"     # 第 1 次轮询
Agent: AgentResult() → "No completed results yet"     # 第 2 次轮询
Agent: AgentResult() → "No completed results yet"     # 第 3 次轮询
...
Agent: AgentResult() → "No completed results yet"     # 第 N 次轮询
Agent: AgentResult() → [返回结果]
```

### 具体数据

- 在本次会话中，派发 3 个后台 agent 后，agent 连续调用 `AgentResult` **17 次**才等到结果
- 每次调用返回相同的 "No completed background agent results available yet" 消息
- 文档明确说明 "Background tasks will notify you when they complete"，但 agent 无视此提示

### 用户反馈

用户不得不手动干预：
> "你不用一直看着，只需要跟我说等待即可"
> "你不用一直看着，我们停下等待就好"

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 使用 `Agent` 工具派发 `run_in_background: true` 的后台任务
  2. 派发完成后观察 agent 行为
- **环境**：所有模型、所有场景

## 根因分析

Agent 的行为模式：
1. 派发后台任务后，agent 的上下文中没有"等待"的概念
2. `AgentResult` 工具描述中虽然说了"Do not retry this query immediately — continue with other work instead"，但 agent 倾向于反复重试
3. Agent 没有"主动等待"的能力——要么继续做其他事，要么轮询
4. 当没有其他任务可做时，轮询成了唯一选项

## 期望改进方向

1. **系统提示词层面**：在派发后台任务后，agent 应输出一条简短的等待消息，然后停止响应，直到收到系统通知
2. **工具层面**：`AgentResult` 可以在返回 "no results" 时附加强制等待提示，或者在连续调用 3 次后拒绝响应
3. **行为约束**：在 agent 的行为准则中明确禁止连续轮询，要求等待通知

## 涉及文件

- `peri-agent/` — Agent 行为控制相关
- `peri-middlewares/src/subagent/agent_result.rs` — AgentResult 工具定义
- 系统提示词 — agent 行为准则

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |
| 2026-06-06 | Open | Fixed | agent | 系统提示词加 Background Tasks 行为准则 + AgentResult 返回文案精简强化 |

## 修复记录

### 修复 #1（2026-06-06）

- **操作人**：agent
- **用户原意**：Agent 派发后台任务后应等待系统通知，不应反复轮询 AgentResult
- **修复内容**：
  1. `11_subagent.md` 新增 Background Tasks 行为准则段（混合式措辞：引导 + 关键强措辞）
  2. `agent_result.rs` 精简 no-results 返回文案，删除误导性 ExecuteExtraTool 引导，强化"do not call this tool again until notified"
- **涉及文件**：`peri-tui/prompts/sections/11_subagent.md`、`peri-middlewares/src/subagent/agent_result.rs`
- **验证状态**：待验证（纯提示词修改，需实际运行观察 LLM 行为变化）
