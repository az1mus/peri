use std::sync::Arc;

use agent_client_protocol::{Client, ConnectionTo, Dispatch, Handled};
use agent_client_protocol::schema::{
    ClientNotification, ClientRequest, CloseSessionRequest,
    CloseSessionResponse, ContentBlock, ListSessionsRequest, ListSessionsResponse, LoadSessionRequest,
    LoadSessionResponse, NewSessionRequest, NewSessionResponse, Plan, PlanEntry, PlanEntryPriority,
    PlanEntryStatus, PromptRequest, PromptResponse, ResumeSessionRequest, ResumeSessionResponse,
    SessionId, SessionInfo, SessionNotification, SessionUpdate, StopReason,
};
use rust_create_agent::agent::events::{AgentEvent as ExecutorEvent, FnEventHandler};
use rust_create_agent::agent::react::AgentInput;
use rust_create_agent::agent::state::AgentState;
use rust_create_agent::agent::AgentCancellationToken;
use rust_create_agent::messages::BaseMessage;
use rust_agent_middlewares::tools::{TodoItem, TodoStatus};
use tokio::sync::OnceCell;

use super::agent_assembler;
use super::broker::AcpInteractionBroker;
use super::event_mapper;
use super::session::SessionManager;

static SESSION_MANAGER: OnceCell<SessionManager> = OnceCell::const_new();

/// 初始化全局 SessionManager（必须在 Agent::builder().connect_to() 之前调用）
pub fn init_session_manager(mgr: SessionManager) {
    let _ = SESSION_MANAGER.set(mgr);
}

fn mgr() -> &'static SessionManager {
    SESSION_MANAGER.get().expect("SessionManager not initialized")
}

// ─── session/new handler ─────────────────────────────────────────────────────

pub async fn handle_new_session(
    req: NewSessionRequest,
    responder: agent_client_protocol::Responder<NewSessionResponse>,
    _conn: ConnectionTo<Client>,
) -> Result<(), agent_client_protocol::Error> {
    let cwd = req.cwd.to_string_lossy().to_string();

    match mgr().new_session(&cwd).await {
        Ok((session_id, _thread_id)) => {
            let _ = responder.respond(NewSessionResponse::new(session_id.clone()));
            tracing::info!(session_id = %session_id, "ACP session created");
        }
        Err(e) => {
            tracing::error!("Failed to create session: {e}");
            let _ = responder.respond(NewSessionResponse::new(""));
        }
    }
    Ok(())
}

// ─── session/close handler ────────────────────────────────────────────────────

pub async fn handle_close_session(
    req: CloseSessionRequest,
    responder: agent_client_protocol::Responder<CloseSessionResponse>,
    _conn: ConnectionTo<Client>,
) -> Result<(), agent_client_protocol::Error> {
    let session_id = req.session_id.0.as_ref();
    let _ = mgr().close_session(session_id).await;
    let _ = responder.respond(CloseSessionResponse::default());
    tracing::info!(session_id = %session_id, "ACP session closed");
    Ok(())
}

// ─── session/list handler ────────────────────────────────────────────────────

pub async fn handle_list_sessions(
    _req: ListSessionsRequest,
    responder: agent_client_protocol::Responder<ListSessionsResponse>,
    _conn: ConnectionTo<Client>,
) -> Result<(), agent_client_protocol::Error> {
    match mgr().list_sessions().await {
        Ok(threads) => {
            let sessions: Vec<SessionInfo> = threads
                .into_iter()
                .map(|t| SessionInfo::new(SessionId::from(t.id), &t.cwd).title(t.title.unwrap_or_default()))
                .collect();
            let _ = responder.respond(ListSessionsResponse::new(sessions));
        }
        Err(e) => {
            tracing::error!("Failed to list sessions: {e}");
            let _ = responder.respond(ListSessionsResponse::new(vec![]));
        }
    }
    Ok(())
}

// ─── session/prompt handler ──────────────────────────────────────────────────

pub async fn handle_prompt(
    req: PromptRequest,
    responder: agent_client_protocol::Responder<PromptResponse>,
    conn: ConnectionTo<Client>,
) -> Result<(), agent_client_protocol::Error> {
    let session_id_str = req.session_id.0.clone();
    let session_id_acp = req.session_id.clone();

    // 从 prompt 中提取文本
    let user_text: String = req
        .prompt
        .iter()
        .filter_map(|block| {
            if let ContentBlock::Text(tc) = block {
                Some(tc.text.as_str())
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join("\n");

    if user_text.is_empty() {
        let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
        return Ok(());
    }

    // 获取 session 元数据
    let (thread_id, cwd, cancel_token) = {
        match mgr().get_session(&session_id_str) {
            Some(s) => (
                s.thread_id.clone(),
                s.cwd.clone(),
                s.cancel_token.clone(),
            ),
            None => {
                tracing::warn!(session_id = %session_id_str, "Session not found for prompt");
                let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                return Ok(());
            }
        }
    };

    tracing::info!(session_id = %session_id_str, text_len = user_text.len(), "ACP prompt received");

    // 将 Responder 和 conn 移入 spawned task，避免阻塞事件循环
    let mgr_provider = mgr().provider().clone();
    let mgr_zen_config = mgr().zen_config().clone();
    let mgr_permission_mode = mgr().permission_mode().clone();
    let mgr_thread_store = mgr().thread_store().clone();

    tokio::spawn(async move {
        // 加载线程历史
        let history = match mgr().load_thread_messages(&thread_id).await {
            Ok(h) => h,
            Err(e) => {
                tracing::error!(error = %e, "Failed to load thread history");
                let _ = responder.respond(PromptResponse::new(StopReason::EndTurn));
                return;
            }
        };

        // 构建系统提示词
        let features = crate::prompt::PromptFeatures::detect();
        let system_prompt =
            crate::prompt::build_system_prompt(None, &cwd, features);

        // 创建 CancellationToken（关联 session cancel_token）
        let cancel = AgentCancellationToken::new();
        let cancel_for_link = cancel.clone();
        let cancel_token_for_link = cancel_token.clone();
        tokio::spawn(async move {
            cancel_token_for_link.cancelled().await;
            cancel_for_link.cancel();
        });

        // 事件处理器：ExecutorEvent → SessionUpdate → SessionNotification → conn.send_notification()
        let conn_for_handler = conn.clone();
        let sid_for_handler = session_id_acp.clone();
        let handler: Arc<dyn rust_create_agent::agent::events::AgentEventHandler> =
            Arc::new(FnEventHandler(move |event: ExecutorEvent| {
                let updates = event_mapper::map_executor_to_updates(&event);
                for update in updates {
                    let notif = SessionNotification::new(sid_for_handler.clone(), update);
                    let _ = conn_for_handler.send_notification(notif);
                }
            }));

        // 创建 ACP 权限桥接 broker + 权限转发循环
        let (perm_tx, perm_rx) = tokio::sync::mpsc::channel(16);
        let broker = Arc::new(AcpInteractionBroker::new(perm_tx));

        // 权限转发：perm_rx → RequestPermissionRequest → conn.send_request() → map → response_tx
        let conn_for_perm = conn.clone();
        let sid_for_perm = session_id_acp.clone();
        tokio::spawn(async move {
            super::broker::permission_forwarding_loop(
                perm_rx,
                conn_for_perm,
                sid_for_perm,
            ).await;
        });

        // 组装 Agent
        let config = agent_assembler::AgentAssembleConfig {
            provider: mgr_provider,
            cwd: cwd.clone(),
            system_prompt,
            broker,
            permission_mode: mgr_permission_mode,
            zen_config: mgr_zen_config,
            preload_skills: vec![],
            event_handler: handler,
            cancel: cancel.clone(),
            cron_scheduler: None,
        };
        let (executor, mut todo_rx) = agent_assembler::assemble_agent(config);

        // 转发 Todo 更新为 SessionUpdate
        let conn_for_todo = conn.clone();
        let sid_for_todo = session_id_acp.clone();
        tokio::spawn(async move {
            while let Some(todos) = todo_rx.recv().await {
                let entries: Vec<_> = todos
                    .iter()
                    .map(|t: &TodoItem| {
                        PlanEntry::new(
                            t.content.clone(),
                            PlanEntryPriority::Medium,
                            match t.status {
                                TodoStatus::Completed => PlanEntryStatus::Completed,
                                TodoStatus::InProgress => PlanEntryStatus::InProgress,
                                TodoStatus::Pending => PlanEntryStatus::Pending,
                            },
                        )
                    })
                    .collect();
                let notif = SessionNotification::new(
                    sid_for_todo.clone(),
                    SessionUpdate::Plan(Plan::new(entries)),
                );
                let _ = conn_for_todo.send_notification(notif);
            }
        });

        // 创建 AgentState（带历史 + 持久化）
        let history_len = history.len();
        let mut state = AgentState::with_messages(cwd, history)
            .with_persistence(mgr_thread_store, thread_id);

        let input = AgentInput::text(user_text);
        let result = executor.execute(input, &mut state, Some(cancel)).await;

        // 持久化新增消息（StateSnapshot 等效）
        let new_msgs: Vec<_> = state
            .into_messages()
            .into_iter()
            .filter(|m| !matches!(m, BaseMessage::System { .. }))
            .skip(history_len)
            .collect();
        // new_msgs 通过 AgentState 的 with_persistence 已自动持久化

        tracing::info!(
            new_msgs = new_msgs.len(),
            "ACP prompt execution finished"
        );

        let stop_reason = match &result {
            Ok(_) => StopReason::EndTurn,
            Err(rust_create_agent::error::AgentError::Interrupted) => StopReason::Cancelled,
            Err(e) => {
                tracing::error!(error = %e, "ACP prompt execution error");
                StopReason::EndTurn
            }
        };

        let _ = responder.respond(PromptResponse::new(stop_reason));
    });

    Ok(())
}

// ─── session/load handler ────────────────────────────────────────────────────

pub async fn handle_load_session(
    req: LoadSessionRequest,
    responder: agent_client_protocol::Responder<LoadSessionResponse>,
    conn: ConnectionTo<Client>,
) -> Result<(), agent_client_protocol::Error> {
    let thread_id_str = req.session_id.0.as_ref().to_string();
    let cwd = req.cwd.to_string_lossy().to_string();
    let session_id_acp = req.session_id.clone();

    tracing::info!(thread_id = %thread_id_str, "ACP session/load request");

    // 加载线程历史
    let thread_id = rust_create_agent::thread::ThreadId::from(thread_id_str.clone());
    let messages = match mgr().load_thread_messages(&thread_id).await {
        Ok(msgs) => msgs,
        Err(e) => {
            tracing::error!(error = %e, "Failed to load session");
            let _ = responder.respond(LoadSessionResponse::new());
            return Ok(());
        }
    };

    // 创建 AcpSession 注册到 SessionManager
    let _ = mgr().new_session_with_id(&thread_id_str, &cwd).await;

    // 回放历史消息为 SessionNotification
    for msg in &messages {
        let updates = map_message_to_updates(msg);
        for update in updates {
            let notif = SessionNotification::new(session_id_acp.clone(), update);
            let _ = conn.send_notification(notif);
        }
    }

    tracing::info!(msg_count = messages.len(), "ACP session loaded and replayed");
    let _ = responder.respond(LoadSessionResponse::new());
    Ok(())
}

// ─── session/resume handler ──────────────────────────────────────────────────

pub async fn handle_resume_session(
    req: ResumeSessionRequest,
    responder: agent_client_protocol::Responder<ResumeSessionResponse>,
    _conn: ConnectionTo<Client>,
) -> Result<(), agent_client_protocol::Error> {
    let thread_id_str = req.session_id.0.as_ref().to_string();
    let cwd = req.cwd.to_string_lossy().to_string();

    tracing::info!(thread_id = %thread_id_str, "ACP session/resume request");

    // 创建 AcpSession 注册到 SessionManager（不回放消息）
    let _ = mgr().new_session_with_id(&thread_id_str, &cwd).await;

    let _ = responder.respond(ResumeSessionResponse::default());
    Ok(())
}

/// 将持久化的 BaseMessage 映射为 SessionUpdate（用于 session/load 回放）
fn map_message_to_updates(msg: &BaseMessage) -> Vec<SessionUpdate> {
    use agent_client_protocol::schema::{Content, ContentChunk, TextContent};

    match msg {
        BaseMessage::Human { content, .. } => {
            let text = content.text_content();
            vec![SessionUpdate::UserMessageChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new(text)),
            ))]
        }
        BaseMessage::Ai { content, tool_calls, .. } => {
            let mut updates = Vec::new();

            // AI 文本消息
            let text = content.text_content();
            if !text.is_empty() {
                updates.push(SessionUpdate::AgentMessageChunk(ContentChunk::new(
                    ContentBlock::Text(TextContent::new(text)),
                )));
            }

            // 工具调用
            for tc in tool_calls {
                use agent_client_protocol::schema::{
                    ToolCall, ToolCallStatus, ToolCallContent,
                };
                updates.push(SessionUpdate::ToolCall(
                    ToolCall::new(tc.id.clone(), tc.name.clone())
                        .status(ToolCallStatus::Completed)
                        .content(vec![ToolCallContent::Content(Content::new(
                            ContentBlock::Text(TextContent::new(truncate_str(
                                &tc.arguments.to_string(),
                                500,
                            ))),
                        ))]),
                ));
            }

            updates
        }
        BaseMessage::Tool { content, tool_call_id, is_error, .. } => {
            use agent_client_protocol::schema::{
                ToolCallUpdate, ToolCallUpdateFields, ToolCallStatus, ToolCallContent,
            };
            vec![SessionUpdate::ToolCallUpdate(ToolCallUpdate::new(
                tool_call_id.clone(),
                ToolCallUpdateFields::new()
                    .status(if *is_error {
                        ToolCallStatus::Failed
                    } else {
                        ToolCallStatus::Completed
                    })
                    .content(vec![ToolCallContent::Content(Content::new(
                        ContentBlock::Text(TextContent::new(truncate_str(
                            &content.text_content(),
                            500,
                        ))),
                    ))]),
            ))]
        }
        _ => vec![],
    }
}

fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let boundary = s.floor_char_boundary(max_len);
        format!("{}...", &s[..boundary])
    }
}

// ─── dispatch handler（通知 + 未匹配请求）────────────────────────────────────

pub async fn handle_dispatch(
    msg: Dispatch<ClientRequest, ClientNotification>,
    _conn: ConnectionTo<Client>,
) -> Result<Handled<Dispatch<ClientRequest, ClientNotification>>, agent_client_protocol::Error> {
    match msg {
        Dispatch::Notification(notif) => match notif {
            ClientNotification::CancelNotification(cancel) => {
                let session_id = cancel.session_id.0.as_ref();
                mgr().cancel_session(session_id);
                tracing::info!(session_id = %session_id, "ACP session cancelled");
                Ok(Handled::Yes)
            }
            _ => Ok(Handled::Yes),
        },
        // 未匹配的请求传递给下一个 handler
        other => Ok(Handled::No {
            message: other,
            retry: false,
        }),
    }
}
