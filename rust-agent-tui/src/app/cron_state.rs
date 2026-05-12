use std::any::Any;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use ratatui::layout::Rect;
use ratatui::Frame;
use rust_agent_middlewares::cron::{CronScheduler, CronTask, CronTrigger};
use tokio::sync::mpsc;
use tui_textarea::Input;

use super::panel_component::PanelComponent;
use super::panel_manager::{EventResult, PanelContext, PanelKind};
use super::App;

/// CronPanel 面板状态
#[derive(Debug, Clone)]
pub struct CronPanel {
    pub tasks: Vec<CronTask>,
    pub cursor: usize,
    pub scroll_offset: u16,
    /// 是否处于删除确认状态
    pub confirm_delete: bool,
}

impl CronPanel {
    pub fn new(tasks: Vec<CronTask>) -> Self {
        Self {
            tasks,
            cursor: 0,
            scroll_offset: 0,
            confirm_delete: false,
        }
    }

    pub fn move_cursor(&mut self, delta: i32) {
        if self.tasks.is_empty() {
            return;
        }
        let max = self.tasks.len() - 1;
        let new = self.cursor as i32 + delta;
        self.cursor = new.clamp(0, max as i32) as usize;
    }

    pub fn refresh(&mut self, scheduler: &Mutex<CronScheduler>) {
        self.tasks = scheduler.lock().list_tasks().into_iter().cloned().collect();
        if self.cursor >= self.tasks.len() && !self.tasks.is_empty() {
            self.cursor = self.tasks.len() - 1;
        }
    }
}

impl PanelComponent for CronPanel {
    fn kind(&self) -> PanelKind {
        PanelKind::Cron
    }

    fn handle_key(&mut self, input: Input, ctx: &mut PanelContext<'_>) -> EventResult {
        use tui_textarea::Key;

        // confirm_delete mode
        if self.confirm_delete {
            match input {
                Input {
                    key: Key::Enter, ..
                } => {
                    self.do_confirm_delete(ctx);
                    // if tasks empty after delete, close
                    if self.tasks.is_empty() {
                        EventResult::ClosePanel
                    } else {
                        EventResult::Consumed
                    }
                }
                _ => {
                    self.confirm_delete = false;
                    EventResult::Consumed
                }
            }
        } else {
            match input {
                Input { key: Key::Up, .. } => {
                    self.move_cursor(-1);
                    EventResult::Consumed
                }
                Input { key: Key::Down, .. } => {
                    self.move_cursor(1);
                    EventResult::Consumed
                }
                Input {
                    key: Key::Enter, ..
                }
                | Input {
                    key: Key::Char(' '),
                    ..
                } => {
                    self.do_toggle(ctx);
                    EventResult::Consumed
                }
                Input { key: Key::Esc, .. } => EventResult::ClosePanel,
                Input {
                    key: Key::Char('d'),
                    ctrl: true,
                    ..
                } => {
                    if self.cursor < self.tasks.len() {
                        self.confirm_delete = true;
                    }
                    EventResult::Consumed
                }
                _ => EventResult::Consumed,
            }
        }
    }

    fn desired_height(&self, _screen_height: u16, _screen_width: u16) -> u16 {
        14
    }

    fn render(&mut self, f: &mut Frame, app: &mut App, area: Rect) {
        crate::ui::main_ui::panels::cron::render_cron_panel(f, self, app, area);
    }

    fn as_any_ref(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn status_bar_hints(&self) -> Vec<(&'static str, &'static str)> {
        if self.confirm_delete {
            return vec![
                ("Enter", "\u{786e}\u{8ba4}\u{5220}\u{9664}"),
                ("Esc", "\u{53d6}\u{6d88}"),
            ];
        }
        vec![
            ("\u{2191}\u{2193}", "\u{5bfc}\u{822a}"),
            ("Enter/Space", "\u{5207}\u{6362}"),
            ("Ctrl+D", "\u{5220}\u{9664}"),
            ("Esc", "\u{5173}\u{95ed}"),
        ]
    }
}

impl CronPanel {
    fn do_toggle(&mut self, ctx: &mut PanelContext<'_>) {
        let idx = self.cursor;
        if idx < self.tasks.len() {
            let id = self.tasks[idx].id.clone();
            ctx.services.cron.scheduler.lock().toggle(&id);
            self.refresh(&ctx.services.cron.scheduler);
        }
    }

    fn do_confirm_delete(&mut self, ctx: &mut PanelContext<'_>) {
        self.confirm_delete = false;
        let idx = self.cursor;
        if idx < self.tasks.len() {
            let prompt_preview: String = self.tasks[idx].prompt.chars().take(30).collect();
            let id = self.tasks[idx].id.clone();
            ctx.services.cron.scheduler.lock().remove(&id);
            self.refresh(&ctx.services.cron.scheduler);
            ctx.session_mgr.sessions[ctx.session_mgr.active]
                .messages
                .push_system_note(format!(
                    "\u{5df2}\u{5220}\u{9664}\u{5b9a}\u{65f6}\u{4efb}\u{52a1}: {}",
                    prompt_preview
                ));
        }
    }
}

/// Cron 状态（App 子结构体）
pub struct CronState {
    pub scheduler: Arc<Mutex<CronScheduler>>,
    pub trigger_rx: Option<mpsc::UnboundedReceiver<CronTrigger>>,
}

impl CronState {
    pub fn new() -> (Self, Arc<Mutex<CronScheduler>>) {
        let (trigger_tx, trigger_rx) = mpsc::unbounded_channel();
        let scheduler = CronScheduler::new(trigger_tx);
        let scheduler = Arc::new(Mutex::new(scheduler));

        let state = Self {
            scheduler: scheduler.clone(),
            trigger_rx: Some(trigger_rx),
        };
        (state, scheduler)
    }

    /// Spawn CronManager tick task
    pub fn spawn_tick_task(scheduler: Arc<Mutex<CronScheduler>>) {
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(Duration::from_secs(1));
            loop {
                interval.tick().await;
                scheduler.lock().tick();
            }
        });
    }
}

impl Default for CronState {
    fn default() -> Self {
        let (state, _scheduler) = Self::new();
        state
    }
}
