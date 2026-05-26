use crate::app::App;
use crate::command::Command;
use crate::i18n::LcRegistry;

pub struct CompactCommand;

impl Command for CompactCommand {
    fn name(&self) -> &str {
        "compact"
    }

    fn description(&self, _lc: &LcRegistry) -> String {
        _lc.tr("command-compact-description")
    }

    fn execute(&self, app: &mut App, _args: &str) {
        if app.session_mgr.sessions[app.session_mgr.active].ui.loading {
            app.session_mgr.sessions[app.session_mgr.active]
                .messages
                .view_messages
                .push(crate::app::MessageViewModel::system(
                    app.services.lc.tr("compact-agent-running"),
                ));
            return;
        }
        // 标记为手动 compact，handle_compact_completed 据此调用 set_loading(false)
        app.session_mgr.sessions[app.session_mgr.active]
            .agent
            .compact_manual = true;
        // 通过 ACP compact 通道触发手动压缩，而非将 /compact 当作普通消息发送
        if let Some(ref acp_client) = app.acp_client {
            let client = acp_client.clone();
            tokio::spawn(async move {
                match client.compact().await {
                    Ok(()) => tracing::info!("Manual compact triggered via ACP"),
                    Err(e) => tracing::error!(error = %e, "Manual compact failed"),
                }
            });
        }
    }
}
