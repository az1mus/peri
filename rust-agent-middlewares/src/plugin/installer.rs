use crate::plugin::config::{load_installed_plugins, load_plugin_manifest, save_installed_plugins};
use crate::plugin::marketplace::read_manifest_from_path;
use crate::plugin::types::{InstallScope, InstalledPlugin, InstalledPlugins};
use crate::plugin::PluginConfigError;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum InstallerError {
    #[error("插件未找到: {name} (marketplace: {marketplace})")]
    PluginNotFound { name: String, marketplace: String },
    #[error("插件清单解析失败: {path}")]
    ManifestInvalid {
        path: PathBuf,
        #[source]
        source: serde_json::Error,
    },
    #[error("复制失败: {src} -> {dst}")]
    CopyFailed {
        src: PathBuf,
        dst: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("配置错误: {0}")]
    ConfigError(#[from] PluginConfigError),
    #[error("Settings 错误: {0}")]
    SettingsError(String),
    #[error("IO 错误: {0}")]
    Io(#[from] std::io::Error),
}

#[derive(Debug, Clone)]
pub struct PluginUpdateInfo {
    pub plugin_id: String,
    pub current_version: String,
    pub latest_version: String,
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let file_name = entry.file_name();
        // Skip .git directories
        if file_name == ".git" {
            continue;
        }
        let src_path = entry.path();
        let dst_path = dst.join(&file_name);
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dst_path)?;
        } else {
            std::fs::copy(&src_path, &dst_path)?;
        }
    }
    Ok(())
}

/// 从 marketplace 条目生成合成 plugin.json（用于无原生 manifest 的 LSP/MCP 插件）
pub(crate) fn generate_synthetic_manifest(
    target_dir: &Path,
    marketplace_plugin: &crate::plugin::types::MarketplacePlugin,
) -> Result<(), std::io::Error> {
    let mut manifest = serde_json::Map::new();
    manifest.insert("name".into(), serde_json::json!(marketplace_plugin.name));
    if !marketplace_plugin.version.is_empty() {
        manifest.insert(
            "version".into(),
            serde_json::json!(marketplace_plugin.version),
        );
    }
    if !marketplace_plugin.description.is_empty() {
        manifest.insert(
            "description".into(),
            serde_json::json!(marketplace_plugin.description),
        );
    }
    if let Some(ref author) = marketplace_plugin.author {
        if let Ok(val) = serde_json::to_value(author) {
            manifest.insert("author".into(), val);
        }
    }

    // 转换 lspServers: HashMap -> Vec（marketplace 用 {name: config}，plugin.json 用 [{name, ...}]）
    if let Some(lsp_servers) = marketplace_plugin.extra.get("lspServers") {
        if let Some(map) = lsp_servers.as_object() {
            let entries: Vec<serde_json::Value> = map
                .iter()
                .map(|(server_name, config)| {
                    let mut entry = config.clone();
                    if let Some(obj) = entry.as_object_mut() {
                        obj.insert("name".into(), serde_json::json!(server_name));
                    }
                    entry
                })
                .collect();
            if !entries.is_empty() {
                manifest.insert("lspServers".into(), serde_json::json!(entries));
            }
        }
    }

    // 转换 mcpServers
    if let Some(mcp_servers) = marketplace_plugin.extra.get("mcpServers") {
        manifest.insert("mcpServers".into(), mcp_servers.clone());
    }

    let claude_plugin_dir = target_dir.join(".claude-plugin");
    std::fs::create_dir_all(&claude_plugin_dir)?;
    let manifest_path = claude_plugin_dir.join("plugin.json");
    let json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, json)?;

    Ok(())
}

fn get_marketplace_manifest(
    marketplace: &str,
    marketplace_cache_dir: &Path,
) -> Result<crate::plugin::types::MarketplaceManifest, InstallerError> {
    let path = marketplace_cache_dir.join(marketplace);
    // Try root first, then .claude-plugin subdir
    let root = path.join("marketplace.json");
    let subdir = path.join(".claude-plugin").join("marketplace.json");
    let manifest_path = if root.exists() {
        root
    } else if subdir.exists() {
        subdir
    } else {
        return Err(InstallerError::PluginNotFound {
            name: String::new(),
            marketplace: marketplace.into(),
        });
    };
    read_manifest_from_path(&manifest_path)
        .map_err(|e| InstallerError::SettingsError(e.to_string()))
}

fn atomic_write_settings(path: &Path, value: &serde_json::Value) -> Result<(), InstallerError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(value)
        .map_err(|e| InstallerError::SettingsError(e.to_string()))?;
    let tmp_path = path.with_extension(format!("tmp.{}", uuid::Uuid::new_v4()));
    std::fs::write(&tmp_path, &json)?;
    std::fs::rename(&tmp_path, path)
        .map_err(|e| InstallerError::SettingsError(format!("rename 失败: {e}")))?;
    Ok(())
}

fn update_enabled_plugins(
    plugin_id: &str,
    scope: InstallScope,
    claude_dir: &Path,
    project_dir: Option<&Path>,
) -> Result<(), InstallerError> {
    let settings_path = match scope {
        InstallScope::User => claude_dir.join("settings.json"),
        InstallScope::Project => {
            if let Some(pd) = project_dir {
                pd.join(".claude").join("settings.json")
            } else {
                claude_dir.join("settings.json")
            }
        }
        InstallScope::Local => claude_dir.join("settings.json"),
    };

    let mut value = if settings_path.exists() {
        let content = std::fs::read_to_string(&settings_path)?;
        serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(serde_json::Map::new()))
    } else {
        serde_json::Value::Object(serde_json::Map::new())
    };

    let obj = value.as_object_mut().unwrap();
    let enabled = obj
        .entry("enabledPlugins")
        .or_insert(serde_json::Value::Object(serde_json::Map::new()));

    // 兼容两种格式：将现有的数组格式转换为对象格式
    let enabled_map = if let Some(arr) = enabled.as_array() {
        // 数组格式 → 对象格式
        let map: serde_json::Map<String, serde_json::Value> = arr
            .iter()
            .filter_map(|v| v.as_str())
            .map(|s| (s.to_string(), serde_json::Value::Bool(true)))
            .collect();
        *enabled = serde_json::Value::Object(map.clone());
        map
    } else {
        enabled.as_object().cloned().unwrap_or_default()
    };

    // 添加或更新插件
    if !enabled_map.contains_key(plugin_id) {
        if let Some(obj) = enabled.as_object_mut() {
            obj.insert(plugin_id.to_string(), serde_json::Value::Bool(true));
        }
    }

    atomic_write_settings(&settings_path, &value)
}

fn remove_from_enabled_plugins(
    plugin_id: &str,
    scope: &InstallScope,
    claude_dir: &Path,
    project_dir: Option<&Path>,
) -> Result<(), InstallerError> {
    let settings_path = match scope {
        InstallScope::User => claude_dir.join("settings.json"),
        InstallScope::Project => {
            if let Some(pd) = project_dir {
                pd.join(".claude").join("settings.json")
            } else {
                claude_dir.join("settings.json")
            }
        }
        InstallScope::Local => claude_dir.join("settings.json"),
    };

    if !settings_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&settings_path)?;
    let mut value: serde_json::Value =
        serde_json::from_str(&content).map_err(|e| InstallerError::SettingsError(e.to_string()))?;

    if let Some(obj) = value.as_object_mut() {
        if let Some(enabled) = obj.get_mut("enabledPlugins") {
            if let Some(arr) = enabled.as_array_mut() {
                // 数组格式
                arr.retain(|v| v.as_str() != Some(plugin_id));
            } else if let Some(map) = enabled.as_object_mut() {
                // 对象格式
                map.remove(plugin_id);
            }
        }
    }

    atomic_write_settings(&settings_path, &value)
}

pub async fn install_plugin(
    name: &str,
    marketplace: &str,
    scope: InstallScope,
    marketplace_cache_dir: &Path,
    claude_dir: &Path,
    project_dir: Option<&Path>,
) -> Result<InstalledPlugin, InstallerError> {
    let plugins_path = claude_dir.join("plugins").join("installed_plugins.json");
    let mut installed = load_installed_plugins(Some(&plugins_path))?;

    let manifest = get_marketplace_manifest(marketplace, marketplace_cache_dir)?;

    let marketplace_plugin = manifest
        .plugins
        .into_iter()
        .find(|p| p.name == name)
        .ok_or_else(|| InstallerError::PluginNotFound {
            name: name.into(),
            marketplace: marketplace.into(),
        })?;

    let source_dir = {
        // 检查是否为外部 URL 源
        if let Some(obj) = marketplace_plugin.source.as_object() {
            if obj.get("source").and_then(|v| v.as_str()) == Some("url") {
                let url = obj.get("url").and_then(|v| v.as_str()).ok_or_else(|| {
                    InstallerError::SettingsError("URL 源缺少 url 字段".to_string())
                })?;

                // 外部插件缓存到 ~/.claude/plugins/external/{name}/
                let external_cache = claude_dir.join("plugins").join("external").join(name);

                // 如果缓存不存在或需要更新，执行 git clone
                if !external_cache.exists() {
                    tokio::task::spawn_blocking({
                        let url = url.to_string();
                        let cache_dir = external_cache.clone();
                        move || {
                            let _ = std::fs::create_dir_all(&cache_dir);
                            let output = std::process::Command::new("git")
                                .args(["clone", "--depth", "1", &url, cache_dir.to_str().unwrap()])
                                .output();
                            match output {
                                Ok(o) if o.status.success() => Ok(()),
                                Ok(o) => Err(format!(
                                    "git clone 失败: {}",
                                    String::from_utf8_lossy(&o.stderr)
                                )),
                                Err(e) => Err(format!("git clone 执行失败: {e}")),
                            }
                        }
                    })
                    .await
                    .map_err(|e| {
                        InstallerError::SettingsError(format!("spawn_blocking 失败: {e}"))
                    })?
                    .map_err(InstallerError::SettingsError)?;
                }

                external_cache
            } else {
                return Err(InstallerError::SettingsError(
                    "不支持的 source 对象格式".to_string(),
                ));
            }
        } else {
            // 本地路径源
            let raw = marketplace_plugin.source.as_str().unwrap_or(".");
            let normalized: std::path::PathBuf = std::path::Path::new(raw)
                .components()
                .filter(|c| matches!(c, std::path::Component::Normal(_)))
                .collect();
            marketplace_cache_dir.join(marketplace).join(normalized)
        }
    };
    if !source_dir.exists() {
        return Err(InstallerError::PluginNotFound {
            name: name.into(),
            marketplace: marketplace.into(),
        });
    }
    let manifest_path = source_dir.join(".claude-plugin").join("plugin.json");
    let has_native_manifest = if manifest_path.exists() {
        // 文件存在时仍然验证其内容
        load_plugin_manifest(&source_dir)?;
        true
    } else {
        false
    };

    let version = marketplace_plugin
        .sha
        .as_ref()
        .map(|s| s.chars().take(7).collect())
        .unwrap_or_else(|| {
            let v = marketplace_plugin.version.clone();
            if v.is_empty() {
                // 无版本信息时使用时间戳作为版本
                chrono::Utc::now().format("%Y%m%d%H%M%S").to_string()
            } else {
                v
            }
        });

    let target_dir = claude_dir
        .join("plugins")
        .join("cache")
        .join(marketplace)
        .join(name)
        .join(&version);

    tokio::task::spawn_blocking({
        let source_dir = source_dir.clone();
        let target_dir = target_dir.clone();
        move || -> Result<(), InstallerError> {
            if target_dir.exists() {
                let _ = std::fs::remove_dir_all(&target_dir);
            }
            std::fs::create_dir_all(&target_dir)?;
            copy_dir_recursive(&source_dir, &target_dir).map_err(|e| {
                InstallerError::CopyFailed {
                    src: source_dir.clone(),
                    dst: target_dir.clone(),
                    source: e,
                }
            })?;

            // 如果插件没有原生 plugin.json，从 marketplace 条目生成合成的 manifest
            if !has_native_manifest {
                generate_synthetic_manifest(&target_dir, &marketplace_plugin)?;
            }

            Ok(())
        }
    })
    .await
    .map_err(|e| InstallerError::SettingsError(format!("spawn_blocking 失败: {e}")))??;

    let plugin_id = format!("{name}@{marketplace}");
    let project_path = project_dir.and_then(|p| p.to_str()).map(|s| s.to_string());
    let installed_plugin = InstalledPlugin {
        id: plugin_id.clone(),
        name: name.into(),
        version,
        marketplace: marketplace.into(),
        install_path: target_dir,
        scope,
        project_path,
    };

    // Remove old entry with same id, scope, and project_path
    installed.plugins.retain(|p| {
        !(p.id == plugin_id && p.scope == scope && match_project_path(&p.project_path, project_dir))
    });
    installed.plugins.push(installed_plugin.clone());
    save_installed_plugins(&installed, Some(&plugins_path))?;

    update_enabled_plugins(&plugin_id, scope, claude_dir, project_dir)?;

    Ok(installed_plugin)
}

pub async fn uninstall_plugin(
    plugin_id: &str,
    claude_dir: &Path,
    project_dir: Option<&Path>,
) -> Result<(), InstallerError> {
    let (name, marketplace) = plugin_id.split_once('@').unwrap_or((plugin_id, ""));

    let plugins_path = claude_dir.join("plugins").join("installed_plugins.json");
    let mut installed = load_installed_plugins(Some(&plugins_path))?;

    // 找到匹配的条目（考虑 project_path）
    let entry = installed
        .plugins
        .iter()
        .find(|p| p.id == plugin_id && match_project_path(&p.project_path, project_dir))
        .ok_or_else(|| InstallerError::PluginNotFound {
            name: name.into(),
            marketplace: marketplace.into(),
        })?;

    let install_path = entry.install_path.clone();
    let scope = entry.scope;

    // 检查是否为最后一个作用域（插件完全卸载）
    // 需要排除当前条目（相同 scope + 相同 project_path）
    let is_last_scope = !installed.plugins.iter().any(|p| {
        p.id == plugin_id
            && (p.scope != scope
                || (p.scope == scope && !match_project_path(&p.project_path, project_dir)))
    });

    // 只删除当前 scope + project_path 的条目
    installed.plugins.retain(|p| {
        !(p.id == plugin_id && p.scope == scope && match_project_path(&p.project_path, project_dir))
    });
    save_installed_plugins(&installed, Some(&plugins_path))?;

    remove_from_enabled_plugins(plugin_id, &scope, claude_dir, project_dir)?;

    // 如果是最后一个作用域，删除插件数据和选项，并标记版本为孤儿
    if is_last_scope {
        // 1. 删除插件数据目录 ~/.claude/plugins/data/{sanitized_plugin_id}/
        let sanitized_id = sanitize_plugin_id(plugin_id);
        let data_dir = claude_dir.join("plugins").join("data").join(&sanitized_id);
        if data_dir.exists() {
            tokio::fs::remove_dir_all(&data_dir).await.ok();
        }

        // 2. 删除插件配置选项 settings.json -> pluginConfigs[plugin_id]
        remove_plugin_options(plugin_id, claude_dir)?;

        // 3. 标记版本为孤儿（延迟删除），而不是立即删除
        // 这允许并发会话继续使用旧版本
        let _ = mark_orphaned(&install_path).await;
    }

    Ok(())
}

/// 匹配 project_path：两者都为 None，或者路径字符串匹配
fn match_project_path(stored: &Option<String>, given: Option<&Path>) -> bool {
    match (stored, given) {
        (None, None) => true,
        (None, Some(_)) => false,
        (Some(_), None) => false,
        (Some(s), Some(p)) => {
            // 规范化后比较（处理相对/绝对路径差异）
            let given_str = p.to_str().unwrap_or("");
            s == given_str || s.ends_with(given_str) || given_str.ends_with(s)
        }
    }
}

/// 清理插件 ID 中的特殊字符，用于目录名
fn sanitize_plugin_id(plugin_id: &str) -> String {
    plugin_id
        .chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '-'
            }
        })
        .collect()
}

/// 标记插件版本为孤儿（延迟删除）
async fn mark_orphaned(install_path: &Path) -> Result<(), InstallerError> {
    if !install_path.exists() {
        return Ok(());
    }

    tokio::task::spawn_blocking({
        let path = install_path.to_path_buf();
        move || {
            // 创建 .orphaned_at 文件记录时间戳
            let orphaned_file = path.join(".orphaned_at");
            let _ = std::fs::write(&orphaned_file, chrono::Utc::now().to_rfc3339());
            Ok::<(), InstallerError>(())
        }
    })
    .await
    .map_err(|e| InstallerError::SettingsError(format!("spawn_blocking 失败: {e}")))?
}

/// 从 settings.json 删除插件配置选项
fn remove_plugin_options(plugin_id: &str, claude_dir: &Path) -> Result<(), InstallerError> {
    let settings_path = claude_dir.join("settings.json");
    if !settings_path.exists() {
        return Ok(());
    }

    let content = std::fs::read_to_string(&settings_path)?;
    let mut value: serde_json::Value =
        serde_json::from_str(&content).unwrap_or(serde_json::Value::Object(serde_json::Map::new()));

    if let Some(obj) = value.as_object_mut() {
        // 删除 pluginConfigs[plugin_id]
        if let Some(configs) = obj.get_mut("pluginConfigs").and_then(|v| v.as_object_mut()) {
            configs.remove(plugin_id);
        }

        // 保存更新后的 settings.json
        atomic_write_settings(&settings_path, &value)?;
    }

    Ok(())
}

pub async fn update_plugin(
    plugin_id: &str,
    marketplace_cache_dir: &Path,
    claude_dir: &Path,
    project_dir: Option<&Path>,
) -> Result<InstalledPlugin, InstallerError> {
    let (name, marketplace) = plugin_id.split_once('@').unwrap_or((plugin_id, ""));

    let plugins_path = claude_dir.join("plugins").join("installed_plugins.json");
    let installed = load_installed_plugins(Some(&plugins_path))?;
    let current = installed
        .plugins
        .iter()
        .find(|p| p.id == plugin_id)
        .ok_or_else(|| InstallerError::PluginNotFound {
            name: name.into(),
            marketplace: marketplace.into(),
        })?;

    let manifest = get_marketplace_manifest(marketplace, marketplace_cache_dir)?;
    let latest = manifest
        .plugins
        .iter()
        .find(|p| p.name == name)
        .ok_or_else(|| InstallerError::PluginNotFound {
            name: name.into(),
            marketplace: marketplace.into(),
        })?;

    let latest_version = latest
        .sha
        .as_ref()
        .map(|s| s.chars().take(7).collect::<String>())
        .unwrap_or_else(|| latest.version.clone());

    if latest_version == current.version {
        return Ok(current.clone());
    }

    uninstall_plugin(plugin_id, claude_dir, project_dir).await?;
    install_plugin(
        name,
        marketplace,
        current.scope,
        marketplace_cache_dir,
        claude_dir,
        project_dir,
    )
    .await
}

pub async fn check_updates(
    installed: &InstalledPlugins,
    marketplace_cache_dir: &Path,
) -> Vec<PluginUpdateInfo> {
    let mut manifest_cache: HashMap<String, crate::plugin::types::MarketplaceManifest> =
        HashMap::new();
    let mut result = Vec::new();

    for plugin in &installed.plugins {
        let (name, marketplace) = plugin.id.split_once('@').unwrap_or((&plugin.id, ""));

        if !manifest_cache.contains_key(marketplace) {
            if let Ok(manifest) = get_marketplace_manifest(marketplace, marketplace_cache_dir) {
                manifest_cache.insert(marketplace.to_string(), manifest);
            } else {
                continue;
            }
        }

        let manifest = &manifest_cache[marketplace];
        if let Some(latest) = manifest.plugins.iter().find(|p| p.name == name) {
            let latest_version = latest
                .sha
                .as_ref()
                .map(|s| s.chars().take(7).collect::<String>())
                .unwrap_or_else(|| latest.version.clone());

            if latest_version != plugin.version {
                result.push(PluginUpdateInfo {
                    plugin_id: plugin.id.clone(),
                    current_version: plugin.version.clone(),
                    latest_version,
                });
            }
        }
    }

    result
}

/// 清理孤儿插件版本（超过 7 天未使用）
///
/// 扫描 `~/.claude/plugins/cache/` 目录，删除标记为孤儿且超过 7 天的版本。
/// 应在应用启动时或定期调用。
pub async fn cleanup_orphaned_plugins(claude_dir: &Path) -> Result<usize, InstallerError> {
    const CLEANUP_AGE_MS: i64 = 7 * 24 * 60 * 60 * 1000; // 7 天

    let cache_dir = claude_dir.join("plugins").join("cache");
    if !cache_dir.exists() {
        return Ok(0);
    }

    let installed = load_installed_plugins(Some(
        &claude_dir.join("plugins").join("installed_plugins.json"),
    ))?;
    let installed_paths: std::collections::HashSet<PathBuf> = installed
        .plugins
        .iter()
        .map(|p| p.install_path.clone())
        .collect();

    let now = chrono::Utc::now().timestamp_millis();
    let mut deleted_count = 0;

    // 扫描 cache 目录下的所有 marketplace
    let mut entries = tokio::fs::read_dir(&cache_dir)
        .await
        .map_err(|e| InstallerError::SettingsError(format!("读取 cache 目录失败: {e}")))?;

    let mut tasks = Vec::new();
    while let Some(entry) = entries
        .next_entry()
        .await
        .map_err(|e| InstallerError::SettingsError(format!("读取目录条目失败: {e}")))?
    {
        if !entry.file_type().await?.is_dir() {
            continue;
        }

        let marketplace_path = entry.path();
        let installed_paths_clone = installed_paths.clone();

        let task = tokio::task::spawn_blocking(move || {
            let mut count = 0;

            // 扫描每个插件
            if let Ok(plugin_entries) = std::fs::read_dir(&marketplace_path) {
                for plugin_entry in plugin_entries.flatten() {
                    if !plugin_entry.file_type()?.is_dir() {
                        continue;
                    }

                    let plugin_path = plugin_entry.path();

                    // 扫描每个版本
                    if let Ok(version_entries) = std::fs::read_dir(&plugin_path) {
                        for version_entry in version_entries.flatten() {
                            if !version_entry.file_type()?.is_dir() {
                                continue;
                            }

                            let version_path = version_entry.path();

                            // 跳过已安装的版本
                            if installed_paths_clone.contains(&version_path) {
                                // 移除 .orphaned_at 标记（如果存在）
                                let _ = std::fs::remove_file(version_path.join(".orphaned_at"));
                                continue;
                            }

                            // 检查是否为孤儿版本
                            let orphaned_file = version_path.join(".orphaned_at");
                            if let Ok(metadata) = std::fs::metadata(&orphaned_file) {
                                if let Ok(modified) = metadata.modified() {
                                    // 计算文件修改时间距今的毫秒数
                                    let age_ms = now
                                        - modified
                                            .duration_since(std::time::SystemTime::UNIX_EPOCH)
                                            .unwrap_or_default()
                                            .as_millis()
                                            as i64;

                                    if age_ms > CLEANUP_AGE_MS {
                                        // 删除孤儿版本
                                        if std::fs::remove_dir_all(&version_path).is_ok() {
                                            count += 1;
                                        }
                                    }
                                }
                            }
                        }

                        // 删除空的插件目录
                        if plugin_path.read_dir()?.count() == 0 {
                            let _ = std::fs::remove_dir(&plugin_path);
                        }
                    }
                }

                // 删除空的 marketplace 目录
                if marketplace_path.read_dir()?.count() == 0 {
                    let _ = std::fs::remove_dir(&marketplace_path);
                }
            }

            Ok::<usize, InstallerError>(count)
        });

        tasks.push(task);
    }

    // 等待所有任务完成
    for task in tasks {
        if let Ok(Ok(count)) = task.await {
            deleted_count += count;
        }
    }

    Ok(deleted_count)
}

#[cfg(test)]
#[path = "installer_test.rs"]
mod tests;
