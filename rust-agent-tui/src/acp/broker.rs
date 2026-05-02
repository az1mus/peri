use agent_client_protocol::schema::{
    Content, ContentBlock, PermissionOption, PermissionOptionKind, RequestPermissionRequest,
    RequestPermissionOutcome, RequestPermissionResponse, SelectedPermissionOutcome, SessionId,
    TextContent, ToolCallContent, ToolCallStatus, ToolCallUpdate, ToolCallUpdateFields,
};
use agent_client_protocol::ConnectionTo;
use agent_client_protocol::role::acp::Client;
use async_trait::async_trait;
use rust_create_agent::interaction::{
    ApprovalDecision, InteractionContext, InteractionResponse, UserInteractionBroker,
};
use tokio::sync::{mpsc, oneshot};

pub struct PendingPermission {
    pub context: InteractionContext,
    pub response_tx: oneshot::Sender<InteractionResponse>,
}

pub struct AcpInteractionBroker {
    permission_tx: mpsc::Sender<PendingPermission>,
}

impl AcpInteractionBroker {
    pub fn new(permission_tx: mpsc::Sender<PendingPermission>) -> Self {
        Self { permission_tx }
    }
}

#[async_trait]
impl UserInteractionBroker for AcpInteractionBroker {
    async fn request(
        &self,
        context: InteractionContext,
    ) -> InteractionResponse {
        let (response_tx, response_rx) = oneshot::channel();

        if self
            .permission_tx
            .send(PendingPermission {
                context,
                response_tx,
            })
            .await
            .is_err()
        {
            return InteractionResponse::Decisions(vec![ApprovalDecision::Reject {
                reason: "ACP connection closed".into(),
            }]);
        }

        response_rx.await.unwrap_or(InteractionResponse::Decisions(
            vec![ApprovalDecision::Reject {
                reason: "Permission timeout".into(),
            }],
        ))
    }
}

/// 权限转发循环：将 HITL 权限请求通过 ACP request_permission 转发给 Client
pub async fn permission_forwarding_loop(
    mut rx: mpsc::Receiver<PendingPermission>,
    conn: ConnectionTo<Client>,
    session_id: SessionId,
) {
    while let Some(pending) = rx.recv().await {
        let response = handle_pending_permission(pending.context, &conn, &session_id).await;
        let _ = pending.response_tx.send(response);
    }
}

async fn handle_pending_permission(
    ctx: InteractionContext,
    conn: &ConnectionTo<Client>,
    session_id: &SessionId,
) -> InteractionResponse {
    match ctx {
        InteractionContext::Approval { items } => {
            let mut decisions = Vec::with_capacity(items.len());
            for item in &items {
                // 构建 ToolCallUpdate 描述待审批的工具调用
                let tool_update = ToolCallUpdate::new(
                    item.tool_call_id.clone(),
                    ToolCallUpdateFields::new()
                        .status(ToolCallStatus::Pending)
                        .content(vec![ToolCallContent::Content(Content::new(
                            ContentBlock::Text(TextContent::new(truncate_str(
                                &item.tool_input.to_string(),
                                500,
                            ))),
                        ))]),
                );

                // 构建权限选项
                let options = vec![
                    PermissionOption::new(
                        "allow_once",
                        "Allow once",
                        PermissionOptionKind::AllowOnce,
                    ),
                    PermissionOption::new(
                        "reject_once",
                        "Reject",
                        PermissionOptionKind::RejectOnce,
                    ),
                ];

                let request = RequestPermissionRequest::new(
                    session_id.clone(),
                    tool_update,
                    options,
                );

                // 发送请求并等待 Client 响应（block_task 仅在 spawned task 中安全）
                let decision = match conn.send_request(request).block_task().await {
                    Ok(resp) => map_permission_response(resp),
                    Err(e) => {
                        tracing::warn!(error = %e, "Permission request failed, defaulting to reject");
                        ApprovalDecision::Reject {
                            reason: format!("Permission request failed: {e}"),
                        }
                    }
                };
                decisions.push(decision);
            }
            InteractionResponse::Decisions(decisions)
        }
        InteractionContext::Questions { requests } => {
            // ACP 没有 AskUser 等价机制，返回空答案
            tracing::warn!(
                count = requests.len(),
                "AskUser questions not supported in ACP mode, returning empty answers"
            );
            InteractionResponse::Answers(
                requests
                    .into_iter()
                    .map(|q| rust_create_agent::interaction::QuestionAnswer {
                        id: q.id,
                        selected: vec![],
                        text: Some(String::new()),
                    })
                    .collect(),
            )
        }
    }
}

fn map_permission_response(resp: RequestPermissionResponse) -> ApprovalDecision {
    match resp.outcome {
        RequestPermissionOutcome::Selected(selected) => {
            let SelectedPermissionOutcome { option_id, .. } = selected;
            match option_id.0.as_ref() {
                "allow_once" | "allow_always" => ApprovalDecision::Approve,
                _ => ApprovalDecision::Reject {
                    reason: format!("User selected {option_id}"),
                },
            }
        }
        RequestPermissionOutcome::Cancelled => ApprovalDecision::Reject {
            reason: "Cancelled by user".into(),
        },
        _ => ApprovalDecision::Reject {
            reason: "Unknown response".into(),
        },
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
