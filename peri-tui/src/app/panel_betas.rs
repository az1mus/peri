use super::*;

impl App {
    /// 打开 /betas 面板
    pub fn open_betas_panel(&mut self) {
        let cfg = self
            .services
            .peri_config
            .as_ref()
            .cloned()
            .unwrap_or_default();
        let panel = crate::app::betas_panel::BetasPanel::from_config(&cfg);
        self.open_panel(PanelState::Betas(panel));
    }

    /// 关闭 /betas 面板
    pub fn close_betas_panel(&mut self) {
        self.global_panels.close_if(PanelKind::Betas);
    }
}
