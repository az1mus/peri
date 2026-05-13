# 分屏模式下非活跃 Session 命令浮层显示异常

**状态**：Open
**优先级**：中
**创建日期**：2026-05-12

## 问题描述

使用 `/split` 分屏后，左侧（非活跃）session 输入 `/` 时，命令浮层显示异常。具体表现为：
- 左侧 session 输入 `/` 后，命令浮层缺失部分内置命令
- 右侧（活跃） session 可以正常显示完整的命令列表

## 症状详情

### 预期行为
- 在任一 session 输入 `/` 后，应显示该 session 的完整命令列表（包括所有内置命令）
- 或：点击非活跃 session 时自动切换焦点，然后显示命令浮层

### 实际行为
- 左侧 session 输入 `/` 后，命令浮层缺失部分内置命令
- 右侧 session 命令浮层显示正常

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 启动 TUI
  2. 输入 `/split` 创建分屏
  3. 在左侧 session 输入 `/`
  4. 观察命令浮层显示的命令列表
- **环境**：多 session 分屏模式

## 相关代码

- `rust-agent-tui/src/ui/main_ui.rs:297-299` — 命令浮层只在 `is_active == true` 时渲染
  ```rust
  if is_active {
      // 统一命令/Skills 提示条
      popups::hints::render_unified_hint(f, app, chunks[5]);
  }
  ```

- `rust-agent-tui/src/ui/main_ui.rs:80-82` — `render_session_column` 临时切换 active
  ```rust
  let prev_active = app.session_mgr.active;
  app.session_mgr.active = session_idx;
  ```

- `rust-agent-tui/src/ui/main_ui/popups/hints.rs:24-33` — `render_unified_hint` 读取 active session 的输入和命令列表
  ```rust
  let first_line = app.session_mgr.sessions[app.session_mgr.active]
      .ui
      .textarea
      .lines()
      .first()
      .map(|s| s.as_str())
      .unwrap_or("");
  ```

## 根因分析

1. **命令浮层渲染条件限制**：`render_unified_hint` 只在 `is_active == true` 时被调用
2. **数据来源单一**：命令浮层始终读取 `app.session_mgr.sessions[app.session_mgr.active]` 的数据，而不是当前正在渲染的 session
3. **临时 active 切换不生效**：虽然 `render_session_column` 临时切换了 `active`，但由于 `is_active` 参数的存在，非活跃 session 的命令浮层根本不会渲染

## 影响范围

- 所有使用 `/split` 分屏功能的用户
- 特别是在左侧 session 输入命令时

## 可能的修复方向

1. **方案 A**：让命令浮层读取当前渲染 session 的数据，而非 active session
   - 修改 `render_unified_hint` 接受 `session_idx` 参数
   - 移除 `is_active` 条件限制

2. **方案 B**：点击非活跃 session 的输入区域时自动切换焦点
   - 在鼠标点击事件处理中检测是否点击了输入区域
   - 自动切换 `app.session_mgr.active`

3. **方案 C**：仅对 active session 显示命令浮层（当前行为）
   - 需要更新用户文档说明此行为
