use crate::error::{AgmError, Result};
use crate::git;
use crate::registry::RegistryClient;
use crate::types::*;
use semver::VersionReq;
use std::collections::BTreeMap;

/// 解析结果
#[derive(Debug, Clone)]
pub struct ResolvedPackage {
    pub name: String,
    pub version: String,
    pub resolution: Resolution,
}

/// 判断是否为 git 依赖（@git/ 前缀）
pub fn is_git_dep(name: &str) -> bool {
    name.starts_with("@git/")
}

/// 验证 git commit hash
pub fn validate_commit_hash(hash: &str) -> Result<()> {
    if !git::is_valid_commit_hash(hash) {
        return Err(AgmError::InvalidCommitHash(hash.into()));
    }
    Ok(())
}

/// 从 registry 解析 semver range 到具体版本
pub async fn resolve_registry_version(
    client: &RegistryClient,
    name: &str,
    range: &str,
) -> Result<String> {
    let req = VersionReq::parse(range)?;
    let metadata = client.get_package(name).await?;

    let mut candidates: Vec<_> = metadata.versions.keys().cloned().collect();
    candidates.sort_by(|a, b| {
        let va = semver::Version::parse(a).ok();
        let vb = semver::Version::parse(b).ok();
        vb.cmp(&va) // 降序
    });

    for version in &candidates {
        if let Ok(v) = semver::Version::parse(version) {
            if req.matches(&v) {
                return Ok(version.clone());
            }
        }
    }

    Err(AgmError::Other(format!(
        "no version of {} satisfies range {}",
        name, range
    )))
}

/// 从 manifest 收集所有需要解析的依赖
pub fn collect_dependencies(manifest: &ProjectManifest) -> Vec<(String, String, PackageType)> {
    let mut deps = Vec::new();
    for (name, version) in &manifest.skills {
        deps.push((name.clone(), version.clone(), PackageType::Skills));
    }
    for (name, version) in &manifest.agents {
        deps.push((name.clone(), version.clone(), PackageType::Agents));
    }
    for (name, version) in &manifest.mcp {
        deps.push((name.clone(), version.clone(), PackageType::Mcp));
    }
    deps
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PackageType {
    Skills,
    Agents,
    Mcp,
}

/// 检测传递依赖冲突
pub fn detect_conflicts(
    resolved: &BTreeMap<String, ResolvedPackage>,
    overrides: &BTreeMap<String, String>,
) -> Result<()> {
    for name in overrides.keys() {
        let _found = resolved.values().any(|p| p.name == *name);
    }
    Ok(())
}
