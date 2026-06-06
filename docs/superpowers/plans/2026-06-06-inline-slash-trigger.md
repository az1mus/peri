# Inline Slash Trigger for Skills and Commands Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 支持在 TUI textarea 任意位置（空白字符后）输入 `/` 触发 skill/command 候选弹窗，Tab/Enter 补全时仅替换当前 token 而非整个 textarea。

**Architecture:** 参考 `@mention` 的 `AtMentionState` 模式，新增 `SlashHintState` 结构体管理内联触发状态。在键盘事件处理中，每次文本变更时检测光标前最近的 `/` token。`build_hint_items()` 和 `render_unified_hint()` 从 `slash_hint` 读取 prefix 和 active 状态替代原有的 `first_line.starts_with('/')`。`hint_complete()` 仿照 `inject_at_mention_path()` 实现局部替换。Enter 提交和 slash command dispatch 逻辑不变。

**Tech Stack:** Rust, ratatui, tui-textarea

---

## File Structure

| 文件 | 职责 | 变更类型 |
|------|------|----------|
| `peri-tui/src/app/hint_ops.rs` | `SlashHintState` 结构体 + 检测逻辑 + `build_hint_items`/`hint_complete` 改造 | Modify |
| `peri-tui/src/app/hint_ops_test.rs` | 新增检测 + 局部替换单元测试 | Modify |
| `peri-tui/src/app/ui_state.rs` | 新增 `slash_hint: SlashHintState` 字段 | Modify |
| `peri-tui/src/ui/main_ui/popups/hints.rs` | `render_unified_hint` 改为检查 `slash_hint` 状态 | Modify |
| `peri-tui/src/event/keyboard.rs` | 新增 `update_slash_hint_detection()` | Modify |
| `peri-tui/src/event/keyboard/normal_keys.rs` | 文本变更时调用检测 + Esc 关闭 slash hint | Modify |

---

### Task 1: Add SlashHintState with detection logic

**Files:**
- Modify: `peri-tui/src/app/hint_ops.rs`
- Modify: `peri-tui/src/app/ui_state.rs`

- [ ] **Step 1: 在 hint_ops.rs 顶部添加 SlashHintState 结构体**

在 `use super::*;` 行之后、`enum HintItem` 之前插入：

```rust
/// Inline / skill/command 补全触发状态。
/// 参考 `AtMentionState::detect()` 模式：在光标前回溯查找 `/` token，
/// 要求 `/` 前为空白字符或行首以避免 `and/or` 等正常文本误触发。
#[derive(Default)]
pub struct SlashHintState {
    pub active: bool,
    /// `/` 符号在全文中的字节偏移
    pub token_start: usize,
    /// `/` 之后的文本（用于过滤候选，如 "code" 匹配 /code-review）
    pub prefix: String,
}

impl SlashHintState {
    /// 在 `text[..cursor_pos]` 中检测光标前最近的 `/` token。
    /// 要求 `/` 前为空白字符/行首，`/` 后为有效 skill/command 名字符（字母数字 `-` `_` `:` `.`）。
    /// 返回 `(prefix, slash_byte_offset)` 或 `None`。
    pub fn detect(text: &str, cursor_pos: usize) -> Option<(String, usize)> {
        if cursor_pos == 0 || cursor_pos > text.len() {
            return None;
        }
        let before_cursor = &text[..cursor_pos];
        let slash_pos = before_cursor.rfind('/')?;
        let after_slash = &before_cursor[slash_pos + '/'.len_utf8()..];
        
        // 检查 / 前是否为空白字符或行首
        if slash_pos > 0 {
            let char_before = before_cursor[..slash_pos]
                .chars()
                .next_back()
                .unwrap();
            if !char_before.is_whitespace() && char_before != '\n' {
                return None;
            }
        }
        
        // / 后允许空（仅 / 时展示全部候选）或仅含有效名字符
        if !after_slash.is_empty() {
            if !after_slash
                .chars()
                .all(|c| c.is_alphanumeric() || c == '-' || c == '_' || c == ':' || c == '.')
            {
                return None;
            }
        }
        
        Some((after_slash.to_string(), slash_pos))
    }

    pub fn activate(&mut self, prefix: String, token_start: usize) {
        self.active = true;
        self.prefix = prefix;
        self.token_start = token_start;
    }

    pub fn deactivate(&mut self) {
        self.active = false;
        self.prefix.clear();
    }
}
```

- [ ] **Step 2: 在 UiState 中添加 slash_hint 字段**

修改 `peri-tui/src/app/ui_state.rs`：

```rust
use super::hint_ops::SlashHintState;
```

在 `at_mention: AtMentionState,` 后添加：

```rust
    /// / skill/command 内联补全状态
    pub slash_hint: SlashHintState,
```

在 `UiState::new()` 的构造器中添加：

```rust
            slash_hint: SlashHintState::default(),
```

- [ ] **Step 3: 编写 SlashHintState::detect 单元测试**

在 `peri-tui/src/app/hint_ops_test.rs` 文件开头（`make_skill` 之前）添加：

```rust
    mod slash_hint_tests {
        use super::*;

        #[test]
        fn test_detect_slash_at_line_start() {
            let (prefix, pos) = SlashHintState::detect("/code", 5).expect("行首 / 应被检测");
            assert_eq!(prefix, "code");
            assert_eq!(pos, 0);
        }

        #[test]
        fn test_detect_slash_after_space() {
            let (prefix, pos) = SlashHintState::detect("帮我 review /code", 14)
                .expect("空格后 / 应被检测");
            assert_eq!(prefix, "code");
            assert_eq!(pos, "帮我 review ".len());
        }

        #[test]
        fn test_detect_slash_only_no_prefix() {
            let (prefix, pos) = SlashHintState::detect("/", 1).expect("仅有 / 应被检测");
            assert_eq!(prefix, "");
            assert_eq!(pos, 0);
        }

        #[test]
        fn test_detect_slash_after_space_no_prefix() {
            let (prefix, pos) = SlashHintState::detect("hello /", 7)
                .expect("空格后 / 应被检测");
            assert_eq!(prefix, "");
            assert_eq!(pos, 6);
        }

        #[test]
        fn test_detect_slash_preceded_by_letter_is_none() {
            // "and/or" 中的 / 不应触发
            assert!(SlashHintState::detect("and/or", 5).is_none());
        }

        #[test]
        fn test_detect_slash_after_newline() {
            let text = "第一行\n/command";
            let (prefix, pos) = SlashHintState::detect(text, text.len())
                .expect("换行后 / 应被检测");
            assert_eq!(prefix, "command");
        }

        #[test]
        fn test_detect_slash_with_path_like() {
            // /foo/bar 中含有 / 但 token 以空格分隔，第一个 / 会被检测
            let (prefix, pos) = SlashHintState::detect("查看 /src/main.rs", 17)
                .expect("空格后 / 应被检测");
            assert_eq!(prefix, "src");
            assert_eq!(pos, "查看 ".len());
        }

        #[test]
        fn test_detect_cursor_before_slash_is_none() {
            // 光标在 / 之前
            assert!(SlashHintState::detect("abc /code", 3).is_none());
        }

        #[test]
        fn test_detect_cursor_at_zero_is_none() {
            assert!(SlashHintState::detect("/code", 0).is_none());
        }
    }
```

- [ ] **Step 4: 运行测试验证**

Run: `cargo test -p peri-tui --lib -- hint_ops::tests::slash_hint_tests 2>&1 | tail -20`
Expected: 全部 9 个测试通过

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/hint_ops.rs peri-tui/src/app/hint_ops_test.rs peri-tui/src/app/ui_state.rs
git commit -m "feat: 添加 SlashHintState 及 / token 检测逻辑

新增 SlashHintState 结构体（active/token_start/prefix），参考
AtMentionState::detect() 模式在光标前回溯查找 / token，
要求 / 前为空白字符或行首以避免正常文本误触发。

Part of: spec/issues/2026-06-06-inline-slash-trigger-for-skills-and-commands.md

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 2: Update build_hint_items and hint_complete for inline slash

**Files:**
- Modify: `peri-tui/src/app/hint_ops.rs` — `build_hint_items()`, `hint_complete()`
- Modify: `peri-tui/src/app/hint_ops_test.rs` — 更新现有测试

- [ ] **Step 1: 重写 build_hint_items()——使用 slash_hint 状态**

替换 `build_hint_items()` 方法体。删除从 `first_line` 提取 prefix 的逻辑，改为从 `slash_hint` 读取：

```rust
    fn build_hint_items(&self) -> Vec<HintItem> {
        let slash_hint = &self.session_mgr.current().ui.slash_hint;
        if !slash_hint.active {
            return vec![];
        }
        let prefix = &slash_hint.prefix;
        let cmd_candidates: Vec<_> = self
            .session_mgr
            .current()
            .commands
            .command_registry
            .match_prefix(prefix, &self.services.lc);
        let skill_candidates: Vec<_> = self
            .session_mgr
            .current()
            .commands
            .skills
            .iter()
            .filter(|s| prefix.is_empty() || s.name.contains(prefix))
            .collect();
        let agent_cmd_candidates: Vec<_> = self
            .session_mgr
            .current()
            .commands
            .agent_commands
            .iter()
            .filter(|n| prefix.is_empty() || n.contains(prefix))
            .collect();

        let mut items: Vec<HintItem> = Vec::new();
        for (name, _) in &cmd_candidates {
            items.push(HintItem::Cmd { name: name.clone() });
        }
        for skill in &skill_candidates {
            items.push(HintItem::Skill {
                name: skill.name.clone(),
            });
        }
        for name in &agent_cmd_candidates {
            items.push(HintItem::AgentCmd {
                name: (*name).clone(),
            });
        }
        items.sort_by(|a, b| {
            let a_starts = a.name().starts_with(prefix) as u8;
            let b_starts = b.name().starts_with(prefix) as u8;
            let a_rank = match a {
                HintItem::Cmd { .. } => 2,
                HintItem::Skill { .. } => 1,
                HintItem::AgentCmd { .. } => 0,
            };
            let b_rank = match b {
                HintItem::Cmd { .. } => 2,
                HintItem::Skill { .. } => 1,
                HintItem::AgentCmd { .. } => 0,
            };
            b_starts
                .cmp(&a_starts)
                .then_with(|| b_rank.cmp(&a_rank))
                .then_with(|| a.name().cmp(b.name()))
        });
        items
    }
```

变化点：删除 `first_line.starts_with('/')`，新增 `if !slash_hint.active` 早退 + `let prefix = &slash_hint.prefix;`。

- [ ] **Step 2: 重写 hint_complete()——局部替换 token**

替换 `hint_complete()` 方法体，仿照 `inject_at_mention_path()` 实现局部替换：

```rust
    pub fn hint_complete(&mut self) {
        let selected_name = {
            let items = self.build_hint_items();
            let cursor = self.session_mgr.current().ui.hint_cursor.unwrap_or(0);
            items.get(cursor).map(|item| item.name().to_string())
        };

        if let Some(name) = selected_name {
            let slash_hint = &self.session_mgr.current().ui.slash_hint;
            let full_text: String = self
                .session_mgr
                .current()
                .ui
                .textarea
                .lines()
                .join("\n");
            let slash_pos = slash_hint.token_start;
            // token 总长度：/ + prefix
            let token_len = 1 + slash_hint.prefix.len();
            let replacement = format!("/{} ", name);

            // 构造新文本：保留 / 之前和 token 之后的内容
            let mut new_text = String::with_capacity(full_text.len() + replacement.len());
            new_text.push_str(&full_text[..slash_pos]);
            new_text.push_str(&replacement);
            let after_end = slash_pos + token_len;
            if after_end < full_text.len() {
                new_text.push_str(&full_text[after_end..]);
            }

            let mut new_ta = crate::app::build_textarea(false);
            new_ta.insert_str(&new_text);
            self.session_mgr.current_mut().ui.textarea = new_ta;

            // 关闭 slash hint
            self.session_mgr.current_mut().ui.slash_hint.deactivate();
            self.session_mgr.current_mut().ui.hint_cursor = None;
        }
    }
```

- [ ] **Step 3: 更新现有测试适配新的 slash_hint 依赖**

`test_candidates_count_slash_prefix_returns_cmd_plus_skills`：注入 `/` 后需手动设置 `slash_hint.active = true` + `slash_hint.prefix = ""`

```rust
    #[tokio::test]
    async fn test_candidates_count_slash_prefix_returns_cmd_plus_skills() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.current_mut().ui.textarea = build_textarea(false);
        app.session_mgr.current_mut()
            .ui
            .textarea
            .insert_str("/");
        // 手动设置 slash hint 状态（无用户键入事件时手动激活）
        app.session_mgr.current_mut().ui.slash_hint.activate(String::new(), 0);
        app.session_mgr.current_mut()
            .commands
            .skills
            .push(make_skill("aaa-skill"));
        app.session_mgr.current_mut()
            .commands
            .skills
            .push(make_skill("zzz-skill"));

        let count = app.hint_candidates_count();
        let cmd_count = app.session_mgr.current_mut()
            .commands
            .command_registry
            .match_prefix("", &app.services.lc)
            .len();
        let expected = cmd_count + 2;
        assert_eq!(count, expected, "/ 前缀应返回命令数 + Skills 数");
    }
```

`test_candidates_count_slash_prefix_filters_both`：同上，设置 prefix 为 "mo"

```rust
    #[tokio::test]
    async fn test_candidates_count_slash_prefix_filters_both() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.current_mut().ui.textarea = build_textarea(false);
        app.session_mgr.current_mut()
            .ui
            .textarea
            .insert_str("/mo");
        app.session_mgr.current_mut().ui.slash_hint.activate("mo".to_string(), 0);
        app.session_mgr.current_mut()
            .commands
            .skills
            .push(make_skill("commit"));
        app.session_mgr.current_mut()
            .commands
            .skills
            .push(make_skill("model-skill"));

        let count = app.hint_candidates_count();
        assert!(
            count >= 2,
            "/mo 前缀应至少返回 model 命令 + model-skill 技能"
        );
    }
```

`test_candidates_count_no_prefix_returns_zero`：不变（slash_hint 默认 inactive → 返回 0）

`test_hint_complete_command_at_cursor_0`：设置 slash_hint 状态

```rust
    #[tokio::test]
    async fn test_hint_complete_command_at_cursor_0() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.current_mut().ui.textarea = build_textarea(false);
        app.session_mgr.current_mut()
            .ui
            .textarea
            .insert_str("/m");
        app.session_mgr.current_mut().ui.slash_hint.activate("m".to_string(), 0);
        app.session_mgr.current_mut()
            .ui
            .hint_cursor = Some(0);

        app.hint_complete();
        let text: String = app.session_mgr.current_mut()
            .ui
            .textarea
            .lines()
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert!(text.starts_with("/"), "补全后应以 / 开头，实际: {}", text);
        assert!(
            app.session_mgr.current_mut()
                .ui
                .hint_cursor
                .is_none(),
            "补全后 hint_cursor 应为 None"
        );
        // slash_hint 应被 deactivate
        assert!(
            !app.session_mgr.current_mut().ui.slash_hint.active,
            "补全后 slash_hint 应 inactive"
        );
    }
```

`test_hint_complete_clears_hint_cursor`：同上加 slash_hint 激活

```rust
    #[tokio::test]
    async fn test_hint_complete_clears_hint_cursor() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.current_mut().ui.textarea = build_textarea(false);
        app.session_mgr.current_mut()
            .ui
            .textarea
            .insert_str("/m");
        app.session_mgr.current_mut().ui.slash_hint.activate("m".to_string(), 0);
        app.session_mgr.current_mut()
            .ui
            .hint_cursor = Some(0);

        app.hint_complete();
        assert_eq!(
            app.session_mgr.current_mut()
                .ui
                .hint_cursor,
            None,
            "补全后 hint_cursor 应为 None"
        );
    }
```

`test_hint_complete_skill_item`：同上加 slash_hint 激活

```rust
    #[tokio::test]
    async fn test_hint_complete_skill_item() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.current_mut().ui.textarea = build_textarea(false);
        app.session_mgr.current_mut()
            .ui
            .textarea
            .insert_str("/aaa");
        app.session_mgr.current_mut().ui.slash_hint.activate("aaa".to_string(), 0);
        app.session_mgr.current_mut()
            .commands
            .skills
            .push(make_skill("aaa-skill"));

        let items = app.build_hint_items();
        let idx = items
            .iter()
            .position(|it| it.name() == "aaa-skill")
            .expect("应有 aaa-skill 候选");
        app.session_mgr.current_mut()
            .ui
            .hint_cursor = Some(idx);

        app.hint_complete();
        let text: String = app.session_mgr.current_mut()
            .ui
            .textarea
            .lines()
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert!(
            text.starts_with("/aaa-skill "),
            "应补全 Skill aaa-skill，实际: {}",
            text
        );
    }
```

- [ ] **Step 4: 添加局部替换的单元测试**

在 `hint_ops_test.rs` 末尾添加：

```rust
    #[tokio::test]
    async fn test_hint_complete_inline_replaces_only_token() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        // 用户输入: "帮我 review /code"
        app.session_mgr.current_mut().ui.textarea = build_textarea(false);
        app.session_mgr.current_mut()
            .ui
            .textarea
            .insert_str("帮我 review /code");
        // / 在字节偏移 9 (按字节数: "帮我 review " = 3*3+1+7+1=...)
        // 实际: 帮助 = 6 bytes,  review = 7 bytes, space = 1 byte = total 14 bytes
        // Wait: "帮" = 3, "我" = 3, " " = 1, "r"=1, "e"=1, "v"=1, "i"=1, "e"=1, "w"=1, " "=1, "/"=1 = 15
        // Actually: Chinese chars "帮我" = 6 bytes(3 each), " review " = 8 bytes, "/" = 1, "code" = 4
        // = 6 + 8 + 1 + 4 = 19... 
        // Let me compute more carefully:
        // "帮"(3) + "我"(3) + " "(1) + "r"(1) + "e"(1) + "v"(1) + "i"(1) + "e"(1) + "w"(1) + " "(1) + "/"(1) + "code"(4)
        // = 3+3+1+1+1+1+1+1+1+1+1+4 = 19. Slash at byte 14 (6+8=14)
        let slash_pos = "帮我 review ".len(); // 6+8=14
        app.session_mgr.current_mut().ui.slash_hint.activate("code".to_string(), slash_pos);
        // 注册一个匹配的 skill
        app.session_mgr.current_mut()
            .commands
            .skills
            .push(make_skill("code-review"));
        // 找到 code-review 在排序后的索引
        let items = app.build_hint_items();
        let idx = items.iter().position(|it| it.name() == "code-review")
            .expect("应有 code-review 候选");
        app.session_mgr.current_mut().ui.hint_cursor = Some(idx);

        app.hint_complete();
        let text: String = app.session_mgr.current_mut()
            .ui
            .textarea
            .lines()
            .iter()
            .map(|s| s.as_str())
            .collect();
        // 期望: "帮我 review /code-review " — 仅 /code 被替换
        assert_eq!(
            text,
            "帮我 review /code-review ",
            "应仅替换 /code token，保留前缀文本"
        );
        assert!(!app.session_mgr.current_mut().ui.slash_hint.active);
    }

    #[tokio::test]
    async fn test_hint_complete_inline_middle_of_text() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        // 用户输入: "hello /code world"
        app.session_mgr.current_mut().ui.textarea = build_textarea(false);
        app.session_mgr.current_mut()
            .ui
            .textarea
            .insert_str("hello /code world");
        // hello = 5, space = 1, / = 1 => slash_pos = 6
        let slash_pos = 6;
        app.session_mgr.current_mut().ui.slash_hint.activate("code".to_string(), slash_pos);
        app.session_mgr.current_mut()
            .commands
            .skills
            .push(make_skill("code-review"));
        let items = app.build_hint_items();
        let idx = items.iter().position(|it| it.name() == "code-review")
            .expect("应有 code-review 候选");
        app.session_mgr.current_mut().ui.hint_cursor = Some(idx);

        app.hint_complete();
        let text: String = app.session_mgr.current_mut()
            .ui
            .textarea
            .lines()
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert_eq!(
            text,
            "hello /code-review  world",
            "应仅替换中间的 /code token"
        );
    }
```

- [ ] **Step 5: 运行测试验证**

Run: `cargo test -p peri-tui --lib -- hint_ops::tests 2>&1 | tail -20`
Expected: 所有测试通过（包括新增和更新的）

- [ ] **Step 6: Commit**

```bash
git add peri-tui/src/app/hint_ops.rs peri-tui/src/app/hint_ops_test.rs
git commit -m "feat: build_hint_items/hint_complete 改为基于 slash_hint 状态的内联补全

build_hint_items() 从 slash_hint.active/prefix 读取触发状态替代
first_line.starts_with('/')。hint_complete() 仿照
inject_at_mention_path 实现局部 token 替换，仅替换 /prefix
token 保留消息其余内容。

Part of: spec/issues/2026-06-06-inline-slash-trigger-for-skills-and-commands.md

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 3: Update render_unified_hint for slash_hint state

**Files:**
- Modify: `peri-tui/src/ui/main_ui/popups/hints.rs`

- [ ] **Step 1: 替换 render_unified_hint 触发检测逻辑**

将 `render_unified_hint()` 中第 23-35 行的 `first_line` 检测替换为 `slash_hint` 检测：

替换前（第 23-35 行）：
```rust
    let first_line = app
        .session_mgr
        .current()
        .ui
        .textarea
        .lines()
        .first()
        .map(|s| s.as_str())
        .unwrap_or("");
    if !first_line.starts_with('/') {
        return;
    }

    let prefix = first_line.trim_start_matches('/');
```

替换为：
```rust
    let slash_hint = &app.session_mgr.current().ui.slash_hint;
    if !slash_hint.active {
        return;
    }
    let prefix = &slash_hint.prefix;
```

注意后续代码中 `prefix` 类型从 `&str` 变为 `&&str`（因为是 `slash_hint.prefix` 的引用）。需要将代码中的 `prefix` 改为 `*prefix` 或 `prefix.as_str()`。但 Rust 的 auto-deref 在大多数场景下会处理 — 需要检查调用点：

- `s.name.contains(prefix)` → `contains` 接受 `&str`，`&&str` 会 auto-deref ✓
- `s.name.starts_with(prefix)` → 同上 ✓
- `match_prefix(prefix, &app.services.lc)` → 需要确认 `match_prefix` 签名
- `name.find(prefix)` → `find` 接受 `&str`，`&&str` 会 auto-deref ✓
- `prefix.is_empty()` → auto-deref ✓

- [ ] **Step 2: 补齐 prefix 类型引用调整**

检查 `cmd_candidates` 处对 `match_prefix` 的调用（第 38-43 行）。该函数签名为 `match_prefix(&self, prefix: &str, lc: &LanguageClassifier) -> Vec<(String, String)>`。`&&str` 到 `&str` 应该能 auto-deref，但保守起见显式解引用：

```rust
    let cmd_candidates: Vec<(String, String)> = app
        .session_mgr
        .current()
        .commands
        .command_registry
        .match_prefix(prefix, &app.services.lc);
```

如果编译报类型不匹配，改为：
```rust
        .match_prefix(&prefix, &app.services.lc);
```
（Rust 的 `&&str` 自动 deref 为 `&str` 通常没问题）

- [ ] **Step 3: 构建验证**

Run: `cargo build -p peri-tui 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add peri-tui/src/ui/main_ui/popups/hints.rs
git commit -m "feat: render_unified_hint 改为基于 slash_hint 状态的触发检测

render_unified_hint() 从 slash_hint.active/prefix 读取触发状态
替代 first_line.starts_with('/')，支持在消息任意位置触发
hint 弹窗。

Part of: spec/issues/2026-06-06-inline-slash-trigger-for-skills-and-commands.md

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 4: Add keyboard event integration

**Files:**
- Modify: `peri-tui/src/event/keyboard.rs` — 新增 `update_slash_hint_detection()`
- Modify: `peri-tui/src/event/keyboard/normal_keys.rs` — 文本变更时调用 + Esc 关闭

- [ ] **Step 1: 在 keyboard.rs 中添加 update_slash_hint_detection 函数**

在 `update_at_mention_detection` 函数之后（约第 207 行）添加：

```rust
/// 检测 textarea 中是否有 / skill/command token 触发，更新 slash_hint 状态
pub(super) fn update_slash_hint_detection(app: &mut App) {
    let textarea = &app.session_mgr.current_mut().ui.textarea;
    let text = textarea.lines().join("\n");
    let (row, col) = textarea.cursor();
    // 将 (row, col) 转为字节偏移
    let mut pos = 0usize;
    for (i, line) in textarea.lines().iter().enumerate() {
        if i == row {
            pos += line.chars().take(col).map(|c| c.len_utf8()).sum::<usize>();
            break;
        }
        pos += line.len() + 1; // +1 for \n
    }

    let slash = &mut app.session_mgr.current_mut().ui.slash_hint;

    // 优先关闭（如果 at_mention 活跃则不激活 slash hint，避免双弹窗）
    if app.session_mgr.current().ui.at_mention.active {
        slash.deactivate();
        return;
    }

    if let Some((prefix, start)) = SlashHintState::detect(&text, pos) {
        if slash.active && slash.prefix == prefix && slash.token_start == start {
            return; // 未变化
        }
        slash.activate(prefix, start);
        // hint_cursor 由键盘事件的重置逻辑处理（normal_keys.rs L268）
    } else {
        slash.deactivate();
    }
}
```

- [ ] **Step 2: 在 normal_keys.rs 的文本变更分支中调用检测**

修改 `peri-tui/src/event/keyboard/normal_keys.rs` 第 265-270 行的文本变更处理分支。

将 `use super::{inject_at_mention_path, update_at_mention_detection};` 扩展为：

```rust
    use super::{
        inject_at_mention_path, update_at_mention_detection,
        update_slash_hint_detection,
    };
```

在第 269 行 `update_at_mention_detection(app);` 之后添加：

```rust
                update_slash_hint_detection(app);
```

最终代码（第 260-271 行）：
```rust
        input if input.key != Key::Enter => {
            // Exit history browsing
            if app.session_mgr.current_mut().ui.history_index.is_some() {
                app.exit_history();
            }
            app.session_mgr.current_mut().ui.textarea.input(input);
            // When input changes: reset cursor (don't pre-select; wait for user to press Tab/Up/Down)
            if !app.session_mgr.current_mut().ui.loading {
                app.session_mgr.current_mut().ui.hint_cursor = None;
                update_at_mention_detection(app);
                update_slash_hint_detection(app);
            }
        }
```

- [ ] **Step 3: 添加 Esc 关闭 slash hint**

在 `normal_keys.rs` 中 `@mention Esc` 分支后（第 45-46 行后）添加 slash hint 的 Esc 关闭：

第 44-47 行当前为：
```rust
        // Esc: 关闭 @ 提及弹窗
        Input { key: Key::Esc, .. } if app.session_mgr.current_mut().ui.at_mention.active => {
            app.session_mgr.current_mut().ui.at_mention.close();
        }
```

在其后插入：
```rust
        // Esc: 关闭 slash hint 弹窗
        Input { key: Key::Esc, .. } if app.session_mgr.current_mut().ui.slash_hint.active => {
            app.session_mgr.current_mut().ui.slash_hint.deactivate();
            app.session_mgr.current_mut().ui.hint_cursor = None;
        }
```

- [ ] **Step 4: 构建验证**

Run: `cargo build -p peri-tui 2>&1 | tail -10`
Expected: 编译成功

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/event/keyboard.rs peri-tui/src/event/keyboard/normal_keys.rs
git commit -m "feat: 键盘事件集成 slash hint 检测与 Esc 关闭

每次文本变更时调用 update_slash_hint_detection() 检测光标前
/ token。Esc 关闭 slash hint 弹窗。当 @mention 活跃时
slash hint 自动 deactivate 避免双弹窗。

Part of: spec/issues/2026-06-06-inline-slash-trigger-for-skills-and-commands.md

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 5: Integration verification

**Files:**
- No new code — 运行全量测试和手动验证

- [ ] **Step 1: 运行全量测试**

Run: `cargo test -p peri-tui --lib 2>&1 | tail -30`
Expected: 所有测试通过（特别是 `hint_ops::tests` 和 `hints` 相关）

- [ ] **Step 2: 运行 clippy**

Run: `cargo clippy -p peri-tui -- -D warnings 2>&1 | tail -20`
Expected: 无 warning

- [ ] **Step 3: 运行完整构建**

Run: `cargo build 2>&1 | tail -5`
Expected: 编译成功

---

## Manual Verification Checklist

构建成功后，启动 TUI 手动验证以下场景：

| # | 场景 | 操作 | 期望结果 |
|---|------|------|----------|
| 1 | 行首 / | 输入 `/` | 弹出所有命令+Skills 候选项 |
| 2 | 行首 /mo | 输入 `/mo` | 弹出 `/model` 和 Skill 候选项 |
| 3 | 空格后 / | 输入 `abc /` | 弹出所有候选项 |
| 4 | 空格后 /co | 输入 `帮我review /co` | 弹出 `/code-review` 等候选项 |
| 5 | Tab 补全 | 选中候选项按 Tab | 仅替换 `/co` 为 `/code-review ` |
| 6 | Enter 确认 | 选中候选项按 Enter | 仅替换 token，保留前后文本 |
| 7 | Esc 关闭 | 弹窗激活时按 Esc | 弹窗消失 |
| 8 | 行首 /model Enter | 输入 `/model` 按 Enter | 正常执行命令切换模型（旧行为保持） |
| 9 | and/or 不触发 | 输入 `and/or` | 无弹窗（`/` 前非空白） |
| 10 | 消息中 /skill 提交 | 输入 `用 /code-review` 并按 Enter | 消息正常发送给 Agent |

---

## 自检

**1. Spec 覆盖：**
- ✅ 检测逻辑：Task 1 实现 `SlashHintState::detect()`
- ✅ 局部替换：Task 2 `hint_complete()` 局部替换
- ✅ Enter 提交：无需修改（SkillPreloadMiddleware 已支持消息中任意位置 `/skill-name`，行首 `/command` 的 dispatch 逻辑不变）
- ✅ 多行支持：`detect()` 和 `update_slash_hint_detection()` 使用全文 + 字节偏移，天然支持多行

**2. Placeholder 扫描：** 无 TBD/TODO/占位符。所有代码块完整。

**3. 类型一致性：**
- `SlashHintState` 定义在 `hint_ops.rs`，在 `ui_state.rs` 中作为字段使用
- `SlashHintState::detect()` 返回 `Option<(String, usize)>` 与 `AtMentionState::detect()` 签名一致
- `build_hint_items()` 返回 `Vec<HintItem>` 类型不变
- `hint_complete()` 签名不变
- `render_unified_hint()` 签名不变
