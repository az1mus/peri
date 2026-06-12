use crate::error::Result;
use crate::resolver::PackageType;
use std::path::{Path, PathBuf};

/// 工具适配器 trait
pub trait ToolAdapter: Send + Sync {
    /// 工具标识名
    fn target_name(&self) -> &str;

    /// 类型到目标目录的映射
    fn map_dir(&self, typ: PackageType, project_root: &Path) -> PathBuf {
        let subdir = match typ {
            PackageType::Skills => "skills",
            PackageType::Agents => "agents",
            PackageType::Mcp => "mcp",
        };
        let dot_dir = format!(".{}", self.target_name());
        project_root.join(dot_dir).join(subdir)
    }

    /// 安装：从 store 建 symlink 到工具目录
    fn install(&self, store_path: &Path, target_dir: &Path, pkg_name: &str) -> Result<()> {
        std::fs::create_dir_all(target_dir)?;

        let link_path = target_dir.join(pkg_name);

        if link_path.exists() {
            if let Ok(meta) = std::fs::symlink_metadata(&link_path) {
                if meta.file_type().is_symlink() {
                    let existing = std::fs::read_link(&link_path)?;
                    if existing == store_path {
                        return Ok(());
                    }
                }
            }
        }

        if link_path.exists() {
            if link_path.is_symlink() || link_path.is_file() {
                std::fs::remove_file(&link_path)?;
            } else if link_path.is_dir() {
                std::fs::remove_dir_all(&link_path)?;
            }
        }

        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(store_path, &link_path)?;
        }
        #[cfg(windows)]
        {
            if store_path.is_dir() {
                std::os::windows::fs::symlink_dir(store_path, &link_path)?;
            } else {
                std::os::windows::fs::symlink_file(store_path, &link_path)?;
            }
        }

        Ok(())
    }

    /// 安装后的收尾钩子
    fn post_install(&self) -> Result<()> {
        Ok(())
    }

    /// 卸载：删除 symlink
    fn uninstall(&self, target_dir: &Path, pkg_name: &str) -> Result<()> {
        let link_path = target_dir.join(pkg_name);
        if link_path.exists() {
            if link_path.is_symlink() {
                std::fs::remove_file(&link_path)?;
            } else if link_path.is_dir() {
                std::fs::remove_dir_all(&link_path)?;
            }
        }
        Ok(())
    }
}

// ---- 内置适配器 ----

pub struct ClaudeAdapter;
impl ToolAdapter for ClaudeAdapter {
    fn target_name(&self) -> &str {
        "claude"
    }
}

pub struct CodexAdapter;
impl ToolAdapter for CodexAdapter {
    fn target_name(&self) -> &str {
        "codex"
    }
}

pub struct CopilotAdapter;
impl ToolAdapter for CopilotAdapter {
    fn target_name(&self) -> &str {
        "copilot"
    }
}

/// 根据名称获取适配器
pub fn get_adapter(name: &str) -> Option<Box<dyn ToolAdapter>> {
    match name.to_lowercase().as_str() {
        "claude" => Some(Box::new(ClaudeAdapter)),
        "codex" => Some(Box::new(CodexAdapter)),
        "copilot" => Some(Box::new(CopilotAdapter)),
        _ => None,
    }
}

/// 列出所有内置适配器名称
pub fn list_adapters() -> Vec<&'static str> {
    vec!["claude", "codex", "copilot"]
}

/// 适配器名称到 symlink 名称的映射（冲突时添加 scope 前缀）
pub fn symlink_name(pkg_name: &str, existing_names: &[String]) -> String {
    let base = pkg_name.replace('/', "_").replace('@', "");
    if existing_names.contains(&base) {
        let parts: Vec<&str> = pkg_name.split('/').collect();
        if parts.len() >= 2 {
            let prefix = parts[0].trim_start_matches('@');
            format!("{}_{}", prefix, parts.last().unwrap())
        } else {
            base
        }
    } else {
        base
    }
}
