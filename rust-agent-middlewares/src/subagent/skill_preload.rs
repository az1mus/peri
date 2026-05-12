use std::path::PathBuf;

use async_trait::async_trait;
use rust_create_agent::agent::state::State;
use rust_create_agent::error::AgentResult;
use rust_create_agent::messages::{BaseMessage, ContentBlock};
use rust_create_agent::middleware::r#trait::Middleware;

use crate::skills::{list_skills, load_global_skills_dir};

/// SkillPreloadMiddleware - 将指定 skill 全文以 fake Read 工具调用注入到 agent state
///
/// 在 `before_agent` 时，根据 `skill_names` 列表找到对应 SKILL.md 文件，
/// 将其内容以 Ai[ToolUse] → Tool[ToolResult] 消息序列追加到用户消息之后（executor
/// 在 `before_agent` 之前已将用户消息 `add_message` 到 state），使 LLM 从第一轮推理
/// 就能看到完整 skill 内容。
///
/// 使用 `add_message` 而非 `prepend_message`，确保工具调用出现在用户消息之后，
/// 不影响 Anthropic messages 数组的 prompt cache（cache_control 在第一条 user 消息上）。
///
/// # 注入消息结构
///
/// ```text
/// [Human "用户消息"]  ← 已由 executor 添加
/// [Ai]    [ToolUse{Read, call_{hex}}, ToolUse{Read, call_{hex}}, ...]
/// [Tool]  ToolResult{call_{hex}, skill_0_content}
/// [Tool]  ToolResult{call_{hex}, skill_1_content}
/// ...
/// ```
///
/// 找不到的 skill 名称静默跳过，不报错。
pub struct SkillPreloadMiddleware {
    skill_names: Vec<String>,
    cwd: String,
}

impl SkillPreloadMiddleware {
    pub fn new(skill_names: Vec<String>, cwd: &str) -> Self {
        Self {
            skill_names,
            cwd: cwd.to_string(),
        }
    }

    /// 解析 skills 搜索目录：`~/.claude/skills/` → globalConfig → `{cwd}/.claude/skills/`
    fn resolve_dirs(&self) -> Vec<PathBuf> {
        let user_dir = dirs_next::home_dir()
            .map(|h| h.join(".claude").join("skills"))
            .unwrap_or_default();

        let global_dir = load_global_skills_dir();

        let project_dir = PathBuf::from(&self.cwd).join(".claude").join("skills");

        let mut dirs = vec![user_dir];
        if let Some(g) = global_dir {
            dirs.push(g);
        }
        dirs.push(project_dir);
        dirs
    }
}

#[async_trait]
impl<S: State> Middleware<S> for SkillPreloadMiddleware {
    fn name(&self) -> &str {
        "SkillPreloadMiddleware"
    }

    async fn before_agent(&self, state: &mut S) -> AgentResult<()> {
        if self.skill_names.is_empty() {
            return Ok(());
        }

        let dirs = self.resolve_dirs();
        let names_lower: Vec<String> = self.skill_names.iter().map(|s| s.to_lowercase()).collect();

        // 在 blocking 线程中扫描目录 + 读取文件内容
        let skill_contents = tokio::task::spawn_blocking(move || {
            let all_skills = list_skills(&dirs);
            all_skills
                .into_iter()
                .filter(|s| names_lower.contains(&s.name.to_lowercase()))
                .filter_map(|s| {
                    let content = std::fs::read_to_string(&s.path).ok()?;
                    Some((s.path.to_string_lossy().to_string(), content))
                })
                .collect::<Vec<_>>()
        })
        .await
        .map_err(|e| rust_create_agent::error::AgentError::MiddlewareError {
            middleware: "SkillPreloadMiddleware".to_string(),
            reason: format!("spawn_blocking 失败: {e}"),
        })?;

        if skill_contents.is_empty() {
            return Ok(());
        }

        // Generate tool_call_ids: call_{uuid hex without hyphens, 32 chars}
        let call_ids: Vec<String> = (0..skill_contents.len())
            .map(|_| format!("call_{}", uuid::Uuid::new_v4().simple()))
            .collect();

        // 构造 Ai 消息的 ToolUse ContentBlock 列表
        let tool_use_blocks: Vec<ContentBlock> = skill_contents
            .iter()
            .zip(call_ids.iter())
            .map(|((path, _), id)| {
                ContentBlock::tool_use(id.clone(), "Read", serde_json::json!({ "path": path }))
            })
            .collect();

        // 追加 Ai 消息（ai_from_blocks 自动双写 tool_calls）
        state.add_message(BaseMessage::ai_from_blocks(tool_use_blocks));

        // 追加 Tool 结果消息
        for (id, (_, content)) in call_ids.iter().zip(skill_contents.iter()) {
            state.add_message(BaseMessage::tool_result(id.clone(), content.clone()));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_create_agent::agent::state::AgentState;
    use rust_create_agent::middleware::r#trait::Middleware;
    use tempfile::tempdir;

    fn write_skill(dir: &std::path::Path, name: &str, desc: &str) {
        let skill_dir = dir.join(name);
        std::fs::create_dir_all(&skill_dir).unwrap();
        let content = format!(
            "---\nname: '{}'\ndescription: '{}'\n---\n\n# {}\n\nSkill content for {}.\n",
            name, desc, name, name
        );
        std::fs::write(skill_dir.join("SKILL.md"), content).unwrap();
    }

    #[tokio::test]
    async fn test_no_op_when_empty_names() {
        // Arrange
        let dir = tempdir().unwrap();
        let mw = SkillPreloadMiddleware::new(vec![], dir.path().to_str().unwrap());
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        assert_eq!(state.messages().len(), 0);
    }

    #[tokio::test]
    async fn test_inject_single_skill() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "api-guide", "API 开发指南");

        let mw = SkillPreloadMiddleware::new(
            vec!["api-guide".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert: Ai + Tool = 2 条消息
        assert_eq!(state.messages().len(), 2, "应注入 2 条消息（Ai + Tool）");
        assert!(
            matches!(&state.messages()[0], BaseMessage::Ai { .. }),
            "第一条应为 Ai"
        );
        assert!(
            matches!(&state.messages()[1], BaseMessage::Tool { .. }),
            "第二条应为 Tool"
        );
    }

    #[tokio::test]
    async fn test_inject_multiple_skills() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "skill-a", "技能 A");
        write_skill(&skills_dir, "skill-b", "技能 B");
        write_skill(&skills_dir, "skill-c", "技能 C");

        let mw = SkillPreloadMiddleware::new(
            vec![
                "skill-a".to_string(),
                "skill-b".to_string(),
                "skill-c".to_string(),
            ],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert: Ai + Tool × 3 = 4 条消息
        assert_eq!(state.messages().len(), 4, "3 个 skill 应注入 4 条消息");
    }

    #[tokio::test]
    async fn test_skip_missing_skill() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "exists", "存在的 skill");

        let mw = SkillPreloadMiddleware::new(
            vec!["exists".to_string(), "nonexistent".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert: 只有 "exists" → Ai + Tool = 2 条
        assert_eq!(state.messages().len(), 2, "不存在的 skill 应静默跳过");
    }

    #[tokio::test]
    async fn test_no_op_when_all_skills_missing() {
        // Arrange
        let dir = tempdir().unwrap();
        let mw = SkillPreloadMiddleware::new(
            vec!["nonexistent".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        assert_eq!(state.messages().len(), 0, "全部找不到时应 no-op");
    }

    #[tokio::test]
    async fn test_message_order() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "skill-x", "技能 X");
        write_skill(&skills_dir, "skill-y", "技能 Y");

        let mw = SkillPreloadMiddleware::new(
            vec!["skill-x".to_string(), "skill-y".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        let msgs = state.messages();
        assert!(
            matches!(&msgs[0], BaseMessage::Ai { .. }),
            "messages[0] 应为 Ai"
        );
        assert!(msgs[0].has_tool_calls(), "Ai 消息应包含工具调用");
        assert_eq!(msgs[0].tool_calls().len(), 2, "Ai 消息应有 2 个工具调用");
        assert!(
            matches!(&msgs[1], BaseMessage::Tool { .. }),
            "messages[1] 应为 Tool"
        );
        assert!(
            matches!(&msgs[2], BaseMessage::Tool { .. }),
            "messages[2] 应为 Tool"
        );
    }

    #[tokio::test]
    async fn test_tool_call_ids_match() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "my-skill", "My skill");

        let mw =
            SkillPreloadMiddleware::new(vec!["my-skill".to_string()], dir.path().to_str().unwrap());
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        let msgs = state.messages();
        let ai_id = &msgs[0].tool_calls()[0].id;
        if let BaseMessage::Tool { tool_call_id, .. } = &msgs[1] {
            assert_eq!(
                tool_call_id, ai_id,
                "Tool 消息的 tool_call_id 应与 Ai 消息一致"
            );
        } else {
            unreachable!("messages[1] 应为 Tool");
        }
    }

    #[tokio::test]
    async fn test_tool_result_contains_skill_content() {
        // Arrange
        let dir = tempdir().unwrap();
        let skills_dir = dir.path().join(".claude").join("skills");
        std::fs::create_dir_all(&skills_dir).unwrap();
        write_skill(&skills_dir, "commit-skill", "提交技能");

        let mw = SkillPreloadMiddleware::new(
            vec!["commit-skill".to_string()],
            dir.path().to_str().unwrap(),
        );
        let mut state = AgentState::new(dir.path().to_str().unwrap());

        // Act
        mw.before_agent(&mut state).await.unwrap();

        // Assert
        let tool_content = state.messages()[1].content();
        assert!(
            tool_content.contains("Skill content for commit-skill"),
            "Tool 结果应包含 skill 全文内容"
        );
    }
}
