# WebFetch 截断后未落盘，长网页内容直接丢弃

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-10

## 问题描述

WebFetch 工具在网页内容超过 2000 行时截断，截断后的完整内容直接丢弃。Agent 无法获取被截断的部分，只能重新请求抓取——浪费 token 和时间。

项目中已有通用的落盘机制 `output_persist.rs`（`persist_truncated_output`），Bash、Grep、Glob、Folder、MCP 工具均已接入，但 WebFetch 遗漏了。

## 症状详情

**当前行为**：WebFetch 返回前 2000 行，尾部提示 `[内容已截断，原始内容共 N 行]`，超出部分不可恢复。

**期望行为**：与其他工具一致——截断显示 + 完整内容写入 `/tmp/peri-tool-output-{uuid}.txt`，尾部提示 Agent 可用 Read 工具读取完整内容。

## 涉及文件

- `peri-middlewares/src/middleware/web_fetch.rs` — `truncate_content()` 函数仅截断，未调用 `persist_truncated_output`
- `peri-middlewares/src/tools/output_persist.rs` — 已有的通用落盘函数

## 历史关联

- 归档 issue `spec/archive-issues/2026-05-15-tool-output-truncation-with-disk-persist.md` 的解决方案列表中包含了 WebFetch，但实际实现遗漏了 `web_fetch.rs` 的接入

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-10 | — | Open | agent | 创建 |
| 2026-06-10 | Open | Fixed | agent | WebFetch 截断时调用 persist_truncated_output |
| 2026-06-10 | Fixed | Verified | user | 用户验证通过 |

## 修复记录

### 修复 #1（2026-06-10）

- **操作人**：agent
- **用户原意**：WebFetch 截断后完整内容不应丢弃，应落盘并提示 Agent 用 Read 读取
- **修复内容**：在 `web_fetch.rs` 的 `truncate_content()` 中调用已有的 `persist_truncated_output()`，与其他工具行为一致
- **涉及 commit**：`6da59035`
- **验证状态**：已验证

### 验证 #1（2026-06-10）—— 通过

用户确认 WebFetch 截断后完整内容已正确落盘。
