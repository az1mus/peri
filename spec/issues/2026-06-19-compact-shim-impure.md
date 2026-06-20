# compact/compact.rs shim 不纯——execute 映射逻辑残留

**状态**：Fixed
**优先级**：低
**创建日期**：2026-06-19

## 问题描述

commit `b6c55550` 声称拆分模式为"facade + re-export shim"，但 `compact/compact.rs`（78 行）仍包含 16 行 `execute` 方法的 `match pipeline::PipelineOutcome` 映射逻辑（含 `stop_reason` 赋值）。与 `panel_plugin.rs`（28 行纯 `mod` 声明，零逻辑）对比，`compact.rs` 的 shim 纯度不够。

## 症状详情

`compact.rs:53-70` 的 `execute` 方法：

```rust
match pipeline::run_pipeline(ctx).await {
    pipeline::PipelineOutcome::Completed { messages } => CommandResult { ... },
    pipeline::PipelineOutcome::Cancelled { history } => CommandResult { ... },
    pipeline::PipelineOutcome::EarlyReturn { history, stop_reason } => CommandResult { ... },
}
```

这 16 行映射逻辑是 Pipeline 终态到 `CommandResult` 的转换，语义上属于 Pipeline 编排层而非纯 shim。

## 期望改进方向

选项 A：将 `execute` 方法也迁入 `pipeline.rs`（变为 `pub async fn execute_compact(ctx) -> CommandResult`），`compact.rs` 仅保留 `mod` + `pub use CompactCommand`，纯化为真 shim。

选项 B：新建 `compact/command.rs` 存放 `CompactCommand` struct + `AgentCommand` trait impl，`compact.rs` 变为纯 shim。

## 涉及文件

- `peri-acp/src/session/command/compact.rs`（78 行）—— 含 execute 映射逻辑
- `peri-acp/src/session/command/compact/pipeline.rs`（279 行）—— 候选接收方

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-19 | — | Open | agent | 创建 |

## 修复记录

| 日期 | 操作人 | 说明 |
|------|--------|------|
| 2026-06-20 | agent | 将 execute 内的 match PipelineOutcome 映射逻辑提取为 pipeline::execute_compact()，compact.rs 纯化为仅含 mod + pub struct + trait impl 的真 shim（选项 A） |
