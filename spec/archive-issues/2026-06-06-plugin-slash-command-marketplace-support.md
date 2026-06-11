> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-plugin-slash-command-marketplace-support.md

# /plugin 命令缺少 marketplace add、install@marketplace、marketplace update 子命令

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

`/plugin` 斜杠命令目前只打开插件面板，不支持子命令解析。CLI 已支持 `plugin install name@marketplace` 格式，Plugin Panel UI 已支持 marketplace 的 add/update/delete 操作，但斜杠命令路径无法直接执行这些操作。用户需要绕过 UI 面板、通过文本命令来添加市场源、从指定市场安装插件、或刷新市场缓存。

## 症状详情

`/plugin` 命令的行为与 CLI `plugin` 子命令不一致：

| 操作 | CLI | 斜杠命令 `/plugin` | Plugin Panel UI |
|------|-----|-------------------|-----------------|
| 列出已安装插件 | `plugin list` | ❌ 不支持 | ✅ |
| 安装插件 | `plugin install foo@marketplace` | ❌ 不支持（只能打开面板） | ✅ |
| 卸载插件 | `plugin uninstall <id>` | ❌ 不支持 | ✅ |
| 添加 marketplace | ❌ 不支持 | ❌ 不支持 | ✅ |
| 更新 marketplace | ❌ 不支持 | ❌ 不支持 | ✅ |
| 删除 marketplace | ❌ 不支持 | ❌ 不支持 | ✅ |

用户期望的三种新子命令格式：
1. `/plugin marketplace add https://github.com/robzilla1738/supergoal.git` — 将 Git 仓库添加为新的市场源
2. `/plugin install supergoal@supergoal` — 从指定市场源安装插件
3. `/plugin marketplace update supergoal` — 刷新指定市场源的缓存

### 验证 #1（2026-06-06）—— 通过

验证方式：路由正确性通过 5 个单元测试，全量 639 测试通过，clippy 无警告。

- `test_plugin_empty_args_opens_panel` — 无参数打开面板 ✅
- `test_plugin_marketplace_add_to_existing_shows_error` — 重复 marketplace 报错 ✅
- `test_plugin_marketplace_update_missing_shows_error` — 不存在的 marketplace 报错 ✅
- `test_plugin_install_missing_shows_error` — 不存在的 marketplace 报错 ✅
- `test_plugin_unknown_subcommand_shows_usage` — 未知子命令显示用法 ✅

异步路径（`install_plugin` / `refresh_marketplace`）复用已有函数，错误通过 `PluginActionCompleted` 事件反馈。

## 涉及文件

- `peri-tui/src/command/panel/plugin.rs` — `/plugin` 命令的 dispatch 入口，目前只调用 `app.open_plugin_panel()`
- `peri-tui/src/cli_plugin.rs` — CLI 已实现 `plugin install name@marketplace` 解析逻辑（split_once('@')），可复用参考
- `peri-middlewares/src/plugin/marketplace/mod.rs` — `parse_marketplace_input()` 和 `refresh_marketplace()` 已存在，可直接调用
- `peri-middlewares/src/plugin/installer/install.rs` — `install_plugin()` 已支持从指定 marketplace 安装，可直接调用
- `peri-middlewares/src/plugin/config.rs` — `load_known_marketplaces()` / `save_known_marketplaces()` 已存在
- `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/persistence.rs` — Plugin Panel UI 中的 add/update marketplace 逻辑，可参考复用
- `peri-tui/src/app/panel_plugin.rs` — Panel 层 marketplace add/update 逻辑的另一个入口

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |
| 2026-06-06 | Open | Verified | agent | 功能已实现，639 测试通过 |

## 修复记录

### 修复 #1（2026-06-06）

- **操作人**：agent
- **用户原意**：为 `/plugin` 命令添加 `marketplace add`、`install name@marketplace`、`marketplace update` 三种子命令
- **修复内容**：
  - `PluginCommand::execute` 新增 5 路分支路由（空/marketplace add/install/marketplace update/未知）
  - `App::plugin_install_by_marketplace` — 同步检查 + 异步 spawn install（bg_event_tx 反馈）
  - `App::marketplace_update_and_refresh` — 同步查找 + 异步 spawn refresh（bg_event_tx 反馈）
  - `marketplace_add_and_save` 复用已有方法
- **涉及 commit**：
  - `2976b2a` — test: routing tests
  - `e23a29f` — feat: subcommand routing
  - `cc4a174` — feat: plugin_install_by_marketplace + marketplace_update_and_refresh
- **验证状态**：已验证
