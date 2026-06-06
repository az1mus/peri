# /plugin 斜杠命令 Marketplace 子命令 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 `/plugin` 斜杠命令添加 `marketplace add`、`install`、`marketplace update` 三个子命令支持。

**Architecture:** 在 `PluginCommand::execute` 中按 `split_whitespace` 分词路由到 App 方法；`marketplace add` 复用已有方法；`install` 和 `marketplace update` 各新增一个 App 方法，采用同步（保存+推送消息）+异步 spawn（`bg_event_tx` 发事件）的模式。

**Tech Stack:** Rust 2021, tokio async, ratatui TUI

**Spec:** docs/superpowers/specs/2026-06-06-plugin-slash-command-marketplace-support-design.md

---

### Task 1: PluginCommand 子命令路由测试

**Files:**
- Create: `peri-tui/src/command/panel/plugin_test.rs`

**Context:** 先写测试覆盖 5 种路由分支，验证执行后的可观察副作用。`MessageViewModel` 是 enum，用模式匹配检查 `SystemNote` 变体。

- [ ] **Step 1: 创建测试文件并编写路由测试**

```rust
// peri-tui/src/command/panel/plugin_test.rs
use super::*;
use crate::app::App;
use crate::command::Command;

async fn make_headless() -> App {
    let (app, _handle) = App::new_headless(80, 24).await;
    app
}

/// 辅助：获取最近一条系统消息文本
fn last_system_note(app: &App) -> Option<String> {
    app.session_mgr
        .current()
        .messages
        .view_messages
        .iter()
        .rev()
        .find(|vm| matches!(vm, crate::ui::message_view::MessageViewModel::SystemNote { .. }))
        .map(|vm| match vm {
            crate::ui::message_view::MessageViewModel::SystemNote { content, .. } => content.clone(),
            _ => unreachable!(),
        })
}

#[tokio::test]
async fn test_plugin_empty_args_opens_panel() {
    let mut app = make_headless().await;
    let cmd = PluginCommand;
    cmd.execute(&mut app, "");
    // 空参数应打开 Plugin Panel
    assert!(
        app.global_panels.get::<crate::app::plugin_panel::PluginPanel>().is_some(),
        "无参数应打开插件面板"
    );
}

#[tokio::test]
async fn test_plugin_marketplace_add_to_official_works() {
    let mut app = make_headless().await;
    let cmd = PluginCommand;
    // anthropics/claude-plugins-official 已内置，add 会触发"已存在"错误
    cmd.execute(&mut app, "marketplace add anthropics/claude-plugins-official");
    let msg = last_system_note(&app);
    // 已存在 → marketplace_add_and_save 返回 Err → execute push 错误消息
    assert!(msg.is_some(), "marketplace add（重复）应产生错误消息");
}

#[tokio::test]
async fn test_plugin_marketplace_update_missing_shows_error() {
    let mut app = make_headless().await;
    let cmd = PluginCommand;
    cmd.execute(&mut app, "marketplace update nonexistent-marketplace");
    let msg = last_system_note(&app);
    assert!(msg.is_some(), "marketplace update（不存在）应产生错误消息");
    assert!(msg.unwrap().contains("未找到"), "错误消息应提及未找到");
}

#[tokio::test]
async fn test_plugin_install_missing_shows_error() {
    let mut app = make_headless().await;
    let cmd = PluginCommand;
    cmd.execute(&mut app, "install none@none");
    let msg = last_system_note(&app);
    assert!(msg.is_some(), "install（不存在）应产生错误消息");
}

#[tokio::test]
async fn test_plugin_unknown_subcommand_shows_usage() {
    let mut app = make_headless().await;
    let cmd = PluginCommand;
    cmd.execute(&mut app, "unknown sub command");
    let msg = last_system_note(&app);
    assert!(msg.is_some(), "未知子命令应显示用法提示");
    assert!(
        msg.unwrap().contains("用法"),
        "未知子命令的消息应包含用法说明"
    );
}
```

- [ ] **Step 2: 注册测试文件并运行**

在 `peri-tui/src/command/panel/mod.rs` 的 `plugin` 行下方添加测试模块声明：
```rust
pub mod plugin;
#[cfg(test)]
mod plugin_test;
```

运行：
```bash
cargo test -p peri-tui -- plugin_test --nocapture
```
预期：`test_plugin_empty_args_opens_panel` 通过（现有行为），其余 4 个失败（`execute` 未处理 args）。

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/command/panel/plugin_test.rs peri-tui/src/command/panel/mod.rs
git commit -m "test: add PluginCommand routing tests for marketplace subcommands

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 2: 实现 PluginCommand 子命令路由

**Files:**
- Modify: `peri-tui/src/command/panel/plugin.rs`

**Key decision:** 用 `app.session_mgr.current_mut().messages.push_system_note()` 推送错误和帮助消息，避免跨模块导入 `MessageViewModel`/`PipelineAction`。App 方法（在 `app` 模块内）内部已使用 `apply_pipeline_action` 处理反馈。

- [ ] **Step 1: 修改 PluginCommand::execute**

```rust
// peri-tui/src/command/panel/plugin.rs
use crate::{app::App, command::Command};

pub struct PluginCommand;

impl Command for PluginCommand {
    fn name(&self) -> &str {
        "plugin"
    }
    fn description(&self, _lc: &crate::i18n::LcRegistry) -> String {
        _lc.tr("command-plugin-description")
    }
    fn execute(&self, app: &mut App, args: &str) {
        let parts: Vec<&str> = args.split_whitespace().collect();
        match parts.as_slice() {
            // /plugin（无参数）→ 打开面板（现有行为）
            [] => app.open_plugin_panel(),

            // /plugin marketplace add <url>
            ["marketplace", "add", rest @ ..] if !rest.is_empty() => {
                let input = rest.join(" ");
                if let Err(e) = app.marketplace_add_and_save(&input) {
                    app.session_mgr
                        .current_mut()
                        .messages
                        .push_system_note(format!("添加 marketplace 失败: {}", e));
                }
            }

            // /plugin install <name@marketplace>
            ["install", name_at_marketplace] => {
                let (name, marketplace) = name_at_marketplace
                    .split_once('@')
                    .unwrap_or((name_at_marketplace, "claude-plugins-official"));
                if let Err(e) = app.plugin_install_by_marketplace(name, marketplace) {
                    app.session_mgr
                        .current_mut()
                        .messages
                        .push_system_note(format!("安装插件失败: {}", e));
                }
            }

            // /plugin marketplace update <name>
            ["marketplace", "update", name] => {
                if let Err(e) = app.marketplace_update_and_refresh(name) {
                    app.session_mgr
                        .current_mut()
                        .messages
                        .push_system_note(format!("更新 marketplace 失败: {}", e));
                }
            }

            // 未知用法 → 显示帮助
            _ => {
                let help = "用法:\n\
                    /plugin                                    — 打开插件面板\n\
                    /plugin marketplace add <url>              — 添加市场源\n\
                    /plugin install <name>@<marketplace>       — 安装插件\n\
                    /plugin marketplace update <name>          — 更新市场缓存";
                app.session_mgr
                    .current_mut()
                    .messages
                    .push_system_note(help.to_string());
            }
        }
    }
}
```

- [ ] **Step 2: 编译检查（预期部分报错）**

```bash
cargo build -p peri-tui 2>&1 | head -30
```
预期：`plugin_install_by_marketplace` 和 `marketplace_update_and_refresh` 尚不存在，编译报错。Task 3-4 会实现。

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/command/panel/plugin.rs
git commit -m "feat: add PluginCommand subcommand routing for marketplace add/install/update

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 3: 实现 App::plugin_install_by_marketplace

**Files:**
- Modify: `peri-tui/src/app/panel_plugin.rs`

**Context:** 该方法在 `app` 模块内，可直接使用 `MessageViewModel::system()` 和 `PipelineAction::AddMessage`（通过 `use super::*` 已有）。同步部分查缓存+检查重复，异步 spawn 安装。

- [ ] **Step 1: 在 panel_plugin.rs `marketplace_add_and_save` 之后追加**

```rust
/// 从 marketplace 缓存安装指定插件（供 /plugin install 斜杠命令使用）
pub fn plugin_install_by_marketplace(
    &mut self,
    name: &str,
    marketplace: &str,
) -> anyhow::Result<()> {
    use peri_middlewares::plugin::{
        install_plugin, load_installed_plugins, marketplaces_cache_dir, InstallScope,
    };

    // 1. 从 marketplace 缓存查找插件
    let found = {
        use peri_middlewares::plugin::{load_known_marketplaces, MarketplaceManager};
        let known = load_known_marketplaces(None).unwrap_or_default();
        let cache_base = marketplaces_cache_dir();
        let mut result = None;
        for km in &known {
            let km_name = MarketplaceManager::extract_name(&km.source);
            if km_name != marketplace {
                continue;
            }
            let manifest = peri_middlewares::plugin::marketplace::find_marketplace_json(
                &cache_base.join(&km_name),
            )
            .and_then(|p| {
                peri_middlewares::plugin::marketplace::read_manifest_from_path(&p).ok()
            });
            if let Some(ref m) = manifest {
                if m.plugins.iter().any(|p| p.name == name) {
                    result = Some(());
                    break;
                }
            }
        }
        result
    };

    if found.is_none() {
        anyhow::bail!(
            "未找到插件 '{}' (marketplace: {})。请确保 marketplace 已添加且缓存已刷新。",
            name,
            marketplace
        );
    }

    // 2. 检查是否已安装
    let installed = load_installed_plugins(None).unwrap_or_default();
    let plugin_id = format!("{}@{}", name, marketplace);
    if installed.plugins.iter().any(|p| p.id == plugin_id) {
        let vm = MessageViewModel::system(format!(
            "插件 '{}' 已安装，无需重复安装",
            plugin_id
        ));
        self.apply_pipeline_action(PipelineAction::AddMessage(vm));
        return Ok(());
    }

    // 3. 推送进度消息
    let vm = MessageViewModel::system(format!("正在安装 {}@{} ...", name, marketplace));
    self.apply_pipeline_action(PipelineAction::AddMessage(vm));

    // 4. Spawn 异步安装
    let name = name.to_string();
    let mkt = marketplace.to_string();
    let cache_dir = marketplaces_cache_dir();
    let claude_dir = dirs_next::home_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join(".claude");
    let tx = self.services.bg_event_tx.clone();

    tokio::spawn(async move {
        let result = install_plugin(
            &name, &mkt, InstallScope::User, &cache_dir, &claude_dir, None,
        ).await;
        let plugin_id = format!("{}@{}", name, mkt);
        let (success, msg) = match &result {
            Ok(r) => (true, format!("已安装: {} v{}", r.id, r.version)),
            Err(e) => (false, format!("安装失败: {}", e)),
        };
        let _ = tx
            .send(AgentEvent::PluginActionCompleted {
                plugin_id,
                action: "install".to_string(),
                success,
                message: msg,
            })
            .await;
    });

    Ok(())
}
```

- [ ] **Step 2: 编译验证**

```bash
cargo build -p peri-tui 2>&1 | head -20
```
预期：只剩 `marketplace_update_and_refresh` 报错。

- [ ] **Step 3: 运行路由测试**

```bash
cargo test -p peri-tui -- plugin_test --nocapture
```
预期：`test_plugin_install_missing_shows_error` 通过（显示"未找到插件"错误）。

- [ ] **Step 4: Commit**

```bash
git add peri-tui/src/app/panel_plugin.rs
git commit -m "feat: add App::plugin_install_by_marketplace for slash command install

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 4: 实现 App::marketplace_update_and_refresh

**Files:**
- Modify: `peri-tui/src/app/panel_plugin.rs`

- [ ] **Step 1: 在 panel_plugin.rs 追加（紧跟 plugin_install_by_marketplace 之后）**

```rust
/// 刷新指定 marketplace 缓存（供 /plugin marketplace update 斜杠命令使用）
pub fn marketplace_update_and_refresh(&mut self, name: &str) -> anyhow::Result<()> {
    use peri_middlewares::plugin::{
        load_known_marketplaces, save_known_marketplaces, MarketplaceManager,
    };

    // 1. 查找 marketplace
    let known = load_known_marketplaces(None).unwrap_or_default();
    let target = known
        .iter()
        .find(|km| MarketplaceManager::extract_name(&km.source) == name);

    let source = match target {
        Some(km) => km.source.clone(),
        None => {
            anyhow::bail!(
                "未找到 marketplace '{}'。请先通过 /plugin marketplace add 添加。",
                name
            );
        }
    };

    // 2. 推送进度消息
    let vm = MessageViewModel::system(format!("正在刷新 marketplace '{}' ...", name));
    self.apply_pipeline_action(PipelineAction::AddMessage(vm));

    // 3. Spawn 后台刷新
    let name = name.to_string();
    let tx = self.services.bg_event_tx.clone();

    tokio::spawn(async move {
        use peri_middlewares::plugin::marketplace::refresh_marketplace;
        match refresh_marketplace(&source, &name).await {
            Ok((_manifest, install_location)) => {
                if let Ok(mut marketplaces) = load_known_marketplaces(None) {
                    if let Some(entry) = marketplaces.iter_mut().find(|km| {
                        MarketplaceManager::extract_name(&km.source) == name
                    }) {
                        entry.install_location = install_location;
                        entry.last_updated = chrono::Utc::now().to_rfc3339();
                        let _ = save_known_marketplaces(&marketplaces, None);
                    }
                }
                let _ = tx
                    .send(AgentEvent::PluginActionCompleted {
                        plugin_id: name.clone(),
                        action: "refresh".to_string(),
                        success: true,
                        message: format!("Marketplace '{}' 已更新", name),
                    })
                    .await;
            }
            Err(e) => {
                let _ = tx
                    .send(AgentEvent::PluginActionCompleted {
                        plugin_id: name.clone(),
                        action: "refresh".to_string(),
                        success: false,
                        message: format!("更新失败: {}", e),
                    })
                    .await;
            }
        }
    });

    Ok(())
}
```

- [ ] **Step 2: 编译验证**

```bash
cargo build -p peri-tui 2>&1 | tail -5
```
预期：编译成功。

- [ ] **Step 3: 运行全部路由测试**

```bash
cargo test -p peri-tui -- plugin_test --nocapture
```
预期：全部 5 个测试通过。

- [ ] **Step 4: Commit**

```bash
git add peri-tui/src/app/panel_plugin.rs
git commit -m "feat: add App::marketplace_update_and_refresh for slash command update

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 5: 全量验证

- [ ] **Step 1: 运行全量测试**

```bash
cargo test 2>&1 | tail -20
```
预期：所有测试通过。

- [ ] **Step 2: Clippy 检查**

```bash
cargo clippy -p peri-tui -- -D warnings 2>&1 | tail -10
```
预期：无 warning。

- [ ] **Step 3: 确认改动范围并最终 commit**

```bash
git diff --stat HEAD~4
```
确认改动文件：`plugin.rs`、`plugin_test.rs`、`panel_plugin.rs`、`mod.rs`。

```bash
git commit --allow-empty -m "chore: verify all tests pass for plugin marketplace subcommand feature

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```
