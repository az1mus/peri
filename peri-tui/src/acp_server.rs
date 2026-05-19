//! ACP Server — transport-agnostic request handler.
//!
//! Accepts any [`AcpTransport`] implementation (mpsc for TUI, stdio for IDE),
//! builds and executes ReAct agents, and pushes [`SessionUpdate`] notifications
//! back through the transport.
//!
//! **Cancel architecture**: `session/prompt` execution is spawned into a
//! background tokio task so the main server loop remains responsive to
//! `$/cancel_request` notifications. Sessions are shared via
//! `Arc<tokio::sync::Mutex<HashMap>>`.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::Value;
use tracing::{debug, info};

use peri_acp::broker::AcpTransportBroker;
use peri_acp::session::event_sink::TransportEventSink;
use peri_acp::session::executor;
pub use peri_acp::session::state_builders::{
    apply_thinking_effort, build_config_options, build_mode_state, build_model_state,
    parse_permission_mode,
};
use peri_acp::transport::types::{AcpError, IncomingMessage};
use peri_agent::agent::AgentCancellationToken;
use peri_agent::messages::BaseMessage;
use peri_agent::thread::{ThreadId, ThreadMeta};
use peri_middlewares::prelude::*;

use agent_client_protocol::schema::{
    AgentCapabilities, AvailableCommand, AvailableCommandsUpdate, CloseSessionResponse,
    ConfigOptionUpdate, ForkSessionResponse, InitializeResponse, ListSessionsResponse,
    LoadSessionResponse, NewSessionResponse, PromptCapabilities, PromptResponse, ProtocolVersion,
    ResumeSessionResponse, SessionCapabilities, SessionCloseCapabilities, SessionForkCapabilities,
    SessionId, SessionInfo, SessionListCapabilities, SessionNotification,
    SessionResumeCapabilities, SessionUpdate, SetSessionConfigOptionResponse,
    SetSessionModeResponse, SetSessionModelResponse, StopReason,
};

use crate::app::agent::LlmProvider;
use crate::config::PeriConfig;

// ── Session state ────────────────────────────────────────────────────────────

struct SessionState {
    #[allow(dead_code)]
    session_id: String,
    thread_id: String,
    cwd: String,
    history: Vec<BaseMessage>,
    cancel_token: Option<AgentCancellationToken>,
}

// ── Server config ────────────────────────────────────────────────────────────

/// All cross-session configuration needed by the ACP server.
pub struct AcpServerConfig {
    pub provider: Arc<RwLock<LlmProvider>>,
    pub peri_config: Arc<RwLock<PeriConfig>>,
    pub permission_mode: Arc<SharedPermissionMode>,
    pub cron_scheduler: Option<Arc<parking_lot::Mutex<CronScheduler>>>,
    pub mcp_pool: Option<Arc<peri_middlewares::mcp::McpClientPool>>,
    pub plugin_skill_dirs: Vec<std::path::PathBuf>,
    pub plugin_agent_dirs: Vec<std::path::PathBuf>,
    pub plugin_hooks: Vec<peri_middlewares::hooks::RegisteredHook>,
    pub hook_groups: Vec<Vec<peri_middlewares::hooks::RegisteredHook>>,
    pub plugin_lsp_servers: Vec<peri_lsp::config::LspServerConfig>,
    pub tool_search_index: Arc<peri_middlewares::tool_search::ToolSearchIndex>,
    pub shared_tools: Arc<RwLock<HashMap<String, Arc<dyn peri_agent::tools::BaseTool>>>>,
    pub thread_store: Arc<dyn peri_agent::thread::ThreadStore>,
}

// ── Main server loop ────────────────────────────────────────────────────────

type SharedSessions = Arc<tokio::sync::Mutex<HashMap<String, SessionState>>>;

/// Main ACP server loop. Accepts any `AcpTransport` (mpsc for TUI, stdio for IDE).
///
/// `session/prompt` is spawned into a background task so the loop stays
/// responsive to `$/cancel_request` and other incoming messages.
pub async fn run_acp_server(
    transport: Arc<dyn peri_acp::transport::AcpTransport>,
    cfg: AcpServerConfig,
) {
    let sessions: SharedSessions = Arc::new(tokio::sync::Mutex::new(HashMap::new()));

    while let Some(msg) = transport.recv().await {
        match msg {
            IncomingMessage::Request { id, method, params } => {
                if method == "session/prompt" || method == "session/compact" {
                    // Spawn long-running prompt execution so the server loop
                    // continues processing $/cancel_request notifications.
                    let sessions = sessions.clone();
                    let transport = Arc::clone(&transport);
                    let provider = cfg.provider.clone();
                    let peri_config = cfg.peri_config.clone();
                    let permission_mode = cfg.permission_mode.clone();
                    let cron_scheduler = cfg.cron_scheduler.clone();
                    let plugin_skill_dirs = cfg.plugin_skill_dirs.clone();
                    let plugin_agent_dirs = cfg.plugin_agent_dirs.clone();
                    let hook_groups = cfg.hook_groups.clone();
                    let mcp_pool = cfg.mcp_pool.clone();
                    let tool_search_index = cfg.tool_search_index.clone();
                    let shared_tools = cfg.shared_tools.clone();
                    let plugin_lsp_servers = cfg.plugin_lsp_servers.clone();
                    let thread_store = cfg.thread_store.clone();
                    let method_clone = method.clone();
                    let prompt_session_id = extract_session_id(&params, "").to_string();
                    tokio::spawn(async move {
                        let result = if method_clone == "session/compact" {
                            execute_compact(
                                params,
                                &sessions,
                                &provider,
                                &peri_config,
                                &hook_groups,
                                &transport,
                                &thread_store,
                            )
                            .await
                        } else {
                            execute_prompt(
                                params,
                                &sessions,
                                &provider,
                                &peri_config,
                                &permission_mode,
                                cron_scheduler,
                                &plugin_skill_dirs,
                                &plugin_agent_dirs,
                                &hook_groups,
                                mcp_pool,
                                tool_search_index,
                                shared_tools,
                                &plugin_lsp_servers,
                                &transport,
                                &thread_store,
                            )
                            .await
                        };
                        let _ = transport.send_response(id, result).await;
                        // Send SessionInfoUpdate after prompt/compact completes
                        if !prompt_session_id.is_empty() {
                            send_session_info_update(transport.as_ref(), &prompt_session_id).await;
                        }
                    });
                } else {
                    let mut sessions = sessions.lock().await;
                    let result =
                        handle_request(&method, &params, &cfg, &mut sessions, transport.as_ref())
                            .await;
                    let _ = transport.send_response(id, result).await;
                }
            }
            IncomingMessage::Notification { method, params } => {
                let sessions = sessions.lock().await;
                handle_notification(&method, &params, &sessions);
            }
            IncomingMessage::Response { .. } => {
                // Responses are routed internally by the transport's pending map.
            }
        }
    }
}

// ── Request dispatch (quick handlers only) ───────────────────────────────────

async fn handle_request(
    method: &str,
    params: &Value,
    cfg: &AcpServerConfig,
    sessions: &mut HashMap<String, SessionState>,
    transport: &dyn peri_acp::transport::AcpTransport,
) -> Result<Value, AcpError> {
    match method {
        "initialize" => {
            let version = params
                .get("protocolVersion")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            info!(protocol_version = %version, "ACP initialize");
            let caps = AgentCapabilities::new()
                .load_session(true)
                .prompt_capabilities(PromptCapabilities::new())
                .session_capabilities(
                    SessionCapabilities::new()
                        .list(SessionListCapabilities::new())
                        .close(SessionCloseCapabilities::new())
                        .resume(SessionResumeCapabilities::new())
                        .fork(SessionForkCapabilities::new()),
                );
            let resp = InitializeResponse::new(ProtocolVersion::V1).agent_capabilities(caps);
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/new" => {
            let cwd = params
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            let meta = ThreadMeta::new(&cwd);
            let thread_id = cfg
                .thread_store
                .create_thread(meta)
                .await
                .map_err(|e| AcpError::new(-32603, format!("Thread creation failed: {e}")))?;
            let session_id = thread_id.clone();
            sessions.insert(
                session_id.clone(),
                SessionState {
                    session_id: session_id.clone(),
                    thread_id: thread_id.clone(),
                    cwd,
                    history: Vec::new(),
                    cancel_token: None,
                },
            );
            info!(session_id = %session_id, "ACP session created with ThreadStore");
            let modes = build_mode_state(&cfg.permission_mode);
            let models = {
                let p = cfg.provider.read();
                let c = cfg.peri_config.read();
                build_model_state(&p, &c)
            };
            let config_options = {
                let c = cfg.peri_config.read();
                let p = cfg.provider.read();
                build_config_options(&c, &p, cfg.permission_mode.load())
            };
            let resp = NewSessionResponse::new(SessionId::new(&*session_id))
                .modes(modes)
                .models(models)
                .config_options(config_options);
            send_available_commands_update(transport, &session_id).await;
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/set_model" => {
            let model_id = params.get("modelId").and_then(|v| v.as_str()).unwrap_or("");
            let session_id = extract_session_id(params, "");
            let new_provider = {
                let cfg = cfg.peri_config.read();
                LlmProvider::from_config_for_alias(&cfg, model_id)
            };
            if let Some(new_provider) = new_provider {
                info!(model_id = %model_id, model = %new_provider.model_name(), "Model changed");
                *cfg.provider.write() = new_provider;
            }
            let resp = SetSessionModelResponse::new();
            send_config_option_update(transport, session_id, cfg).await;
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/set_mode" => {
            let mode_id = params
                .get("modeId")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            let session_id = extract_session_id(params, "");
            let mode = parse_permission_mode(mode_id);
            cfg.permission_mode.store(mode);
            info!(mode_id = %mode_id, "Permission mode changed");
            let resp = SetSessionModeResponse::new();
            send_config_option_update(transport, session_id, cfg).await;
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/set_config_option" => {
            let config_id = params
                .get("configId")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let session_id = extract_session_id(params, "");
            let value = params.get("value").and_then(|v| v.as_str()).unwrap_or("");
            match config_id {
                "mode" => {
                    let mode = parse_permission_mode(value);
                    cfg.permission_mode.store(mode);
                    info!(mode = %value, "Permission mode changed via configOption");
                }
                "model" => {
                    let new_provider = {
                        let c = cfg.peri_config.read();
                        LlmProvider::from_config_for_alias(&c, value)
                    };
                    if let Some(new_provider) = new_provider {
                        info!(model_id = %value, model = %new_provider.model_name(), "Model changed via configOption");
                        *cfg.provider.write() = new_provider;
                    }
                }
                "thinking_effort" => {
                    apply_thinking_effort(&cfg.peri_config, value);
                    info!(effort = %value, "Thinking effort changed via configOption");
                }
                _ => {
                    debug!(config_id = %config_id, "Unknown config option");
                }
            }
            let config_options = {
                let c = cfg.peri_config.read();
                let p = cfg.provider.read();
                build_config_options(&c, &p, cfg.permission_mode.load())
            };
            let resp = SetSessionConfigOptionResponse::new(config_options);
            send_config_option_update(transport, session_id, cfg).await;
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/set_thinking" => {
            let effort = params
                .get("effort")
                .and_then(|v| v.as_str())
                .unwrap_or("medium");
            let enabled = params
                .get("enabled")
                .and_then(|v| v.as_bool())
                .unwrap_or(true);
            let session_id = extract_session_id(params, "");
            apply_thinking_effort(&cfg.peri_config, effort);
            {
                let mut cfg_guard = cfg.peri_config.write();
                if let Some(ref mut thinking) = cfg_guard.config.thinking {
                    thinking.enabled = enabled;
                }
            }
            info!(effort = %effort, enabled = %enabled, "Thinking config changed");
            let config_options = {
                let c = cfg.peri_config.read();
                let p = cfg.provider.read();
                build_config_options(&c, &p, cfg.permission_mode.load())
            };
            let resp = SetSessionConfigOptionResponse::new(config_options);
            send_config_option_update(transport, session_id, cfg).await;
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/load" => {
            let req_session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AcpError::new(-32602, "missing sessionId"))?;
            let cwd = params.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");

            // Load history from ThreadStore
            let history = match cfg
                .thread_store
                .load_messages(&ThreadId::from(req_session_id.to_string()))
                .await
            {
                Ok(msgs) => msgs,
                Err(e) => {
                    tracing::warn!(session_id = %req_session_id, error = %e, "session/load: thread not found, creating empty session");
                    Vec::new()
                }
            };

            // Insert into sessions if not already present
            if let Some(state) = sessions.get_mut(req_session_id) {
                if state.history.is_empty() {
                    state.history = history;
                }
            } else {
                sessions.insert(
                    req_session_id.to_string(),
                    SessionState {
                        session_id: req_session_id.to_string(),
                        thread_id: req_session_id.to_string(),
                        cwd: cwd.to_string(),
                        history,
                        cancel_token: None,
                    },
                );
            }

            let modes = build_mode_state(&cfg.permission_mode);
            let models = {
                let p = cfg.provider.read();
                let c = cfg.peri_config.read();
                build_model_state(&p, &c)
            };
            let config_options = {
                let c = cfg.peri_config.read();
                let p = cfg.provider.read();
                build_config_options(&c, &p, cfg.permission_mode.load())
            };
            let resp = LoadSessionResponse::new()
                .modes(modes)
                .models(models)
                .config_options(config_options);
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/list" => {
            let threads = cfg
                .thread_store
                .list_threads()
                .await
                .map_err(|e| AcpError::new(-32603, format!("Failed to list sessions: {e}")))?;

            let cwd_filter = params.get("cwd").and_then(|v| v.as_str());

            let entries: Vec<SessionInfo> = threads
                .into_iter()
                .filter(|t| {
                    if let Some(cwd) = cwd_filter {
                        t.cwd == cwd
                    } else {
                        true
                    }
                })
                .map(|t| {
                    SessionInfo::new(
                        SessionId::new(t.id.clone()),
                        std::path::PathBuf::from(t.cwd.clone()),
                    )
                    .title(t.title.clone())
                    .updated_at(t.updated_at.to_rfc3339())
                })
                .collect();

            let resp = ListSessionsResponse::new(entries);
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/close" => {
            let req_session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AcpError::new(-32602, "missing sessionId"))?;

            if let Some(state) = sessions.remove(req_session_id) {
                if let Some(ref token) = state.cancel_token {
                    token.cancel();
                }
                info!(session_id = %req_session_id, "Session closed");
            }
            let resp = CloseSessionResponse::new();
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/resume" => {
            let req_session_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AcpError::new(-32602, "missing sessionId"))?;
            let cwd = params.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");

            if !sessions.contains_key(req_session_id) {
                sessions.insert(
                    req_session_id.to_string(),
                    SessionState {
                        session_id: req_session_id.to_string(),
                        thread_id: req_session_id.to_string(),
                        cwd: cwd.to_string(),
                        history: Vec::new(),
                        cancel_token: None,
                    },
                );
                info!(session_id = %req_session_id, "Session resumed (new)");
            } else {
                info!(session_id = %req_session_id, "Session resumed (existing)");
            }

            let resp = ResumeSessionResponse::new();
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/fork" => {
            let source_id = params
                .get("sessionId")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AcpError::new(-32602, "missing sessionId"))?;
            let cwd = params.get("cwd").and_then(|v| v.as_str()).unwrap_or(".");

            let source_history = sessions
                .get(source_id)
                .map(|s| s.history.clone())
                .ok_or_else(|| {
                    AcpError::new(-32602, format!("source session not found: {source_id}"))
                })?;

            let meta = ThreadMeta::new(cwd);
            let new_thread_id = cfg
                .thread_store
                .create_thread(meta)
                .await
                .map_err(|e| AcpError::new(-32603, format!("Thread creation failed: {e}")))?;

            if !source_history.is_empty() {
                if let Err(e) = cfg
                    .thread_store
                    .append_messages(&new_thread_id, &source_history)
                    .await
                {
                    tracing::warn!(error = %e, "session/fork: failed to copy messages to new thread");
                }
            }

            let new_session_id = new_thread_id.clone();
            sessions.insert(
                new_session_id.clone(),
                SessionState {
                    session_id: new_session_id.clone(),
                    thread_id: new_thread_id.clone(),
                    cwd: cwd.to_string(),
                    history: source_history,
                    cancel_token: None,
                },
            );

            info!(source = %source_id, new = %new_session_id, "Session forked");
            let resp = ForkSessionResponse::new(SessionId::new(new_session_id));
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        _ => Err(AcpError::new(-32601, format!("Method not found: {method}"))),
    }
}

// ── Notification dispatch ────────────────────────────────────────────────────

fn handle_notification(method: &str, params: &Value, sessions: &HashMap<String, SessionState>) {
    if method == "$/cancel_request" {
        let session_id = params
            .get("sessionId")
            .or_else(|| params.get("session_id"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let Some(state) = sessions.get(session_id) {
            if let Some(ref token) = state.cancel_token {
                token.cancel();
                info!(session_id = %session_id, "Cancel requested");
            }
        }
    } else {
        debug!(method = %method, "Unhandled notification");
    }
}

// ── Notification helpers ───────────────────────────────────────────────────────

/// Extract `sessionId` from JSON-RPC params, returning `default_value` if absent.
fn extract_session_id<'a>(params: &'a Value, default_value: &'a str) -> &'a str {
    params
        .get("sessionId")
        .or_else(|| params.get("session_id"))
        .and_then(|v| v.as_str())
        .unwrap_or(default_value)
}

/// Build the current set of config options and push a `ConfigOptionUpdate` notification.
async fn send_config_option_update(
    transport: &dyn peri_acp::transport::AcpTransport,
    session_id: &str,
    cfg: &AcpServerConfig,
) {
    if session_id.is_empty() {
        return;
    }
    let config_options = {
        let c = cfg.peri_config.read();
        let p = cfg.provider.read();
        build_config_options(&c, &p, cfg.permission_mode.load())
    };
    let update = SessionUpdate::ConfigOptionUpdate(ConfigOptionUpdate::new(config_options));
    let notif = SessionNotification::new(SessionId::new(session_id.to_string()), update);
    let payload = match serde_json::to_value(&notif) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "Failed to serialize ConfigOptionUpdate notification");
            return;
        }
    };
    let _ = transport.send_notification("session/update", payload).await;
}

/// Push an `AvailableCommandsUpdate` notification for the given session.
async fn send_available_commands_update(
    transport: &dyn peri_acp::transport::AcpTransport,
    session_id: &str,
) {
    if session_id.is_empty() {
        return;
    }
    let commands = build_available_commands();
    let update = SessionUpdate::AvailableCommandsUpdate(AvailableCommandsUpdate::new(commands));
    let notif = SessionNotification::new(SessionId::new(session_id.to_string()), update);
    let payload = match serde_json::to_value(&notif) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "Failed to serialize AvailableCommandsUpdate notification");
            return;
        }
    };
    let _ = transport.send_notification("session/update", payload).await;
}

/// Push a `SessionInfoUpdate` notification after prompt/compact completes.
async fn send_session_info_update(
    transport: &dyn peri_acp::transport::AcpTransport,
    session_id: &str,
) {
    use agent_client_protocol::schema::SessionInfoUpdate;
    let info = SessionInfoUpdate::new().updated_at(chrono::Utc::now().to_rfc3339());
    let update = SessionUpdate::SessionInfoUpdate(info);
    let notif = SessionNotification::new(SessionId::new(session_id.to_string()), update);
    let payload = match serde_json::to_value(&notif) {
        Ok(p) => p,
        Err(e) => {
            tracing::error!(error = %e, "Failed to serialize SessionInfoUpdate notification");
            return;
        }
    };
    let _ = transport.send_notification("session/update", payload).await;
}

/// Build the list of available slash commands for ACP clients.
fn build_available_commands() -> Vec<AvailableCommand> {
    vec![
        AvailableCommand::new("help", "Show available commands and their descriptions"),
        AvailableCommand::new("clear", "Clear the current conversation"),
        AvailableCommand::new(
            "compact",
            "Compress the conversation history to save context",
        ),
        AvailableCommand::new("context", "Display context usage / token statistics"),
        AvailableCommand::new("cost", "Show token usage and estimated cost"),
        AvailableCommand::new("model", "Switch the current LLM model"),
        AvailableCommand::new("mode", "Switch the current permission mode"),
        AvailableCommand::new("effort", "Configure LLM reasoning/thinking effort"),
        AvailableCommand::new("loop", "Control agent iteration loop"),
        AvailableCommand::new("history", "View and resume previous conversations"),
        AvailableCommand::new("doctor", "Diagnose configuration and connection issues"),
        AvailableCommand::new("mcp", "Manage MCP (Model Context Protocol) servers"),
        AvailableCommand::new("hooks", "Manage Claude Code hooks"),
        AvailableCommand::new("plugin", "Manage installed plugins"),
        AvailableCommand::new("cron", "Manage scheduled/cron tasks"),
        AvailableCommand::new("agents", "Manage sub-agent definitions"),
        AvailableCommand::new("memory", "Manage persistent memory entries"),
        AvailableCommand::new("login", "Configure authentication"),
        AvailableCommand::new("split", "Manage split session layouts"),
        AvailableCommand::new("rename", "Rename the current session"),
        AvailableCommand::new("lang", "Switch display language / locale"),
        AvailableCommand::new("exit", "Exit the application"),
    ]
}

// ── Prompt execution (spawned into background task) ──────────────────────────

#[allow(clippy::too_many_arguments)]
async fn execute_prompt(
    params: Value,
    sessions: &SharedSessions,
    provider: &Arc<RwLock<LlmProvider>>,
    peri_config: &Arc<RwLock<PeriConfig>>,
    permission_mode: &Arc<SharedPermissionMode>,
    cron_scheduler: Option<Arc<parking_lot::Mutex<CronScheduler>>>,
    plugin_skill_dirs: &[std::path::PathBuf],
    plugin_agent_dirs: &[std::path::PathBuf],
    hook_groups: &[Vec<peri_middlewares::hooks::RegisteredHook>],
    mcp_pool: Option<Arc<peri_middlewares::mcp::McpClientPool>>,
    tool_search_index: Arc<peri_middlewares::tool_search::ToolSearchIndex>,
    shared_tools: Arc<RwLock<HashMap<String, Arc<dyn peri_agent::tools::BaseTool>>>>,
    plugin_lsp_servers: &[peri_lsp::config::LspServerConfig],
    transport: &Arc<dyn peri_acp::transport::AcpTransport>,
    thread_store: &Arc<dyn peri_agent::thread::ThreadStore>,
) -> Result<Value, AcpError> {
    let session_id = params
        .get("sessionId")
        .or_else(|| params.get("session_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AcpError::new(-32602, "missing sessionId"))?
        .to_string();
    let message = params
        .get("message")
        .ok_or_else(|| AcpError::new(-32602, "missing message"))?;
    let content = message
        .get("content")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Create cancel token and register in sessions.
    let cancel = AgentCancellationToken::new();
    {
        let mut sessions = sessions.lock().await;
        let state = sessions
            .get_mut(&session_id)
            .ok_or_else(|| AcpError::new(-32602, "session not found"))?;
        state.cancel_token = Some(cancel.clone());
    }

    // Read session data under lock, then release immediately.
    let (cwd, history, is_empty, thread_id) = {
        let sessions = sessions.lock().await;
        let state = sessions
            .get(&session_id)
            .ok_or_else(|| AcpError::new(-32602, "session not found"))?;
        (
            state.cwd.clone(),
            state.history.clone(),
            state.history.is_empty(),
            state.thread_id.clone(),
        )
    };
    let history_len = history.len();

    let broker: Arc<dyn peri_agent::interaction::UserInteractionBroker> = Arc::new(
        AcpTransportBroker::new(Arc::clone(transport), session_id.clone().into()),
    );
    let event_sink = Arc::new(TransportEventSink::new(Arc::clone(transport)));

    let provider_snapshot = provider.read().clone();
    let peri_config_snapshot = Arc::new(peri_config.read().clone());

    let result = executor::execute_prompt(
        &provider_snapshot,
        peri_config_snapshot,
        &cwd,
        content,
        history,
        is_empty,
        permission_mode.clone(),
        event_sink,
        cancel,
        broker,
        plugin_skill_dirs.to_vec(),
        plugin_agent_dirs.to_vec(),
        hook_groups.to_vec(),
        cron_scheduler,
        session_id.clone(),
        mcp_pool,
        tool_search_index,
        shared_tools,
        plugin_lsp_servers.to_vec(),
    )
    .await;

    // Persist new messages to ThreadStore and update in-memory state.
    {
        let mut sessions = sessions.lock().await;
        if let Some(state) = sessions.get_mut(&session_id) {
            if result.ok {
                info!(session_id = %session_id, messages = result.messages.len(), "Agent execution completed");
                // Persist only the newly added messages.
                if history_len < result.messages.len() {
                    let new_msgs = &result.messages[history_len..];
                    if let Err(e) = thread_store.append_messages(&thread_id, new_msgs).await {
                        tracing::warn!(error = %e, "Failed to persist messages to ThreadStore");
                    }
                }
            }
            state.history = result.messages;
            state.cancel_token = None;
        }
    }

    let acp_stop_reason = match result.stop_reason {
        executor::PromptStopReason::Cancelled => StopReason::Cancelled,
        executor::PromptStopReason::MaxTurnRequests => StopReason::MaxTurnRequests,
        executor::PromptStopReason::EndTurn => StopReason::EndTurn,
    };
    let resp = PromptResponse::new(acp_stop_reason);
    serde_json::to_value(resp).map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
}

// ── Manual compact (spawned into background task) ────────────────────────────

#[allow(clippy::too_many_arguments)]
async fn execute_compact(
    params: Value,
    sessions: &SharedSessions,
    provider: &Arc<RwLock<LlmProvider>>,
    peri_config: &Arc<RwLock<PeriConfig>>,
    hook_groups: &[Vec<peri_middlewares::hooks::RegisteredHook>],
    transport: &Arc<dyn peri_acp::transport::AcpTransport>,
    thread_store: &Arc<dyn peri_agent::thread::ThreadStore>,
) -> Result<Value, AcpError> {
    use peri_acp::session::compact_runner::{self, HookContext};
    use peri_acp::session::event_sink::EventSink;
    use peri_agent::agent::events::AgentEvent as ExecutorEvent;

    let session_id = params
        .get("sessionId")
        .or_else(|| params.get("session_id"))
        .and_then(|v| v.as_str())
        .ok_or_else(|| AcpError::new(-32602, "missing sessionId"))?
        .to_string();
    let instructions = params
        .get("instructions")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();

    // Read session data under lock.
    let (cwd, history, _thread_id) = {
        let sessions = sessions.lock().await;
        let state = sessions
            .get(&session_id)
            .ok_or_else(|| AcpError::new(-32602, "session not found"))?;
        (
            state.cwd.clone(),
            state.history.clone(),
            state.thread_id.clone(),
        )
    };

    if history.is_empty() {
        return Err(AcpError::new(-32602, "no messages to compact"));
    }

    let cancel = AgentCancellationToken::new();
    {
        let mut sessions = sessions.lock().await;
        if let Some(state) = sessions.get_mut(&session_id) {
            state.cancel_token = Some(cancel.clone());
        }
    }

    // Event channel + pump for compact events.
    let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutorEvent>();
    let event_tx = Arc::new(std::sync::Mutex::new(Some(event_tx)));
    let event_sink: Arc<dyn EventSink> = Arc::new(TransportEventSink::new(Arc::clone(transport)));
    let sink = event_sink.clone();
    let sid = session_id.clone();
    let (pump_done_tx, pump_done_rx) = tokio::sync::oneshot::channel();
    tokio::spawn(async move {
        while let Some(exec_event) = event_rx.recv().await {
            sink.push_event(&sid, &exec_event, 0).await;
        }
        sink.push_done(&sid).await;
        let _ = pump_done_tx.send(());
    });

    let provider_snapshot = provider.read().clone();
    let peri_config_snapshot = peri_config.read().clone();
    let mut compact_config = peri_config_snapshot
        .config
        .compact
        .clone()
        .unwrap_or_default();
    compact_config.apply_env_overrides();

    let all_hooks: Vec<_> = hook_groups.iter().flatten().cloned().collect();
    let hook_ctx = HookContext {
        cwd: cwd.clone(),
        session_id: session_id.clone(),
        transcript_path: String::new(),
        provider_name: provider_snapshot.display_name().to_string(),
        instructions,
    };

    let result = compact_runner::run_full_compact(
        &history,
        provider_snapshot.into_model().as_ref(),
        &compact_config,
        &cwd,
        &event_tx,
        &cancel,
        &all_hooks,
        &hook_ctx,
    )
    .await;

    // Close event channel and wait for pump.
    {
        let mut tx_guard = event_tx.lock().unwrap();
        *tx_guard = None;
    }
    let _ = pump_done_rx.await;

    match result {
        Ok(output) => {
            // Update in-memory session history with compacted messages.
            let new_messages = output.new_messages;
            {
                let mut sessions = sessions.lock().await;
                if let Some(state) = sessions.get_mut(&session_id) {
                    state.history = new_messages.clone();
                    state.cancel_token = None;
                }
            }
            // Persist compacted messages as a new thread.
            let mut meta = ThreadMeta::new(&cwd);
            let truncated: String = output.summary.chars().take(30).collect();
            meta.title = Some(format!("Compact: {}…", truncated));
            match thread_store.create_thread(meta).await {
                Ok(new_tid) => {
                    if let Err(e) = thread_store.append_messages(&new_tid, &new_messages).await {
                        tracing::warn!(error = %e, "compact: failed to persist messages");
                    }
                    info!(session_id = %session_id, new_thread = %new_tid, "Manual compact completed");
                }
                Err(e) => {
                    tracing::warn!(error = %e, "compact: failed to create thread");
                }
            }
            serde_json::to_value(serde_json::json!({ "status": "ok" }))
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }
        Err(e) => {
            {
                let mut sessions = sessions.lock().await;
                if let Some(state) = sessions.get_mut(&session_id) {
                    state.cancel_token = None;
                }
            }
            Err(AcpError::new(-32603, format!("Compact failed: {e}")))
        }
    }
}

// ── ACP standard state builders (re-exported from peri-acp) ──────────────────

// Re-exports live at the top of this file as `pub use peri_acp::session::state_builders::*`.
