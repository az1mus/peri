# TUI 输入框不支持在消息任意位置内联触发 / 补全弹窗

**状态**：Open
**优先级**：低
**创建日期**：2026-06-06

## 问题描述

当前 TUI 的 slash command 和 skill 提示弹窗只能在消息**行首**输入 `/` 时触发。用户期望像 IDE 的 IntelliSense 或 @mention 一样，在消息**任意位置**（空白字符后）输入 `/` 都能弹出补全列表，选中后只替换当前 `/xxx` token，不影响消息其余内容。

## 症状详情

| 场景 | 当前行为 | 期望行为 |
|------|----------|----------|
| 输入 `帮我 review 一下 /code` | 无弹窗，直接作为普通文本提交 | 光标处弹出 skill/command 候选，Tab/Enter 补全为 `/code-review` |
| 输入 `/model` 后按空格继续打字 | 整段被替换为 `/model `，用户需重新输入后续内容 | 仅替换 `/model` token，后续内容保留 |
| 多行消息第二行输入 `/help` | 无弹窗 | 正常弹出命令候选 |

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 启动 TUI
  2. 在输入框中输入任意文本（不以 `/` 开头）
  3. 按空格后输入 `/`
  4. 观察：无任何提示浮层出现

## 涉及文件

- `peri-tui/src/app/hint_ops.rs` — `build_hint_items()` 仅检查 `first_line.starts_with('/')`；`hint_complete()` 替换整个 textarea
- `peri-tui/src/ui/main_ui/popups/hints.rs` — `render_unified_hint()` 仅检查 `first_line.starts_with('/')`
- `peri-tui/src/event/keyboard/normal_keys.rs` — Enter 提交逻辑 `text.starts_with('/')` 仅处理行首 slash
- `peri-tui/src/app/ui_state.rs` — 可能需要新增字段记录当前 hint 的 token 起始位置（类似 `AtMentionState.query_start`）

## 期望改进方向

1. **检测逻辑**：参考 `@mention` 的 `AtMentionState::detect()`，在光标前回溯查找最近的 `/` token，要求 `/` 前为空白字符或行首，避免 `and/or` 等正常文本误触发
2. **局部替换**：`hint_complete()` 补全时仅替换 `/xxx` token 为 `/{name} `，保留消息其他部分不变
3. **Enter 提交**：消息中包含的 `/command` 或 `/skill` 仍需正常触发对应逻辑（SkillPreloadMiddleware 已支持消息中任意位置出现 `/skill-name`）
4. **多行支持**：不局限于第一行，任意行、任意光标位置均支持

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |

## 修复记录

（由 fix-issue 或 issue-verify skill 追加，创建时留空）
