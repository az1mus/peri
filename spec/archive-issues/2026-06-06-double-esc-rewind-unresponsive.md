> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-double-esc-rewind-unresponsive.md

# 双击 ESC 偶发完全无响应（rewind 选择器不弹出）

**状态**：Fixed
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

Agent 空闲时双击 ESC，偶发完全无响应——rewind 选择器弹窗不弹出，界面上无任何视觉变化（无 toast、无状态栏提示、无闪烁）。大部分时候双击 ESC 能正常弹出 rewind 选择器，但偶尔两次 ESC 按下后什么都没发生。

## 症状详情

| 维度 | 描述 |
|------|------|
| 操作 | Agent 空闲时，快速双击 ESC（间隔 < 2 秒） |
| 期望 | 弹出 rewind 选择器弹窗，显示可回退的用户消息列表 |
| 实际 | 偶发情况下，两次 ESC 按下后完全无反应 |
| 频率 | 偶发（大部分时候正常，偶尔失灵） |
| 状态 | Agent 已完成回答、输入框空闲、无面板/弹窗打开 |
| 视觉反馈 | 无任何变化（第一次 ESC 本身也无可见反馈——`rewind_pending_since` 是内部状态） |

## 复现条件

- **复现频率**：偶发
- **触发步骤**：
  1. 与 Agent 进行对话
  2. Agent 完成回答后处于空闲状态
  3. 快速双击 ESC
  4. 偶尔两次 ESC 完全无响应
- **环境**：macOS，具体终端未确认

## 根因分析

**crossterm 0.28.1 的 ESC 字节合并问题**（`event/sys/unix/parse.rs:77`）：

当用户快速双击 ESC 时，终端可能在同一轮 `read_complete` 中将两个 `0x1B` 字节一起送入 crossterm 的 `Parser::advance`。`Parser` 逐字节处理：

1. 第 1 字节 `0x1B` → buffer=`[0x1B]`, more=true → `parse_event` 返回 `Ok(None)`（可能是转义序列，等待更多字节）
2. 第 2 字节 `0x1B` → buffer=`[0x1B, 0x1B]` → 匹配 `b'\x1B' => Ok(Some(Event::Key(KeyCode::Esc)))` → **返回一个 ESC 事件，清空 buffer**

结果：两次物理 ESC 按键只产生一个 `KeyCode::Esc` 事件。第一个（也是唯一一个）ESC 事件设置了 `rewind_pending_since`（无可见反馈），没有第二个事件来触发 rewind 弹窗。

对比 Ctrl+C 双击（一直稳定）：Ctrl+C 是 `0x03`，不受转义序列解析影响，总是产生独立事件。

## 修复方案

无法修改 crossterm 行为，在应用层通过**添加视觉反馈**补偿：

1. **状态栏提示**：第一次 ESC 后，状态栏左侧显示 "再按 ESC 回滚对话"（ACCENT 色），右侧快捷键显示 "Esc 回滚对话 | 其他键 取消"
2. **自动过期**：`next_event` 中添加 `rewind_pending_since` 2 秒自动过期（类似 `quit_pending_since` 1 秒过期），过期后触发 Redraw 清除提示
3. **效果**：即使 crossterm 吞掉了第二个 ESC 事件，用户也能看到提示，再按一次 ESC 即可触发 rewind

## 涉及文件

- `peri-tui/src/event/keyboard/normal_keys.rs:48-61` —— 双击 ESC 触发 rewind 的按键处理逻辑
- `peri-tui/src/event/mod.rs:46-52` —— `rewind_pending_since` 2 秒自动过期
- `peri-tui/src/ui/main_ui/status_bar.rs:338-346` —— 状态栏 rewind 提示
- `peri-tui/src/ui/main_ui/status_bar.rs:425-429` —— 状态栏快捷键 rewind 提示

## 相关历史

- 归档 issue `2026-06-02-rewind-loses-messages-esc-unresponsive` 曾修复了兜底分支重置 `rewind_pending_since` 的问题。当前代码中该修复仍在，但问题仍然偶发。本次修复针对的是不同的根因（crossterm 字节合并）。

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |
| 2026-06-06 | Open | Fixed | agent | 添加状态栏视觉反馈 + 自动过期 |

## 修复记录

| 日期 | 提交 | 说明 |
|------|------|------|
| 2026-06-06 | — | 在 status_bar 添加 rewind_pending 视觉提示，在 next_event 添加 2 秒自动过期 |
