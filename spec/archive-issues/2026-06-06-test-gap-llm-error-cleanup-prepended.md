> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-test-gap-llm-error-cleanup-prepended.md

# 测试缺口：LLM 错误路径下 system 消息 cleanup 行为无测试

**状态**：✅ 已完成
**优先级**：高
**创建日期**：2026-06-06
**完成日期**：2026-06-06
**类型**：Testing / Bug Fix

## 背景

CLAUDE.md 记录了 `Interrupted`/`Error` + `Done` 互斥的 TRAP：`Interrupted`/`Error` 先 `request_rebuild()` + 添加通知，设 `reconcile_already_done=true`，后续 `Done` 跳过 `request_rebuild()` 防止覆盖通知。

在 executor 层（`mod.rs:335-336`），`cleanup_prepended` 位于 ReAct 循环之后：

```rust
for step in 0..self.max_iterations {
    let reasoning = call_llm(...).await?; // LLM 错误通过 ? 传播出函数
}
// cleanup 在此处，但 ? 传播会跳过
Self::cleanup_prepended(state, &prepended_ids);
```

当 LLM 返回错误时，`?` 传播会跳过 `cleanup_prepended`，导致 before_agent + with_system_prompt 注入的 system 消息泄漏到 state。

## 修复方案

用 `try_break!` 宏替换循环内所有 `?` 传播，将错误捕获到 `loop_error: Option<AgentError>` 中。循环结束后，**无论成功、失败、还是循环耗尽**，`cleanup_prepended` 始终执行。然后传播捕获的错误。

### 修改文件

| 文件 | 改动 |
|------|------|
| `peri-agent/src/agent/executor/mod.rs` | 引入 `try_break!` 宏 + `loop_error` 变量，5 处 `?` 替换为 `try_break!(expr, loop_error)` |
| `peri-agent/src/agent/executor/mod_test.rs` | `test_llm_error_cleanup_prepended_behavior` 断言从 `system_count == 2`(泄漏) 改为 `system_count == 0`(已清理) |

### 对应 TRAP

- `spec/global/domains/agent.md#issue_2026-05-25-interrupt-undo-last-user-message`
