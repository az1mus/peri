use std::path::{Path, PathBuf};

use gray_matter::{engine::YAML, Matter};
use serde::Deserialize;

/// Skill 元数据（来自 SKILL.md frontmatter）
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}

/// frontmatter 反序列化结构
#[derive(Debug, Deserialize)]
struct SkillFrontmatter {
    name: String,
    description: String,
}

/// 加载单个 SKILL.md，解析 frontmatter，返回元数据
pub fn load_skill_metadata(path: &Path) -> Option<SkillMetadata> {
    let content = std::fs::read_to_string(path).ok()?;
    let matter = Matter::<YAML>::new();
    let result: gray_matter::ParsedEntity = matter.parse(&content).ok()?;

    let data = result.data?;
    let fm: SkillFrontmatter = data.deserialize().ok()?;

    Some(SkillMetadata {
        name: fm.name,
        description: fm.description,
        path: path.to_path_buf(),
    })
}

/// 扫描多个目录，返回所有可用 skill 元数据
///
/// 同名 skill 以先出现的为准（dirs 中靠前的目录优先）。
pub fn list_skills(dirs: &[PathBuf]) -> Vec<SkillMetadata> {
    let mut seen_names = std::collections::HashSet::new();
    let mut skills = Vec::new();

    for dir in dirs {
        if !dir.is_dir() {
            continue;
        }

        // 遍历直接子目录，每个子目录中寻找 SKILL.md
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => continue,
        };

        let mut dir_skills: Vec<SkillMetadata> = entries
            .filter_map(|entry| {
                let entry = entry.ok()?;
                let path = entry.path();
                if path.is_dir() {
                    let skill_file = path.join("SKILL.md");
                    if skill_file.is_file() {
                        return load_skill_metadata(&skill_file);
                    }
                } else if path.is_file()
                    && path.file_name().map(|n| n == "SKILL.md").unwrap_or(false)
                {
                    return load_skill_metadata(&path);
                }
                None
            })
            .collect();

        // 按名称排序保持稳定顺序
        dir_skills.sort_by(|a, b| a.name.cmp(&b.name));

        for skill in dir_skills {
            if seen_names.insert(skill.name.clone()) {
                skills.push(skill);
            }
        }
    }

    skills
}

/// 统一的 skill 搜索目录解析
///
/// 优先级：~/.claude/skills → globalConfig skillsDir → ./.claude/skills → extra_dirs
/// 这是 skill 目录解析的 single source of truth，SkillsMiddleware 和 SkillPreloadMiddleware 都应委托此函数。
pub fn resolve_skill_dirs(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let user_dir = dirs_next::home_dir()
        .map(|h| h.join(".claude").join("skills"))
        .unwrap_or_default();

    let global_dir = super::load_global_skills_dir();

    let project_dir = PathBuf::from(cwd).join(".claude").join("skills");

    let mut dirs = vec![user_dir];
    if let Some(global) = global_dir {
        dirs.push(global);
    }
    dirs.push(project_dir);
    for dir in extra_dirs {
        if dir.is_dir() {
            dirs.push(dir.clone());
        }
    }
    dirs
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;
    include!("loader_test.rs");
}
