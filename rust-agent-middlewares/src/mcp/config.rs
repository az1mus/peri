use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use thiserror::Error;

/// 单个 MCP 服务器配置
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct McpServerConfig {
    /// stdio 传输的可执行命令（如 "npx"）
    pub command: Option<String>,
    /// stdio 传输的命令参数
    #[serde(default)]
    pub args: Option<Vec<String>>,
    /// 传递给子进程的环境变量
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    /// Streamable HTTP 传输的 URL
    pub url: Option<String>,
    /// HTTP 请求的自定义头
    #[serde(default)]
    pub headers: Option<HashMap<String, String>>,
}

/// MCP 配置文件顶层结构（.mcp.json / settings.json 中的 mcpServers 片段）
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct McpConfigFile {
    #[serde(default)]
    pub mcp_servers: HashMap<String, McpServerConfig>,
}

/// MCP 配置加载错误
#[derive(Debug, Error)]
pub enum McpConfigError {
    #[error("MCP 配置文件解析失败: {path}: {source}")]
    ParseError {
        path: String,
        #[source]
        source: serde_json::Error,
    },
    #[error("MCP 配置文件读取失败: {path}: {source}")]
    ReadError {
        path: String,
        #[source]
        source: std::io::Error,
    },
}

/// 从指定 JSON 文件加载 MCP 配置，文件不存在时返回空配置
pub fn load_from_path(path: &Path) -> Result<McpConfigFile, McpConfigError> {
    if !path.exists() {
        return Ok(McpConfigFile::default());
    }
    let content =
        std::fs::read_to_string(path).map_err(|e| McpConfigError::ReadError {
            path: path.display().to_string(),
            source: e,
        })?;
    serde_json::from_str::<McpConfigFile>(&content).map_err(|e| McpConfigError::ParseError {
        path: path.display().to_string(),
        source: e,
    })
}

/// 从全局 settings.json 的 extra 字段中提取 mcpServers
pub fn load_global_config(settings_json_path: &Path) -> Result<McpConfigFile, McpConfigError> {
    if !settings_json_path.exists() {
        return Ok(McpConfigFile::default());
    }
    let content = std::fs::read_to_string(settings_json_path).map_err(|e| {
        McpConfigError::ReadError {
            path: settings_json_path.display().to_string(),
            source: e,
        }
    })?;
    let v: serde_json::Value = serde_json::from_str(&content).map_err(|e| {
        McpConfigError::ParseError {
            path: settings_json_path.display().to_string(),
            source: e,
        }
    })?;
    // 从顶层 value 中提取 "config"."mcpServers" 或 "mcpServers"
    let mcp_servers = v
        .get("config")
        .and_then(|c| c.get("mcpServers"))
        .or_else(|| v.get("mcpServers"))
        .cloned()
        .unwrap_or(serde_json::Value::Object(serde_json::Map::new()));
    let config = McpConfigFile {
        mcp_servers: serde_json::from_value(mcp_servers).unwrap_or_default(),
    };
    Ok(config)
}

/// 展开 s 中所有 ${VAR} 占位符为环境变量值
pub fn expand_env_vars(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '$' && chars.peek() == Some(&'{') {
            chars.next(); // 消耗 '{'
            let var_name: String = chars.by_ref().take_while(|&ch| ch != '}').collect();
            if chars.peek() == Some(&'}') {
                chars.next(); // 消耗 '}'
            }
            match std::env::var(&var_name) {
                Ok(val) => result.push_str(&val),
                Err(_) => {
                    tracing::warn!(
                        var_name = %var_name,
                        "MCP 配置环境变量 ${{{}}} 未设置，替换为空字符串",
                        var_name
                    );
                }
            }
        } else {
            result.push(c);
        }
    }
    result
}

/// 对 McpServerConfig 中所有字符串字段执行环境变量展开
pub fn expand_server_config(config: &McpServerConfig) -> McpServerConfig {
    McpServerConfig {
        command: config.command.as_ref().map(|s| expand_env_vars(s)),
        args: config
            .args
            .as_ref()
            .map(|arr| arr.iter().map(|s| expand_env_vars(s)).collect()),
        env: config
            .env
            .as_ref()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), expand_env_vars(v))).collect()),
        url: config.url.as_ref().map(|s| expand_env_vars(s)),
        headers: config
            .headers
            .as_ref()
            .map(|map| map.iter().map(|(k, v)| (k.clone(), expand_env_vars(v))).collect()),
    }
}

/// 加载并合并 MCP 配置：全局 settings.json + 项目级 .mcp.json
/// 同名 server 以项目级覆盖全局，所有字段执行 ${VAR} 展开
pub fn load_merged_config(cwd: &Path) -> McpConfigFile {
    // 1. 加载全局配置
    let global_path = dirs_next::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".zen-code")
        .join("settings.json");
    let global = load_global_config(&global_path).unwrap_or_else(|e| {
        tracing::warn!(
            path = %global_path.display(),
            error = %e,
            "加载全局 MCP 配置失败，跳过"
        );
        McpConfigFile::default()
    });

    // 2. 加载项目级配置
    let project_path = cwd.join(".mcp.json");
    let project = load_from_path(&project_path).unwrap_or_else(|e| {
        tracing::warn!(
            path = %project_path.display(),
            error = %e,
            "加载项目级 MCP 配置失败，跳过"
        );
        McpConfigFile::default()
    });

    // 3. 合并：项目级覆盖全局
    let mut merged = global;
    for (name, server_config) in project.mcp_servers {
        merged.mcp_servers.insert(name, server_config);
    }

    // 4. 环境变量展开
    let names: Vec<String> = merged.mcp_servers.keys().cloned().collect();
    for name in names {
        if let Some(server_config) = merged.mcp_servers.get(&name).cloned() {
            merged
                .mcp_servers
                .insert(name, expand_server_config(&server_config));
        }
    }

    merged
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::NamedTempFile;

    #[test]
    fn test_load_from_nonexistent_path() {
        let result = load_from_path(Path::new("/nonexistent/path/file.json"));
        assert!(result.is_ok());
        assert!(result.unwrap().mcp_servers.is_empty());
    }

    #[test]
    fn test_load_from_valid_json() {
        let mut f = NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut f,
            br#"{"mcpServers":{"fs":{"command":"npx","args":["-y","@mcp/filesystem"]}}}"#,
        )
        .unwrap();
        let config = load_from_path(f.path()).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        assert_eq!(config.mcp_servers["fs"].command.as_deref(), Some("npx"));
        assert_eq!(
            config.mcp_servers["fs"].args.as_ref().unwrap().len(),
            2
        );
    }

    #[test]
    fn test_load_from_invalid_json() {
        let mut f = NamedTempFile::new().unwrap();
        std::io::Write::write_all(&mut f, b"{invalid json}").unwrap();
        let result = load_from_path(f.path());
        assert!(matches!(result, Err(McpConfigError::ParseError { .. })));
    }

    #[test]
    fn test_load_global_config() {
        let mut f = NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut f,
            br#"{"config":{"mcpServers":{"gh":{"url":"https://api.github.com"}}}}"#,
        )
        .unwrap();
        let config = load_global_config(f.path()).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        assert_eq!(
            config.mcp_servers["gh"].url.as_deref(),
            Some("https://api.github.com")
        );
    }

    #[test]
    fn test_load_global_config_top_level() {
        let mut f = NamedTempFile::new().unwrap();
        std::io::Write::write_all(
            &mut f,
            br#"{"mcpServers":{"gh":{"command":"npx"}}}"#,
        )
        .unwrap();
        let config = load_global_config(f.path()).unwrap();
        assert_eq!(config.mcp_servers.len(), 1);
        assert_eq!(config.mcp_servers["gh"].command.as_deref(), Some("npx"));
    }

    #[test]
    fn test_expand_env_vars() {
        std::env::set_var("TEST_MCP_VAR", "hello");
        let result = expand_env_vars("prefix_${TEST_MCP_VAR}_suffix");
        assert_eq!(result, "prefix_hello_suffix");
        std::env::remove_var("TEST_MCP_VAR");
    }

    #[test]
    fn test_expand_env_vars_missing() {
        let result = expand_env_vars("${NONEXISTENT_MCP_VAR_12345}");
        assert_eq!(result, "");
    }

    #[test]
    fn test_expand_env_vars_no_braces() {
        let result = expand_env_vars("$NO_BRACE");
        assert_eq!(result, "$NO_BRACE");
    }

    #[test]
    fn test_merge_project_overrides_global() {
        let mut global = McpConfigFile::default();
        global.mcp_servers.insert(
            "fs".to_string(),
            McpServerConfig {
                command: Some("npx".to_string()),
                args: None,
                env: None,
                url: None,
                headers: None,
            },
        );
        let mut project = McpConfigFile::default();
        project.mcp_servers.insert(
            "fs".to_string(),
            McpServerConfig {
                command: Some("uvx".to_string()),
                args: None,
                env: None,
                url: None,
                headers: None,
            },
        );
        let mut merged = global;
        for (name, server_config) in project.mcp_servers {
            merged.mcp_servers.insert(name, server_config);
        }
        assert_eq!(merged.mcp_servers["fs"].command.as_deref(), Some("uvx"));
    }

    #[test]
    fn test_merge_project_adds_new_server() {
        let mut global = McpConfigFile::default();
        global.mcp_servers.insert(
            "fs".to_string(),
            McpServerConfig {
                command: Some("npx".to_string()),
                args: None,
                env: None,
                url: None,
                headers: None,
            },
        );
        let mut project = McpConfigFile::default();
        project.mcp_servers.insert(
            "gh".to_string(),
            McpServerConfig {
                command: None,
                args: None,
                env: None,
                url: Some("https://api.github.com".to_string()),
                headers: None,
            },
        );
        let mut merged = global;
        for (name, server_config) in project.mcp_servers {
            merged.mcp_servers.insert(name, server_config);
        }
        assert_eq!(merged.mcp_servers.len(), 2);
        assert!(merged.mcp_servers.contains_key("fs"));
        assert!(merged.mcp_servers.contains_key("gh"));
    }
}
