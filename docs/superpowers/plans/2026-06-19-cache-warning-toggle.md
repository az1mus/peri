# Cache Warning Toggle — Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a `/config` panel toggle to show/hide the yellow cache hit rate warning message in the message flow.

**Architecture:** Add `show_cache_warning: bool` (default `true`) to `AppConfig`, wire it through `ConfigPanel` as an on/off toggle in the General group, and gate the warning push in `subagent.rs` on this config value. Tracing/metrics remain unaffected.

**Tech Stack:** Rust, serde, ratatui, Fluent i18n

**Files to modify (6):**
- `peri-acp/src/provider/config.rs`
- `peri-tui/locales/en/main.ftl`
- `peri-tui/locales/zh-CN/main.ftl`
- `peri-tui/src/app/config_panel.rs`
- `peri-tui/src/ui/main_ui/panels/config.rs`
- `peri-tui/src/app/agent_ops/subagent.rs`

**Files needing test update (1):**
- `peri-tui/src/app/config_panel_test.rs`

---

### Task 1: Add `show_cache_warning` to AppConfig

**Files:**
- Modify: `peri-acp/src/provider/config.rs`

- [ ] **Step 1: Add field to AppConfig**

After `streaming_mode` (line 153), add:

```rust
    /// 是否在消息流中显示缓存命中率过低警告
    #[serde(default = "default_show_cache_warning")]
    pub show_cache_warning: bool,
```

Before `AppConfig` impl block, add default function:

```rust
fn default_show_cache_warning() -> bool {
    true
}
```

- [ ] **Step 2: Add to merge_overrides**

In `merge_overrides` (around line 212, after streaming_mode merge), add:

```rust
        // show_cache_warning: bool 直接覆盖
        self.show_cache_warning = workspace.show_cache_warning;
```

- [ ] **Step 3: Verify build**

```bash
cargo build -p peri-acp 2>&1 | head -20
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Commit**

```bash
git add peri-acp/src/provider/config.rs
git commit -m "feat: add show_cache_warning field to AppConfig"
```

---

### Task 2: Add i18n keys

**Files:**
- Modify: `peri-tui/locales/en/main.ftl`
- Modify: `peri-tui/locales/zh-CN/main.ftl`

- [ ] **Step 1: Add English keys**

In `peri-tui/locales/en/main.ftl`, after `config-field-proactiveness` line (line 205), add:

```ftl
config-field-cache-warning = Cache Warning
```

After `config-desc-proactiveness` line, add:

```ftl
config-desc-cache-warning = (ON/OFF — show low cache hit rate warning in chat)
```

- [ ] **Step 2: Add Chinese keys**

In `peri-tui/locales/zh-CN/main.ftl`, after `config-field-proactiveness` line (line 204), add:

```ftl
config-field-cache-warning = 缓存警告
```

After `config-desc-proactiveness` line, add:

```ftl
config-desc-cache-warning = （开/关 — 在对话中显示缓存命中率过低警告）
```

- [ ] **Step 3: Commit**

```bash
git add peri-tui/locales/en/main.ftl peri-tui/locales/zh-CN/main.ftl
git commit -m "feat: add cache warning i18n keys"
```

---

### Task 3: Update ConfigPanel (buffer, row constants, cycle, apply_edit, key/mouse handlers)

**File:** `peri-tui/src/app/config_panel.rs`

- [ ] **Step 1: Renumber row constants**

Replace lines 16–27:

```rust
pub const ROW_GENERAL_HEADER: usize = 0;
pub const ROW_AUTOCOMPACT: usize = 1;
pub const ROW_CACHE_WARNING: usize = 2;
pub const ROW_THRESHOLD: usize = 3;
pub const ROW_LANGUAGE: usize = 4;
pub const ROW_DIFF: usize = 5;
pub const ROW_STREAMING: usize = 6;
pub const ROW_PROACTIVENESS: usize = 7;
pub const ROW_SEPARATOR: usize = 8;
pub const ROW_OVERRIDES_HEADER: usize = 9;
pub const ROW_PERSONA: usize = 10;
pub const ROW_TONE: usize = 11;
pub const ROW_COUNT: usize = 12;
```

- [ ] **Step 2: Add ROW_CACHE_WARNING to next_editable_row**

In `next_editable_row` (line 29), update the `editable` slice to include `ROW_CACHE_WARNING`:

```rust
    let editable: &[usize] = &[
        ROW_AUTOCOMPACT,
        ROW_CACHE_WARNING,
        ROW_THRESHOLD,
        // ... rest unchanged
    ];
```

- [ ] **Step 3: Add ROW_CACHE_WARNING to is_text_row check (not needed—CACHE_WARNING is a bool toggle, not text)**

No change needed for `is_text_row`.

- [ ] **Step 4: Add ROW_CACHE_WARNING to SCREEN_LAYOUT**

Replace lines 61–82 (the entire `SCREEN_LAYOUT`):

```rust
const SCREEN_LAYOUT: &[usize] = &[
    ROW_GENERAL_HEADER,   // screen 0
    ROW_AUTOCOMPACT,      // screen 1: value
    ROW_AUTOCOMPACT,      // screen 2: desc
    ROW_CACHE_WARNING,    // screen 3: value
    ROW_CACHE_WARNING,    // screen 4: desc
    ROW_THRESHOLD,        // screen 5: value
    ROW_THRESHOLD,        // screen 6: desc
    ROW_LANGUAGE,         // screen 7: value
    ROW_LANGUAGE,         // screen 8: desc
    ROW_DIFF,             // screen 9: value
    ROW_DIFF,             // screen 10: desc
    ROW_STREAMING,        // screen 11: value
    ROW_STREAMING,        // screen 12: desc
    ROW_PROACTIVENESS,    // screen 13: value
    ROW_PROACTIVENESS,    // screen 14: desc
    ROW_SEPARATOR,        // screen 15
    ROW_OVERRIDES_HEADER, // screen 16
    ROW_PERSONA,          // screen 17: value
    ROW_PERSONA,          // screen 18: desc
    ROW_TONE,             // screen 19: value
    ROW_TONE,             // screen 20: desc
];
```

- [ ] **Step 5: Add buf_show_cache_warning to ConfigPanel struct**

In `ConfigPanel` struct (line 101), add field after `buf_autocompact`:

```rust
    pub buf_show_cache_warning: bool,
```

- [ ] **Step 6: Initialize in from_config**

In `from_config` (line 115), read from config and include in Self constructor:

Add before `let diff_enabled` line (line 129):

```rust
        let show_cache_warning = cfg.config.show_cache_warning;
```

In the `Self {` constructor (line 140), add after `buf_autocompact`:

```rust
            buf_show_cache_warning: show_cache_warning,
```

- [ ] **Step 7: Add cycle_cache_warning method**

After `cycle_autocompact` (line 167), add:

```rust
    pub fn cycle_cache_warning(&mut self) {
        self.buf_show_cache_warning = !self.buf_show_cache_warning;
    }
```

- [ ] **Step 8: Add to apply_edit**

In `apply_edit` (line 242), before `Ok(())` (line 299), add:

```rust
        // cache warning
        cfg.config.show_cache_warning = self.buf_show_cache_warning;
```

- [ ] **Step 9: Update Space key handler**

In `handle_key` Space match (line 351), add `ROW_CACHE_WARNING` to the outer match arm and its inner cycle:

```rust
                match self.cursor {
                    ROW_AUTOCOMPACT | ROW_CACHE_WARNING | ROW_LANGUAGE | ROW_PROACTIVENESS
                    | ROW_DIFF | ROW_STREAMING => {
                        match self.cursor {
                            ROW_AUTOCOMPACT => self.cycle_autocompact(),
                            ROW_CACHE_WARNING => self.cycle_cache_warning(),
                            // ... rest unchanged
                        }
                        save_config_now(self, ctx);
                    }
```

- [ ] **Step 10: Update Left key handler**

In `handle_key` Left match (line 373), same pattern—add `ROW_CACHE_WARNING` to outer arm:

```rust
                match self.cursor {
                    ROW_AUTOCOMPACT | ROW_CACHE_WARNING | ROW_LANGUAGE | ROW_PROACTIVENESS
                    | ROW_DIFF | ROW_STREAMING => {
                        match self.cursor {
                            ROW_AUTOCOMPACT => self.cycle_autocompact(),
                            ROW_CACHE_WARNING => self.cycle_cache_warning(),
                            // ... rest unchanged
                        }
```

- [ ] **Step 11: Update Right key handler**

Same as Left (line 397), identical pattern—add `ROW_CACHE_WARNING` to both the outer and inner match arms.

- [ ] **Step 12: Update mouse handler**

In `handle_mouse` (line 439), add `ROW_CACHE_WARNING` to the `matches!` arm (line 451):

```rust
                    if matches!(
                        clicked,
                        ROW_AUTOCOMPACT
                            | ROW_CACHE_WARNING
                            | ROW_THRESHOLD
                            // ... rest unchanged
                    )
```

- [ ] **Step 13: Verify build**

```bash
cargo build -p peri-tui 2>&1 | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 14: Commit**

```bash
git add peri-tui/src/app/config_panel.rs
git commit -m "feat: add cache warning toggle to ConfigPanel"
```

---

### Task 4: Render cache warning row in config panel

**File:** `peri-tui/src/ui/main_ui/panels/config.rs`

- [ ] **Step 1: Add ROW_CACHE_WARNING import**

At line 12, add `ROW_CACHE_WARNING` to the import from `config_panel`:

```rust
    config_panel::{
        ConfigPanel, ROW_AUTOCOMPACT, ROW_CACHE_WARNING, ROW_COUNT, ROW_DIFF, ROW_GENERAL_HEADER,
        ROW_LANGUAGE, ROW_OVERRIDES_HEADER, ROW_PERSONA, ROW_PROACTIVENESS, ROW_SEPARATOR,
        ROW_STREAMING, ROW_THRESHOLD, ROW_TONE,
    },
```

- [ ] **Step 2: Add to field_label_key**

In `field_label_key` (line 24), add case before `_ => "???"`:

```rust
        ROW_CACHE_WARNING => "config-field-cache-warning",
```

- [ ] **Step 3: Add rendering block**

After the `ROW_AUTOCOMPACT` rendering block (ends around line 147, after the desc line push), add a new `ROW_CACHE_WARNING` arm in the match statement:

In the render function's main `for row in 0..ROW_COUNT` loop (line 88), the ROW_CACHE_WARNING case must be handled. Since we inserted it at index 2 in the screen layout (between AUTOCOMPACT and THRESHOLD), the rendering should go right after the ROW_AUTOCOMPACT case.

Insert after the `ROW_AUTOCOMPACT` block (after line 147, before `ROW_LANGUAGE`):

```rust
            ROW_CACHE_WARNING => {
                let is_active = panel.cursor == row;
                let label_style = active_or_text(is_active);
                let active_style = Style::default()
                    .fg(theme::THINKING)
                    .add_modifier(Modifier::BOLD);
                let inactive_style = Style::default().fg(theme::MUTED);
                let desc_style = Style::default().fg(theme::MUTED);

                let on_span = if panel.buf_show_cache_warning {
                    Span::styled(format!("[{}]", lc.tr("config-value-on")), active_style)
                } else {
                    Span::styled(lc.tr("config-value-on"), inactive_style)
                };
                let off_span = if panel.buf_show_cache_warning {
                    Span::styled(lc.tr("config-value-off"), inactive_style)
                } else {
                    Span::styled(format!("[{}]", lc.tr("config-value-off")), active_style)
                };

                lines.push(Line::from(vec![
                    Span::styled("  ", Style::default()),
                    Span::styled(
                        format!(
                            "{:<width$}",
                            lc.tr(field_label_key(row)),
                            width = label_column_width
                        ),
                        label_style,
                    ),
                    on_span,
                    Span::styled("  ", Style::default()),
                    off_span,
                ]));
                lines.push(Line::from(Span::styled(
                    format!("      {}", lc.tr("config-desc-cache-warning")),
                    desc_style,
                )));
            }
```

- [ ] **Step 4: Verify build**

```bash
cargo build -p peri-tui 2>&1 | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/ui/main_ui/panels/config.rs
git commit -m "feat: render cache warning toggle in config panel"
```

---

### Task 5: Gate cache warning push on config value

**File:** `peri-tui/src/app/agent_ops/subagent.rs`

- [ ] **Step 1: Add config check**

In `handle_token_usage_update`, inside the `if rate < 0.8` block (line 31), wrap the `MessageViewModel::system(msg)` creation and pipeline push (lines 54–65) with a config check:

Replace lines 52–65:

```rust
            let percentage = (rate * 100.0) as u32;
            let req_id = tracker.last_request_id.as_deref().unwrap_or("-");
            let msg = format!(
                "⚠ {}",
                self.services.lc.tr_args(
                    "app-prompt-cache-low",
                    &[
                        ("rate".into(), (percentage as i64).into()),
                        ("req".into(), req_id.to_string().into()),
                    ]
                )
            );
            let vm = MessageViewModel::system(msg);
            self.apply_pipeline_action(PipelineAction::AddMessage(vm));
```

With:

```rust
            // 检查配置：show_cache_warning 为 false 时跳过消息流展示
            if self.services.peri_config.read().config.show_cache_warning {
                let percentage = (rate * 100.0) as u32;
                let req_id = tracker.last_request_id.as_deref().unwrap_or("-");
                let msg = format!(
                    "⚠ {}",
                    self.services.lc.tr_args(
                        "app-prompt-cache-low",
                        &[
                            ("rate".into(), (percentage as i64).into()),
                            ("req".into(), req_id.to_string().into()),
                        ]
                    )
                );
                let vm = MessageViewModel::system(msg);
                self.apply_pipeline_action(PipelineAction::AddMessage(vm));
            }
```

Note: tracing::warn and metrics::emit remain outside the check (lines 34–50), always executed.

- [ ] **Step 2: Verify build**

```bash
cargo build -p peri-tui 2>&1 | tail -5
```

Expected: `Finished` with no errors.

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/app/agent_ops/subagent.rs
git commit -m "feat: gate cache warning push on config show_cache_warning"
```

---

### Task 6: Update config_panel tests

**File:** `peri-tui/src/app/config_panel_test.rs`

- [ ] **Step 1: Update cursor navigation test**

In `test_config_panel_cursor_navigation`, add `ROW_CACHE_WARNING` between `ROW_AUTOCOMPACT` and `ROW_THRESHOLD`:

Replace lines 22–56 (the cursor_down and cursor_up blocks):

```rust
    panel.cursor_down();
    assert_eq!(panel.cursor, ROW_THRESHOLD);
    panel.cursor_down();
    assert_eq!(panel.cursor, ROW_LANGUAGE);
```

With:

```rust
    panel.cursor_down();
    assert_eq!(panel.cursor, ROW_CACHE_WARNING);
    panel.cursor_down();
    assert_eq!(panel.cursor, ROW_THRESHOLD);
    panel.cursor_down();
    assert_eq!(panel.cursor, ROW_LANGUAGE);
```

And update cursor_up block similarly, inserting `ROW_CACHE_WARNING` between `ROW_AUTOCOMPACT` and `ROW_THRESHOLD`:

Replace:
```rust
    panel.cursor_up();
    assert_eq!(panel.cursor, ROW_THRESHOLD);
    panel.cursor_up();
    assert_eq!(panel.cursor, ROW_AUTOCOMPACT);
```

With:
```rust
    panel.cursor_up();
    assert_eq!(panel.cursor, ROW_CACHE_WARNING);
    panel.cursor_up();
    assert_eq!(panel.cursor, ROW_AUTOCOMPACT);
```

- [ ] **Step 2: Add cycle_cache_warning test**

After `test_config_panel_cycle_autocompact` (line 91), add:

```rust
#[test]
fn test_config_panel_cycle_cache_warning() {
    let mut panel = ConfigPanel::from_config(&PeriConfig::default());
    assert!(panel.buf_show_cache_warning);
    panel.cycle_cache_warning();
    assert!(!panel.buf_show_cache_warning);
    panel.cycle_cache_warning();
    assert!(panel.buf_show_cache_warning);
}
```

- [ ] **Step 3: Add apply_edit test for show_cache_warning**

After `test_config_panel_apply_edit_diff_enabled` (line 210), add:

```rust
#[test]
fn test_config_panel_apply_edit_show_cache_warning() {
    let lc = make_lc();
    let mut cfg = PeriConfig::default();
    let mut panel = ConfigPanel::from_config(&cfg);
    panel.buf_show_cache_warning = false;
    panel.apply_edit(&mut cfg, &lc).unwrap();
    assert!(!cfg.config.show_cache_warning);
}
```

- [ ] **Step 4: Run tests**

```bash
cargo test -p peri-tui --lib config_panel_test -- --nocapture
```

Expected: All tests PASS.

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/config_panel_test.rs
git commit -m "test: update config_panel tests for cache warning toggle"
```

---

### Task 7: Integration verification

- [ ] **Step 1: Run full config_panel test suite**

```bash
cargo test -p peri-tui --lib config_panel_test
```

Expected: All PASS.

- [ ] **Step 2: Run peri-acp tests**

```bash
cargo test -p peri-acp --lib config_test
```

Expected: All PASS.

- [ ] **Step 3: Full build**

```bash
cargo build
```

Expected: `Finished` with no errors.

- [ ] **Step 4: Final commit (if any leftover)**

```bash
git status
```

---

## Row Renumbering Summary

| Old Constant | Old Value | New Value | Delta |
|---|---|---|---|
| `ROW_AUTOCOMPACT` | 1 | 1 | — |
| `ROW_CACHE_WARNING` | — | **2** | **NEW** |
| `ROW_THRESHOLD` | 2 | 3 | +1 |
| `ROW_LANGUAGE` | 3 | 4 | +1 |
| `ROW_DIFF` | 4 | 5 | +1 |
| `ROW_STREAMING` | 5 | 6 | +1 |
| `ROW_PROACTIVENESS` | 6 | 7 | +1 |
| `ROW_SEPARATOR` | 7 | 8 | +1 |
| `ROW_OVERRIDES_HEADER` | 8 | 9 | +1 |
| `ROW_PERSONA` | 9 | 10 | +1 |
| `ROW_TONE` | 10 | 11 | +1 |
| `ROW_COUNT` | 11 | 12 | +1 |
