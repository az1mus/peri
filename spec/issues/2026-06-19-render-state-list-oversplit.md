# render_state/list.rs 过度拆分（15 行独立文件）

**状态**：Fixed
**优先级**：低
**创建日期**：2026-06-19

## 问题描述

commit `b6c55550` 将 `render_state.rs`（747 行）拆分为 coordinator + table/* + list 后，`list.rs` 仅包含 15 行代码（`ListType` 枚举 + `ListState` 结构体），且这两个类型仅被 `coordinator.rs` 使用。为 15 行代码维护一个独立文件增加了导航成本（打开 2 个文件 vs 读 1 个文件），收益不抵开销。

## 症状详情

- `list.rs`（15 行）：仅定义 `ListType` / `ListState`，被 `coordinator.rs` 的 `list_stack` 字段独占使用
- `ListType` 可见性为 `pub(super)`（仅 coordinator 可见），`ListState` 为 `pub(crate)`（仅 render_state 域可见）
- 无其他文件引用 `list.rs` 的类型

## 期望改进方向

将 `list.rs` 的内容内联回 `coordinator.rs`（或作为 `coordinator.rs` 内的 `mod list` 私有子模块），删除 `list.rs` 文件，更新 `render_state.rs` shim 的 `mod` 声明。

## 涉及文件

- `peri-widgets/src/markdown/render_state/list.rs`（15 行）—— 待删除
- `peri-widgets/src/markdown/render_state/coordinator.rs`（372 行）—— 接收方
- `peri-widgets/src/markdown/render_state.rs`（18 行）—— shim，需移除 `mod list`

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-19 | — | Open | agent | 创建 |

## 修复记录

| 日期 | 操作人 | 说明 |
|------|--------|------|
| 2026-06-20 | agent | 将 list.rs（15 行）的 ListType/ListState 定义内联回 coordinator.rs，删除 list.rs，更新 render_state.rs shim 移除 mod list。visibility 调整为 pub(in crate::markdown) 以匹配 RenderState 可见性 |
