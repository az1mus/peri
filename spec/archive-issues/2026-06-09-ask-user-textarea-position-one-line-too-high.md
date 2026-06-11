> 归档于 2026-06-11，原路径 spec/issues/2026-06-09-ask-user-textarea-position-one-line-too-high.md

# AskUser 弹窗自定义输入 textarea 聚焦时比预期偏上一行

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-09

## 问题描述

AskUser 弹窗中，当光标移动到最后一个编号选项（自定义输入行 / textarea）并聚焦时，textarea 输入区域的实际显示位置比预期的位置偏上约一行。用户看到的内容选项列表与编辑区域之间的视觉间距不正确。

## 症状详情

| 场景 | 预期 | 实际 |
|------|------|------|
| 光标移到最后一个选项（自定义输入）聚焦 | textarea 编辑区与最后一项选项之间保留合理间距 | textarea 编辑区出现在比预期高一行的位置 |
| 光标在普通选项上（非自定义输入） | 正常显示 | 不受影响 |
| 自定义输入未聚焦（placeholder 模式） | "输入自定义内容..." 字面显示在正确位置 | 不受影响 |

问题仅在 `in_custom_input = true`（textarea overlay 激活）时出现。

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. Agent 调用 `AskUserQuestion` 工具，弹出 AskUser 弹窗
  2. 用方向键将光标移至最后一个编号选项（自定义输入行）
  3. 观察自定义输入 textarea 相对于最后一个普通选项的位置——比预期偏上一行
- **环境**：macOS，任意模型

## 背景

该问题由 `2026-06-07-ask-user-custom-input-textarea.md` 引入的 textarea 替换所致。替换前自定义输入行使用 `String + usize cursor` 直接在 `ScrollableArea` 文本内渲染（与选项列表同在一个 `Text` 中）。替换后改为 overlay 模式：

1. `ScrollableArea` 中仅放置 prefix 行（`"❯ N. "`）和预留空行
2. `FieldTextarea` widget 作为 overlay 渲染在 prefix 行右侧

这种 overlay 方案中，textarea 的 Y 坐标通过 `textarea_label_line`（记录在 `ScrollableArea` Text 中的行索引）映射到屏幕坐标。可能在此映射或高度预留中存在一行偏差。

## 涉及文件

- `peri-tui/src/ui/main_ui/popups/ask_user.rs` —— `render_ask_user_popup`：textarea overlay 的 Y 坐标计算和高度预留逻辑
- `peri-tui/src/ui/main_ui/popups/ask_user_height.rs` —— `ask_user_content_height`：高度估算仅分配 1 行给自定义输入区域，未考虑 textarea 的多行动态高度
- `peri-tui/src/app/field_textarea.rs` —— `FieldTextarea::render_height()` 返回逻辑行数（未考虑 `tui_textarea` 的自动换行）

## 关联 Issue

- `spec/issues/2026-06-07-ask-user-custom-input-textarea.md` —— 引入 textarea 替换的 issue（本问题的引入源）
- `spec/issues/2026-05-26-ask-user-popup-height-miscalculation.md` —— 同弹窗的整体高度计算问题（不同根因，但交互影响）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-09 | — | Open | agent | 创建 |
| 2026-06-09 | Open | Verified | agent | 修复验证通过，commit 1609beec |

## 修复记录

### 修复 #1（2026-06-09）

- **操作人**：agent（deepseek-v4-pro）
- **用户原意**：AskUser 弹窗中自定义输入 textarea 聚焦时定位正确，与上方选项列表保留合理间距
- **修复内容**：将 textarea overlay Y 坐标从逻辑行索引（`textarea_label_line`）改为视觉行偏移（`visual_label_offset`）。根因是 `ScrollableArea` 内部使用 `Paragraph` + `WordWrapper` 渲染，逻辑行索引 ≠ 视觉行位置——当前置内容（问题文本、选项）因面板宽度发生换行时，逻辑行索引会产生累积偏移。修复通过 `Paragraph::line_count()`（与 ratatui WordWrapper 算法完全一致）精确计算前置内容的实际视觉行数，替换了原有的简单索引映射。
- **涉及 commit**：`1609beec`
- **验证状态**：已验证

## 症状详情

### 验证 #1（2026-06-09）—— 完全符合预期

textarea 与选项列表间距正常，自定义输入编辑区位置正确，无偏上现象。
