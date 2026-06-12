use crate::error::{AgmError, Result};
use crate::types::{PackageManifest, Resolution};
use std::path::{Path, PathBuf};

/// Store 管理器：~/.agm/store/
pub struct Store {
    pub(crate) root: PathBuf,
}

impl Store {
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// 获取 git 包在 store 中的路径: store/git_{owner}_{repo}@{commit}/
    pub fn git_package_path(&self, repo_url: &str, commit: &str) -> PathBuf {
        let id = sanitize_repo_id(repo_url);
        self.root
            .join(format!("git_{id}@{commit}", id = id, commit = commit))
    }

    /// 获取 registry 包在 store 中的路径: store/<name>@<version>/
    pub fn registry_package_path(&self, name: &str, version: &str) -> PathBuf {
        let safe_name = name.replace('/', "_");
        self.root.join(format!("{}@{}", safe_name, version))
    }

    /// 确保 store 根目录存在
    pub fn ensure_root(&self) -> Result<()> {
        std::fs::create_dir_all(&self.root)?;
        Ok(())
    }

    /// 列出 store 中所有包目录
    pub fn list_packages(&self) -> Result<Vec<PathBuf>> {
        let mut entries = Vec::new();
        if !self.root.exists() {
            return Ok(entries);
        }
        for entry in std::fs::read_dir(&self.root)? {
            let entry = entry?;
            if entry.file_type()?.is_dir() {
                entries.push(entry.path());
            }
        }
        Ok(entries)
    }

    /// 读取包内 agm.package.json
    pub fn read_package_manifest(&self, package_dir: &Path) -> Result<PackageManifest> {
        let manifest_path = package_dir.join("agm.package.json");
        if !manifest_path.exists() {
            return Err(AgmError::Other(format!(
                "agm.package.json not found in {}",
                package_dir.display()
            )));
        }
        PackageManifest::load(&manifest_path)
    }

    /// 删除指定包目录
    pub fn remove(&self, package_dir: &Path) -> Result<()> {
        if package_dir.exists() {
            std::fs::remove_dir_all(package_dir)?;
        }
        Ok(())
    }
}

/// 将 repo URL 转为安全的文件系统标识
fn sanitize_repo_id(url: &str) -> String {
    url.trim_end_matches('/')
        .trim_end_matches(".git")
        .trim_start_matches("https://")
        .trim_start_matches("http://")
        .trim_start_matches("git@")
        .replace("github.com/", "")
        .replace(':', "/")
        .replace(['/', '@'], "_")
}

/// 将包从临时目录安装到 store
pub fn install_to_store(
    store: &Store,
    temp_dir: &Path,
    resolution: &Resolution,
    pkg_name: &str,
    version: &str,
) -> Result<PathBuf> {
    let dest = match resolution {
        Resolution::Git { repo, commit, .. } => store.git_package_path(repo, commit),
        Resolution::Registry { .. } => store.registry_package_path(pkg_name, version),
    };

    if dest.exists() {
        return Ok(dest);
    }

    store.ensure_root()?;

    std::fs::rename(temp_dir, &dest)
        .map_err(|e| AgmError::Other(format!("failed to move package to store: {}", e)))?;

    Ok(dest)
}
