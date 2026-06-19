# Login 面板同 id 多 provider 时 Edit 保存覆盖第一个 provider

**状态**：Open
**优先级**：中
**创建日期**：2026-06-18

## 问题描述

用户配置了三个 provider，`id` 字段均为 `"anthropic"`，通过不同的 `name`、`baseUrl`、`models` 区分（mimo、deepseek、kimi）。在 `/login` 面板中对第二个 provider（deepseek）执行 Edit → 修改 → Enter 保存后，第二个 provider 的内容**未被保存到原位**，反而**覆盖了第一个 provider**（mimo）的配置数据。同时 Browse 模式下选中激活也无法区分同 id 的不同 provider。

## 症状详情

### 现象 1：Edit 保存时覆盖第一个 provider

| 现象 | 详情 |
|------|------|
| Edit 对象错误 | 光标选中列表中第二个 provider（deepseek），Tab 进入 Edit 模式，修改字段后 Enter 保存 |
| 实际写入位置 | 保存的内容覆盖了列表第一个 provider（mimo）的数据，第二个 provider 保持原样 |
| 结果 | settings.json 中前两个 provider 变为完全相同的内容（都是原先 deepseek 的配置），原 mimo 配置丢失 |

### 现象 2：Browse 模式激活无法区分同 id provider

| 现象 | 详情 |
|------|------|
| Browse 模式 Enter 激活 | 光标在第二个 provider 上按 Enter 激活 |
| 状态栏更新 | 状态栏 provider 名称显示为选中项的名称 |
| 实际使用 | 系统通过 `active_provider_id` 解析 provider，由于所有 provider 的 `id` 都是 `"anthropic"`，`LlmProvider::from_config()` 总是匹配列表第一个（mimo），而非用户选中的那个 |

## 复现条件

- **复现频率**：必现（只要 ≥2 个 provider 的 `id` 字段相同）
- **触发步骤**：
  1. 在 `settings.json` 的 `providers` 数组中配置 ≥2 个 provider，`id` 字段设为相同值（如均为 `"anthropic"`），用不同 `name`/`baseUrl`/`models` 区分
  2. 启动 peri，输入 `/login` 打开 Login 面板
  3. 用 ↓ 键将光标移到第二个 provider 上
  4. 按 Tab 进入 Edit 模式，修改任意字段内容
  5. 按 Enter 保存
  6. 观察：面板显示"已保存"，但退出后查看 settings.json，第一个 provider 的内容被本次修改覆盖，第二个 provider 保持原样
- **环境**：macOS，当前最新 peri 构建

## 涉及文件

- `peri-tui/src/app/login_panel/mod.rs:177-181` — `select_provider()`：仅设置 `active_provider_id = p.id`，当多个 provider 共享同一 `id` 时无法区分选中项
- `peri-tui/src/app/login_panel/mod.rs:329` — `apply_edit()` 的 Edit 分支：`find(|x| x.id == id)` 在 `id` 重复时总是匹配第一个 provider，导致写入错位
- `peri-tui/src/app/login_panel/mod.rs:106-108` — `from_config()`：`position(|p| p.id == cfg.config.active_provider_id)` 同样只匹配第一个同 id provider
- `peri-acp/src/provider/mod.rs:90-95` — `LlmProvider::from_config()`：与上同理，`active_provider_id` 不能唯一标识 provider 时总是取第一个
- `peri-tui/src/app/login_panel/login_panel_test.rs` — 现有测试使用唯一 id（`"anthropic"`、`"openrouter"`），未覆盖 id 重复场景

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-18 | — | Open | agent | 创建 |

## 修复记录

（由 fix-issue 或 issue-verify skill 追加，创建时留空）
