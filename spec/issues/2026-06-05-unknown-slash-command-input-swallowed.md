# 未知 Slash Command 输入被吞掉，应作为普通消息提交

**状态**：Open
**优先级**：中
**创建日期**：2026-06-05

## 问题描述

TUI 中输入以 `/` 开头但不是已知命令、Skill 或 Agent 命令的文本时（如 `/user/api`、`/tmp/test`），系统将其视为"未知命令"并显示错误提示，输入内容被吞掉不会提交给 Agent。用户期望这类输入作为普通文本消息发送给 Agent 处理。

## 症状详情

| 输入内容 | 当前行为 | 期望行为 |
|----------|----------|----------|
| `/user/api` | 显示"未知命令或 Skill: /user/api"，输入丢失 | 作为普通文本提交给 Agent |
| `/tmp/test` | 同上 | 同上 |
| `/config` | 正确匹配本地命令 | 不变 |
| `/code-review` | 正确匹配 Skill | 不变 |

- 输入框在错误提示后被清空（`build_textarea(false)` 已在第 142 行执行）
- 错误信息以 System 消息形式显示在聊天界面

## 根因定位

`peri-tui/src/event/keyboard/normal_keys.rs:141-214`：

1. 第 141 行 `text.starts_with('/')` 进入 slash command 分支
2. 第 147 行本地命令匹配失败
3. 第 153-165 行 Skill 匹配（`skill_name` 提取只取 `/` 后的 `user`，遇到 `/` 停止）
4. 第 168-173 行 Agent 命令匹配也失败
5. 第 178-213 行进入 else 分支，构造错误消息并 push 到 `view_messages`
6. **没有 return `Action::Submit(text)`**，输入被丢弃

## 涉及文件

- `peri-tui/src/event/keyboard/normal_keys.rs` —— slash command 分发与未知命令处理逻辑

## 修复方向

将第 178 行的 `else` 分支从"显示错误"改为 `return Ok(Some(Action::Submit(text)))`，静默走普通提交路径。歧义匹配（多个命令前缀匹配）的情况也一并改为普通提交。

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-05 | — | Open | agent | 创建 |

## 修复记录

（由 fix-issue 或 issue-verify skill 追加，创建时留空）
