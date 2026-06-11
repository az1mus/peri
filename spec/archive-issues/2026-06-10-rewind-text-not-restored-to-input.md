> 归档于 2026-06-11，原路径 spec/issues/2026-06-10-rewind-text-not-restored-to-input.md

# Rewind 撤回消息后未将用户输入回填到输入框

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-10

## 问题描述

用户通过双击 ESC 打开 rewind 选择器、选择一条历史用户消息并确认回退后，被撤回的那条用户消息的文本内容没有自动填入输入框。用户期望 rewind 后能直接在输入框中看到刚撤回的文本，修改后重新发送；但实际上输入框是空的，用户需要重新手动输入完整内容。

## 症状详情

| 维度 | 描述 |
|------|------|
| 操作 | 双击 ESC → 选择一条用户消息 → Enter 确认 rewind |
| 期望 | 被撤回的用户消息文本自动出现在输入框中，可直接编辑后重新发送 |
| 实际 | 输入框为空，用户需要从头重新输入 |
| 复现频率 | 必现 |
| Rewind 模式 | MessagesOnly / MessagesAndFiles / ConfirmRevert 均如此 |

### 验证 #1（2026-06-10）—— 通过

用户确认 rewind 功能已完整可用：
- 纯文本对话后 rewind 弹窗正确显示所有用户消息（含第一条）
- rewind 后被撤回的消息文本自动回填到输入框
- 可直接编辑后重新发送

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 与 Agent 进行至少一轮对话
  2. 双击 ESC 打开 rewind 选择器
  3. 选择任意一条用户消息
  4. 按 Enter 确认回退
  5. 回退完成后，输入框为空
- **环境**：所有平台

## 涉及文件

- `peri-tui/src/app/agent_ops/rewind.rs:111-148` —— `rewind_confirm()`：确认 rewind 时构造 `/rewind` 命令并发送，但未提取目标消息的文本内容回填到输入框
- `peri-tui/src/app/history_ops.rs:84-86` —— `restore_history_to_textarea()` / `restore_draft()`：现有的 `textarea.insert_str()` 机制可用于回填文本
- `peri-tui/src/app/ui_state.rs:18` —— `draft_input: Option<String>`：已有的输入框暂存机制

## 期望行为细节

- rewind 确认后，被撤回的**目标用户消息**（即 rewind 选择器中选中的那条 Human 消息）的文本内容应自动填入输入框
- 文本应处于可编辑状态，用户可以直接修改后按 Enter 重新发送
- 填入方式复用现有的 `textarea.insert_str()` 机制（与 history_up/restore_draft 相同模式）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-10 | — | Open | agent | 创建 |
| 2026-06-10 | Open | Fixed | agent | 修复 StateSnapshot 缺失 Human 消息 + 文本回填 |
| 2026-06-10 | Fixed | Verified | user | 用户确认验证通过 |

## 修复记录

### 修复 #1（2026-06-10）

- **操作人**：agent
- **用户原意**：rewind 撤回消息后，被撤回的文本应自动回填到输入框，方便修改后重新发送
- **修复内容**：
  1. **根因修复**（`peri-agent`）：`snapshot_anchor` 原先设为 `human_msg.id()`，`index_after_id` 返回 +1 导致 Human 消息被跳过 → `origin_messages` 只有 AI 消息，rewind 找不到 Human 消息。改为指向 Human 之前的消息 ID；空 state 时用随机 sentinel ID 让 fallback 从 0 开始
  2. **文本回填**（`peri-tui`）：`rewind_confirm` 从 `origin_messages` 提取被撤回消息文本暂存到 `pending_rewind_text`，`handle_rewind_completed` 通过 `textarea.insert_str()` 回填
- **涉及 commit**：`af047905`, `6364251f`, `c6bfd7ec`, `3d794ed5`
- **验证状态**：已验证
