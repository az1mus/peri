# Link 组件（OSC 8 超链接）设计

## 概述

在 `peri-widgets` 中新增 `LinkSpan` 组件，支持通过 OSC 8 escape sequence 渲染终端可点击超链接。Markdown 渲染器中的所有 `[text](url)` 链接自动接入 OSC 8 支持。

## 架构决策

- **方案 2（完整组件）**：`LinkSpan`（Span 工厂）+ `LinkWidget`（薄 Widget 包装）。符合 peri-widgets 三层架构，可独立使用也可内联嵌入
- **ratatui `unstable-hyperlink` feature**：使用 `Style::hyperlink()` API 产生 OSC 8 序列，与 ratatui 生态对齐
- **Markdown 内联链接不走 `LinkSpan`**：直接在 coordinator 的 `push_span` 流程中叠加 `style.hyperlink(url)`，因为 Text 事件分散在 Tag::Link Start/End 之间，用 `LinkSpan` 反而需要颠倒重建

## 组件定义

```rust
/// OSC 8 超链接的 Span 工厂。
pub struct LinkSpan {
    url: String,
    text: String,
    style: Style,
    max_width: Option<u16>,
}

impl LinkSpan {
    pub fn new(url: impl Into<String>, text: impl Into<String>) -> Self;
    pub fn style(mut self, style: Style) -> Self;
    pub fn max_width(mut self, w: u16) -> Self;
    pub fn to_span(&self) -> Span<'static>;
}

/// 独立场景下的薄 Widget 包装
pub struct LinkWidget<'a> { link: &'a LinkSpan }

impl WidgetRef for LinkWidget<'_> { /* Paragraph::new(Line::from(link.to_span())).render() */ }
```

## 文件变更清单

| 文件 | 变更 |
|------|------|
| `peri-widgets/src/link.rs` | 新增：`LinkSpan` + `LinkWidget` 实现 |
| `peri-widgets/src/link_test.rs` | 新增：单元测试 |
| `peri-widgets/src/lib.rs` | 新增 `pub mod link;` + 重导出 |
| `peri-widgets/Cargo.toml` | ratatui 新增 `unstable-hyperlink` feature |
| `peri-tui/Cargo.toml` | ratatui 新增 `unstable-hyperlink` feature |
| `peri-widgets/src/markdown/render_state/coordinator.rs` | `Tag::Link` 捕获 URL、`push_span` 叠加 hyperlink、修复嵌套样式丢失 bug |
| `peri-widgets/src/markdown/mod_test.rs` | 新增 link 集成测试、嵌套样式修复验证 |

## Markdown Coordinator 变更

### 修改前（有 bug）

```rust
Event::Start(Tag::Link { .. }) => {
    self.inline_style = self.inline_style.fg(self.theme.link()).add_modifier(Modifier::UNDERLINED);
}
Event::End(Tag::End) => {
    self.inline_style = Style::default(); // BUG: 丢失嵌套 Strong/Emphasis
}
```

### 修改后

```rust
// RenderState 新增字段
pending_link_url: Option<String>,

// Start: 捕获 URL + 叠加 inline_style
Event::Start(Tag::Link { dest_url, .. }) => {
    self.pending_link_url = Some(dest_url.to_string());
    self.inline_style = self.inline_style.fg(self.theme.link()).add_modifier(Modifier::UNDERLINED);
}
// End: 仅移除 Link 增量效果
Event::End(Tag::End) => {
    if self.pending_link_url.is_some() {
        self.inline_style = self.inline_style.remove_modifier(Modifier::UNDERLINED);
        self.pending_link_url = None;
    }
}

// push_span: 叠加 hyperlink
let mut style = self.inline_style;
if let Some(ref url) = self.pending_link_url {
    if !url.is_empty() {
        style = style.hyperlink(url.clone());
    }
}
self.current_spans.push(Span::styled(text, style));
```

## 边界情况

| 场景 | 处理 |
|------|------|
| URL 为空 | 不写入 OSC 8 序列，仅保留样式 |
| 文本为空 | `LinkSpan::new()` 用 url 作为 fallback text |
| CJK 截断 | `s.chars().take(N).collect()` + `"…"` 后缀 |
| 嵌套链接 | pulldown-cmark 自动拒绝，无需处理 |
| 超长 URL | 不做截断（OSC 8 无明确长度限制） |
| TagEnd 错配 | 判断 `pending_link_url.is_some()` 区分 Link End |
| 嵌套样式修复 | `End` 时只 remove_modifier(UNDERLINED)，不覆盖全样式 |

## 测试策略

| 测试层 | 内容 | 文件 |
|--------|------|------|
| LinkSpan 单元测试 | `to_span()` hyperlink、空 URL、max_width 截断 CJK、builder | `link_test.rs` |
| LinkWidget 渲染测试 | `render_ref` 写 buffer 正确 | `link_test.rs` |
| Coordinator 集成测试 | `[text](url)` hyperlink、嵌套样式、空 URL | `mod_test.rs` |
| 端到端 | TUI 渲染含链接的 markdown 消息（手动验证） | 手动 |
