//! 构建 frozen session data，供 session/new、load、resume、fork 复用。
//!
//! 会话内不可变：system prompt + skills + CLAUDE.md 快照。

use super::context::StdioContext;

/// 构建会话级别的冻结数据。
///
/// 捕获当前时间戳作为 frozen_date，从 peri_config 读取语言偏好。
pub(super) fn build(
    ctx: &StdioContext,
    cwd: &str,
) -> peri_acp::session::executor::FrozenSessionData {
    let frozen_date = chrono::Local::now().format("%Y-%m-%d").to_string();
    let frozen_language = ctx.peri_config.read().config.language.clone();
    peri_acp::session::frozen::build_frozen_session_data(
        cwd,
        frozen_language.as_deref(),
        &ctx.plugin_skill_dirs,
        &ctx.plugin_agent_dirs,
        &frozen_date,
    )
}
