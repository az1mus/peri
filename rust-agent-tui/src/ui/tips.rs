/// Random tips shown below the loading spinner, inspired by Claude Code.
const TIPS: &[&str] = &[
    "按 / 输入命令，Tab 补全",
    "Ctrl+C 中断 Agent，Shift+Tab 切换权限模式",
    "Alt+M 快速切换模型（opus / sonnet / haiku）",
    "Alt+Enter 在输入框中换行",
    "拖拽文件或图片到终端可自动附加到消息",
    "长按 Ctrl+V 粘贴剪贴板图片",
    "Ctrl+U/D 滚动消息历史，↑/↓ 浏览输入历史",
    "Ctrl+N/P 切换 Session，Ctrl+W 关闭",
    "Esc 关闭弹窗或面板，Enter 确认选择",
    "/compact 压缩上下文节省 token",
    "/clear 清空当前对话",
    "/model 切换 LLM 模型",
    "/history 浏览历史对话记录",
    "/loop 创建定时循环任务",
    "/plugin 管理 Claude Code 插件",
    "在 .claude/skills/ 中添加自定义 Skills",
    "在 .claude/agents/ 中定义 SubAgent",
    "对复杂任务让 Agent 先制定计划再执行",
];

/// Pick a tip based on a tick counter. Tip changes every ~180 ticks (roughly every 3 seconds at 60fps).
pub fn pick_tip(tick: u64) -> &'static str {
    TIPS[((tick / 180) as usize) % TIPS.len()]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tips_contains_slash_command_hint() {
        let has_slash = TIPS.iter().any(|t| t.contains("/ 输入命令"));
        assert!(has_slash, "tips 应包含 '/ 输入命令' 提示");
    }

    #[test]
    fn test_tips_tab_hint() {
        let has_tab = TIPS.iter().any(|t| t.contains("Tab 补全"));
        assert!(has_tab, "tips 应包含 'Tab 补全'");
    }

    #[test]
    fn test_tips_only_reference_existing_commands() {
        // tips 中引用的 /xxx 命令必须存在于 command registry
        let existing_commands = [
            "login", "model", "history", "clear", "help", "compact", "cron", "loop", "plugin",
        ];
        for tip in TIPS {
            // 提取 tip 中的 /xxx 命令引用
            for word in tip.split_whitespace() {
                if word.starts_with('/')
                    && word.len() > 1
                    && word.chars().nth(1).is_some_and(|c| c.is_alphabetic())
                {
                    let cmd_name: String = word[1..]
                        .chars()
                        .take_while(|c| c.is_alphanumeric() || *c == '_' || *c == '-')
                        .collect();
                    if !cmd_name.is_empty() {
                        assert!(
                            existing_commands.contains(&cmd_name.as_str()),
                            "tip 引用了不存在的命令 /{}: {}",
                            cmd_name,
                            tip
                        );
                    }
                }
            }
        }
    }
}
