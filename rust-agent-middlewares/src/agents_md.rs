use std::collections::HashSet;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use rust_create_agent::agent::state::State;
use rust_create_agent::error::AgentResult;
use rust_create_agent::messages::BaseMessage;
use rust_create_agent::middleware::r#trait::Middleware;

/// AgentsMdMiddleware - 注入项目指引文件（AGENTS.md / CLAUDE.md）
///
/// 在 `before_agent` 时，按优先级搜索指引文件并将内容前插为系统消息。
///
/// 搜索优先级：
/// 1. `{cwd}/AGENTS.md`
/// 2. `{cwd}/CLAUDE.md`
/// 3. `{cwd}/.claude/AGENTS.md`
/// 4. `{home}/.claude/AGENTS.md`（用户全局）
pub struct AgentsMdMiddleware {
    extra_search_paths: Vec<PathBuf>,
    excludes: Vec<String>,
}

impl AgentsMdMiddleware {
    pub fn new() -> Self {
        Self {
            extra_search_paths: Vec::new(),
            excludes: Vec::new(),
        }
    }

    /// 添加额外搜索路径（应用层可注入）
    pub fn with_extra_paths(mut self, paths: Vec<PathBuf>) -> Self {
        self.extra_search_paths = paths;
        self
    }

    /// 设置 CLAUDE.md 排除 glob 模式
    pub fn with_excludes(mut self, patterns: Vec<String>) -> Self {
        self.excludes = patterns;
        self
    }

    /// 根据 cwd 构建候选路径列表（含默认路径 + 额外路径）
    fn candidate_paths(&self, cwd: &str) -> Vec<PathBuf> {
        let cwd = Path::new(cwd);
        let mut candidates = vec![
            cwd.join("AGENTS.md"),
            cwd.join("CLAUDE.md"),
            cwd.join(".claude").join("AGENTS.md"),
        ];

        if let Some(home) = dirs_next::home_dir() {
            candidates.push(home.join(".claude").join("AGENTS.md"));
        }

        candidates.extend(self.extra_search_paths.iter().cloned());

        candidates
    }

    /// 按优先级找到第一个存在的文件（排除匹配 excludes 模式的路径）
    fn find_file(&self, cwd: &str) -> Option<PathBuf> {
        self.candidate_paths(cwd).into_iter().find(|p| {
            if !p.is_file() {
                return false;
            }
            if self.excludes.is_empty() {
                return true;
            }
            let path_str = p.to_string_lossy();
            !self.excludes.iter().any(|pat| {
                glob::Pattern::new(pat)
                    .map(|g| g.matches(&path_str))
                    .unwrap_or(false)
            })
        })
    }
}

/// 递归解析 `<!-- @import path -->` 引用，替换为引用文件内容。
/// `base_dir` 为包含 @import 的文件所在目录。
/// `depth` 递归深度上限 3，`visited` 防循环。
fn resolve_imports(
    content: &str,
    base_dir: &Path,
    depth: u32,
    visited: &mut HashSet<PathBuf>,
) -> String {
    if depth == 0 {
        return content.to_string();
    }
    let mut result = String::with_capacity(content.len());
    let mut pos = 0;
    while pos < content.len() {
        if let Some(offset) = content[pos..].find("<!-- @import ") {
            let abs_pos = pos + offset;
            result.push_str(&content[pos..abs_pos]);
            // 提取 path：从 "<!-- @import " 之后到 " -->"
            let after = &content[abs_pos + 13..]; // 13 = "<!-- @import ".len()
            if let Some(end) = after.find(" -->") {
                let import_path = after[..end].trim();
                let resolved = base_dir
                    .join(import_path)
                    .canonicalize()
                    .unwrap_or_else(|_| base_dir.join(import_path));
                if visited.contains(&resolved) || !resolved.is_file() {
                    // 循环引用或文件不存在，保留原始占位符
                    result.push_str(&content[abs_pos..abs_pos + 13 + end + 4]);
                } else {
                    visited.insert(resolved.clone());
                    let imported_content = std::fs::read_to_string(&resolved).unwrap_or_default();
                    let import_dir = resolved.parent().unwrap_or(base_dir);
                    let resolved_content =
                        resolve_imports(&imported_content, import_dir, depth - 1, visited);
                    result.push_str(&resolved_content);
                }
                pos = abs_pos + 13 + end + 4; // 4 = " -->".len()
            } else {
                // 没找到 " -->"，不是有效的 @import，原样保留
                result.push_str("<!-- @import ");
                pos = abs_pos + 13;
            }
        } else {
            result.push_str(&content[pos..]);
            break;
        }
    }
    result
}

impl Default for AgentsMdMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl<S: State> Middleware<S> for AgentsMdMiddleware {
    fn name(&self) -> &str {
        "AgentsMdMiddleware"
    }

    async fn before_agent(&self, state: &mut S) -> AgentResult<()> {
        let Some(path) = self.find_file(state.cwd()) else {
            // 即使没有主文件，也尝试读取 CLAUDE.local.md
            let local_path = Path::new(state.cwd()).join("CLAUDE.local.md");
            if local_path.is_file() {
                let lp = local_path.clone();
                let local_content =
                    tokio::task::spawn_blocking(move || std::fs::read_to_string(&lp))
                        .await
                        .map_err(|e| rust_create_agent::error::AgentError::MiddlewareError {
                            middleware: "AgentsMdMiddleware".to_string(),
                            reason: format!("spawn_blocking 失败: {e}"),
                        })?
                        .map_err(|e| rust_create_agent::error::AgentError::MiddlewareError {
                            middleware: "AgentsMdMiddleware".to_string(),
                            reason: format!("读取 CLAUDE.local.md 失败: {e}"),
                        })?;
                if !local_content.trim().is_empty() {
                    state.prepend_message(BaseMessage::system(local_content));
                }
            }
            return Ok(());
        };

        let path_display = path.display().to_string();
        let is_claude_md = path
            .file_name()
            .map(|n| n.to_string_lossy().starts_with("CLAUDE"))
            .unwrap_or(false);
        let import_dir = path.parent().map(|p| p.to_path_buf());
        let main_file_canonical = path.canonicalize().ok();
        let content = tokio::task::spawn_blocking(move || std::fs::read_to_string(&path))
            .await
            .map_err(|e| rust_create_agent::error::AgentError::MiddlewareError {
                middleware: "AgentsMdMiddleware".to_string(),
                reason: format!("spawn_blocking 失败: {e}"),
            })?
            .map_err(|e| rust_create_agent::error::AgentError::MiddlewareError {
                middleware: "AgentsMdMiddleware".to_string(),
                reason: format!("读取 {} 失败: {e}", path_display),
            })?;

        let content = if content.trim().is_empty() {
            return Ok(());
        } else {
            content
        };

        // 追加 CLAUDE.local.md（个人项目级，不入库）
        let local_path = Path::new(state.cwd()).join("CLAUDE.local.md");
        let content = if local_path.is_file() {
            let lp = local_path.clone();
            let local_content = tokio::task::spawn_blocking(move || std::fs::read_to_string(&lp))
                .await
                .map_err(|e| rust_create_agent::error::AgentError::MiddlewareError {
                    middleware: "AgentsMdMiddleware".to_string(),
                    reason: format!("spawn_blocking 失败: {e}"),
                })?
                .map_err(|e| rust_create_agent::error::AgentError::MiddlewareError {
                    middleware: "AgentsMdMiddleware".to_string(),
                    reason: format!("读取 CLAUDE.local.md 失败: {e}"),
                })?;
            if local_content.trim().is_empty() {
                content
            } else {
                format!("{content}\n\n{local_content}")
            }
        } else {
            content
        };

        // 仅对 CLAUDE.md 系列文件解析 @import（AGENTS.md 不处理）
        let content = if is_claude_md {
            let dir = import_dir
                .as_deref()
                .unwrap_or_else(|| Path::new(state.cwd()));
            let mut visited = HashSet::new();
            if let Some(canonical) = main_file_canonical {
                visited.insert(canonical);
            }
            resolve_imports(&content, dir, 3, &mut visited)
        } else {
            content
        };

        // 前插系统消息（置于消息历史开头，优先于 Human 消息）
        state.prepend_message(BaseMessage::system(content));

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_create_agent::agent::state::AgentState;

    #[tokio::test]
    async fn test_no_file_no_op() {
        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new("/nonexistent/path");
        let result = mw.before_agent(&mut state).await;
        assert!(result.is_ok());
        assert_eq!(state.messages().len(), 0);
    }

    #[tokio::test]
    async fn test_with_file() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let agents_md = dir.path().join("AGENTS.md");
        std::fs::write(&agents_md, "# Project Guide\nDo things correctly.").unwrap();

        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        assert_eq!(state.messages().len(), 1);
        assert!(state.messages()[0].is_system());
        assert!(state.messages()[0].content().contains("Project Guide"));
    }

    #[tokio::test]
    async fn test_priority_agents_over_claude() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "agents content").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "claude content").unwrap();

        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        assert_eq!(state.messages().len(), 1);
        assert!(state.messages()[0].content().contains("agents content"));
    }

    #[tokio::test]
    async fn test_prepends_before_existing_messages() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("AGENTS.md"), "system instructions").unwrap();

        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        state.add_message(BaseMessage::human("user question"));
        mw.before_agent(&mut state).await.unwrap();

        // 系统消息应在 human 消息之前
        assert_eq!(state.messages().len(), 2);
        assert!(state.messages()[0].is_system());
        assert!(matches!(state.messages()[1], BaseMessage::Human { .. }));
    }

    #[tokio::test]
    async fn test_excludes_matching_file_skipped() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let claude_md = dir.path().join("CLAUDE.md");
        std::fs::write(&claude_md, "should be excluded").unwrap();

        let mw = AgentsMdMiddleware::new().with_excludes(vec![format!("{}", claude_md.display())]);
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        assert_eq!(
            state.messages().len(),
            0,
            "excluded file should not be loaded"
        );
    }

    #[tokio::test]
    async fn test_excludes_non_matching_file_loaded() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "should be loaded").unwrap();

        let mw = AgentsMdMiddleware::new().with_excludes(vec!["**/node_modules/**".to_string()]);
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        assert_eq!(state.messages().len(), 1);
        assert!(state.messages()[0].content().contains("should be loaded"));
    }

    // ── CLAUDE.local.md tests ──────────────────────────────────────────────

    #[tokio::test]
    async fn test_local_md_only() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.local.md"), "local only content").unwrap();

        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        assert_eq!(state.messages().len(), 1);
        assert!(state.messages()[0].content().contains("local only content"));
    }

    #[tokio::test]
    async fn test_claude_md_and_local_merged() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "main content").unwrap();
        std::fs::write(dir.path().join("CLAUDE.local.md"), "local content").unwrap();

        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        assert_eq!(state.messages().len(), 1);
        let content = state.messages()[0].content();
        assert!(content.contains("main content"));
        assert!(content.contains("local content"));
    }

    #[tokio::test]
    async fn test_local_md_empty_not_appended() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "main content").unwrap();
        std::fs::write(dir.path().join("CLAUDE.local.md"), "   \n  ").unwrap();

        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        assert_eq!(state.messages().len(), 1);
        let content = state.messages()[0].content();
        assert!(content.contains("main content"));
        assert!(!content.contains("local"));
    }

    // ── @import tests ──────────────────────────────────────────────────────

    #[tokio::test]
    async fn test_import_simple() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let imported = dir.path().join("rules.md");
        std::fs::write(&imported, "imported rules").unwrap();
        std::fs::write(
            dir.path().join("CLAUDE.md"),
            "header\n<!-- @import rules.md -->\nfooter",
        )
        .unwrap();

        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        let content = state.messages()[0].content();
        assert!(content.contains("header"));
        assert!(content.contains("imported rules"));
        assert!(content.contains("footer"));
        assert!(!content.contains("@import"));
    }

    #[tokio::test]
    async fn test_import_nested() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let sub_dir = dir.path().join("sub");
        std::fs::create_dir_all(&sub_dir).unwrap();
        let inner = sub_dir.join("inner.md");
        std::fs::write(&inner, "inner content").unwrap();
        let outer = dir.path().join("outer.md");
        std::fs::write(&outer, "outer <!-- @import sub/inner.md --> end").unwrap();
        std::fs::write(dir.path().join("CLAUDE.md"), "<!-- @import outer.md -->").unwrap();

        let mw = AgentsMdMiddleware::new();
        let mut state = AgentState::new(dir.path().to_str().unwrap());
        mw.before_agent(&mut state).await.unwrap();

        let content = state.messages()[0].content();
        assert!(content.contains("inner content"));
    }

    #[test]
    fn test_import_max_depth() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let imported = dir.path().join("deep.md");
        std::fs::write(&imported, "deep content").unwrap();
        let content = "<!-- @import deep.md -->".to_string();
        let mut visited = HashSet::new();
        // depth 0 should return original content
        let result = resolve_imports(&content, dir.path(), 0, &mut visited);
        assert!(result.contains("@import"));
    }

    #[test]
    fn test_import_cycle_detection() {
        use tempfile::tempdir;
        let dir = tempdir().unwrap();
        let a = dir.path().join("a.md");
        let b = dir.path().join("b.md");
        std::fs::write(&a, "<!-- @import b.md -->").unwrap();
        std::fs::write(&b, "<!-- @import a.md -->").unwrap();

        let main = dir.path().join("main.md");
        std::fs::write(&main, "<!-- @import a.md -->").unwrap();

        let mut visited = HashSet::new();
        visited.insert(main.clone());
        // Should not panic or infinite loop
        let result = resolve_imports(
            &std::fs::read_to_string(&main).unwrap(),
            dir.path(),
            3,
            &mut visited,
        );
        // a.md's @import b.md should be resolved, but b.md's @import a.md should be kept as-is (cycle)
        assert!(!result.is_empty());
    }

    #[test]
    fn test_import_nonexistent_file() {
        let content = "<!-- @import nonexistent.md -->";
        let mut visited = HashSet::new();
        let result = resolve_imports(content, Path::new("/tmp"), 3, &mut visited);
        assert!(
            result.contains("@import"),
            "nonexistent file should keep original placeholder"
        );
    }

    #[test]
    fn test_import_invalid_format() {
        let content = "<!-- @import no closing tag";
        let mut visited = HashSet::new();
        let result = resolve_imports(content, Path::new("/tmp"), 3, &mut visited);
        assert!(
            result.contains("@import"),
            "invalid format should preserve original text"
        );
    }
}
