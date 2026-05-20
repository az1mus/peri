# Markdown 与 Theme 颜色体系脱节，存在多处分叉硬编码

**状态**：Fixed (部分)
**优先级**：中
**创建日期**：2026-05-20
**修复日期**：2026-05-20

## 问题描述

项目中有三套独立存在、互不联动的颜色定义体系，且存在色值不一致。改动 `DarkTheme` 的色值不会影响 Markdown 渲染、spinner、diff 高亮等组件。

## 已修复

| 项目 | 状态 | 修复方式 |
|------|------|---------|
| MarkdownTheme-Theme 脱节 | ✅ Fixed | 新增 `ThemeMarkdownAdapter<'a>`（`markdown/mod.rs:46`），将 `Theme` trait 方法映射到 `MarkdownTheme` |
| Spinner 硬编码 | ⚠️ 部分 | 仍有默认硬编码值，但有 `theme_colors()` setter 供调用方覆盖（`spinner/mod.rs:159`） |

`ThemeMarkdownAdapter` 语义映射：heading→warning, code→thinking, link→success, mute→muted, separator→muted。

## 仍存在

| 文件 | 问题 | 现状 |
|------|------|------|
| `peri-tui/src/ui/theme.rs` | TUI 常量独立于 DarkTheme，`THINKING = #A2A9E4` ≠ `DarkTheme::thinking() = #AF87FF` | 未修复，两套体系并存 |
| `peri-widgets/src/message_block/highlight.rs` | diff/keyword 高亮硬编码 ANSI 色（Yellow/Green/DarkGray） | 未修复 |
| `peri-widgets/src/markdown/highlight.rs` | 语法高亮用 syntect `base16-ocean.dark`，与项目 Theme 无关 | 未修复（不影响颜色一致性） |
| `peri-tui/src/ui/message_render.rs` | 箭头颜色硬编码 | 未验证 |

## 涉及文件

- `peri-widgets/src/markdown/mod.rs:46` — 新增 `ThemeMarkdownAdapter`
- `peri-widgets/src/theme/mod.rs` — `Theme` trait 定义
- `peri-widgets/src/theme/presets.rs` — `DarkTheme` 实现
- `peri-widgets/src/message_block/highlight.rs` — diff/代码高亮仍硬编码
- `peri-tui/src/ui/theme.rs` — TUI 常量独立于 DarkTheme
