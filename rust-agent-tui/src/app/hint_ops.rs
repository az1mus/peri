use super::*;

/// 统一候选项：命令或 Skill，与渲染侧 hints.rs 保持一致
enum HintItem<'a> {
    Cmd { name: &'a str },
    Skill { name: &'a str },
}

impl<'a> HintItem<'a> {
    fn name(&self) -> &'a str {
        match self {
            HintItem::Cmd { name } => name,
            HintItem::Skill { name } => name,
        }
    }

    /// 命令优先于 Skill
    fn is_cmd(&self) -> bool {
        matches!(self, HintItem::Cmd { .. })
    }
}

impl App {
    /// 构建统一排序后的候选项列表（与渲染侧一致）
    fn build_hint_items(&self) -> Vec<HintItem<'_>> {
        let first_line = self.session_mgr.sessions[self.session_mgr.active]
            .ui
            .textarea
            .lines()
            .first()
            .map(|s| s.as_str())
            .unwrap_or("");
        if !first_line.starts_with('/') {
            return vec![];
        }
        let prefix = first_line.trim_start_matches('/');
        let cmd_candidates: Vec<_> = self.session_mgr.sessions[self.session_mgr.active]
            .commands
            .command_registry
            .match_prefix(prefix);
        let skill_candidates: Vec<_> = self.session_mgr.sessions[self.session_mgr.active]
            .commands
            .skills
            .iter()
            .filter(|s| prefix.is_empty() || s.name.contains(prefix))
            .collect();

        let mut items: Vec<HintItem<'_>> = Vec::new();
        for (name, _) in &cmd_candidates {
            items.push(HintItem::Cmd { name });
        }
        for skill in &skill_candidates {
            items.push(HintItem::Skill { name: &skill.name });
        }
        items.sort_by(|a, b| {
            let a_starts = a.name().starts_with(prefix) as u8;
            let b_starts = b.name().starts_with(prefix) as u8;
            // 前缀匹配优先 > 命令优先于 Skill > 字母序
            b_starts
                .cmp(&a_starts)
                .then_with(|| b.is_cmd().cmp(&a.is_cmd()))
                .then_with(|| a.name().cmp(b.name()))
        });
        items
    }

    /// 获取当前提示浮层的候选数量
    pub fn hint_candidates_count(&self) -> usize {
        self.build_hint_items().len()
    }

    /// Tab 补全：选中当前光标处的候选项，替换输入框内容
    pub fn hint_complete(&mut self) {
        let selected_name = {
            let items = self.build_hint_items();
            let cursor = self.session_mgr.sessions[self.session_mgr.active]
                .ui
                .hint_cursor
                .unwrap_or(0);
            items.get(cursor).map(|item| item.name().to_string())
        };

        if let Some(name) = selected_name {
            self.session_mgr.sessions[self.session_mgr.active]
                .ui
                .textarea = build_textarea(false);
            self.session_mgr.sessions[self.session_mgr.active]
                .ui
                .textarea
                .insert_str(format!("/{} ", name));
            self.session_mgr.sessions[self.session_mgr.active]
                .ui
                .hint_cursor = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_agent_middlewares::skills::loader::SkillMetadata;

    fn make_skill(name: &str) -> SkillMetadata {
        SkillMetadata {
            name: name.to_string(),
            description: format!("{} skill", name),
            path: std::path::PathBuf::from(format!("/tmp/{}.md", name)),
        }
    }

    #[tokio::test]
    async fn test_candidates_count_slash_prefix_returns_cmd_plus_skills() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.sessions[app.session_mgr.active].ui.textarea = build_textarea(false);
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .textarea
            .insert_str("/");
        app.session_mgr.sessions[app.session_mgr.active]
            .commands
            .skills
            .push(make_skill("aaa-skill"));
        app.session_mgr.sessions[app.session_mgr.active]
            .commands
            .skills
            .push(make_skill("zzz-skill"));

        let count = app.hint_candidates_count();
        // 命令数 + 2 技能，但最多 8 项
        let cmd_count = app.session_mgr.sessions[app.session_mgr.active]
            .commands
            .command_registry
            .match_prefix("")
            .len();
        let expected = cmd_count + 2;
        assert_eq!(count, expected, "/ 前缀应返回命令数 + Skills 数");
    }

    #[tokio::test]
    async fn test_candidates_count_slash_prefix_filters_both() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.sessions[app.session_mgr.active].ui.textarea = build_textarea(false);
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .textarea
            .insert_str("/mo");
        app.session_mgr.sessions[app.session_mgr.active]
            .commands
            .skills
            .push(make_skill("commit"));
        app.session_mgr.sessions[app.session_mgr.active]
            .commands
            .skills
            .push(make_skill("model-skill"));

        let count = app.hint_candidates_count();
        assert!(
            count >= 2,
            "/mo 前缀应至少返回 model 命令 + model-skill 技能"
        );
    }

    #[tokio::test]
    async fn test_candidates_count_no_prefix_returns_zero() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.sessions[app.session_mgr.active].ui.textarea = build_textarea(false);
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .textarea
            .insert_str("hello");

        let count = app.hint_candidates_count();
        assert_eq!(count, 0, "无前缀应返回 0");
    }

    #[tokio::test]
    async fn test_hint_complete_command_at_cursor_0() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.sessions[app.session_mgr.active].ui.textarea = build_textarea(false);
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .textarea
            .insert_str("/m");
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .hint_cursor = Some(0);

        app.hint_complete();
        let text: String = app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .textarea
            .lines()
            .iter()
            .map(|s| s.as_str())
            .collect();
        // 字母排序后第一个匹配 /m 的项
        let _items = app.build_hint_items();
        // hint_complete 已经清空了 hint_cursor 并修改了 textarea，这里直接验证
        assert!(text.starts_with("/"), "补全后应以 / 开头，实际: {}", text);
        assert!(
            app.session_mgr.sessions[app.session_mgr.active]
                .ui
                .hint_cursor
                .is_none(),
            "补全后 hint_cursor 应为 None"
        );
    }

    #[tokio::test]
    async fn test_hint_complete_clears_hint_cursor() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.sessions[app.session_mgr.active].ui.textarea = build_textarea(false);
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .textarea
            .insert_str("/m");
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .hint_cursor = Some(0);

        app.hint_complete();
        assert_eq!(
            app.session_mgr.sessions[app.session_mgr.active]
                .ui
                .hint_cursor,
            None,
            "补全后 hint_cursor 应为 None"
        );
    }

    #[tokio::test]
    async fn test_hint_complete_skill_item() {
        let (mut app, _handle) = crate::app::App::new_headless(80, 24).await;
        app.session_mgr.sessions[app.session_mgr.active].ui.textarea = build_textarea(false);
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .textarea
            .insert_str("/aaa");
        app.session_mgr.sessions[app.session_mgr.active]
            .commands
            .skills
            .push(make_skill("aaa-skill"));

        // 找到 aaa-skill 在排序后的索引
        let items = app.build_hint_items();
        let idx = items
            .iter()
            .position(|it| it.name() == "aaa-skill")
            .expect("应有 aaa-skill 候选");
        app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .hint_cursor = Some(idx);

        app.hint_complete();
        let text: String = app.session_mgr.sessions[app.session_mgr.active]
            .ui
            .textarea
            .lines()
            .iter()
            .map(|s| s.as_str())
            .collect();
        assert!(
            text.starts_with("/aaa-skill "),
            "应补全 Skill aaa-skill，实际: {}",
            text
        );
    }
}
