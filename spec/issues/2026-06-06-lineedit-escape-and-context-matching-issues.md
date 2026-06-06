# LineEdit 工具在转义字符串和上下文匹配场景中的降效问题

**状态**：Fixed
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

在使用 LineEdit 进行 inline slash trigger 功能开发时，遇到两类导致 fallback 到 `Write` 全量重写的问题：brackets 验证对 diff 中的转义字符串 `\\n` 误报，以及上下文匹配不精确导致冗余分支插入。

## 症状详情

### 现象 1：转义字符串 `\\n` 导致 brackets 验证误报

| 维度 | 内容 |
|------|------|
| 操作 | 对 `event/keyboard.rs` 中的 `update_slash_hint_detection` 函数应用 patch |
| patch 中行 | `let text = textarea.lines().join("\\n");` — 源码中包含字符串字面量 `\n`（在 diff 中表示为 `"\\n"`） |
| 预期 | patch 正常应用 |
| 实际 | brackets 验证失败（`brackets:error`），编辑被取消，文件未被修改 |
| 影响 | 2 次尝试均失败，最终用 `Write` 全量重写 250 行文件绕过 |

### 现象 2：上下文匹配不精确导致冗余 `_ => {}` 分支

| 维度 | 内容 |
|------|------|
| 操作 | 对 `normal_keys.rs` 的 match 块末尾插入新的 Esc handler arm |
| 改动 | 在 `_ => {` 之前插入 `Input { key: Key::Esc, .. } if slash_hint.active => { ... }` |
| 预期 | 新 arm 插入到 `_ =>` 之前，`_ =>` 保持唯一 |
| 实际 | `_ => {}` 空分支被插入到原始 `_ => {` 之前，产生两个 unreachable pattern，编译报错 |
| 影响 | 需要额外一次修复 patch 来删除冗余分支 |

### 现象 3（2026-06-06 追加）：Rust lifetime 语法被误判为字符串开启

| 维度 | 内容 |
|------|------|
| 发现 | `escape_next` 修复（b9bd0463）重写了 `verify_brackets` 函数，丢失了 commit 86c31245 的 Rust lifetime 处理逻辑 |
| 表现 | 所有含 `'static`、`'a` 等 lifetime 的 `.rs` 文件被误报 `brackets:error`，LineEdit 完全不可用 |
| 叠加 bug | 恢复 lifetime 处理后，`Char('m')` 等 ASCII 字母 char literal 被误判为 lifetime（因为 `'m'` 的开头符合 lifetime 模式 `'ident`） |
| 根因 | 原 lifetime 检测仅检查「下一字符是否为 ident」，未检查标识符后是否有闭合 `'` 来区分 char literal |
| 影响 | 即使应用了 escape_next 修复，LineEdit 仍对绝大多数 Rust 文件失效 |

## 涉及文件

- `peri-middlewares/src/tools/filesystem/line_edit_verify.rs` — brackets 验证逻辑（`verify_brackets` 函数）
- `peri-middlewares/src/tools/filesystem/line_edit_match.rs` — 上下文匹配策略

## 期望改进方向

1. brackets 验证器应正确处理 diff 上下文中的转义字符串字面量（不应将 `\\n` 中的 `\` 误解释）✅ 已修复
2. 上下文匹配应更精确地定位插入位置，避免在已有模式前引入冗余 arm
3. brackets 验证器应正确处理 Rust lifetime 语法（`'static`、`'a`、`'b`）✅ 已修复
4. brackets 验证器应区分 char literal（`'m'`、`'x'`）与 lifetime（`'static`）✅ 已修复
5. 补充真实 Rust 代码片段的压力测试，防止回归 ✅ 已补充 20 个 P4 测试

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |
| 2026-06-06 | Open | Open | agent | 追加现象 3：lifetime 回归 + char literal 误判；已修复并补充 P4 测试（49 个全通过） |
| 2026-06-06 | Open | Fixed | agent | 修复提交 e55cff00：escape_next 转义处理 + lifetime 检测 + 20 个 P4 压力测试 |

## 修复记录

### 修复 #1（2026-06-06）

- **操作人**：agent
- **用户原意**：修复 LineEdit brackets 验证对转义字符串和 Rust lifetime 的误报，确保工具对真实 Rust 代码可用
- **修复内容**：
  1. `verify_brackets` 转义处理改用 `escape_next` 标记（解决 `\"` 误关闭字符串 → 假阳性/假阴性）
  2. 恢复 Rust lifetime 语法跳过逻辑（`'static`、`'a` 等不误开字符串）
  3. 增强 lifetime 检测：peek 过标识符后检查是否有闭合 `'` 来区分 char literal（`'m'` vs `'static`）
  4. 新增 20 个 P4 压力测试：覆盖 lifetime、char literal、真实 Rust 片段、高复杂度混合场景
- **涉及 commit**：e55cff00
- **验证状态**：49/49 单元测试通过；需进程重启后验证端到端 LineEdit 可用性
