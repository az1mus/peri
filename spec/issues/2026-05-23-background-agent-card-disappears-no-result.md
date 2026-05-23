# Background Agent 完成后 SubAgent 卡片消失且无数据回传

**状态**：Open
**优先级**：高
**创建日期**：2026-05-23

## 问题描述

通过 LLM 调用 Agent 工具并设置 `run_in_background: true` 时，SubAgent 卡片在 TUI 中短暂闪现后消失。Background agent 被标记为完成后，卡片立即消失，且父 agent 后续没有收到任何返回数据。此问题必现，导致 background agent 功能完全不可用。

## 症状详情

| 阶段 | 表现 |
|------|------|
| 调用 Agent(run_in_background: true) | SubAgent 卡片正常出现 |
| background agent 执行中 | 卡片正常显示 |
| background agent 完成（Done） | 卡片立即消失 |
| 之后 | 父 agent 未收到 background agent 的返回数据 |

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 启动 TUI
  2. 让 LLM 调用 Agent 工具，参数包含 `run_in_background: true`
  3. 观察 SubAgent 卡片在完成后消失，且无数据回传
- **环境**：TUI 模式，所有模型均可复现

## 涉及文件

- `peri-middlewares/src/subagent/tool/define.rs` —— Agent 工具定义，包含 `run_in_background` 参数处理和 `BackgroundTaskRegistry`
- `peri-tui/src/app/agent_ops/subagent.rs` —— SubAgent 生命周期处理，background agent 完成后的回调
- `peri-tui/src/ui/headless_test.rs:3651` —— 已有 fork+run_in_background 场景的诊断测试
