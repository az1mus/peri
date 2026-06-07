# settings.json 全局 Hook 配置未加载生效

**状态**：Open
**优先级**：高
**类型**：Bug
**创建日期**：2026-06-06

## 问题描述

用户在 `~/.claude/settings.json` 的 `hooks` 字段中配置了多个事件的 hook（session/turn/tool/compact 等多种类型），但在 peri-tui 中运行时，这些 hook 均不触发。启动日志中**未出现** `"Loaded N hooks from ~/.claude/settings.json"` 记录，表明 hook 加载阶段可能已经失败。

## 症状详情

| 项目 | 描述 |
|------|------|
| Hook 配置位置 | `~/.claude/settings.json`（用户级全局配置） |
| 运行入口 | `peri-tui`（TUI 终端模式） |
| 失效范围 | 多种事件类型的 hook 均不触发（不是单一事件的问题） |
| 加载日志 | 启动时**无** `"Loaded N hooks from ~/.claude/settings.json"` 日志输出 |

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 在 `~/.claude/settings.json` 中配置 `hooks` 字段（至少一个事件 + hook 规则）
  2. 启动 `peri-tui`
  3. 触发对应事件（如发送用户消息 → 应触发 `UserPromptSubmit`）
  4. 观察 hook 是否执行：未执行
- **环境**：macOS，peri-tui

## 涉及文件

- `peri-middlewares/src/hooks/loader.rs` —— `load_global_settings_hooks()` 从 `~/.claude/settings.json` 加载 hook
- `peri-tui/src/main.rs:686` —— TUI 启动时调用 `load_global_settings_hooks()` 的位置
- `peri-acp/src/agent/builder.rs:439-463` —— HookMiddleware 创建逻辑
- `peri-middlewares/src/hooks/middleware.rs` —— HookMiddleware 事件分发

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |

## 修复记录

（由 fix-issue 或 issue-verify skill 追加，创建时留空）
