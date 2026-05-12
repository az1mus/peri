use std::any::Any;

use ratatui::layout::Rect;
use ratatui::Frame;
use tui_textarea::Input;

use crate::command::agents::AgentItem;

use super::ensure_cursor_visible;
use super::panel_component::PanelComponent;
use super::panel_manager::{EventResult, PanelContext, PanelKind};
use super::App;

// ─── AgentPanel ────────────────────────────────────────────────────────────────

#[derive(Clone)]
pub struct AgentPanel {
    /// 可用 agent 列表
    pub agents: Vec<AgentItem>,
    /// 当前选中的 agent_id
    pub selected_id: Option<String>,
    /// 光标位置（0 = "无 Agent" 选项，1+ = agents 列表索引-1）
    pub cursor: usize,
    /// 内容滚动偏移
    pub scroll_offset: u16,
}

impl AgentPanel {
    pub fn new(agents: Vec<AgentItem>, current_id: Option<String>) -> Self {
        // 如果已有选中，定位光标到对应的 agents 索引+1（+1 因为第0项是"无 Agent"）
        let cursor = current_id
            .as_ref()
            .and_then(|id| agents.iter().position(|a| &a.id == id))
            .map(|i| i + 1)
            .unwrap_or(0);

        Self {
            agents,
            selected_id: current_id,
            cursor,
            scroll_offset: 0,
        }
    }

    /// 总项数 = "无 Agent" 选项 + agents 列表
    pub fn total(&self) -> usize {
        1 + self.agents.len()
    }

    /// 上下移动光标
    pub fn move_cursor(&mut self, delta: isize) {
        let total = self.total();
        if total == 0 {
            return;
        }
        self.cursor = ((self.cursor as isize + delta).rem_euclid(total as isize)) as usize;
    }

    /// 选择当前光标处的 agent（Enter 确认选择）
    /// 返回 (is_none: bool, agent_id: Option<String>)
    pub fn get_selection(&self) -> (bool, Option<String>) {
        if self.cursor == 0 {
            (true, None)
        } else if let Some(agent) = self.agents.get(self.cursor - 1) {
            (false, Some(agent.id.clone()))
        } else {
            (true, None)
        }
    }

    /// 获取当前光标处的 agent（不包含"无 Agent"选项）
    pub fn current_agent(&self) -> Option<&AgentItem> {
        if self.cursor == 0 {
            None
        } else {
            self.agents.get(self.cursor - 1)
        }
    }
}

// ─── PanelComponent 实现 ──────────────────────────────────────────────────────

impl PanelComponent for AgentPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Agent
    }

    fn handle_key(&mut self, input: Input, ctx: &mut PanelContext<'_>) -> EventResult {
        use tui_textarea::Key;
        match input {
            Input {
                key: Key::Char('c'),
                ctrl: true,
                ..
            } => EventResult::NotConsumed,
            Input { key: Key::Esc, .. } => EventResult::ClosePanel,
            Input { key: Key::Up, .. } => {
                self.move_cursor(-1);
                self.scroll_offset =
                    ensure_cursor_visible(self.cursor as u16, self.scroll_offset, 10);
                EventResult::Consumed
            }
            Input { key: Key::Down, .. } => {
                self.move_cursor(1);
                self.scroll_offset =
                    ensure_cursor_visible(self.cursor as u16, self.scroll_offset, 10);
                EventResult::Consumed
            }
            Input {
                key: Key::Enter, ..
            } => {
                // Enter 确认选择当前 agent（或取消选择）
                let (is_none, agent_id, agent_name) = {
                    let (is_none, agent_id) = self.get_selection();
                    let agent_name = if is_none {
                        None
                    } else {
                        agent_id
                            .as_ref()
                            .and_then(|_id| self.current_agent().map(|a| a.name.clone()))
                    };
                    (is_none, agent_id, agent_name)
                };

                if is_none {
                    ctx.session_mgr.sessions[ctx.session_mgr.active]
                        .agent
                        .agent_id = None;
                    ctx.session_mgr.sessions[ctx.session_mgr.active]
                        .messages
                        .push_system_note(
                            "Agent \u{5df2}\u{91cd}\u{7f6e}\u{ff08}\u{672a}\u{8bbe}\u{7f6e} agent_id\u{ff09}".to_string(),
                        );
                } else if let Some(id) = agent_id {
                    ctx.session_mgr.sessions[ctx.session_mgr.active]
                        .agent
                        .agent_id = Some(id.clone());
                    let name = agent_name.unwrap_or_else(|| id.clone());
                    ctx.session_mgr.sessions[ctx.session_mgr.active]
                        .messages
                        .push_system_note(format!(
                            "Agent \u{5df2}\u{5207}\u{6362}\u{4e3a}: {} ({})",
                            name, id
                        ));
                }
                EventResult::ClosePanel
            }
            _ => EventResult::Consumed,
        }
    }

    fn desired_height(&self, _screen_height: u16, _screen_width: u16) -> u16 {
        (self.agents.len() as u16 * 2 + 6).max(6)
    }

    fn render(&mut self, f: &mut Frame, app: &mut App, area: Rect) {
        crate::ui::main_ui::panels::agent::render_agent_panel(f, self, app, area);
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn status_bar_hints(&self) -> Vec<(&'static str, &'static str)> {
        vec![
            ("\u{2191}\u{2193}", "\u{9009}\u{62e9}"),
            ("Enter", "\u{786e}\u{8ba4}"),
            ("Esc", "\u{53d6}\u{6d88}"),
        ]
    }
}
