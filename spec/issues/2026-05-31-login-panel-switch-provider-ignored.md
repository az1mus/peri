# Login 面板 / 快捷键切换 provider 后 ACP 侧实际未生效

**状态**：Verified
**优先级**：中
**创建日期**：2026-05-31

## 问题描述

通过 `/login` Login 面板或快捷键 `Ctrl+Shift+T` 切换 provider 后，TUI 侧界面状态栏显示已切换（provider name / model name 更新），持久化配置也已写入磁盘，但当前 session 继续对话时 agent 实际使用的仍是切换前的旧 provider。

注意：`Ctrl+T` 切换 model 路径生效（`set_config_option("model", ...)` 正确），只有 provider 级别的切换（Login 面板 + `Ctrl+Shift+T`，均走 `sync_acp_config()` → `update_config`）不生效。

## 症状详情

### 现象 1：Login 面板选中切换（2026-05-31 创建）

| 现象 | 详情 |
|------|------|
| 界面显示已切换 | 状态栏中 provider name / model name 已更新为新选中的 provider |
| 实际对话未切换 | 当前 session 继续发送消息，agent 回复的风格/能力仍是旧模型的特征 |
| 无错误提示 | 切换过程没有弹出配置保存失败或 provider 无效的错误 |
| agent idle 时切换 | 切换时当前 session 没有在执行 agent（无 pending 的 LLM 调用或工具执行） |

### 现象 2：快捷键 Ctrl+Shift+T 切换 provider 不生效（2026-06-10 追加）

| 现象 | 详情 |
|------|------|
| Ctrl+Shift+T 切换 provider 无效 | 状态栏 provider name 更新了，但 agent 仍用旧 provider 回复 |
| 对比：Ctrl+T 切换 model 有效 | `Ctrl+T`（model cycle）切换后 agent 确实使用了新 model，说明 `set_config_option("model", ...)` 路径工作正常 |
| 两种触发场景 | ① 在 Login 面板 edit 修改 provider 数据后，用快捷键切换仍不生效；② 不经过 Login 面板，纯快捷键切换也不生效 |
| 无错误提示 | 切换时没有配置保存失败或 provider 无效的错误 |

## 复现条件

### 路径 1：Login 面板选中

- **复现频率**：稳定复现
- **触发步骤**：
  1. 在当前 session 中与 agent 对话若干轮
  2. 输入 `/login` 打开 Login 面板
  3. 用方向键选中另一个 provider，按 Enter 激活
  4. 面板关闭，状态栏显示新 provider 名称
  5. 在当前 session 中继续发送消息
  6. 观察：agent 回复仍是旧模型的风格/能力

### 路径 2：快捷键 Ctrl+Shift+T

- **复现频率**：稳定复现
- **触发步骤**：
  1. 配置了 2 个以上 provider
  2. 按 `Ctrl+Shift+T` 切换到下一个 provider
  3. 状态栏显示新 provider 名称
  4. 继续在当前 session 发送消息
  5. 观察：agent 回复仍是切换前旧 provider 的特征

## 涉及文件

- `peri-tui/src/app/panel_login.rs` — `login_panel_select_provider()`，处理 Login 面板的 provider 选中，调 `sync_acp_config()`
- `peri-tui/src/event/keyboard/shortcuts.rs` — `handle_shortcuts()` L76-108，`Ctrl+Shift+T` 切 provider 后调 `sync_acp_config()`；L45-74，`Ctrl+T` 切 model 后调 `set_config_option("model", ...)`（该路径生效）
- `peri-tui/src/app/panel_manager.rs` — `sync_acp_config()` L283-299，通过 `block_in_place + block_on` 调 ACP `update_config`
- `peri-acp/src/session/` — `update_config` / `set_config_option` 的 ACP 侧处理（provider 更新在此处未正确生效）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-05-31 | — | Open | agent | 创建 |
| 2026-06-10 | Open | Open | agent | 追加快捷键 Ctrl+Shift+T 不生效症状 |
| 2026-06-10 | Open | Fixed | agent | 修复：无 session 时通过 notification 更新 ACP config |
| 2026-06-10 | Fixed | Verified | user | 用户验证通过 |

## 修复记录

| 日期 | 操作人 | 说明 |
|------|--------|------|
| 2026-06-10 | agent | Phase 4 实现修复：无 session 时通过 `session/config_update` notification 直接更新 ACP server 侧 `cfg.provider` / `cfg.peri_config`。根因：`client.rs` 的 `update_config()`/`set_config_option()` 在 `current_session_id == None` 时静默返回 `Ok(())`，config 从未到达 ACP server。涉及文件：`client.rs`（notification 分支）、`notify.rs`（新增 notification handler）、`mod.rs`（server loop 传递 `cfg`）。**验证状态：已验证** |

## 根因分析

**第一层缺陷**（Login 面板 / 启动阶段）：
- `sync_acp_config()` → `client.update_config()` → 检查 `current_session_id`
- `current_session_id` 为 `None`（session 尚未创建）→ 静默返回 `Ok(())`
- Config 更新从未到达 ACP server → `cfg.provider` / `cfg.peri_config` 保持启动时的旧值
- 后续 `session/new` 读取旧 `cfg.provider` → Agent 使用旧 provider

**第二层缺陷**（快捷键 Ctrl+Shift+T 活跃期间）：
- 即使 session 存在，也可能因 `current_session_id` 被 `take()` 清空而失败
- 代码注释假设 "ACP Server will load config from disk when session is created" — 此假设不成立，ACP server 不会重新从磁盘加载

**涉及文件（修复）**：
- `peri-tui/src/acp_client/client.rs` — `set_config_option()` / `update_config()`：无 session 时改用 `send_notification("session/config_update", ...)` 发送
- `peri-tui/src/acp_server/notify.rs` — 新增 `session/config_update` notification handler：直接写 `cfg.provider` / `cfg.peri_config`
- `peri-tui/src/acp_server/mod.rs` — server loop 传递 `&cfg` 给 `handle_notification`

### 验证 #1（2026-06-10）—— Verified

用户反馈：修复后 Login 面板和快捷键 Ctrl+Shift+T 切换 provider 均可正常生效，agent 对话使用切换后的新 provider。
