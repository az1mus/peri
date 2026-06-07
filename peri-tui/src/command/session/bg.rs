use crate::{app::App, command::Command, ui::message_view::MessageViewModel};

pub struct BgCommand;

impl Command for BgCommand {
    fn name(&self) -> &str {
        "bg"
    }

    fn description(&self, _lc: &crate::i18n::LcRegistry) -> String {
        _lc.tr("command-bg-description")
    }

    fn aliases(&self) -> Vec<&str> {
        vec!["background"]
    }

    fn execute(&self, app: &mut App, args: &str) {
        let args = args.trim();
        if args.is_empty() {
            let vm = MessageViewModel::system(
                "用法: /bg <命令>\n例如: /bg 用中文搜索 Rust 2026 roadmap 最新进展".to_string(),
            );
            app.session_mgr
                .current_mut()
                .messages
                .view_messages
                .push(vm);
            app.render_rebuild();
            return;
        }
        // Pass through to executor — keep /bg prefix so ACP executor intercepts it
        app.submit_message(format!("/bg {}", args));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    include!("bg_test.rs");
}
