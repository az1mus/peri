> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-test-gap-hitl-cancel-race.md

# HITL 审批与 Cancel 竞态条件缺少测试

**状态**：✅ 已完成
**优先级**：中
**创建日期**：2026-06-06
**完成日期**：2026-06-06

## 问题描述

`HumanInTheLoopMiddleware::before_tool` 在需要审批时调用 `broker.request(ctx).await`，这是一个无超时、无 cancel token 传递的 async 等待。如果 broker 实现永远不返回（如 UI 弹窗被关闭但 channel 未 drop），Agent 将永久挂起。

## 修复方案

1. `broker_approve` 和 `batch_broker_approve` 中的 `broker.request(ctx).await` 包裹在 `tokio::time::timeout` 中，默认超时 300 秒
2. 超时发生时返回 `AgentError::ToolRejected`，reason 包含超时信息
3. 添加 `broker_timeout: Duration` 字段到 `HumanInTheLoopMiddleware`，提供 `with_broker_timeout()` builder 方法供测试使用
4. 测试 `test_broker_hang_rejects_with_timeout` 使用 500ms 短超时验证挂起 broker 被正确拒绝

### 修改文件

| 文件 | 改动 |
|------|------|
| `peri-middlewares/src/hitl/mod.rs` | 添加 `BROKER_TIMEOUT` 常量 + `broker_timeout` 字段 + `with_broker_timeout()` 方法；`broker_approve`/`batch_broker_approve` 加 timeout 保护 |
| `peri-middlewares/src/hitl/mod_test.rs` | `test_broker_hang_rejects_with_timeout` 验证挂起 broker 超时后返回 ToolRejected |
