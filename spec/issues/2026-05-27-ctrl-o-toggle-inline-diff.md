# Ctrl+O 开关 Write/Edit 工具的内联 diff 显示，默认关闭并提供 config 选项

**状态**：Open
**优先级**：中
**创建日期**：2026-05-27

## 问题描述

Write/Edit 工具结果中已实现了内联 diff 渲染（`5fdfef4`），但目前 diff 始终显示，无法关闭。需要提供快捷键切换 diff 显隐，默认关闭，并由用户通过 `~/.peri/settings.json` 配置默认行为。

## 期望功能

1. **快捷键**：`Ctrl+O` 切换消息流中 Write/Edit 工具结果的内联 diff 显隐
2. **默认关闭**：diff 默认不显示
3. **持久化配置**：`~/.peri/settings.json` 中增加选项控制默认值

## 快捷键冲突说明

`Ctrl+O` 当前在 OAuth 流程中用于「在浏览器中打开链接」。该功能仅在 OAuth 弹窗激活时触发（`popups/oauth.rs:39`、`status_bar.rs:363`）。非 OAuth 场景下 `Ctrl+O` 应切换 diff 显隐。

## 配置设计

`~/.peri/settings.json` 中 `config` 段新增：

```json
{
  "config": {
    "diffEnabled": false
  }
}
```

- 字段名 `diffEnabled`（布尔，默认 `false`）
- 启动时读取，设置 diff 初始显隐状态
- 运行时 `Ctrl+O` 切换不写回配置文件（仅会话级切换）

## 涉及文件

- `peri-tui/src/app/ui_state.rs` —— 新增 `diff_visible: bool` 状态字段
- `peri-tui/src/app/thread_ops.rs` —— 新增 `toggle_diff()` 方法
- `peri-tui/src/event/keyboard.rs` —— 绑定 `Ctrl+O` 到 diff 切换（非 OAuth 场景）
- `peri-tui/src/ui/message_view/mod.rs` —— 根据 `diff_visible` 决定是否渲染 diff_lines
- `peri-tui/src/ui/message_render.rs` —— diff 渲染逻辑
- `peri-acp/src/provider/config.rs` —— `AppConfig` 新增 `diff_enabled: bool` 字段
