//! ACP Server — transport-agnostic request handler.
//!
//! Accepts any [`AcpTransport`] implementation (mpsc for TUI, stdio for IDE),
//! builds and executes ReAct agents, and pushes [`SessionUpdate`] notifications
//! back through the transport.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::RwLock;
use serde_json::{json, Value};
use tracing::{error, info, warn};

use peri_acp::broker::AcpTransportBroker;
use peri_acp::event::{map_executor_to_peri_notifications, map_executor_to_updates};
use peri_acp::prompt::{build_system_prompt, PromptFeatures};
use peri_acp::transport::types::{AcpError, IncomingMessage};
use peri_agent::agent::events::{AgentEvent as ExecutorEvent, AgentEventHandler, FnEventHandler};
use peri_agent::agent::react::AgentInput;
use peri_agent::agent::state::AgentState;
use peri_agent::agent::AgentCancellationToken;
use peri_agent::messages::BaseMessage;
use peri_middlewares::prelude::*;

use agent_client_protocol::schema::{
    AgentCapabilities, InitializeResponse, NewSessionResponse, PromptResponse, ProtocolVersion,
    SessionId, StopReason,
};

use crate::app::agent::LlmProvider;
use crate::config::PeriConfig;

// ── Session state ────────────────────────────────────────────────────────────

struct SessionState {
    #[allow(dead_code)]
    session_id: String,
    cwd: String,
    history: Vec<BaseMessage>,
    cancel_token: Option<AgentCancellationToken>,
}

// ── Server config ────────────────────────────────────────────────────────────

/// All cross-session configuration needed by the ACP server.
pub struct AcpServerConfig {
    pub provider: LlmProvider,
    pub peri_config: Arc<PeriConfig>,
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

/// Main ACP server loop. Accepts any `AcpTransport` (mpsc for TUI, stdio for IDE).
pub async fn run_acp_server(
    transport: Arc<dyn peri_acp::transport::AcpTransport>,
    cfg: AcpServerConfig,
) {
    let mut sessions: HashMap<String, SessionState> = HashMap::new();
    let mut session_counter: u64 = 0;

    while let Some(msg) = transport.recv().await {
        match msg {
            IncomingMessage::Request { id, method, params } => {
                let result = handle_request(
                    &method,
                    &params,
                    &cfg,
                    &mut sessions,
                    &mut session_counter,
                    &transport,
                )
                .await;
                match result {
                    Ok(value) => {
                        let _ = transport.send_response(id, Ok(value)).await;
                    }
                    Err(e) => {
                        error!(method = %method, error = %e.message, "ACP request failed");
                        let _ = transport.send_response(id, Err(e)).await;
                    }
                }
            }
            IncomingMessage::Notification { method, params } => {
                handle_notification(&method, &params, &mut sessions).await;
            }
            IncomingMessage::Response { .. } => {
                // Responses are routed internally by the transport's pending map.
            }
        }
    }
}

// ── Request dispatch ─────────────────────────────────────────────────────────

async fn handle_request(
    method: &str,
    params: &Value,
    cfg: &AcpServerConfig,
    sessions: &mut HashMap<String, SessionState>,
    counter: &mut u64,
    transport: &Arc<dyn peri_acp::transport::AcpTransport>,
) -> Result<Value, AcpError> {
    match method {
        "initialize" => {
            let version = params
                .get("protocolVersion")
                .and_then(|v| v.as_u64())
                .unwrap_or(1);
            info!(protocol_version = %version, "ACP initialize");
            let resp = InitializeResponse::new(ProtocolVersion::V1)
                .agent_capabilities(AgentCapabilities::new());
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/new" => {
            let cwd = params
                .get("cwd")
                .and_then(|v| v.as_str())
                .unwrap_or(".")
                .to_string();
            *counter += 1;
            let session_id = format!("session-{}", counter);
            sessions.insert(
                session_id.clone(),
                SessionState {
                    session_id: session_id.clone(),
                    cwd,
                    history: Vec::new(),
                    cancel_token: None,
                },
            );
            info!(session_id = %session_id, "ACP session created");
            let resp = NewSessionResponse::new(SessionId::new(&*session_id));
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/prompt" => {
            let session_id = params
                .get("session_id")
                .and_then(|v| v.as_str())
                .ok_or_else(|| AcpError::new(-32602, "missing session_id"))?
                .to_string();
            let message = params
                .get("message")
                .ok_or_else(|| AcpError::new(-32602, "missing message"))?;
            let content = message
                .get("content")
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_string();

            let state = sessions
                .get_mut(&session_id)
                .ok_or_else(|| AcpError::new(-32602, "session not found"))?;

            let agent_input = AgentInput::text(content.clone());

            // Event channel: ExecutorEvent → SessionUpdate notifications
            let (event_tx, mut event_rx) = tokio::sync::mpsc::unbounded_channel::<ExecutorEvent>();

            let cancel = AgentCancellationToken::new();
            state.cancel_token = Some(cancel.clone());

            let event_handler: Arc<dyn AgentEventHandler> =
                Arc::new(FnEventHandler(move |event: ExecutorEvent| {
                    let _ = event_tx.send(event);
                }));

            let broker: Arc<dyn peri_agent::interaction::UserInteractionBroker> =
                Arc::new(AcpTransportBroker::new(
                    Arc::clone(transport) as Arc<dyn peri_acp::transport::AcpTransport>,
                    session_id.clone().into(),
                ));

            let features = PromptFeatures::detect();
            let system_prompt =
                build_system_prompt(None, &state.cwd, features, &cfg.plugin_agent_dirs);

            let context_window = cfg.provider.context_window();

            // Build agent using peri-acp's builder.
            // Convert TUI config types to peri-acp types at the boundary.
            let agent_output = build_agent_bridge(
                &cfg.provider,
                &state.cwd,
                system_prompt,
                event_handler,
                cancel.clone(),
                cfg.permission_mode.clone(),
                cfg.peri_config.clone(),
                cfg.cron_scheduler.clone(),
                session_id.clone(),
                broker,
                cfg.plugin_skill_dirs.clone(),
                cfg.plugin_agent_dirs.clone(),
                cfg.hook_groups.clone(),
                state.history.is_empty(),
                cfg.mcp_pool.clone(),
                cfg.tool_search_index.clone(),
                cfg.shared_tools.clone(),
                cfg.plugin_lsp_servers.clone(),
            );

            let context_window_u32 = context_window;

            // Background task: pump events to notifications.
            // Uses oneshot to guarantee the pump completes and sends agent_event_done
            // BEFORE we respond to the client. This prevents the loading spinner
            // from being stuck (client must see AgentDone before prompt() returns).
            let transport_clone = Arc::clone(transport);
            let sid = session_id.clone();
            let (pump_done_tx, pump_done_rx) = tokio::sync::oneshot::channel();
            tokio::spawn(async move {
                let mut event_count: u64 = 0;
                warn!(session_id = %sid, "ACP pump: started");
                while let Some(exec_event) = event_rx.recv().await {
                    event_count += 1;

                    // 检查是否有 peri/* 自定义通知映射
                    let peri_notifs = map_executor_to_peri_notifications(&exec_event);

                    // 仅不含 peri/* 映射的事件才发送 notifications/agent_event（向后兼容）
                    if peri_notifs.is_empty() {
                        let event_value = match serde_json::to_value(&exec_event) {
                            Ok(v) => v,
                            Err(e) => {
                                error!(event_count = event_count, error = %e, "ACP pump: serialize failed");
                                continue;
                            }
                        };
                        let agent_event_params = json!({
                            "session_id": sid,
                            "event": event_value,
                        });
                        if let Err(e) = transport_clone
                            .send_notification("notifications/agent_event", agent_event_params)
                            .await
                        {
                            error!(event_count = event_count, error = %e, "ACP pump: send agent_event failed");
                            break;
                        }
                    }

                    // 发送 peri/* 自定义通知
                    for (method, mut payload) in peri_notifs {
                        if let serde_json::Value::Object(ref mut map) = payload {
                            map.insert("session_id".to_string(), json!(sid));
                        }
                        let _ = transport_clone.send_notification(method, payload).await;
                    }

                    // 标准 ACP SessionUpdate（所有事件共用）
                    let updates = map_executor_to_updates(&exec_event, context_window_u32);
                    for update in updates {
                        let payload = match serde_json::to_value(&update) {
                            Ok(p) => p,
                            Err(e) => {
                                error!(error = %e, "ACP pump: serialize SessionUpdate failed");
                                continue;
                            }
                        };
                        let notif_params = json!({
                            "session_id": sid,
                            "update": payload,
                        });
                        let _ = transport_clone
                            .send_notification("notifications/session_update", notif_params)
                            .await;
                    }
                }
                // event_rx closed → agent finished
                warn!(session_id = %sid, event_count = event_count, "ACP pump: sending agent_event_done");
                let send_result = transport_clone
                    .send_notification(
                        "notifications/agent_event_done",
                        json!({
                            "session_id": sid,
                        }),
                    )
                    .await;
                match send_result {
                    Ok(()) => warn!(session_id = %sid, "ACP pump: agent_event_done sent, exiting"),
                    Err(e) => {
                        error!(session_id = %sid, error = %e, "ACP pump: agent_event_done send failed")
                    }
                }
                let _ = pump_done_tx.send(());
            });

            // Execute agent with fresh state
            let cwd = state.cwd.clone();
            let mut agent_state = AgentState::with_messages(cwd, state.history.clone());
            let result = agent_output
                .executor
                .execute(agent_input, &mut agent_state, Some(cancel.clone()))
                .await;
            // Explicitly drop agent_output to release all references to event_handler.
            // This must happen BEFORE we await pump_done_rx, so that event_tx closes
            // and the pump can observe event_rx returning None.
            drop(agent_output);

            // Wait for the pump to finish sending all events, including agent_event_done.
            // This guarantees the client receives AgentDone before prompt() returns.
            warn!(session_id = %session_id, "ACP: waiting for pump to finish...");
            match pump_done_rx.await {
                Ok(()) => warn!(session_id = %session_id, "ACP: pump confirmed done"),
                Err(_) => {
                    error!(session_id = %session_id, "ACP: pump done channel closed unexpectedly")
                }
            }

            match result {
                Ok(_output) => {
                    // Save messages from agent output back to session history
                    state.history = agent_state.into_messages();
                    info!(session_id = %session_id, messages = state.history.len(), "Agent execution completed");
                }
                Err(e) => error!(session_id = %session_id, error = %e, "Agent execution failed"),
            }

            let resp = PromptResponse::new(StopReason::EndTurn);
            serde_json::to_value(resp)
                .map_err(|e| AcpError::new(-32603, format!("Serialize failed: {e}")))
        }

        "session/set_model" => {
            // Model switching handled via peri_config.active_alias updates
            Ok(json!({ "status": "ok" }))
        }

        "session/set_mode" => {
            let session_id = params
                .get("session_id")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let mode = params
                .get("mode")
                .and_then(|v| v.as_str())
                .unwrap_or("default");
            info!(session_id = %session_id, mode = %mode, "Permission mode changed");
            Ok(json!({ "status": "ok" }))
        }

        _ => Err(AcpError::new(-32601, format!("Method not found: {method}"))),
    }
}

async fn handle_notification(
    method: &str,
    params: &Value,
    sessions: &mut HashMap<String, SessionState>,
) {
    if method == "$/cancel_request" {
        let session_id = params
            .get("session_id")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        if let Some(state) = sessions.get(session_id) {
            if let Some(ref token) = state.cancel_token {
                token.cancel();
                info!(session_id = %session_id, "Cancel requested");
            }
        }
    } else {
        warn!(method = %method, "Unhandled notification");
    }
}

// ── Bridge: convert TUI types → peri-acp types for build_agent ───────────────

#[allow(clippy::too_many_arguments)]
pub fn build_agent_bridge(
    provider: &LlmProvider,
    cwd: &str,
    system_prompt: String,
    event_handler: Arc<dyn AgentEventHandler>,
    cancel: AgentCancellationToken,
    permission_mode: Arc<SharedPermissionMode>,
    peri_config: Arc<PeriConfig>,
    cron_scheduler: Option<Arc<parking_lot::Mutex<CronScheduler>>>,
    session_id: String,
    broker: Arc<dyn peri_agent::interaction::UserInteractionBroker>,
    plugin_skill_dirs: Vec<std::path::PathBuf>,
    plugin_agent_dirs: Vec<std::path::PathBuf>,
    hook_groups: Vec<Vec<peri_middlewares::hooks::RegisteredHook>>,
    hook_session_start: bool,
    mcp_pool: Option<Arc<peri_middlewares::mcp::McpClientPool>>,
    tool_search_index: Arc<peri_middlewares::tool_search::ToolSearchIndex>,
    shared_tools: Arc<RwLock<HashMap<String, Arc<dyn peri_agent::tools::BaseTool>>>>,
    lsp_servers: Vec<peri_lsp::config::LspServerConfig>,
) -> peri_acp::agent::builder::AcpAgentOutput {
    // Convert TUI LlmProvider → peri-acp LlmProvider
    let acp_provider = convert_provider(provider);

    // Convert TUI PeriConfig → peri-acp PeriConfig (only fields used by build_agent)
    let acp_peri_config = Arc::new(peri_acp::provider::config::PeriConfig {
        config: peri_acp::provider::config::AppConfig {
            claude_md_excludes: peri_config.config.claude_md_excludes.clone(),
            context_1m: peri_config.config.context_1m,
            compact: peri_config.config.compact.clone(),
            ..Default::default()
        },
        ..Default::default()
    });

    peri_acp::agent::builder::build_agent(peri_acp::agent::builder::AcpAgentConfig {
        provider: acp_provider,
        cwd: cwd.to_string(),
        system_prompt,
        event_handler,
        cancel,
        permission_mode,
        peri_config: acp_peri_config,
        cron_scheduler,
        agent_overrides: None,
        preload_skills: Vec::new(),
        session_id: Some(session_id),
        broker,
        plugin_skill_dirs,
        plugin_agent_dirs,
        hook_groups,
        hook_session_start,
        mcp_pool,
        tool_search_index,
        shared_tools,
        child_handler_factory: None,
        lsp_servers,
    })
}

fn convert_provider(p: &LlmProvider) -> peri_acp::provider::LlmProvider {
    let convert_thinking = |t: &Option<crate::config::ThinkingConfig>| {
        t.as_ref()
            .map(|t| peri_acp::provider::config::ThinkingConfig {
                enabled: t.enabled,
                budget_tokens: t.budget_tokens,
                effort: t.effort.clone(),
                max_tokens: t.max_tokens,
            })
    };
    match p {
        LlmProvider::OpenAi {
            api_key,
            base_url,
            model,
            thinking,
        } => peri_acp::provider::LlmProvider::OpenAi {
            api_key: api_key.clone(),
            base_url: base_url.clone(),
            model: model.clone(),
            thinking: convert_thinking(thinking),
        },
        LlmProvider::Anthropic {
            api_key,
            model,
            base_url,
            thinking,
        } => peri_acp::provider::LlmProvider::Anthropic {
            api_key: api_key.clone(),
            model: model.clone(),
            base_url: base_url.clone(),
            thinking: convert_thinking(thinking),
        },
    }
}
