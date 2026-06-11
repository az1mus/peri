//! AvailableCommands 通知辅助，供 session/new 和 session/load 复用。

use agent_client_protocol::{
    schema::{AvailableCommandsUpdate, SessionId, SessionNotification, SessionUpdate},
    Client, ConnectionTo,
};

/// 扫描 skill 目录并发送 AvailableCommandsUpdate 通知。
pub(super) fn send_available_commands(
    cwd: &str,
    plugin_skill_dirs: &[std::path::PathBuf],
    session_id: &SessionId,
    cx: &ConnectionTo<Client>,
) {
    let skill_dirs =
        peri_middlewares::SkillsMiddleware::resolve_dirs_static(cwd, plugin_skill_dirs);
    let skills = peri_middlewares::skills::list_skills(&skill_dirs);
    let cmds = peri_acp::dispatch::build_available_commands(&skills);
    let notif = SessionNotification::new(
        session_id.clone(),
        SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(cmds)),
    );
    let _ = cx.send_notification(notif);
}
