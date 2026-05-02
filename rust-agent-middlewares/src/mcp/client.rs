use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

use rmcp::model::{Resource, Tool};
use rmcp::service::{Peer, RoleClient, RunningService};

use super::transport::TransportConfig;

/// MCP 客户端连接状态
#[derive(Debug, Clone, PartialEq)]
pub enum ClientStatus {
    Connected,
    Failed(String),
    Disconnected,
}

/// 连接池级别错误
#[derive(Debug, Error)]
pub enum McpPoolError {
    #[error("MCP 服务器 \"{server}\" 连接失败: {reason}")]
    ConnectionFailed { server: String, reason: String },
    #[error("MCP 服务器 \"{server}\" 工具发现失败: {reason}")]
    ToolDiscoveryFailed { server: String, reason: String },
    #[error("MCP 服务器 \"{server}\" 未连接 (状态: {status:?})")]
    NotConnected { server: String, status: ClientStatus },
    #[error("MCP 服务器 \"{server}\" 调用超时")]
    CallTimeout { server: String },
}

/// 单个 MCP 服务器的客户端句柄
pub struct McpClientHandle {
    pub name: String,
    /// None 表示未连接（Failed/Disconnected 状态）
    pub peer: Option<Peer<RoleClient>>,
    pub tools: Vec<Tool>,
    pub resources: Vec<Resource>,
    pub status: ClientStatus,
}

/// MCP 客户端连接池
pub struct McpClientPool {
    clients: HashMap<String, Arc<McpClientHandle>>,
    services: Vec<RunningService<RoleClient, ()>>,
}

const STDIO_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);
const HTTP_CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
const SHUTDOWN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl McpClientPool {
    /// 一次性初始化所有 MCP 服务器连接
    pub async fn initialize(cwd: &Path) -> Self {
        let config = super::load_merged_config(cwd);
        let mut pool = Self {
            clients: HashMap::new(),
            services: Vec::new(),
        };

        for (name, server_config) in &config.mcp_servers {
            let transport_config = match super::transport::TransportConfig::try_from(server_config)
            {
                Ok(tc) => tc,
                Err(e) => {
                    tracing::warn!(server = %name, error = %e, "MCP 服务器传输层构建失败，跳过");
                    pool.insert_failed(name, format!("传输层构建失败: {e}"));
                    continue;
                }
            };

            let timeout = if matches!(transport_config, TransportConfig::StreamableHttp { .. }) {
                HTTP_CONNECT_TIMEOUT
            } else {
                STDIO_CONNECT_TIMEOUT
            };

            let connect_result = match transport_config {
                TransportConfig::Stdio {
                    ref command,
                    ref args,
                    ref env,
                } => {
                    let child_result = spawn_stdio_transport(command, args, env);
                    match child_result {
                        Ok(transport) => {
                            tokio::time::timeout(
                                timeout,
                                rmcp::service::serve_client((), transport),
                            )
                            .await
                        }
                        Err(e) => {
                            tracing::warn!(server = %name, error = %e, "MCP stdio 子进程启动失败");
                            pool.insert_failed(name, format!("stdio 子进程启动失败: {e}"));
                            continue;
                        }
                    }
                }
                TransportConfig::StreamableHttp {
                    ref url,
                    ref headers,
                } => {
                    let transport = build_http_transport(url, headers);
                    tokio::time::timeout(timeout, rmcp::service::serve_client((), transport)).await
                }
            };

            match connect_result {
                Ok(Ok(running_service)) => {
                    let tools = match running_service.list_all_tools().await {
                        Ok(t) => t,
                        Err(e) => {
                            tracing::warn!(server = %name, error = %e, "MCP 服务器工具发现失败");
                            vec![]
                        }
                    };
                    let resources = match running_service.list_all_resources().await {
                        Ok(r) => r,
                        Err(e) => {
                            tracing::warn!(server = %name, error = %e, "MCP 服务器资源发现失败");
                            vec![]
                        }
                    };

                    tracing::info!(
                        server = %name,
                        tools_count = tools.len(),
                        resources_count = resources.len(),
                        "MCP 服务器连接成功"
                    );

                    let peer = running_service.peer().clone();
                    let handle = Arc::new(McpClientHandle {
                        name: name.clone(),
                        peer: Some(peer),
                        tools,
                        resources,
                        status: ClientStatus::Connected,
                    });
                    pool.clients.insert(name.clone(), handle);
                    pool.services.push(running_service);
                }
                Ok(Err(e)) => {
                    tracing::warn!(server = %name, error = %e, "MCP 服务器连接失败，跳过");
                    pool.insert_failed(name, e.to_string());
                }
                Err(_) => {
                    tracing::warn!(server = %name, timeout_secs = timeout.as_secs(), "MCP 服务器连接超时，跳过");
                    pool.insert_failed(name, format!("连接超时 ({}s)", timeout.as_secs()));
                }
            }
        }

        pool
    }

    fn insert_failed(&mut self, name: &str, reason: String) {
        self.clients.insert(
            name.to_string(),
            Arc::new(McpClientHandle {
                name: name.to_string(),
                peer: None,
                tools: vec![],
                resources: vec![],
                status: ClientStatus::Failed(reason),
            }),
        );
    }

    /// 创建空连接池（用于测试）
    #[cfg(test)]
    pub fn new_empty() -> Self {
        Self {
            clients: HashMap::new(),
            services: Vec::new(),
        }
    }

    /// 获取指定名称的客户端句柄
    pub fn get_client(&self, name: &str) -> Option<&Arc<McpClientHandle>> {
        self.clients.get(name)
    }

    /// 获取所有已连接的客户端句柄
    pub fn get_all_clients(&self) -> Vec<&Arc<McpClientHandle>> {
        self.clients
            .values()
            .filter(|c| matches!(c.status, ClientStatus::Connected))
            .collect()
    }

    /// 判断是否有任何已连接的 server 提供资源
    pub fn has_resources(&self) -> bool {
        self.clients.values().any(|c| {
            matches!(c.status, ClientStatus::Connected) && !c.resources.is_empty()
        })
    }

    /// 获取所有已连接 server 的资源摘要
    pub fn resource_summary(&self) -> String {
        let mut lines = Vec::new();
        for client in self.clients.values() {
            if matches!(client.status, ClientStatus::Connected) && !client.resources.is_empty() {
                lines.push(format!(
                    "- server \"{}\": {} ({} resources)",
                    client.name,
                    client
                        .resources
                        .iter()
                        .map(|r| r.raw.uri.clone())
                        .collect::<Vec<_>>()
                        .join(", "),
                    client.resources.len()
                ));
            }
        }
        lines.join("\n")
    }

    /// 关闭所有 MCP 服务器连接
    pub async fn shutdown(&mut self) {
        // 先记录关闭日志并更新状态
        let names: Vec<String> = self.clients.keys().cloned().collect();
        for name in &names {
            if let Some(client) = Arc::get_mut(self.clients.get_mut(name).unwrap()) {
                if matches!(client.status, ClientStatus::Connected) {
                    tracing::info!(server = %name, "关闭 MCP 服务器连接");
                }
                client.status = ClientStatus::Disconnected;
                client.peer = None;
            }
        }
        for service in &mut self.services {
            match service.close_with_timeout(SHUTDOWN_TIMEOUT).await {
                Ok(Some(reason)) => tracing::debug!(?reason, "MCP 连接已关闭"),
                Ok(None) => tracing::warn!("MCP 连接关闭超时"),
                Err(e) => tracing::warn!(error = %e, "MCP 连接关闭异常"),
            }
        }
        self.services.clear();
    }
}

/// 创建 stdio transport（使用 tokio::process::Command）
fn spawn_stdio_transport(
    command: &str,
    args: &[String],
    env: &HashMap<String, String>,
) -> std::io::Result<rmcp::transport::child_process::TokioChildProcess> {
    let mut child = tokio::process::Command::new(command);
    child.args(args);
    child.envs(env);
    child.stdin(std::process::Stdio::piped());
    child.stdout(std::process::Stdio::piped());
    child.stderr(std::process::Stdio::piped());
    rmcp::transport::child_process::TokioChildProcess::new(child)
}

/// 创建 HTTP transport，将自定义 headers（如 Authorization）注入 transport config
fn build_http_transport(
    url: &str,
    headers: &HashMap<String, String>,
) -> rmcp::transport::StreamableHttpClientTransport<reqwest::Client> {
    use rmcp::transport::streamable_http_client::StreamableHttpClientTransportConfig;

    let mut config = StreamableHttpClientTransportConfig::with_uri(url);

    let mut custom_headers = std::collections::HashMap::new();
    for (key, value) in headers {
        match reqwest::header::HeaderName::try_from(key.as_str()) {
            Ok(name) => match reqwest::header::HeaderValue::from_str(value) {
                Ok(val) => {
                    custom_headers.insert(name, val);
                }
                Err(e) => {
                    tracing::warn!(header = %key, error = %e, "MCP HTTP header 值无效，跳过");
                }
            },
            Err(e) => {
                tracing::warn!(header = %key, error = %e, "MCP HTTP header 名称无效，跳过");
            }
        }
    }

    if !custom_headers.is_empty() {
        config = config.custom_headers(custom_headers);
    }

    let client = reqwest::Client::new();
    rmcp::transport::StreamableHttpClientTransport::with_client(client, config)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pool_get_all_clients_filters_disconnected() {
        let pool = McpClientPool::new_empty();
        assert!(pool.get_all_clients().is_empty());
    }

    #[test]
    fn test_pool_has_no_resources() {
        let pool = McpClientPool::new_empty();
        assert!(!pool.has_resources());
    }

    #[test]
    fn test_resource_summary_empty() {
        let pool = McpClientPool::new_empty();
        assert!(pool.resource_summary().is_empty());
    }

    #[test]
    fn test_client_status_equality() {
        assert_eq!(ClientStatus::Connected, ClientStatus::Connected);
        assert_ne!(
            ClientStatus::Failed("a".to_string()),
            ClientStatus::Failed("b".to_string())
        );
        assert_ne!(ClientStatus::Connected, ClientStatus::Disconnected);
    }
}
