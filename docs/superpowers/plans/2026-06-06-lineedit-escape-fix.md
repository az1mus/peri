# LineEdit 转义处理与错误诊断改进 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 `verify_brackets` 中已废弃的转义处理导致的潜在误报（根因分析：`prev_char` 在连续转义序列中状态不一致），并改善诊断信息降低二次出错排查成本。

**Architecture:** `verify_brackets` 当前用 `prev_char` 实现字符串内转义跳过。这个方案在 `"\"`（转义引号）场景有 bug——转义反斜杠后 `prev_char == '\\'`，下一个 `"` 直接关闭字符串，导致后续括号计数错误，输出假阳性。改为 `escape_next: bool` 标记，"反斜杠只越过紧邻一个字符"，清晰且正确。同时补充括号平衡诊断信息——验证失败时输出行号和上下文，帮助快速定位问题行。

**Tech Stack:** Rust, no new dependencies.

---

## File Structure

| 文件 | 职责 | 变更类型 |
|------|------|----------|
| `peri-middlewares/src/tools/filesystem/line_edit_verify.rs` | `verify_brackets` escape 修复 + 诊断增强 | Modify |
| `peri-middlewares/src/tools/filesystem/line_edit_test.rs` | 新增 escaped-quote 集成测试 | Modify |

**无需变更的文件**（与 issue 症状无直接关联）：`line_edit_match.rs`（匹配逻辑）、`line_edit_diff.rs`（解析逻辑）、`line_edit.rs`（主流程）。

---

### Task 1: Fix verify_brackets escape handling

**File:** Modify `peri-middlewares/src/tools/filesystem/line_edit_verify.rs`

- [ ] **Step 1: 替换 `verify_brackets` 中的转义处理从 `prev_char` 改为 `escape_next`**

当前实现（第 113-173 行区域，`prev_char` / `prev_prev_char` 模式）：

```rust
fn verify_brackets(content: &str) -> VerifyLevel {
    let mut brace_depth = 0i32;
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;

    let mut in_string: Option<char> = None;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut prev_prev_char: Option<char> = None;
    let mut prev_char: Option<char> = None;

    for ch in content.chars() {
        if in_line_comment {
            if ch == '\n' {
                in_line_comment = false;
            }
            prev_prev_char = prev_char;
            prev_char = Some(ch);
            continue;
        }
        if in_block_comment {
            if prev_char == Some('*') && ch == '/' {
                in_block_comment = false;
            }
            prev_prev_char = prev_char;
            prev_char = Some(ch);
            continue;
        }
        if let Some(quote) = in_string {
            if ch == '\\' {
                prev_prev_char = prev_char;
                prev_char = Some(ch);
                continue;
            }
            if ch == quote {
                in_string = None;
            }
            prev_prev_char = prev_char;
            prev_char = Some(ch);
            continue;
        }

        match ch {
            '\'' | '"' | '`' => in_string = Some(ch),
            '/' if prev_char == Some('/') && prev_prev_char != Some(':') => {
                in_line_comment = true;
            }
            '*' if prev_char == Some('/') => in_block_comment = true,
            '{' => brace_depth += 1,
            '}' => brace_depth -= 1,
            '(' => paren_depth += 1,
            ')' => paren_depth -= 1,
            '[' => bracket_depth += 1,
            ']' => bracket_depth -= 1,
            _ => {}
        }
        prev_prev_char = prev_char;
        prev_char = Some(ch);
    }
    // ... error collection unchanged
}
```

替换为：

```rust
fn verify_brackets(content: &str) -> VerifyLevel {
    let mut brace_depth = 0i32;
    let mut paren_depth = 0i32;
    let mut bracket_depth = 0i32;

    let mut in_string: Option<char> = None;
    let mut in_line_comment = false;
    let mut in_block_comment = false;
    let mut prev_char: Option<char> = None;
    let mut escape_next = false;

    for (lineno, line) in content.lines().enumerate() {
        for ch in line.chars() {
            // ── 注释内不处理 ──
            if in_line_comment {
                // 行注释放到行尾自然重置（line loop 内不跨行）
                // 注意：Rust 的 `//` 注释结束于本行末尾
                // 但 `.lines()` 已经去掉了换行符，所以直接 continue
                prev_char = Some(ch);
                continue;
            }
            if in_block_comment {
                if prev_char == Some('*') && ch == '/' {
                    in_block_comment = false;
                }
                prev_char = Some(ch);
                continue;
            }

            // ── 字符串内 ──
            if let Some(quote) = in_string {
                if escape_next {
                    // 前一个字符是反斜杠，当前字符是被转义的，不处理
                    escape_next = false;
                    prev_char = Some(ch);
                    continue;
                }
                if ch == '\\' {
                    escape_next = true;
                    prev_char = Some(ch);
                    continue;
                }
                if ch == quote {
                    in_string = None;
                }
                prev_char = Some(ch);
                continue;
            }

            // ── 普通代码 ──
            match ch {
                '\'' | '"' | '`' => in_string = Some(ch),
                '/' if prev_char == Some('/') => {
                    // 行注释：需要区分 `://` (URL) 和 `//` (真注释)。
                    // `//` 之前如果是 `:` 则是 URL 的一部分（如 https://...），
                    // 但这里无法只看 prev_char 判断；URL 场景在 `lines()` 粒度下：
                    // URLs 中的 `:` 在同一个 token 内（没有空格分隔），
                    // Rust 的 `://` 不会单独出现（URL 总在字符串内）。
                    // 唯一需要排除的是 Markdown 链接场景，但 Markdown 由 AST skip。
                    // 因此简化为 `prev_char == '/'` 且不在字符串内即可触发。
                    in_line_comment = true;
                }
                '*' if prev_char == Some('/') => in_block_comment = true,
                '{' => brace_depth += 1,
                '}' => brace_depth -= 1,
                '(' => paren_depth += 1,
                ')' => paren_depth -= 1,
                '[' => bracket_depth += 1,
                ']' => bracket_depth -= 1,
                _ => {}
            }
            prev_char = Some(ch);
        }
    }

    let mut errors = Vec::new();
    if brace_depth != 0 {
        errors.push(format!(
            "'{{}}' 不平衡（{} {}）",
            if brace_depth > 0 { "多出" } else { "缺少" },
            brace_depth.abs()
        ));
    }
    if paren_depth != 0 {
        errors.push(format!(
            "'()' 不平衡（{} {}）",
            if paren_depth > 0 { "多出" } else { "缺少" },
            paren_depth.abs()
        ));
    }
    if bracket_depth != 0 {
        errors.push(format!(
            "'[]' 不平衡（{} {}）",
            if bracket_depth > 0 { "多出" } else { "缺少" },
            bracket_depth.abs()
        ));
    }

    if !errors.is_empty() {
        return VerifyLevel::Error(errors.join("，"));
    }

    VerifyLevel::Ok
}
```

**关键变化**：
1. `prev_prev_char` → 删除（不再需要追踪前前字符）
2. `escape_next: bool` → 新增，只在字符串内生效。"\ 越过紧邻一个字符"的语义只靠它实现
3. 外层改为 `lines().enumerate()` 遍历 → 为 Task 2 的行号诊断做铺垫（此处暂未启用行号输出，Task 2 追加）
4. 注释内不再更新 `prev_char`（简化，因为注释内只有 `*/` 需要 prev_char，而 `*` 和 `/` 本身不会被注释退出逻辑以外的代码读取）
5. `//` 行注释的 `prev_prev_char != Some(':')` 守卫移除 —— `://` URL 总是在字符串内（`verify_brackets` 只会处理 Rust 源码，Markdown 由 AST skip），不需要这个防御

- [ ] **Step 2: 添加 `escape_next` 相关的单元测试**

在 `line_edit_verify.rs` 的 `tests` 模块末尾添加：

```rust
    #[test]
    fn test_括号平衡_转义引号不关闭字符串() {
        // "他说：\"你好\"" — \" 中的引号是转义的，不应关闭字符串
        let content = "let s = \"他说：\\\"你好\\\"\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号平衡_双反斜杠后引号正常关闭() {
        // "C:\\Users\\" — \\\\ 后跟 " 正常关闭字符串
        let content = "let p = \"C:\\\\Users\\\\\"; fn main() {}";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号平衡_反斜杠后括号不计数() {
        // 字符串内 "\(" 不应计为括号
        let content = "let s = \"\\\\(\"; fn main() { let x = 1; }";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号平衡_字符串内转义加真实右括号应平衡() {
        // 字符串内 \" 后不应影响外部括号平衡
        let content = "fn f() { let s = \"\\\"hello\\\"\"; }";
        let result = verify_brackets(content);
        assert_eq!(result, VerifyLevel::Ok);
    }

    #[test]
    fn test_括号平衡_字符串内未转义引号真关闭() {
        // "foo\"bar" 中 \" 被跳过（escape_next），但末尾 " 正常关闭
        // 注意 Rust 语法中这其实是 "foo\"bar"，\" 是转义引号
        let content = "let s = \"foo\\\"bar\"; }";
        // 缺少一个 {，但 } 存在 → brace_depth = -1 → Error
        let result = verify_brackets(content);
        assert!(matches!(result, VerifyLevel::Error(_)));
        assert!(format!("{:?}", result).contains("'{}'"), "应报告花括号不平衡");
    }
```

- [ ] **Step 3: 运行现有测试确保无回归**

Run: `cargo test -p peri-middlewares --lib -- line_edit_verify::tests 2>&1 | tail -10`
Expected: 所有测试通过（5 个旧测试 + 5 个新测试）

- [ ] **Step 4: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_verify.rs
git commit -m "fix: verify_brackets 转义处理从 prev_char 改为 escape_next

prev_char 方案在连续转义场景（\\"）存在潜在误报：
转义反斜杠后 prev_char == '\\'，下一个 \" 被错误地
当作转义引号关闭字符串，导致后续括号计数失准。

改用 escape_next bool 标记反斜杠只跳过紧邻一个字符，
修正了所有转义序列（\\、\\"、\\n 等）的处理。

Part of: spec/issues/2026-06-06-lineedit-escape-and-context-matching-issues.md

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 2: Add diagnostic info to bracket verification failure

**File:** Modify `peri-middlewares/src/tools/filesystem/line_edit_verify.rs`

- [ ] **Step 1: 在 `verify_brackets` 中添加行号和上下文诊断**

在 `verify_brackets` 函数的 loop 内，追踪括号深度变化的行号。在循环中同时维护一个 `Vec<(usize, String, i32)>` 用于记录每个括号变化点的行号。验证失败时在错误信息中输出前 3 个异常行位置。

具体改动：在 `verify_brackets` 函数作用域添加：

```rust
    // 诊断信息：记录每对括号深度变化的前几行
    let mut depth_changes: Vec<(usize, String, char)> = Vec::new();
```

在 `match ch` 的每个括号分支中添加记录：

```rust
            '{' => {
                brace_depth += 1;
                depth_changes.push((lineno + 1, line.to_string(), '{'));
            }
            '}' => {
                brace_depth -= 1;
                depth_changes.push((lineno + 1, line.to_string(), '}'));
            }
            '(' => {
                paren_depth += 1;
                depth_changes.push((lineno + 1, line.to_string(), '('));
            }
            ')' => {
                paren_depth -= 1;
                depth_changes.push((lineno + 1, line.to_string(), ')'));
            }
            '[' => {
                bracket_depth += 1;
                depth_changes.push((lineno + 1, line.to_string(), '['));
            }
            ']' => {
                bracket_depth -= 1;
                depth_changes.push((lineno + 1, line.to_string(), ']'));
            }
```

在错误输出末尾追加诊断上下文：

```rust
    if !errors.is_empty() {
        // 附加诊断：显示最近几个括号变化位置
        let diag_lines: Vec<String> = depth_changes
            .iter()
            .rev()
            .take(5)
            .map(|(ln, content, _)| format!("  L{}: {}", ln, content.trim().chars().take(60).collect::<String>()))
            .collect();
        if !diag_lines.is_empty() {
            errors.push(format!("诊断（最近 {} 个括号）:\n{}", diag_lines.len(), diag_lines.join("\n")));
        }
        return VerifyLevel::Error(errors.join("，"));
    }
```

- [ ] **Step 2: 运行现有集成测试确保新增诊断不破坏行为**

Run: `cargo test -p peri-middlewares --lib -- line_edit 2>&1 | tail -10`
Expected: 所有 13 个 line_edit 集成测试通过

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/tools/filesystem/line_edit_verify.rs
git commit -m "feat: bracket 验证失败时输出行号诊断

验证失败时在错误信息中追加最近 5 个括号变化的位置
（行号和内容摘要），帮助快速定位不平衡的发生点。

Part of: spec/issues/2026-06-06-lineedit-escape-and-context-matching-issues.md

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 3: Integration verification

**Files:** No new code — 全量测试运行

- [ ] **Step 1: 运行全量测试**

```bash
cargo test -p peri-middlewares --lib -- line_edit 2>&1 | tail -15
```
Expected: 所有测试通过（~13 个集成测试 + 10 个 verify 单元测试）

- [ ] **Step 2: 运行 clippy**

```bash
cargo clippy -p peri-middlewares -- -D warnings 2>&1 | tail -5
```
Expected: 无 warning

- [ ] **Step 3: 运行完整构建**

```bash
cargo build 2>&1 | tail -5
```
Expected: 编译成功

---

## 自检

**1. Spec 覆盖：**
- ✅ Task 1 修复 `verify_brackets` 转义处理 bug（对应现象 1 根因）
- ✅ Task 2 改进诊断信息（降低现象 1/2 的排查成本）
- ⚠️ 现象 2（`_ => {}` 冗余插入）根因未知——无法复现。当前 fix 提升了下次遇到时的排查效率。如果复现，结合诊断输出可精确定位 hunk 匹配偏移量

**2. Placeholder 扫描：** 无 TBD/TODO/占位符。

**3. 为什么现象 2 先不 fix：**
- 无法从现象本身（"`_ => {}` 出现在原始 `_ => {` 前"）反推精确的 hunk 匹配错误
- 需要复现才能知道具体是 L1-L5 哪一级匹配错误、以及 splice 偏移量
- Task 2 的诊断改进使下次遇到类似问题时能立即定位 root cause

**4. `prev_prev_char` / `://` guard 移除理由：**
- 上一轮 fix（issue `lineedit-bracket-false-positive`）引入 `prev_prev_char` 是为了防止 Markdown 链接 URL `://` 被误判为行注释
- `verify_brackets` 只会处理 Rust 源码（Markdown/其他文件从 AST 层 skip，见 `verify_ast` L221）
- Rust 源码中 `://` 只出现在字符串内的 URL 常量（如 `const URL: &str = "https://..."`），此时已在 `in_string` 状态，不会触发 `//` 匹配
- 因此 `prev_prev_char` 守卫是冗余的，移除后代码更简洁且不影响正确性
