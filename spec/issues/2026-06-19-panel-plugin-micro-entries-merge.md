# panel_plugin 四个微入口文件可合并为 entries.rs

**状态**：Fixed
**优先级**：低
**创建日期**：2026-06-19

## 问题描述

commit `b6c55550` 将 `panel_plugin.rs`（779 行）拆分为 9 个子模块后，其中 4 个入口文件（`cron_entry`、`mcp_entry`、`tasks_entry`、`plugin_entry`）各自不足 75 行，且模式完全一致（`impl crate::app::App` + 单个 pub 方法）。这 4 个文件合计 193 行，合并为一个 `entries.rs` 完全合理，可减少文件导航成本。

## 症状详情

| 文件 | 行数 | 包含方法 |
|------|------|----------|
| `cron_entry.rs` | 36 | `open_cron_panel` |
| `mcp_entry.rs` | 33 | `open_mcp_panel` |
| `tasks_entry.rs` | 73 | `open_tasks_panel` + `load_agent_thread_entries` |
| `plugin_entry.rs` | 51 | `open_plugin_panel` + `close_plugin_panel` |
| **合计** | **193** | — |

所有文件均为 `impl crate::app::App { ... }` 的简单入口方法，不依赖各自的内部类型或模块级状态，合并无耦合风险。

## 期望改进方向

将 4 个文件合并为 `panel_plugin/entries.rs`（约 200 行），每个方法用注释分隔，更新 `panel_plugin.rs` shim 的 `mod` 声明。

## 涉及文件

- `peri-tui/src/app/panel_plugin/cron_entry.rs`（36 行）—— 待删除
- `peri-tui/src/app/panel_plugin/mcp_entry.rs`（33 行）—— 待删除
- `peri-tui/src/app/panel_plugin/tasks_entry.rs`（73 行）—— 待删除
- `peri-tui/src/app/panel_plugin/plugin_entry.rs`（51 行）—— 待删除
- `peri-tui/src/app/panel_plugin/entries.rs`（新建，~200 行）
- `peri-tui/src/app/panel_plugin.rs`（28 行）—— shim，需更新 `mod` 声明

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-19 | — | Open | agent | 创建 |

## 修复记录

| 日期 | 操作人 | 说明 |
|------|--------|------|
| 2026-06-20 | agent | 将 cron_entry / mcp_entry / tasks_entry / plugin_entry 四个文件（合计 193 行）合并为 entries.rs（183 行），更新 panel_plugin.rs shim 的 mod 声明，删除 4 个旧文件 |
