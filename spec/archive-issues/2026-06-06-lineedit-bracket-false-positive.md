> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-lineedit-bracket-false-positive.md

# LineEdit bracket 校验对 Markdown 内容中 URL `://` 的误报

**状态**：Fixed
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

使用 LineEdit 编辑 Markdown 博客文件时，bracket 校验失败（`brackets:error`），导致合法编辑被拒绝。

**根因**：`verify_brackets` 的 `//` 行注释检测在遇到 Markdown 链接 URL（如 `https://github.com/...`）中的 `://` 时错误地进入行注释模式，导致 URL 末尾的 `)` 被跳过，`paren_depth` 无法归零，产出 `'()' 不平衡` 假阳性。`/plugin` 只是表象（blog 文件恰好触发了 URL 问题），实际触发的是文件中已有的 Markdown 链接 URL。

## 症状详情

**复现场景**：编辑 `docs/blogs/introducing-peri/introducing-peri.md`。

**错误输出**：
```
✗ introducing-peri.md 验证失败 [sanity:ok brackets:error ast:skip]
编辑已取消，文件未被修改。
```

## 复现条件

- 对包含 `[text](https://...)` 链接的 Markdown 文件使用 LineEdit 即可触发
- 旧 bracket checker 遇到 `://` 后进入行注释，后续所有字符（含 `)`) 被跳过

## 涉及文件

- `peri-middlewares/src/tools/filesystem/line_edit_verify.rs` —— bracket 校验逻辑（误报来源，已修复）

## 修复

**文件**: `peri-middlewares/src/tools/filesystem/line_edit_verify.rs:121,152-156`

**改动**：
- 新增 `prev_prev_char` 追踪前前一个字符
- `//` 行注释检测增加前置条件：`prev_prev_char != Some(':')` — `://` 不视为行注释
- 字符串/注释/反斜杠处理路径同步更新 `prev_prev_char`
- 新增测试 `test_括号平衡_url不触发行注释` 和 `test_括号平衡_真正注释仍触发`

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |
| 2026-06-06 | Open | Fixed | agent | 修复 URL :// 误触发行注释，39 测试全过 |
