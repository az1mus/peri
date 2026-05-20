# Markdown 与 Theme 颜色体系脱节，存在多处分叉硬编码

**状态**：Open
**优先级**：中
**创建日期**：2026-05-20

## 问题描述

项目中有三套独立存在、互不联动的颜色定义体系，且存在色值不一致。改动 `DarkTheme` 的色值不会影响 Markdown 渲染、spinner、diff 高亮等组件，导致潜在的颜色偏离风险。

## 症状详情

### 体系 1：`Theme` trait（`peri-widgets/src/theme/`）

```rust
// DarkTheme 实现
fn thinking() -> Color { Color::Rgb(175, 135, 255) }  // #AF87FF
fn accent()  -> Color { Color::Rgb(215, 119, 87)  }  // #D77757
fn muted()   -> Color { Color::Rgb(153, 153, 153)  }  // #999999
```

### 体系 2：`MarkdownTheme` trait（`peri-widgets/src/markdown/mod.rs`）

与 `Theme` 完全独立，`DefaultMarkdownTheme` 直接硬编码色值，不引用 `Theme`：

```rust
fn heading() -> Color { Color::Rgb(255, 193, 7)   }  // #FFC107
fn code()    -> Color { Color::Rgb(162, 169, 228) }  // #A2A9E4
fn link()    -> Color { Color::Rgb(78, 186, 101)  }  // #4EBA65
fn muted()   -> Color { Color::Rgb(153, 153, 153) }  // #999999
```

### 体系 3：TUI 常量（`peri-tui/src/ui/theme.rs`）

与 `DarkTheme` 色值**大部分一致但存在分叉**：

```rust
pub const THINKING: Color = Color::Rgb(162, 169, 228);  // #A2A9E4 ≠ DarkTheme::thinking() #AF87FF
pub const ACCENT:   Color = Color::Rgb(215, 119, 87);   // #D77757 ✓
pub const SAGE:     Color = Color::Rgb(78, 186, 101);   // #4EBA65 ✓
pub const MUTED:    Color = Color::Rgb(153, 153, 153);  // #999999 ✓
```

**色值冲突**：`DarkTheme::thinking() = #AF87FF`，但 `TUI theme::THINKING = #A2A9E4`。而 `DefaultMarkdownTheme::code() = #A2A9E4` 恰好等于 TUI 的 `THINKING`。

### 其他硬编码散落点

| 文件 | 硬编码颜色 | 问题 |
|------|-----------|------|
| `peri-widgets/src/spinner/mod.rs:142-143` | `Accent #D77757`、`Muted #999999` | `new()` 默认值硬编码，有 `theme_colors()` setter 但调用方不一定用 |
| `peri-widgets/src/message_block/highlight.rs:4-6` | `DIFF_ADD #6EB56A`、`DIFF_REMOVE #CC463E`、`DIFF_HUNK Cyan` | diff 颜色完全独立，无 Theme 接口 |
| `peri-widgets/src/message_block/highlight.rs:78-83` | `Color::Yellow`、`Color::Green`、`Color::DarkGray` | 代码关键词高亮用标准 ANSI 色 |
| `peri-widgets/src/markdown/highlight.rs` | 通过 `syntect::ThemeSet` 加载 `base16-ocean.dark` | 代码块语法高亮用第三方主题，与项目 Theme 无关 |
| `peri-tui/src/ui/message_render.rs:367,407` | `arrow_color #93C1FD` | 箭头颜色硬编码，重复两次 |
| `peri-tui/src/ui/main_ui/panels/thread_browser.rs:19` | `SELECTED #B2B9F9` | 列表选中行颜色硬编码 |

## 涉及文件

- `peri-widgets/src/theme/mod.rs` — `Theme` trait 定义
- `peri-widgets/src/theme/presets.rs` — `DarkTheme` 实现
- `peri-widgets/src/markdown/mod.rs` — 独立的 `MarkdownTheme` + `DefaultMarkdownTheme`
- `peri-widgets/src/markdown/highlight.rs` — syntect 语法高亮主题
- `peri-widgets/src/markdown/render_state.rs` — 通过 `&dyn MarkdownTheme` 查询颜色
- `peri-widgets/src/spinner/mod.rs` — 硬编码 spinner 颜色
- `peri-widgets/src/message_block/highlight.rs` — 硬编码 diff/代码高亮色
- `peri-tui/src/ui/theme.rs` — TUI 常量，与 DarkTheme 存在分叉
- `peri-tui/src/ui/message_render.rs` — 箭头颜色硬编码
- `peri-tui/src/ui/main_ui/panels/thread_browser.rs` — SELECTED 颜色硬编码

## 期望改进方向

1. `MarkdownTheme` 与 `Theme` 联动——通过 `From<&dyn Theme>` 或适配器桥接，消除独立硬编码
2. 统一 `DarkTheme::thinking()` 与 `TUI theme::THINKING` 的色值
3. Spinner 通过 `Theme` 获取默认色而非硬编码
4. Diff 高亮色纳入 `Theme` trait（或新增 `MessageBlockTheme` 子 trait）
5. `message_render.rs` 中箭头颜色提取为常量或 Theme 方法
6. `thread_browser.rs` 中 `SELECTED` 使用 `Theme::cursor_bg()` 或类似语义方法
