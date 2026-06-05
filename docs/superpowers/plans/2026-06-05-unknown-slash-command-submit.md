# 未知 Slash Command 改为普通提交 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 以 `/` 开头但不匹配任何已知命令/Skill/Agent 命令的输入，静默作为普通文本提交给 Agent，不再显示"未知命令"错误并吞掉输入。

**Architecture:** 修改 TUI 键盘事件处理中的 slash command 分发逻辑。三层命令匹配（本地命令 → Skill → Agent 命令）全部失败时，改为 `return Ok(Some(Action::Submit(text)))` 走普通提交路径。ACP 侧无需修改（未知命令已自然 fall through）。

**Tech Stack:** Rust, ratatui, peri-tui 事件系统

---

### Task 1: 修改未知 slash command 的处理逻辑

**Files:**
- Modify: `peri-tui/src/event/keyboard/normal_keys.rs:178-213`

- [ ] **Step 1: 替换 else 分支为普通提交**

将第 178-213 行的整个 `else` 块（从 `tracing::debug!` 到 `push(MessageViewModel::system(error_msg))`）替换为：

```rust
                        } else {
                            // 未知命令/Skill：作为普通输入提交给 Agent
                            tracing::debug!(
                                skill_name,
                                "Unknown slash command, submitting as normal input"
                            );
                            return Ok(Some(Action::Submit(text)));
                        }
```

这同时移除了 `match_prefix` 歧义判断的错误提示逻辑——因为无论歧义还是完全未知，都走普通提交。

- [ ] **Step 2: 清理未使用的 import**

移除不再使用的 `MessageViewModel`（如果该文件中没有其他地方使用它）。检查第 3 行：

```rust
use crate::app::{App, MessageViewModel, PendingAttachment};
```

如果 `MessageViewModel` 在文件其他地方仍有使用则保留。用 `grep` 确认：

Run: `grep -n 'MessageViewModel' peri-tui/src/event/keyboard/normal_keys.rs`

如果只在被删除的代码中使用，改为：

```rust
use crate::app::{App, PendingAttachment};
```

- [ ] **Step 3: 构建验证**

Run: `cargo build -p peri-tui`
Expected: 编译成功，无 warning

- [ ] **Step 4: 运行现有测试**

Run: `cargo test -p peri-tui --lib -- command`
Expected: 所有 command 相关测试通过（`mod_test.rs` 中的 dispatch 测试不受影响——修改在 TUI 事件层，不在 CommandRegistry 层）

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/event/keyboard/normal_keys.rs
git commit -m "fix: 未知 slash command 静默走普通提交而非吞掉输入

以 / 开头但不匹配本地命令/Skill/Agent 命令的输入（如 /user/api），
此前显示'未知命令'错误并丢弃输入。现改为作为普通文本提交给 Agent。

Fixes: spec/issues/2026-06-05-unknown-slash-command-input-swallowed.md"
```

---

## 自检

**1. Spec 覆盖：** issue 要求所有以 `/` 开头但不匹配已知命令的输入静默走普通提交 → Task 1 完全覆盖。

**2. Placeholder 扫描：** 无 TBD/TODO/等占位符。代码块完整。

**3. 类型一致性：** `Action::Submit(text)` 与第 167、177、218 行已有的用法一致，`text` 类型为 `String`。
