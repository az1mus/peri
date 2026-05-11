use crate::plugin::config::{load_claude_settings, marketplaces_cache_dir};
use crate::plugin::types::{
    KnownMarketplace, MarketplaceManifest, MarketplacePlugin, MarketplaceSource, PluginAuthor,
};
use chrono::{DateTime, Utc};
use std::path::{Path, PathBuf};
use thiserror::Error;
use tokio::sync::mpsc;
use tracing::{debug, warn};

#[derive(Debug, Error)]
pub enum MarketplaceError {
    #[error("Git 操作失败: {0}")]
    GitFailed(String),
    #[error("HTTP 请求失败: {0}")]
    HttpFailed(String),
    #[error("JSON 解析失败: {0}")]
    ParseFailed(String),
    #[error("marketplace.json 未找到: {path}")]
    ManifestNotFound { path: String },
    #[error("NPM 操作失败: {0}")]
    NpmFailed(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone, PartialEq)]
pub enum MarketplaceStatus {
    Cached,
    Fetching,
    Fresh,
    Stale(String),
    NotFetched,
}

pub struct MarketplaceEntry {
    pub name: String,
    pub source: MarketplaceSource,
    pub manifest: Option<MarketplaceManifest>,
    pub status: MarketplaceStatus,
    pub last_updated: Option<DateTime<Utc>>,
    pub auto_update: bool,
}

#[derive(Debug, Clone)]
pub struct AvailablePlugin {
    pub name: String,
    pub description: String,
    pub version: String,
    pub marketplace: String,
    pub source: serde_json::Value,
    pub author: Option<PluginAuthor>,
    pub category: Option<String>,
    pub homepage: Option<String>,
}

#[derive(Debug, Clone)]
pub enum MarketplaceRefreshEvent {
    Updated {
        index: usize,
        name: String,
    },
    Failed {
        index: usize,
        name: String,
        error: String,
    },
}

pub fn find_marketplace_json(dir: &Path) -> Option<PathBuf> {
    let root = dir.join("marketplace.json");
    if root.exists() {
        return Some(root);
    }
    let subdir = dir.join(".claude-plugin").join("marketplace.json");
    if subdir.exists() {
        return Some(subdir);
    }
    None
}

pub fn read_manifest_from_path(path: &Path) -> Result<MarketplaceManifest, MarketplaceError> {
    let content = std::fs::read_to_string(path)?;
    serde_json::from_str(&content).map_err(|e| MarketplaceError::ParseFailed(e.to_string()))
}

async fn fetch_github(
    name: &str,
    repo: &str,
    cache_base: &Path,
    auto_update: bool,
) -> Result<MarketplaceManifest, MarketplaceError> {
    let url = format!("https://github.com/{repo}.git");
    fetch_git(name, &url, cache_base, auto_update).await
}

/// 通用的 git 仓库（任意 git URL）
async fn fetch_git(
    name: &str,
    url: &str,
    cache_base: &Path,
    auto_update: bool,
) -> Result<MarketplaceManifest, MarketplaceError> {
    let cache_dir = cache_base.join(name);

    if !cache_dir.exists() {
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("git")
                .args([
                    "clone",
                    "--depth",
                    "1",
                    url,
                    &cache_dir.display().to_string(),
                ])
                .output(),
        )
        .await
        .map_err(|e| MarketplaceError::GitFailed(format!("clone 超时: {e}")))?
        .map_err(|e| MarketplaceError::GitFailed(format!("clone 执行失败: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(MarketplaceError::GitFailed(format!("clone 失败: {stderr}")));
        }
    } else if auto_update {
        let output = tokio::time::timeout(
            std::time::Duration::from_secs(30),
            tokio::process::Command::new("git")
                .args(["-C", &cache_dir.display().to_string(), "pull", "--ff-only"])
                .output(),
        )
        .await
        .map_err(|e| MarketplaceError::GitFailed(format!("pull 超时: {e}")))?
        .map_err(|e| MarketplaceError::GitFailed(format!("pull 执行失败: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            warn!("git pull 失败 '{}', 回退到缓存: {stderr}", url);
            // fall through to read cache
        }
    }

    let manifest_path =
        find_marketplace_json(&cache_dir).ok_or_else(|| MarketplaceError::ManifestNotFound {
            path: cache_dir.display().to_string(),
        })?;
    read_manifest_from_path(&manifest_path)
}

async fn fetch_url(
    name: &str,
    url: &str,
    cache_base: &Path,
) -> Result<MarketplaceManifest, MarketplaceError> {
    let cache_file = cache_base.join(format!("{name}.json"));

    let last_modified = std::fs::metadata(&cache_file)
        .ok()
        .and_then(|m| m.modified().ok())
        .map(|t| {
            let dt: DateTime<Utc> = t.into();
            dt.format("%a, %d %b %Y %H:%M:%S GMT").to_string()
        });

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| MarketplaceError::HttpFailed(e.to_string()))?;

    let mut req = client.get(url);
    if let Some(ref lm) = last_modified {
        req = req.header("If-Modified-Since", lm);
    }

    let result = req.send().await;

    match result {
        Ok(response) => match response.status().as_u16() {
            304 => read_manifest_from_path(&cache_file),
            200 => {
                let body = response
                    .text()
                    .await
                    .map_err(|e| MarketplaceError::HttpFailed(e.to_string()))?;
                if let Some(parent) = cache_file.parent() {
                    std::fs::create_dir_all(parent)?;
                }
                std::fs::write(&cache_file, &body)?;
                serde_json::from_str(&body)
                    .map_err(|e| MarketplaceError::ParseFailed(e.to_string()))
            }
            status => Err(MarketplaceError::HttpFailed(format!("HTTP {status}"))),
        },
        Err(e) => {
            if cache_file.exists() {
                warn!("URL 拉取失败 '{}', 回退到缓存: {}", url, e);
                read_manifest_from_path(&cache_file)
            } else {
                Err(MarketplaceError::HttpFailed(e.to_string()))
            }
        }
    }
}

fn read_file(path: &Path) -> Result<MarketplaceManifest, MarketplaceError> {
    read_manifest_from_path(path)
}

fn read_directory(path: &Path) -> Result<MarketplaceManifest, MarketplaceError> {
    let manifest_path =
        find_marketplace_json(path).ok_or_else(|| MarketplaceError::ManifestNotFound {
            path: path.display().to_string(),
        })?;
    read_manifest_from_path(&manifest_path)
}

async fn fetch_npm(
    name: &str,
    package: &str,
    cache_base: &Path,
) -> Result<MarketplaceManifest, MarketplaceError> {
    let cache_dir = cache_base.join(name);

    if let Some(manifest_path) = find_marketplace_json(&cache_dir) {
        return read_manifest_from_path(&manifest_path);
    }

    let tmp_dir = std::env::temp_dir().join(format!("npm-pack-{package}-{}", uuid::Uuid::new_v4()));
    std::fs::create_dir_all(&tmp_dir)?;
    let output = tokio::time::timeout(
        std::time::Duration::from_secs(60),
        tokio::process::Command::new("npm")
            .args([
                "pack",
                package,
                "--pack-destination",
                &tmp_dir.display().to_string(),
            ])
            .output(),
    )
    .await
    .map_err(|e| MarketplaceError::NpmFailed(format!("npm pack 超时: {e}")))?
    .map_err(|e| MarketplaceError::NpmFailed(format!("npm pack 执行失败: {e}")))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(MarketplaceError::NpmFailed(format!(
            "npm pack 失败: {stderr}"
        )));
    }

    let tgz_path = std::fs::read_dir(&tmp_dir)?
        .find_map(|e| {
            e.ok().and_then(|f| {
                if f.path()
                    .extension()
                    .map(|ext| ext == "tgz")
                    .unwrap_or(false)
                {
                    Some(f.path())
                } else {
                    None
                }
            })
        })
        .ok_or_else(|| MarketplaceError::NpmFailed("未找到 .tgz 文件".into()))?;

    let file = std::fs::File::open(&tgz_path)?;
    let decoder = flate2::read::GzDecoder::new(file);
    let mut archive = tar::Archive::new(decoder);
    std::fs::create_dir_all(&cache_dir)?;
    archive.unpack(&cache_dir)?;

    let manifest_path =
        find_marketplace_json(&cache_dir).ok_or_else(|| MarketplaceError::ManifestNotFound {
            path: cache_dir.display().to_string(),
        })?;
    read_manifest_from_path(&manifest_path)
}

pub struct MarketplaceManager {
    entries: Vec<MarketplaceEntry>,
    override_dir: Option<PathBuf>,
}

impl MarketplaceManager {
    pub fn new(override_dir: Option<PathBuf>) -> Self {
        MarketplaceManager {
            entries: Vec::new(),
            override_dir,
        }
    }

    fn cache_base(&self) -> PathBuf {
        if let Some(ref dir) = self.override_dir {
            dir.join("marketplaces")
        } else {
            marketplaces_cache_dir()
        }
    }

    fn known_marketplaces_path(&self) -> PathBuf {
        if let Some(ref dir) = self.override_dir {
            dir.join("known_marketplaces.json")
        } else {
            crate::plugin::config::known_marketplaces_path()
        }
    }

    fn claude_settings_path(&self) -> PathBuf {
        if let Some(ref dir) = self.override_dir {
            dir.join("settings.json")
        } else {
            crate::plugin::config::claude_settings_path()
        }
    }

    pub fn try_load_cache(
        &self,
        source: &MarketplaceSource,
        name: &str,
    ) -> Option<MarketplaceManifest> {
        let cache_base = self.cache_base();
        let path: Option<PathBuf> = match source {
            MarketplaceSource::GitHub { .. } => find_marketplace_json(&cache_base.join(name)),
            MarketplaceSource::Git { .. } => find_marketplace_json(&cache_base.join(name)),
            MarketplaceSource::Url { .. } => {
                let p = cache_base.join(format!("{name}.json"));
                if p.exists() {
                    Some(p)
                } else {
                    None
                }
            }
            MarketplaceSource::File { path } => {
                let p = PathBuf::from(path);
                if p.exists() {
                    Some(p)
                } else {
                    None
                }
            }
            MarketplaceSource::Directory { path } => find_marketplace_json(Path::new(path)),
            MarketplaceSource::Npm { .. } => find_marketplace_json(&cache_base.join(name)),
        };
        path.and_then(|p| {
            read_manifest_from_path(&p)
                .map_err(|e| {
                    debug!("缓存 manifest 读取失败 {:?}: {}", p, e);
                    e
                })
                .ok()
        })
    }

    pub fn extract_name(source: &MarketplaceSource) -> String {
        match source {
            MarketplaceSource::GitHub { repo } => {
                repo.split('/').next_back().unwrap_or(repo).to_string()
            }
            MarketplaceSource::Git { url } => {
                // 从 git URL 中提取目录名（类似 GitHub 处理）
                if let Some(last) = url.rsplit('/').next() {
                    last.strip_suffix(".git").unwrap_or(last).to_string()
                } else {
                    "git-marketplace".into()
                }
            }
            MarketplaceSource::Url { url } => url::Url::parse(url)
                .ok()
                .and_then(|u| {
                    u.path_segments()
                        .and_then(|mut segs| segs.next_back().map(|s| s.to_string()))
                })
                .map(|s| s.trim_end_matches(".json").to_string())
                .unwrap_or_else(|| "url-marketplace".into()),
            MarketplaceSource::File { path } => PathBuf::from(path)
                .file_stem()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "file-marketplace".into()),
            MarketplaceSource::Directory { path } => PathBuf::from(path)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| "dir-marketplace".into()),
            MarketplaceSource::Npm { package } => package.clone(),
        }
    }

    pub async fn init(
        &mut self,
        tx: mpsc::Sender<MarketplaceRefreshEvent>,
    ) -> Vec<tokio::task::JoinHandle<()>> {
        use crate::plugin::config::{load_known_marketplaces, save_known_marketplaces};

        let km_path = self.known_marketplaces_path();
        let settings_path = self.claude_settings_path();
        let mut known = load_known_marketplaces(Some(&km_path)).unwrap_or_default();
        let settings = load_claude_settings(Some(&settings_path)).unwrap_or_default();

        // Merge extra known marketplaces from settings.json
        for extra in &settings.extra_known_marketplaces {
            let extra_json = serde_json::to_string(&extra.source).unwrap_or_default();
            let already_exists = known
                .iter()
                .any(|km| serde_json::to_string(&km.source).unwrap_or_default() == extra_json);
            if !already_exists {
                // 将 DeclaredMarketplace 转换为 KnownMarketplace
                known.push(KnownMarketplace::from(extra.clone()));
            }
        }

        // Auto-register official marketplace
        let has_official = known.iter().any(|km| match &km.source {
            MarketplaceSource::GitHub { repo } => repo == "anthropics/claude-plugins-official",
            _ => false,
        });
        if !has_official {
            let official = KnownMarketplace {
                source: MarketplaceSource::GitHub {
                    repo: "anthropics/claude-plugins-official".into(),
                },
                install_location: String::new(),
                auto_update: true,
                last_updated: String::new(),
            };
            known.push(official);
            let _ = save_known_marketplaces(&known, Some(&km_path));
        }

        // Build entries
        self.entries.clear();
        for km in &known {
            let name = Self::extract_name(&km.source);
            let cached_manifest = self.try_load_cache(&km.source, &name);
            let status = if cached_manifest.is_some() {
                MarketplaceStatus::Cached
            } else {
                MarketplaceStatus::NotFetched
            };
            let last_updated = chrono::DateTime::parse_from_rfc3339(&km.last_updated)
                .ok()
                .map(|dt| dt.with_timezone(&Utc));
            self.entries.push(MarketplaceEntry {
                name,
                source: km.source.clone(),
                manifest: cached_manifest,
                status,
                last_updated,
                auto_update: km.auto_update,
            });
        }

        // Spawn background refresh tasks
        let mut handles = Vec::new();
        for i in 0..self.entries.len() {
            handles.push(self.spawn_refresh(i, tx.clone()));
        }
        handles
    }

    pub fn spawn_refresh(
        &self,
        index: usize,
        tx: mpsc::Sender<MarketplaceRefreshEvent>,
    ) -> tokio::task::JoinHandle<()> {
        let name = self.entries[index].name.clone();
        let source = self.entries[index].source.clone();
        let auto_update = self.entries[index].auto_update;
        let cache_base = self.cache_base();

        tokio::spawn(async move {
            let result = match &source {
                MarketplaceSource::GitHub { repo } => {
                    fetch_github(&name, repo, &cache_base, auto_update).await
                }
                MarketplaceSource::Git { url } => {
                    fetch_git(&name, url, &cache_base, auto_update).await
                }
                MarketplaceSource::Url { url } => fetch_url(&name, url, &cache_base).await,
                MarketplaceSource::File { path } => {
                    let p = path.clone();
                    tokio::task::spawn_blocking(move || read_file(Path::new(&p)))
                        .await
                        .expect("spawn_blocking panicked")
                }
                MarketplaceSource::Directory { path } => {
                    let p = path.clone();
                    tokio::task::spawn_blocking(move || read_directory(Path::new(&p)))
                        .await
                        .expect("spawn_blocking panicked")
                }
                MarketplaceSource::Npm { package } => fetch_npm(&name, package, &cache_base).await,
            };

            match result {
                Ok(manifest) => {
                    let _ = tx
                        .send(MarketplaceRefreshEvent::Updated {
                            index,
                            name: name.clone(),
                        })
                        .await;
                    // Update entry is handled by caller via update_entry
                    let _ = manifest; // caller uses event to trigger update
                }
                Err(e) => {
                    warn!("Marketplace '{}' 刷新失败: {}", name, e);
                    let _ = tx
                        .send(MarketplaceRefreshEvent::Failed {
                            index,
                            name: name.clone(),
                            error: e.to_string(),
                        })
                        .await;
                }
            }
        })
    }

    pub fn entries(&self) -> &[MarketplaceEntry] {
        &self.entries
    }

    pub fn update_entry(
        &mut self,
        index: usize,
        manifest: MarketplaceManifest,
        status: MarketplaceStatus,
    ) {
        if let Some(entry) = self.entries.get_mut(index) {
            entry.manifest = Some(manifest);
            entry.status = status;
            entry.last_updated = Some(Utc::now());
        }
    }

    pub fn find_plugin(&self, plugin_name: &str) -> Option<(&MarketplacePlugin, &str)> {
        for entry in &self.entries {
            if entry.status != MarketplaceStatus::Cached && entry.status != MarketplaceStatus::Fresh
            {
                continue;
            }
            if let Some(ref manifest) = entry.manifest {
                for plugin in &manifest.plugins {
                    if plugin.name == plugin_name {
                        return Some((plugin, &entry.name));
                    }
                }
            }
        }
        None
    }

    pub fn available_plugins(&self) -> Vec<AvailablePlugin> {
        let mut result = Vec::new();
        for entry in &self.entries {
            if entry.status != MarketplaceStatus::Cached && entry.status != MarketplaceStatus::Fresh
            {
                continue;
            }
            if let Some(ref manifest) = entry.manifest {
                for p in &manifest.plugins {
                    result.push(AvailablePlugin {
                        name: p.name.clone(),
                        description: p.description.clone(),
                        version: p.version.clone(),
                        marketplace: entry.name.clone(),
                        source: p.source.clone(),
                        author: p.author.clone(),
                        category: p.category.clone(),
                        homepage: p.homepage.clone(),
                    });
                }
            }
        }
        result
    }
}

/// 解析用户输入的 marketplace source 字符串
///
/// 支持的格式：
/// - `owner/repo` (GitHub shorthand)
/// - `git@github.com:owner/repo.git` (SSH)
/// - `https://example.com/marketplace.json` (URL)
/// - `./path/to/marketplace` (本地路径)
pub fn parse_marketplace_input(input: &str) -> Result<MarketplaceSource, String> {
    let trimmed = input.trim();

    if trimmed.is_empty() {
        return Err("输入不能为空".to_string());
    }

    // 1. Git SSH URLs: user@host:path 或 user@host:path.git
    if let Some(ssh_match) = trimmed.strip_prefix("git@") {
        if let Some((host, path)) = ssh_match.split_once(':') {
            // 移除 .git 后缀（如果存在）
            let path = path.strip_suffix(".git").unwrap_or(path);
            return Ok(MarketplaceSource::GitHub {
                repo: format!("git@{}:{}", host, path),
            });
        }
    }

    // 2. HTTP/HTTPS URLs
    if trimmed.starts_with("http://") || trimmed.starts_with("https://") {
        // GitHub URL 转换
        if trimmed.contains("github.com/") {
            // 提取 owner/repo 部分
            let parts: Vec<&str> = trimmed.split('/').collect();
            if parts.len() >= 5 {
                // https://github.com/owner/repo
                let owner = parts[3];
                let repo = parts[4].trim_end_matches(".git");
                return Ok(MarketplaceSource::GitHub {
                    repo: format!("{}/{}", owner, repo),
                });
            }
        }
        // 其他 URL 作为 marketplace.json URL
        return Ok(MarketplaceSource::Url {
            url: trimmed.to_string(),
        });
    }

    // 3. 本地路径：./, ../, /, ~ 开头
    if trimmed.starts_with("./")
        || trimmed.starts_with("../")
        || trimmed.starts_with('/')
        || trimmed.starts_with('~')
        || trimmed.starts_with(".\\")
        || trimmed.starts_with("..\\")
        || (trimmed.len() >= 3 && trimmed.as_bytes()[1] == b'\\')
        // Windows 路径 C:\...
        || (trimmed.len() >= 2
            && trimmed.as_bytes()[0].is_ascii_alphabetic()
            && trimmed.as_bytes()[1] == b':')
    {
        let path = shellexpand::tilde(trimmed).to_string();
        // 判断是文件还是目录
        let path_obj = Path::new(&path);
        if path_obj.ends_with(".json") || path_obj.extension().is_some_and(|e| e == "json") {
            return Ok(MarketplaceSource::File { path });
        } else {
            return Ok(MarketplaceSource::Directory { path });
        }
    }

    // 4. GitHub shorthand: owner/repo
    if trimmed.contains('/') && !trimmed.starts_with('@') {
        // owner/repo 格式
        let parts: Vec<&str> = trimmed.split('/').collect();
        if parts.len() == 2 {
            return Ok(MarketplaceSource::GitHub {
                repo: trimmed.to_string(),
            });
        }
    }

    // 5. NPM package: @scope/name 或 name
    if trimmed.starts_with('@') || !trimmed.contains('/') {
        return Ok(MarketplaceSource::Npm {
            package: trimmed.to_string(),
        });
    }

    Err(format!("无法识别的 marketplace source: {}", trimmed))
}

/// 刷新单个 marketplace 的缓存，返回 manifest 和缓存路径
///
/// 异步获取 marketplace manifest 并缓存到本地
/// 返回: (manifest, install_location)
pub async fn refresh_marketplace(
    source: &MarketplaceSource,
    name: &str,
) -> Result<(MarketplaceManifest, String), MarketplaceError> {
    let cache_base = marketplaces_cache_dir();
    let auto_update = true; // 默认启用自动更新

    let manifest = match source {
        MarketplaceSource::GitHub { repo } => {
            fetch_github(name, repo, &cache_base, auto_update).await?
        }
        MarketplaceSource::Git { url } => fetch_git(name, url, &cache_base, auto_update).await?,
        MarketplaceSource::Url { url } => fetch_url(name, url, &cache_base).await?,
        MarketplaceSource::File { path } => {
            let path = path.clone();
            tokio::task::spawn_blocking(move || read_file(Path::new(&path)))
                .await
                .expect("spawn_blocking panicked")?
        }
        MarketplaceSource::Directory { path } => {
            let path = path.clone();
            tokio::task::spawn_blocking(move || read_directory(Path::new(&path)))
                .await
                .expect("spawn_blocking panicked")?
        }
        MarketplaceSource::Npm { package } => fetch_npm(name, package, &cache_base).await?,
    };

    // 计算缓存路径
    let install_location = match source {
        MarketplaceSource::GitHub { .. }
        | MarketplaceSource::Git { .. }
        | MarketplaceSource::Npm { .. } => {
            // 目录型缓存：返回目录路径
            cache_base.join(name).display().to_string()
        }
        MarketplaceSource::Url { .. } => {
            // 文件型缓存：返回 .json 文件路径
            cache_base
                .join(format!("{name}.json"))
                .display()
                .to_string()
        }
        MarketplaceSource::File { path } => path.clone(),
        MarketplaceSource::Directory { path } => path.clone(),
    };

    Ok((manifest, install_location))
}

#[cfg(test)]
#[path = "marketplace_test.rs"]
mod tests;
