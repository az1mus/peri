use crate::error::{AgmError, Result};
use std::path::Path;
use std::process::Command;

/// git clone 指定仓库到目标目录，checkout 到指定 commit
pub fn clone_at_commit(repo_url: &str, commit: &str, dest: &Path) -> Result<()> {
    let output = Command::new("git")
        .args(["clone", "--no-checkout", repo_url])
        .arg(dest.as_os_str())
        .output()
        .map_err(|e| AgmError::Git(format!("git clone failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AgmError::Git(format!("git clone failed: {}", stderr)));
    }

    let output = Command::new("git")
        .args(["checkout", commit])
        .current_dir(dest)
        .output()
        .map_err(|e| AgmError::Git(format!("git checkout failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AgmError::Git(format!("git checkout failed: {}", stderr)));
    }

    Ok(())
}

/// 验证 commit hash 是否合法（40 字符 hex）
pub fn is_valid_commit_hash(hash: &str) -> bool {
    hash.len() == 40 && hash.chars().all(|c| c.is_ascii_hexdigit())
}

/// 获取仓库的所有 tags
pub fn list_tags(repo_dir: &Path) -> Result<Vec<String>> {
    let output = Command::new("git")
        .args(["tag", "--list"])
        .current_dir(repo_dir)
        .output()
        .map_err(|e| AgmError::Git(format!("git tag failed: {}", e)))?;

    if !output.status.success() {
        return Ok(Vec::new());
    }

    Ok(String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .collect())
}

/// clone 仓库到目标目录（浅层，仅 HEAD），返回 HEAD commit hash
pub fn clone_head(repo_url: &str, dest: &Path) -> Result<String> {
    let output = Command::new("git")
        .args(["clone", "--depth", "1", "--single-branch", repo_url])
        .arg(dest.as_os_str())
        .output()
        .map_err(|e| AgmError::Git(format!("git clone failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AgmError::Git(format!("git clone failed: {}", stderr)));
    }

    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(dest)
        .output()
        .map_err(|e| AgmError::Git(format!("git rev-parse failed: {}", e)))?;

    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

/// 解析仓库 HEAD commit hash（不 clone，用 git ls-remote）
pub fn resolve_head(repo_url: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["ls-remote", "--symref", repo_url, "HEAD"])
        .output()
        .map_err(|e| AgmError::Git(format!("git ls-remote failed: {}", e)))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(AgmError::Git(format!("git ls-remote failed: {}", stderr)));
    }

    String::from_utf8_lossy(&output.stdout)
        .lines()
        .find_map(|line| {
            if line.contains("HEAD") && !line.contains("ref:") {
                line.split_whitespace().next().map(|s| s.to_string())
            } else {
                None
            }
        })
        .ok_or_else(|| AgmError::Git("failed to parse HEAD from ls-remote output".into()))
}

/// 从 GitHub URL 解析 owner/repo
pub fn parse_github_url(url: &str) -> Option<(String, String)> {
    let url = url.trim_end_matches('/').trim_end_matches(".git");
    // https://github.com/owner/repo
    if let Some(rest) = url.strip_prefix("https://github.com/") {
        let parts: Vec<&str> = rest.split('/').collect();
        if parts.len() >= 2 {
            return Some((parts[0].to_string(), parts[1].to_string()));
        }
    }
    None
}
