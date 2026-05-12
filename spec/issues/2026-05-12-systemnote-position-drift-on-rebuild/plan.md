# 修复 SystemNote 在 RebuildAll 后位置漂移

## 目标

TUI 层通过 `AddMessage` / 直接 `push` 添加的 `SystemNote` 在 `RebuildAll` 后被推到 `view_messages` 末尾。需要让 SystemNote 保持在产生时的位置。

## 方案概述

为每个 ephemeral SystemNote 记录其创建时的 `view_messages` 索引（锚点）。`RebuildAll` 时根据锚点将 SystemNote 插入到 `tail_vms` 的对应位置，而非追加到末尾。

## 核心设计

### 锚点语义

- 锚点 = SystemNote 创建时 `view_messages.len()`（即它应该插入的位置索引）
- 锚点在 prefix 范围内（被 drain）→ 丢弃
- 锚点超过 tail_vms 长度 → clamp 到末尾

### 两种 AddMessage 路径统一

| 路径 | 当前代码 | 迁移目标 |
|------|---------|---------|
| **路径 A**：`apply_pipeline_action(AddMessage(vm))` | agent_ops.rs, agent_events_*.rs, agent_compact.rs, agent_submit.rs | 保留 `AddMessage` 变体不变，`apply_pipeline_action` 内部同时记录锚点 |
| **路径 B**：直接 `view_messages.push(system_vm)` | mod.rs, *_panel.rs, panel_ops.rs, cron_ops.rs, cron_state.rs, thread_ops.rs, command/agent.rs | 统一改为调用 `App::push_system_note(content)` |

两条路径最终都往 `ephemeral_notes` 记录锚点。

## 实施步骤

### Step 1: `MessageState` 新增 `ephemeral_notes` 字段

**文件**：`rust-agent-tui/src/app/message_state.rs`

- 新增字段：`pub ephemeral_notes: Vec<(usize, MessageViewModel)>`
- `new()` 中初始化为空 Vec

### Step 2: `apply_pipeline_action` 的 `AddMessage` 分支记录锚点

**文件**：`rust-agent-tui/src/app/agent_render.rs:43-48`

在 `AddMessage` 分支中，push 之前记录锚点：

```rust
PipelineAction::AddMessage(vm) => {
    let anchor = session.messages.view_messages.len();
    session.messages.ephemeral_notes.push((anchor, vm.clone()));
    session.messages.view_messages.push(vm);
    self.render_rebuild();
}
```

### Step 3: 新增 `App::push_system_note()` 方法

**文件**：`rust-agent-tui/src/app/agent_render.rs`

为路径 B 提供统一入口：

```rust
pub(crate) fn push_system_note(&mut self, content: String) {
    let session = &mut self.session_mgr.sessions[self.session_mgr.active];
    let anchor = session.messages.view_messages.len();
    let vm = MessageViewModel::system(content);
    session.messages.ephemeral_notes.push((anchor, vm.clone()));
    session.messages.view_messages.push(vm);
}
```

### Step 4: 重写 `RebuildAll` 的 saved_notes 逻辑

**文件**：`rust-agent-tui/src/app/agent_render.rs:50-128`

替换当前的 `saved_notes` 机制：

```
旧逻辑：
  drain(prefix_len..) → filter SystemNote → extend(tail_vms) → extend(saved_notes)

新逻辑：
  drain ephemeral_notes → 保留 anchor >= prefix_len 的
  drain view_messages(prefix_len..)
  extend(tail_vms)
  按 anchor 排序，逐个 insert 到 anchor-prefix_len 的位置
  重新注册到 ephemeral_notes（更新锚点为实际插入位置）
```

### Step 5: 迁移路径 B 的所有直接 push 调用

将以下位置的 `view_messages.push(MessageViewModel::system(...))` 改为 `self.push_system_note(...)`：

| 文件 | 行数 | 场景 |
|------|------|------|
| `mod.rs` | 468-473 | 强制中断（有恢复文本） |
| `mod.rs` | 476-482 | 强制中断（无恢复文本） |
| `mod.rs` | 486-491 | 强制中断（非 agent_replied） |
| `panel_ops.rs` | ~51, ~61, ~113 | 面板操作反馈 |
| `login_panel.rs` | ~433, ~446, ~568, ~583, ~636, ~649 | Provider 管理 |
| `config_panel.rs` | ~336, ~344 | 配置保存 |
| `model_panel.rs` | ~264, ~273 | 模型切换 |
| `agent_panel.rs` | ~132, ~143 | Agent 重置/切换 |
| `plugin_panel.rs` | ~923, ~974, ~1005, ~1188 | 插件管理 |
| `cron_ops.rs` | ~52 | Cron 任务删除 |
| `cron_state.rs` | ~169 | Cron 任务操作 |
| `thread_ops.rs` | ~311, ~329, ~365 | 压缩/历史操作 |
| `command/agent.rs` | ~20, ~26 | Agent 命令 |

**注意**：面板代码中无法直接调用 `self.push_system_note()`，因为面板只有 `PanelContext`（包含 `&mut SessionManager`）。需要提供一个接受 `SessionManager` 的辅助函数，或让 `PanelContext` 暴露一个 `push_system_note` 方法。

### Step 6: 生命周期事件清理 `ephemeral_notes`

| 事件 | 文件 | 行为 |
|------|------|------|
| Ctrl+C 中断 truncate | `mod.rs:424` | `ephemeral_notes.retain(\|a, _\| *a < round_start)` |
| 历史恢复（view_messages 完全重赋值） | `thread_ops.rs:160` | `ephemeral_notes.clear()` |
| `/clear` 清空消息 | 清空 view_messages 的位置 | `ephemeral_notes.clear()` |

### Step 7: 更新现有测试 + 新增测试

**文件**：`rust-agent-tui/src/app/agent_ops.rs:1195-1240`

更新 `test_rebuildall_preserves_system_notes` 验证锚点插入位置。

新增测试用例：
1. SystemNote 在 RebuildAll 后保持原位
2. 锚点在 prefix 内 → 丢弃
3. 多次连续 RebuildAll → 位置不变
4. 多个 SystemNote → 保持相对顺序
5. 锚点超过 tail 长度 → clamp 到末尾

## 边界情况

| 场景 | 处理方式 | 风险 |
|------|---------|------|
| anchor >= prefix_len 但 > view_messages.len() | `min()` clamp 到末尾 | 无 panic |
| anchor 在 prefix 内（被 drain） | 丢弃 | 无 panic |
| 多个 SystemNote 同一 anchor | 稳定排序保持插入顺序 | 无 panic |
| 节流 check_throttle + RebuildAll | throttle 不碰 ephemeral_notes，只有 apply_pipeline_action 处理 | 无冲突 |
| Vec::insert 性能 | O(n) 但 SystemNote 数量少（<5），消息列表通常 <1000 | 可接受 |

## 涉及文件汇总

| 文件 | 变更类型 |
|------|---------|
| `message_state.rs` | 新增 `ephemeral_notes` 字段 |
| `agent_render.rs` | 核心逻辑：AddMessage 记录锚点 + RebuildAll 锚点插入 + 新增 `push_system_note` |
| `mod.rs` | 3 处 push 迁移 + truncate 时清理 ephemeral_notes |
| `agent_ops.rs` | 测试更新 + 新增测试 |
| `agent_events_oauth.rs` | 无需改动（已走 AddMessage 路径，Step 2 自动记录锚点） |
| `agent_events_plugin.rs` | 同上 |
| `agent_compact.rs` | 同上 |
| `agent_submit.rs` | 同上 |
| `panel_ops.rs` | ~3 处 push 迁移 |
| `login_panel.rs` | ~6 处 push 迁移 |
| `config_panel.rs` | ~2 处 push 迁移 |
| `model_panel.rs` | ~2 处 push 迁移 |
| `agent_panel.rs` | ~2 处 push 迁移 |
| `plugin_panel.rs` | ~4 处 push 迁移 |
| `cron_ops.rs` | ~1 处 push 迁移 |
| `cron_state.rs` | ~1 处 push 迁移 |
| `thread_ops.rs` | ~3 处 push 迁移 + 历史恢复时清理 ephemeral_notes |
| `command/agent.rs` | ~2 处 push 迁移 |

**总计**：~18 个文件修改
