# Link 组件（OSC 8 超链接）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 在 peri-widgets 中新增 `LinkSpan` 组件（Span 工厂），支持 OSC 8 终端可点击超链接；Markdown 渲染器集成 hyperlink 支持并修复嵌套样式丢失 bug。

**Architecture:** `LinkSpan` 是纯数据→Span 转换工厂，通过 builder 模式配置；`LinkWidget` 薄包装独立使用场景。Markdown coordinator 中 `Tag::Link` 捕获 URL，`push_span` 叠加 `Style::hyperlink()`；`RenderState` 新增 `pending_link_url` 字段追踪内联链接状态。

**Tech Stack:** Rust, ratatui 0.30+ (unstable-hyperlink), pulldown-cmark 0.13

---

## 文件结构

| 文件 | 职责 | 操作 |
|------|------|------|
| `peri-widgets/src/link.rs` | LinkSpan + LinkWidget 实现 | 新建 |
| `peri-widgets/src/link_test.rs` | LinkSpan + LinkWidget 单元测试 | 新建 |
| `peri-widgets/src/lib.rs` | 模块声明 + 重导出 | 修改 |
| `peri-widgets/Cargo.toml` | 启用 unstable-hyperlink feature | 修改 |
| `peri-tui/Cargo.toml` | 启用 unstable-hyperlink feature | 修改 |
| `peri-widgets/src/markdown/render_state/coordinator.rs` | Tag::Link 捕获 URL、push_span 叠加 hyperlink、修复嵌套样式 bug | 修改 |
| `peri-widgets/src/markdown/mod_test.rs` | link 集成测试、嵌套样式修复验证 | 修改 |

---

### Task 1: 启用 `unstable-hyperlink` feature

**Files:**
- Modify: `peri-widgets/Cargo.toml:8`
- Modify: `peri-tui/Cargo.toml:25`

- [ ] **Step 1: 修改 peri-widgets/Cargo.toml**

```diff
- ratatui = { version = ">=0.30", features = ["unstable-rendered-line-info", "unstable-widget-ref"] }
+ ratatui = { version = ">=0.30", features = ["unstable-rendered-line-info", "unstable-widget-ref", "unstable-hyperlink"] }
```

- [ ] **Step 2: 修改 peri-tui/Cargo.toml**

```diff
- ratatui = { version = "0.30.1", features = ["unstable-rendered-line-info", "unstable-widget-ref"] }
+ ratatui = { version = "0.30.1", features = ["unstable-rendered-line-info", "unstable-widget-ref", "unstable-hyperlink"] }
```

- [ ] **Step 3: 验证编译**

```bash
cargo build -p peri-widgets
```
预期：feature 启用成功，无编译错误。

- [ ] **Step 4: Commit**

```bash
git add peri-widgets/Cargo.toml peri-tui/Cargo.toml
git commit -m "feat: enable ratatui unstable-hyperlink feature for OSC 8 support"
```

---

### Task 2: 创建 `LinkSpan` + `LinkWidget`

**Files:**
- Create: `peri-widgets/src/link.rs`
- Create: `peri-widgets/src/link_test.rs`
- Modify: `peri-widgets/src/lib.rs`

- [ ] **Step 1: 编写 LinkSpan 测试（先写测试，红阶段）**

创建 `peri-widgets/src/link_test.rs`：

```rust
use ratatui::style::{Hyperlink, Modifier, Style};
use super::link::LinkSpan;

#[test]
fn link_span_to_span_has_hyperlink() {
    let link = LinkSpan::new("https://example.com", "Example");
    let span = link.to_span();
    assert_eq!(span.content, "Example");
    assert_eq!(span.style.hyperlink, Some(Hyperlink::from("https://example.com")));
    assert!(span.style.add_modifier == Modifier::UNDERLINED);
}

#[test]
fn link_span_empty_url_skips_hyperlink() {
    let link = LinkSpan::new("", "No URL");
    let span = link.to_span();
    assert_eq!(span.style.hyperlink, None, "空 URL 不应产生 hyperlink");
    assert_eq!(span.content, "No URL");
}

#[test]
fn link_span_empty_text_uses_url_as_fallback() {
    let link = LinkSpan::new("https://example.com", "");
    let span = link.to_span();
    assert_eq!(span.content, "https://example.com");
}

#[test]
fn link_span_max_width_truncates_text() {
    let link = LinkSpan::new("https://example.com", "Very Long Text").max_width(4);
    let span = link.to_span();
    assert_eq!(span.content, "Very…");
}

#[test]
fn link_span_cjk_truncation() {
    let link = LinkSpan::new("https://example.com", "你好世界").max_width(2);
    let span = link.to_span();
    assert_eq!(span.content, "你好…");
}

#[test]
fn link_span_custom_style() {
    let link = LinkSpan::new("https://example.com", "Click")
        .style(Style::new().fg(ratatui::style::Color::Red));
    let span = link.to_span();
    assert_eq!(span.style.fg, Some(ratatui::style::Color::Red));
    assert_eq!(span.style.hyperlink, Some(Hyperlink::from("https://example.com")));
}

#[test]
fn link_span_no_truncate_when_text_fits() {
    let link = LinkSpan::new("https://example.com", "hi").max_width(4);
    let span = link.to_span();
    assert_eq!(span.content, "hi");
}

#[test]
fn link_widget_renders() {
    use ratatui::layout::Rect;
    use ratatui::buffer::Buffer;
    use ratatui::widgets::WidgetRef;
    use super::link::LinkWidget;

    let link = LinkSpan::new("https://example.com", "Click me");
    let widget = LinkWidget { link: &link };
    let mut buf = Buffer::empty(Rect::new(0, 0, 20, 1));
    widget.render_ref(Rect::new(0, 0, 20, 1), &mut buf);
    let cell = buf.cell((0, 0)).unwrap();
    assert_eq!(cell.symbol(), "C");
    assert_eq!(cell.style().hyperlink, Some(Hyperlink::from("https://example.com")));
}
```

- [ ] **Step 2: 运行测试验证失败**

```bash
cargo test -p peri-widgets -- link_test --lib
```
预期：编译失败（模块未创建）或测试全部失败。

- [ ] **Step 3: 实现 LinkSpan + LinkWidget**

创建 `peri-widgets/src/link.rs`：

```rust
use ratatui::{
    layout::Rect,
    style::{Hyperlink, Modifier, Style},
    text::{Line, Span},
    widgets::WidgetRef,
    buffer::Buffer,
};

/// OSC 8 超链接的 Span 工厂。
///
/// 通过 builder 模式配置样式、截断宽度，调用 `to_span()` 产出带
/// [`Style::hyperlink`] 的 [`Span`]，可直接嵌入任意 [`Line`] 或
/// 通过 [`LinkWidget`] 独立渲染。
pub struct LinkSpan {
    url: String,
    text: String,
    style: Style,
    max_width: Option<u16>,
}

impl LinkSpan {
    /// 创建 LinkSpan。
    ///
    /// - 若 `text` 为空，使用 `url` 作为 fallback 文本。
    pub fn new(url: impl Into<String>, text: impl Into<String>) -> Self {
        let url: String = url.into();
        let text: String = text.into();
        let text = if text.is_empty() { url.clone() } else { text };
        Self {
            url,
            text,
            style: Style::default().add_modifier(Modifier::UNDERLINED),
            max_width: None,
        }
    }

    /// 设置自定义样式（会覆盖默认的 UNDERLINED）。
    pub fn style(mut self, style: Style) -> Self {
        self.style = style;
        self
    }

    /// 设置最大显示宽度（字符数，CJK 安全）。
    /// 超出部分截断并追加 "…"。
    pub fn max_width(mut self, w: u16) -> Self {
        self.max_width = Some(w);
        self
    }

    /// 产出带 OSC 8 hyperlink 的 Span。
    ///
    /// - 若 URL 为空，不写入 hyperlink，仅保留样式。
    /// - 若设置了 `max_width` 且文本超宽，截断并追加 "…"。
    pub fn to_span(&self) -> Span<'static> {
        let text = match self.max_width {
            None => self.text.clone(),
            Some(w) if self.text.chars().count() <= w as usize => self.text.clone(),
            Some(w) => {
                let mut truncated: String = self.text.chars().take(w as usize).collect();
                truncated.push('…');
                truncated
            }
        };

        let style = if self.url.is_empty() {
            self.style
        } else {
            self.style.hyperlink(Hyperlink::from(self.url.clone()))
        };

        Span::styled(text, style)
    }
}

/// 独立场景下的薄 Widget 包装，将 LinkSpan 渲染到指定 Rect。
pub struct LinkWidget<'a> {
    pub link: &'a LinkSpan,
}

impl WidgetRef for LinkWidget<'_> {
    fn render_ref(&self, area: Rect, buf: &mut Buffer) {
        let line = Line::from(self.link.to_span());
        ratatui::widgets::Paragraph::new(line).render(area, buf);
    }
}
```

- [ ] **Step 4: 运行测试验证通过**

```bash
cargo test -p peri-widgets -- link_test --lib
```
预期：全部 8 个测试通过。

- [ ] **Step 5: 在 lib.rs 中声明模块和重导出**

修改 `peri-widgets/src/lib.rs`：

在 `pub mod` 声明区添加：
```rust
pub mod link;
```

在重导出区添加：
```rust
pub use link::{LinkSpan, LinkWidget};
```

- [ ] **Step 6: 全量编译验证**

```bash
cargo build -p peri-widgets -p peri-tui
```
预期：无编译错误。

- [ ] **Step 7: Commit**

```bash
git add peri-widgets/src/link.rs peri-widgets/src/link_test.rs peri-widgets/src/lib.rs
git commit -m "feat: add LinkSpan component with OSC 8 hyperlink support"
```

---

### Task 3: Markdown Coordinator 集成 OSC 8 Hyperlink

**Files:**
- Modify: `peri-widgets/src/markdown/render_state/coordinator.rs:5,17,52-63,113-116,287-296,330-339`
- Modify: `peri-widgets/src/markdown/mod_test.rs:86-95, +新测试`

- [ ] **Step 1: 在 RenderState 中添加 pending_link_url 字段**

修改 `coordinator.rs` 的 `RenderState` struct（在 `max_width` 之后添加）：

```rust
pub(in crate::markdown) struct RenderState<'a> {
    pub lines: Vec<Line<'static>>,
    pub current_spans: Vec<Span<'static>>,
    pub inline_style: Style,
    pub list_stack: Vec<ListState>,
    pub quote_depth: u32,
    pub in_code_block: bool,
    pub code_block_lang: String,
    code_lines: Vec<String>,
    table: Option<TableBuilder>,
    theme: &'a dyn MarkdownTheme,
    max_width: usize,
    pending_link_url: Option<String>,  // 当前正在处理的 link URL
}
```

- [ ] **Step 2: 更新 new() 初始化 pending_link_url**

在 `new()` 方法中添加：

```rust
pending_link_url: None,
```

- [ ] **Step 3: 修改 push_span 叠加 hyperlink**

修改 `push_span` 方法（line 113-116）：

```rust
pub fn push_span(&mut self, text: String, extra: Style) {
    let mut style = self.inline_style.patch(extra);
    if let Some(ref url) = self.pending_link_url {
        if !url.is_empty() {
            style = style.hyperlink(Hyperlink::from(url.clone()));
        }
    }
    self.current_spans.push(Span::styled(text, style));
}
```

- [ ] **Step 4: 修改 Tag::Link Start/End 处理（捕获 URL + 修复嵌套样式 bug）**

替换 line 330-339：

```rust
// ── 链接 ─────────────────────────────────────────────────────────
Event::Start(Tag::Link { dest_url, .. }) => {
    self.pending_link_url = Some(dest_url.to_string());
    self.inline_style = self
        .inline_style
        .fg(self.theme.link())
        .add_modifier(Modifier::UNDERLINED);
}
Event::End(TagEnd::Link) => {
    // 仅移除 Link 增量样式（UNDERLINED），保留嵌套的 Strong/Emphasis
    self.inline_style = self.inline_style.remove_modifier(Modifier::UNDERLINED);
    self.pending_link_url = None;
}
```

- [ ] **Step 5: 修改表格内联文本路径，也叠加 hyperlink**

修改 line 287-293 的表格 Text 处理：

```rust
} else if self.table.is_some() {
    let mut style = self.inline_style;
    if let Some(ref url) = self.pending_link_url {
        if !url.is_empty() {
            style = style.hyperlink(Hyperlink::from(url.clone()));
        }
    }
    self.table
        .as_mut()
        .unwrap()
        .current_cell
        .push(Span::styled(text_str, style));
}
```

- [ ] **Step 6: 添加 Hyperlink import**

在 `coordinator.rs` 顶部 imports 添加：

```rust
use ratatui::style::Hyperlink;
```

- [ ] **Step 7: 更新文件头注释中的不变量**

在 coordinator.rs 顶部注释块中添加一条不变量：

```
// [不变量] link 内联状态：pending_link_url 在 Start(Tag::Link) 时设置，
//          push_span / 表格 Text 路径叠加 Style::hyperlink，
//          End(TagEnd::Link) 时置 None。End 时仅 remove_modifier(UNDERLINED)，
//          不覆盖 inline_style（保持嵌套 Strong/Emphasis 样式）。
```

- [ ] **Step 8: 验证编译无错误**

```bash
cargo build -p peri-widgets
```
预期：编译成功，无警告。

- [ ] **Step 9: 更新现有 parse_link 测试**

修改 `mod_test.rs` 的 `parse_link` 测试（line 86-95），增加 hyperlink 验证：

```rust
#[test]
fn parse_link() {
    let text = parse_markdown("[text](url)", &default_theme(), 80);
    assert!(!text.lines.is_empty());
    let mut link_found = false;
    for l in &text.lines {
        for s in &l.spans {
            if s.content.contains("text") && s.style.fg == Some(default_theme().link()) {
                assert_eq!(
                    s.style.hyperlink,
                    Some(ratatui::style::Hyperlink::from("url")),
                    "链接 span 应有 hyperlink"
                );
                link_found = true;
            }
        }
    }
    assert!(link_found, "Expected link text with link color and hyperlink");
}
```

- [ ] **Step 10: 新增嵌套样式修复测试**

在 `mod_test.rs` 的 `parse_link` 测试之后添加：

```rust
#[test]
fn parse_link_with_nested_emphasis() {
    let text = parse_markdown("[**bold** text](url)", &default_theme(), 80);
    // bold span 应该有 BOLD + link color + hyperlink
    let bold_found = text.lines.iter().any(|l| {
        l.spans.iter().any(|s| {
            s.content.contains("bold")
                && s.style.add_modifier.contains(Modifier::BOLD)
                && s.style.fg == Some(default_theme().link())
                && s.style.hyperlink == Some(ratatui::style::Hyperlink::from("url"))
        })
    });
    assert!(bold_found, "嵌套 Bold 应保留 BOLD + link color + hyperlink");
    // text span 应该有 link color + hyperlink，无 BOLD
    let text_found = text.lines.iter().any(|l| {
        l.spans.iter().any(|s| {
            s.content.contains("text")
                && !s.style.add_modifier.contains(Modifier::BOLD)
                && s.style.fg == Some(default_theme().link())
                && s.style.hyperlink == Some(ratatui::style::Hyperlink::from("url"))
        })
    });
    assert!(text_found, "链接内普通文本应有 link color + hyperlink");
}

#[test]
fn parse_link_with_nested_italic() {
    let text = parse_markdown("[*italic* text](url)", &default_theme(), 80);
    let italic_found = text.lines.iter().any(|l| {
        l.spans.iter().any(|s| {
            s.content.contains("italic")
                && s.style.add_modifier.contains(Modifier::ITALIC)
                && s.style.fg == Some(default_theme().link())
                && s.style.hyperlink == Some(ratatui::style::Hyperlink::from("url"))
        })
    });
    assert!(italic_found, "嵌套 Italic 应保留 ITALIC + link color + hyperlink");
}

#[test]
fn parse_link_empty_url_skips_hyperlink() {
    let text = parse_markdown("[text]()", &default_theme(), 80);
    let link_found = text.lines.iter().any(|l| {
        l.spans.iter().any(|s| {
            s.content.contains("text")
                && s.style.fg == Some(default_theme().link())
                && s.style.hyperlink.is_none()
        })
    });
    assert!(link_found, "空 URL 链接不应有 hyperlink，但保留 link color");
}
```

- [ ] **Step 11: 运行所有相关测试**

```bash
cargo test -p peri-widgets -- link_test parse_link parse_link_with_nested_emphasis parse_link_with_nested_italic parse_link_empty_url_skips_hyperlink --lib
```
预期：全部测试通过。

- [ ] **Step 12: 运行全量 markdown 测试确保无回归**

```bash
cargo test -p peri-widgets -- markdown --lib
```
预期：全部测试通过（含原有测试）。

- [ ] **Step 13: Commit**

```bash
git add peri-widgets/src/markdown/render_state/coordinator.rs peri-widgets/src/markdown/mod_test.rs
git commit -m "feat: integrate OSC 8 hyperlink into markdown renderer, fix nested style loss bug"
```

---

### Task 4: 全量回归验证

**Files:**
- 无代码变更，仅运行验证。

- [ ] **Step 1: 全量构建**

```bash
cargo build
```
预期：所有 crate 编译成功。

- [ ] **Step 2: 全量测试**

```bash
cargo test
```
预期：所有测试通过，无回归。

- [ ] **Step 3: 运行 clippy**

```bash
cargo clippy --all-targets
```
预期：无新增警告。

- [ ] **Step 4: 运行 fmt**

```bash
cargo fmt --all -- --check
```
预期：格式正确。

- [ ] **Step 5: Commit（如有格式修复）**

```bash
git add . && git commit -m "chore: apply formatting and clippy fixes"
```
