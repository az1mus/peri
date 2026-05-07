use perihelion_widgets::InputState;
use rust_agent_middlewares::plugin::InstallScope;
use std::collections::HashSet;

/// Discover 视图中展示的可用插件
#[derive(Debug, Clone)]
pub struct DiscoverPlugin {
    pub name: String,
    pub description: String,
    pub marketplace: String,
    pub version: String,
    pub author: Option<String>,
    pub installed: bool,
    pub plugin_id: String,
    pub install_count: Option<u64>,
}

/// Discover 详情页操作菜单
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscoverDetailAction {
    InstallUser,
    InstallProject,
    BackToList,
}

impl DiscoverDetailAction {
    pub const ALL: [DiscoverDetailAction; 3] = [
        DiscoverDetailAction::InstallUser,
        DiscoverDetailAction::InstallProject,
        DiscoverDetailAction::BackToList,
    ];

    pub fn label(&self) -> &'static str {
        match self {
            Self::InstallUser => "Install (User scope)",
            Self::InstallProject => "Install (Project scope)",
            Self::BackToList => "Back to list",
        }
    }
}

/// Marketplace 条目（Marketplaces 视图用）
#[derive(Debug, Clone)]
pub struct MarketplaceViewEntry {
    pub name: String,
    pub source: rust_agent_middlewares::plugin::MarketplaceSource,
    pub source_label: String,
    pub plugin_count: usize,
    pub installed_count: usize,
    pub status: MarketplaceViewStatus,
    pub last_updated: Option<String>,
    pub auto_update: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MarketplaceViewStatus {
    Fresh,
    Cached,
    Fetching,
    Stale,
    Failed,
}

/// 插件条目类型
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginItemType {
    Plugin,
    Mcp,
}

/// 面板中展示的插件条目
#[derive(Debug, Clone)]
pub struct PluginEntry {
    pub id: String,
    pub name: String,
    pub plugin_type: PluginItemType,
    pub marketplace: String,
    pub enabled: bool,
    pub scope: InstallScope,
    pub version: String,
    pub install_path: std::path::PathBuf,
    pub project_path: Option<String>,
    pub load_error: Option<String>,
    pub description: String,
    pub author: Option<String>,
    pub commands: Vec<String>,
    pub skills: Vec<String>,
    pub agents: Vec<String>,
    pub mcp_servers: Vec<String>,
}

/// 详情页操作菜单
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DetailAction {
    ToggleEnabled,
    Uninstall,
    BackToList,
}

impl DetailAction {
    pub const ALL: [DetailAction; 3] = [
        DetailAction::ToggleEnabled,
        DetailAction::Uninstall,
        DetailAction::BackToList,
    ];

    pub fn label(&self, enabled: bool) -> &'static str {
        match self {
            Self::ToggleEnabled => {
                if enabled {
                    "Disable plugin"
                } else {
                    "Enable plugin"
                }
            }
            Self::Uninstall => "Uninstall",
            Self::BackToList => "Back to plugin list",
        }
    }
}

/// 插件面板视图
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PluginPanelView {
    Installed,
    Discover,
    Marketplaces,
    Errors,
}

impl PluginPanelView {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Installed => "Installed",
            Self::Discover => "Discover",
            Self::Marketplaces => "Marketplaces",
            Self::Errors => "Errors",
        }
    }

    pub const ALL: [PluginPanelView; 4] = [
        PluginPanelView::Installed,
        PluginPanelView::Discover,
        PluginPanelView::Marketplaces,
        PluginPanelView::Errors,
    ];

    pub fn next(&mut self) {
        *self = match self {
            Self::Installed => Self::Discover,
            Self::Discover => Self::Marketplaces,
            Self::Marketplaces => Self::Errors,
            Self::Errors => Self::Installed,
        };
    }

    pub fn prev(&mut self) {
        *self = match self {
            Self::Installed => Self::Errors,
            Self::Discover => Self::Installed,
            Self::Marketplaces => Self::Discover,
            Self::Errors => Self::Marketplaces,
        };
    }
}

/// /plugin 面板状态
#[derive(Debug, Clone)]
pub struct PluginPanel {
    pub cursor: usize,
    pub view: PluginPanelView,
    pub scroll_offset: u16,
    pub entries: Vec<PluginEntry>,
    pub confirm_delete: Option<String>,
    /// 详情视图：已进入时为 Some(entry_index)
    pub detail_index: Option<usize>,
    /// 详情页操作菜单光标
    pub detail_cursor: usize,

    // --- Discover 视图状态 ---
    pub discover_plugins: Vec<DiscoverPlugin>,
    pub discover_search: InputState,
    pub discover_searching: bool,
    pub discover_cursor: usize,
    pub discover_scroll: u16,
    pub discover_loading: bool,
    pub discover_selected: HashSet<String>,
    pub discover_detail_index: Option<usize>,
    pub discover_detail_cursor: usize,

    // --- Marketplaces 视图状态 ---
    pub marketplace_entries: Vec<MarketplaceViewEntry>,
    pub marketplace_cursor: usize,
    pub marketplace_scroll: u16,
    pub marketplace_confirm_delete: Option<usize>,
    pub marketplace_updating: HashSet<String>,
    /// 添加 marketplace 输入框
    pub add_marketplace_input: InputState,
    /// 是否处于添加 marketplace 模式
    pub add_marketplace_active: bool,

    // --- 安装/卸载进度 ---
    pub installing: HashSet<String>,
    pub uninstalling: HashSet<String>,
}

impl PluginPanel {
    pub fn new(entries: Vec<PluginEntry>) -> Self {
        Self {
            cursor: 0,
            view: PluginPanelView::Installed,
            scroll_offset: 0,
            entries,
            confirm_delete: None,
            detail_index: None,
            detail_cursor: 0,
            discover_plugins: Vec::new(),
            discover_search: InputState::new(),
            discover_searching: false,
            discover_cursor: 0,
            discover_scroll: 0,
            discover_loading: false,
            discover_selected: HashSet::new(),
            discover_detail_index: None,
            discover_detail_cursor: 0,
            marketplace_entries: Vec::new(),
            marketplace_cursor: 0,
            marketplace_scroll: 0,
            marketplace_confirm_delete: None,
            marketplace_updating: HashSet::new(),
            add_marketplace_input: InputState::new(),
            add_marketplace_active: false,
            installing: HashSet::new(),
            uninstalling: HashSet::new(),
        }
    }

    pub fn is_detail(&self) -> bool {
        self.detail_index.is_some()
            || self.discover_detail_index.is_some()
            || self.add_marketplace_active
    }

    /// 按搜索词过滤后的 Discover 插件列表
    pub fn discover_filtered_plugins(&self) -> Vec<&DiscoverPlugin> {
        let search = self.discover_search.value();
        if search.is_empty() {
            self.discover_plugins.iter().collect()
        } else {
            let query = search.to_lowercase();
            self.discover_plugins
                .iter()
                .filter(|p| {
                    p.name.to_lowercase().contains(&query)
                        || p.description.to_lowercase().contains(&query)
                        || p.marketplace.to_lowercase().contains(&query)
                })
                .collect()
        }
    }

    /// 获取当前光标处的 Discover 插件
    pub fn discover_current_plugin(&self) -> Option<&DiscoverPlugin> {
        let filtered = self.discover_filtered_plugins();
        filtered.get(self.discover_cursor).copied()
    }

    /// 根据当前视图过滤后的可见条目索引列表
    pub fn visible_indices(&self) -> Vec<usize> {
        self.entries
            .iter()
            .enumerate()
            .filter(|(_, e)| match self.view {
                PluginPanelView::Installed => true,
                PluginPanelView::Errors => e.load_error.is_some(),
                PluginPanelView::Discover | PluginPanelView::Marketplaces => false,
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub fn current_list_len(&self) -> usize {
        self.visible_indices().len()
    }

    pub fn selected_entry(&self) -> Option<&PluginEntry> {
        let indices = self.visible_indices();
        indices.get(self.cursor).and_then(|&i| self.entries.get(i))
    }
}

// ─── App 操作方法 ────────────────────────────────────────────────────────────

use super::App;

impl App {
    pub fn plugin_panel_move_up(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if panel.cursor > 0 {
                panel.cursor -= 1;
            }
        }
    }

    pub fn plugin_panel_move_down(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            let max = panel.current_list_len().saturating_sub(1);
            if panel.cursor < max {
                panel.cursor += 1;
            }
        }
    }

    pub fn plugin_panel_tab(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.view.next();
            panel.cursor = 0;
            panel.scroll_offset = 0;
        }
    }

    pub fn plugin_panel_shift_tab(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.view.prev();
            panel.cursor = 0;
            panel.scroll_offset = 0;
        }
    }

    pub fn plugin_panel_close(&mut self) {
        self.plugin_panel = None;
    }

    pub fn plugin_panel_request_delete(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if let Some(entry) = panel.selected_entry() {
                panel.confirm_delete = Some(entry.id.clone());
            }
        }
    }

    pub fn plugin_panel_cancel_delete(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.confirm_delete = None;
        }
    }

    pub fn plugin_panel_confirm_delete(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if let Some(id) = panel.confirm_delete.take() {
                panel.entries.retain(|p| p.id != id);
            }
        }
    }

    pub fn plugin_panel_toggle_enabled(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if let Some(entry_idx) = panel.visible_indices().get(panel.cursor).copied() {
                if let Some(entry) = panel.entries.get_mut(entry_idx) {
                    entry.enabled = !entry.enabled;
                    self.persist_plugin_enabled_state();
                }
            }
        }
    }

    /// 将当前面板中所有插件的启用状态持久化到 ~/.claude/settings.json
    fn persist_plugin_enabled_state(&self) {
        if let Some(panel) = &self.plugin_panel {
            let states: Vec<(String, bool)> = panel
                .entries
                .iter()
                .map(|e| (e.id.clone(), e.enabled))
                .collect();
            if let Err(e) = rust_agent_middlewares::plugin::save_claude_settings_enabled_plugins(
                &states,
                self.claude_settings_override.as_deref(),
            ) {
                tracing::warn!(error = %e, "保存 enabledPlugins 失败");
            }
        }
    }

    /// 进入选中插件的详情视图
    pub fn plugin_panel_enter_detail(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if let Some(&entry_idx) = panel.visible_indices().get(panel.cursor) {
                panel.detail_index = Some(entry_idx);
                panel.detail_cursor = 0;
                panel.scroll_offset = 0;
            }
        }
    }

    /// 退出详情视图回到列表
    pub fn plugin_panel_exit_detail(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.detail_index = None;
            panel.detail_cursor = 0;
            panel.scroll_offset = 0;
        }
    }

    /// 详情页操作菜单上移
    pub fn plugin_panel_detail_up(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if panel.detail_cursor > 0 {
                panel.detail_cursor -= 1;
            }
        }
    }

    /// 详情页操作菜单下移
    pub fn plugin_panel_detail_down(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            let max = DetailAction::ALL.len().saturating_sub(1);
            if panel.detail_cursor < max {
                panel.detail_cursor += 1;
            }
        }
    }

    /// 执行详情页当前操作
    pub fn plugin_panel_detail_action(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            let action = DetailAction::ALL.get(panel.detail_cursor).copied();
            let entry_idx = panel.detail_index;
            match action {
                Some(DetailAction::ToggleEnabled) => {
                    if let Some(idx) = entry_idx {
                        if let Some(entry) = panel.entries.get_mut(idx) {
                            entry.enabled = !entry.enabled;
                        }
                        // 面板引用已释放，调用保存
                        let states: Vec<(String, bool)> = panel
                            .entries
                            .iter()
                            .map(|e| (e.id.clone(), e.enabled))
                            .collect();
                        if let Err(e) =
                            rust_agent_middlewares::plugin::save_claude_settings_enabled_plugins(
                                &states,
                                self.claude_settings_override.as_deref(),
                            )
                        {
                            tracing::warn!(error = %e, "保存 enabledPlugins 失败");
                        }
                    }
                }
                Some(DetailAction::Uninstall) => {
                    if let Some(idx) = entry_idx {
                        let id = panel.entries.get(idx).map(|e| e.id.clone());
                        if let Some(id) = id {
                            panel.confirm_delete = Some(id);
                        }
                    }
                }
                Some(DetailAction::BackToList) => {
                    panel.detail_index = None;
                    panel.detail_cursor = 0;
                    panel.scroll_offset = 0;
                }
                None => {}
            }
        }
    }

    // ─── Discover 视图操作 ─────────────────────────────────────────────────────

    pub fn discover_move_up(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if panel.discover_cursor > 0 {
                panel.discover_cursor -= 1;
            }
        }
    }

    pub fn discover_move_down(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            let max = panel.discover_filtered_plugins().len().saturating_sub(1);
            if panel.discover_cursor < max {
                panel.discover_cursor += 1;
            }
        }
    }

    #[allow(dead_code)]
    pub fn discover_toggle_selected(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if let Some(plugin) = panel.discover_current_plugin() {
                let id = plugin.plugin_id.clone();
                if panel.discover_selected.contains(&id) {
                    panel.discover_selected.remove(&id);
                } else {
                    panel.discover_selected.insert(id);
                }
            }
        }
    }

    pub fn discover_enter_search(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.discover_searching = true;
        }
    }

    pub fn discover_exit_search(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.discover_searching = false;
            panel.discover_cursor = 0;
        }
    }

    pub fn discover_search_input(&mut self, ch: char) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.discover_search.insert(ch);
            panel.discover_cursor = 0;
        }
    }

    pub fn discover_search_backspace(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.discover_search.backspace();
            panel.discover_cursor = 0;
        }
    }

    pub fn discover_enter_detail(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if panel.discover_current_plugin().is_some() {
                panel.discover_detail_index = Some(panel.discover_cursor);
                panel.discover_detail_cursor = 0;
            }
        }
    }

    pub fn discover_exit_detail(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.discover_detail_index = None;
            panel.discover_detail_cursor = 0;
        }
    }

    pub fn discover_detail_up(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if panel.discover_detail_cursor > 0 {
                panel.discover_detail_cursor -= 1;
            }
        }
    }

    pub fn discover_detail_down(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            let max = DiscoverDetailAction::ALL.len().saturating_sub(1);
            if panel.discover_detail_cursor < max {
                panel.discover_detail_cursor += 1;
            }
        }
    }

    /// 执行 Discover 详情页操作（安装或返回）
    pub fn discover_detail_action(&mut self) -> Option<(String, String, InstallScope)> {
        if let Some(panel) = &mut self.plugin_panel {
            let action = DiscoverDetailAction::ALL
                .get(panel.discover_detail_cursor)
                .copied();
            let plugin_idx = panel.discover_detail_index;
            match action {
                Some(DiscoverDetailAction::InstallUser) => {
                    if let Some(dp) = plugin_idx.and_then(|i| panel.discover_plugins.get(i)) {
                        return Some((dp.name.clone(), dp.marketplace.clone(), InstallScope::User));
                    }
                }
                Some(DiscoverDetailAction::InstallProject) => {
                    if let Some(dp) = plugin_idx.and_then(|i| panel.discover_plugins.get(i)) {
                        return Some((
                            dp.name.clone(),
                            dp.marketplace.clone(),
                            InstallScope::Project,
                        ));
                    }
                }
                Some(DiscoverDetailAction::BackToList) => {
                    panel.discover_detail_index = None;
                    panel.discover_detail_cursor = 0;
                }
                None => {}
            }
        }
        None
    }

    // ─── Marketplaces 视图操作 ──────────────────────────────────────────────────

    pub fn marketplace_move_up(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            if panel.marketplace_cursor > 0 {
                panel.marketplace_cursor -= 1;
            }
        }
    }

    pub fn marketplace_move_down(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            // cursor = 0 是 Add Marketplace，最大值是 marketplace_entries.len()
            let max = panel.marketplace_entries.len();
            if panel.marketplace_cursor < max {
                panel.marketplace_cursor += 1;
            }
        }
    }

    /// 检查当前是否选中了 "Add Marketplace" 选项
    pub fn marketplace_is_add_selected(&self) -> bool {
        self.plugin_panel
            .as_ref()
            .map(|p| p.marketplace_cursor == 0)
            .unwrap_or(false)
    }

    /// 获取当前选中的 marketplace 名称（如果选中 Add Marketplace 则返回 None）
    pub fn marketplace_current_name(&self) -> Option<String> {
        self.plugin_panel
            .as_ref()
            .filter(|p| p.marketplace_cursor > 0)
            .and_then(|p| p.marketplace_entries.get(p.marketplace_cursor - 1))
            .map(|m| m.name.clone())
    }

    /// 请求删除当前 marketplace（进入确认状态）
    pub fn marketplace_request_delete(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            // cursor = 0 是 Add Marketplace，不能删除
            if panel.marketplace_cursor > 0 {
                let idx = panel.marketplace_cursor - 1;
                if panel.marketplace_entries.get(idx).is_some() {
                    panel.marketplace_confirm_delete = Some(idx);
                }
            }
        }
    }

    /// 取消删除 marketplace
    pub fn marketplace_cancel_delete(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.marketplace_confirm_delete = None;
        }
    }

    /// 确认删除当前 marketplace，返回要删除的 marketplace 名称
    pub fn marketplace_confirm_delete(&mut self) -> Option<String> {
        if let Some(panel) = &mut self.plugin_panel {
            if let Some(idx) = panel.marketplace_confirm_delete.take() {
                if let Some(entry) = panel.marketplace_entries.get(idx) {
                    let name = entry.name.clone();
                    // 从列表中移除
                    panel.marketplace_entries.remove(idx);
                    // 调整光标位置（确保不超出范围）
                    let max = panel.marketplace_entries.len();
                    if panel.marketplace_cursor > max {
                        panel.marketplace_cursor = max;
                    }
                    return Some(name);
                }
            }
        }
        None
    }

    /// 请求更新当前 marketplace（添加到 updating 集合）
    pub fn marketplace_request_update(&mut self) -> Option<String> {
        if let Some(panel) = &mut self.plugin_panel {
            // cursor = 0 是 Add Marketplace，不能更新
            if panel.marketplace_cursor > 0 {
                let idx = panel.marketplace_cursor - 1;
                if let Some(entry) = panel.marketplace_entries.get(idx) {
                    let name = entry.name.clone();
                    panel.marketplace_updating.insert(name.clone());
                    return Some(name);
                }
            }
        }
        None
    }

    /// 请求更新当前 marketplace，返回名称和 source
    pub fn marketplace_request_update_with_source(
        &mut self,
    ) -> Option<(String, rust_agent_middlewares::plugin::MarketplaceSource)> {
        if let Some(panel) = &mut self.plugin_panel {
            // cursor = 0 是 Add Marketplace，不能更新
            if panel.marketplace_cursor > 0 {
                let idx = panel.marketplace_cursor - 1;
                if let Some(entry) = panel.marketplace_entries.get(idx) {
                    let name = entry.name.clone();
                    let source = entry.source.clone();
                    panel.marketplace_updating.insert(name.clone());
                    return Some((name, source));
                }
            }
        }
        None
    }

    /// 标记 marketplace 更新完成
    pub fn marketplace_update_done(&mut self, name: &str) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.marketplace_updating.remove(name);
        }
    }

    /// 进入添加 marketplace 模式
    pub fn marketplace_enter_add(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.add_marketplace_input = InputState::new();
            panel.add_marketplace_active = true;
        }
    }

    /// 退出添加 marketplace 模式
    pub fn marketplace_exit_add(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.add_marketplace_active = false;
            panel.add_marketplace_input = InputState::new();
        }
    }

    /// 添加 marketplace 输入字符
    pub fn marketplace_add_input(&mut self, ch: char) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.add_marketplace_input.insert(ch);
        }
    }

    /// 添加 marketplace 退格
    pub fn marketplace_add_backspace(&mut self) {
        if let Some(panel) = &mut self.plugin_panel {
            panel.add_marketplace_input.backspace();
        }
    }

    /// 确认添加 marketplace，返回输入的 source 字符串
    pub fn marketplace_add_confirm(&mut self) -> Option<String> {
        if let Some(panel) = &mut self.plugin_panel {
            let input = panel.add_marketplace_input.value().trim().to_string();
            panel.add_marketplace_active = false;
            panel.add_marketplace_input = InputState::new();
            if input.is_empty() {
                None
            } else {
                Some(input)
            }
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_entry(id: &str, name: &str, enabled: bool) -> PluginEntry {
        PluginEntry {
            id: id.into(),
            name: name.into(),
            plugin_type: PluginItemType::Plugin,
            marketplace: "test".into(),
            enabled,
            scope: InstallScope::User,
            version: "1.0.0".into(),
            install_path: std::path::PathBuf::new(),
            project_path: None,
            load_error: None,
            description: String::new(),
            author: None,
            commands: vec![],
            skills: vec![],
            agents: vec![],
            mcp_servers: vec![],
        }
    }

    #[test]
    fn test_plugin_panel_new() {
        let panel = PluginPanel::new(vec![]);
        assert_eq!(panel.cursor, 0);
        assert_eq!(panel.view, PluginPanelView::Installed);
        assert!(panel.confirm_delete.is_none());
    }

    #[tokio::test]
    async fn test_plugin_panel_move_cursor() {
        let panel = PluginPanel::new(vec![
            make_entry("a@test", "a", true),
            make_entry("b@test", "b", true),
            make_entry("c@test", "c", true),
        ]);
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.plugin_panel = Some(panel);

        for _ in 0..5 {
            app.plugin_panel_move_up();
        }
        assert_eq!(app.plugin_panel.as_ref().unwrap().cursor, 0);

        for _ in 0..5 {
            app.plugin_panel_move_down();
        }
        assert_eq!(app.plugin_panel.as_ref().unwrap().cursor, 2);
    }

    #[tokio::test]
    async fn test_plugin_panel_tab_cycles_views() {
        let panel = PluginPanel::new(vec![]);
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.plugin_panel = Some(panel);

        app.plugin_panel_tab();
        assert_eq!(
            app.plugin_panel.as_ref().unwrap().view,
            PluginPanelView::Discover
        );
        app.plugin_panel_tab();
        assert_eq!(
            app.plugin_panel.as_ref().unwrap().view,
            PluginPanelView::Marketplaces
        );
        app.plugin_panel_tab();
        assert_eq!(
            app.plugin_panel.as_ref().unwrap().view,
            PluginPanelView::Errors
        );
        app.plugin_panel_tab();
        assert_eq!(
            app.plugin_panel.as_ref().unwrap().view,
            PluginPanelView::Installed
        );
    }

    #[tokio::test]
    async fn test_plugin_panel_shift_tab_cycles_back() {
        let panel = PluginPanel::new(vec![]);
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.plugin_panel = Some(panel);

        app.plugin_panel_shift_tab();
        assert_eq!(
            app.plugin_panel.as_ref().unwrap().view,
            PluginPanelView::Errors
        );
        app.plugin_panel_shift_tab();
        assert_eq!(
            app.plugin_panel.as_ref().unwrap().view,
            PluginPanelView::Marketplaces
        );
    }

    #[tokio::test]
    async fn test_plugin_panel_close() {
        let panel = PluginPanel::new(vec![]);
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.plugin_panel = Some(panel);
        app.plugin_panel_close();
        assert!(app.plugin_panel.is_none());
    }

    #[tokio::test]
    async fn test_plugin_panel_request_cancel_delete() {
        let panel = PluginPanel::new(vec![make_entry("my-plugin@test", "my-plugin", true)]);
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.plugin_panel = Some(panel);

        app.plugin_panel_request_delete();
        assert_eq!(
            app.plugin_panel.as_ref().unwrap().confirm_delete,
            Some("my-plugin@test".into())
        );

        app.plugin_panel_cancel_delete();
        assert!(app.plugin_panel.as_ref().unwrap().confirm_delete.is_none());
    }

    #[tokio::test]
    async fn test_plugin_panel_toggle_enabled() {
        let panel = PluginPanel::new(vec![make_entry("p@test", "p", true)]);
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.plugin_panel = Some(panel);

        app.plugin_panel_toggle_enabled();
        assert!(!app.plugin_panel.as_ref().unwrap().entries[0].enabled);

        app.plugin_panel_toggle_enabled();
        assert!(app.plugin_panel.as_ref().unwrap().entries[0].enabled);
    }

    #[tokio::test]
    async fn test_plugin_panel_errors_view() {
        let mut entry = make_entry("bad@t", "bad-plugin", true);
        entry.load_error = Some("missing manifest".into());
        let panel = PluginPanel::new(vec![make_entry("good@t", "good-plugin", true), entry]);
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.plugin_panel = Some(panel);

        // Default view (Installed): 2 items
        assert_eq!(app.plugin_panel.as_ref().unwrap().current_list_len(), 2);

        // Switch to Errors view: 1 item
        app.plugin_panel_tab(); // -> Discover
        app.plugin_panel_tab(); // -> Marketplaces
        app.plugin_panel_tab(); // -> Errors
        assert_eq!(app.plugin_panel.as_ref().unwrap().current_list_len(), 1);
    }
}
