use crate::hooks::types::{HookAction, HookDecision, HookSpecificOutput, SyncHookResponse};

/// 解析 command hook stdout 输出
///
/// 对齐 Claude Code parseHookOutput + processHookJSONOutput:
/// - 不以 `{` 开头 → 纯文本输出，视为 Allow
/// - 以 `{` 开头 → 尝试解析为 SyncHookResponse JSON
pub fn parse_command_hook_output(stdout: &str) -> HookAction {
    let trimmed = stdout.trim();

    // 不以 { 开头 → 纯文本输出，视为 Allow
    if !trimmed.starts_with('{') {
        return HookAction::Allow;
    }

    // 尝试解析为 SyncHookResponse JSON
    match serde_json::from_str::<SyncHookResponse>(trimmed) {
        Ok(response) => sync_response_to_action(&response),
        Err(e) => {
            // JSON 解析失败 → 纯文本，视为 Allow（记录日志）
            tracing::warn!("Hook stdout JSON parse failed: {}", e);
            HookAction::Allow
        }
    }
}

/// 解析 HTTP hook 响应
///
/// 对齐 Claude Code parseHttpHookOutput：
/// - 空 body → 视为 {}（有效 JSON）
/// - 不以 `{` 开头 → 非法（HTTP hook 必须返回 JSON）
pub fn parse_http_hook_response(body: &str) -> HookAction {
    let trimmed = body.trim();

    // 空 body → 视为 {}（有效 JSON）
    if trimmed.is_empty() {
        return HookAction::Allow;
    }

    // 不以 { 开头 → 非法（HTTP hook 必须返回 JSON）
    if !trimmed.starts_with('{') {
        tracing::warn!(
            "HTTP hook must return JSON, got non-JSON body: {}",
            if trimmed.len() > 200 {
                format!("{}...", &trimmed[..trimmed.floor_char_boundary(200)])
            } else {
                trimmed.to_string()
            }
        );
        return HookAction::Allow;
    }

    match serde_json::from_str::<SyncHookResponse>(trimmed) {
        Ok(response) => sync_response_to_action(&response),
        Err(e) => {
            tracing::warn!("HTTP hook JSON parse failed: {}", e);
            HookAction::Allow
        }
    }
}

/// 将 SyncHookResponse 转换为内部 HookAction
///
/// 优先级（严格按顺序）：
/// 1. continue=false → PreventContinuation
/// 2. decision=block → Block
/// 3. systemMessage → SystemMessage
/// 4. hookSpecificOutput → 事件特定处理
/// 5. 以上都不满足 → Allow
fn sync_response_to_action(response: &SyncHookResponse) -> HookAction {
    // 1. continue=false → 阻止继续
    if response.continue_run == Some(false) {
        return HookAction::PreventContinuation {
            stop_reason: response.stop_reason.clone(),
        };
    }

    // 2. decision=block → 阻止操作
    if response.decision == Some(HookDecision::Block) {
        return HookAction::Block {
            reason: response
                .reason
                .clone()
                .unwrap_or_else(|| "Blocked by hook".into()),
        };
    }

    // 3. systemMessage → 注入系统消息
    if let Some(ref msg) = response.system_message {
        return HookAction::SystemMessage {
            message: msg.clone(),
        };
    }

    // 4. hookSpecificOutput → 事件特定处理
    if let Some(ref specific) = response.hook_specific_output {
        return hook_specific_to_action(specific);
    }

    HookAction::Allow
}

/// 将 HookSpecificOutput 转换为内部 HookAction
fn hook_specific_to_action(specific: &HookSpecificOutput) -> HookAction {
    match specific {
        HookSpecificOutput::PreToolUse {
            updated_input: Some(input),
            ..
        } => HookAction::ModifyInput {
            new_input: input.clone(),
        },
        HookSpecificOutput::PreToolUse {
            permission_decision: Some(decision),
            ..
        } => HookAction::PermissionOverride {
            decision: decision.clone(),
            reason: None,
        },
        HookSpecificOutput::UserPromptSubmit {
            additional_context: Some(ctx),
            ..
        } => HookAction::AdditionalContext {
            context: ctx.clone(),
        },
        HookSpecificOutput::SessionStart {
            initial_user_message: Some(msg),
            ..
        } => HookAction::InitialUserMessage {
            message: msg.clone(),
        },
        HookSpecificOutput::SessionStart {
            additional_context: Some(ctx),
            ..
        } => HookAction::AdditionalContext {
            context: ctx.clone(),
        },
        _ => HookAction::Allow,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hooks::types::PermissionDecision;

    // === parse_command_hook_output tests ===

    #[test]
    fn test_parse_command_plain_text() {
        assert!(matches!(
            parse_command_hook_output("hello world"),
            HookAction::Allow
        ));
    }

    #[test]
    fn test_parse_command_continue_false() {
        assert!(matches!(
            parse_command_hook_output(r#"{"continue": false}"#),
            HookAction::PreventContinuation { stop_reason: None }
        ));
    }

    #[test]
    fn test_parse_command_decision_block() {
        assert!(matches!(
            parse_command_hook_output(r#"{"decision": "block", "reason": "test"}"#),
            HookAction::Block { reason } if reason == "test"
        ));
    }

    #[test]
    fn test_parse_command_system_message() {
        assert!(matches!(
            parse_command_hook_output(r#"{"systemMessage": "warning"}"#),
            HookAction::SystemMessage { message } if message == "warning"
        ));
    }

    #[test]
    fn test_parse_command_invalid_json() {
        assert!(matches!(
            parse_command_hook_output("{invalid json}"),
            HookAction::Allow
        ));
    }

    #[test]
    fn test_parse_command_empty() {
        assert!(matches!(parse_command_hook_output(""), HookAction::Allow));
    }

    // === parse_http_hook_response tests ===

    #[test]
    fn test_parse_http_empty_body() {
        assert!(matches!(parse_http_hook_response(""), HookAction::Allow));
    }

    #[test]
    fn test_parse_http_whitespace_body() {
        assert!(matches!(parse_http_hook_response("   "), HookAction::Allow));
    }

    #[test]
    fn test_parse_http_non_json_body() {
        assert!(matches!(
            parse_http_hook_response("plain text"),
            HookAction::Allow
        ));
    }

    #[test]
    fn test_parse_http_valid_json() {
        assert!(matches!(
            parse_http_hook_response(r#"{"continue": false, "stopReason": "test"}"#),
            HookAction::PreventContinuation { stop_reason } if stop_reason.as_deref() == Some("test")
        ));
    }

    #[test]
    fn test_parse_http_invalid_json() {
        assert!(matches!(
            parse_http_hook_response("{invalid}"),
            HookAction::Allow
        ));
    }

    // === sync_response_to_action tests ===

    #[test]
    fn test_sync_response_priority_continue_over_decision() {
        let resp = SyncHookResponse {
            continue_run: Some(false),
            decision: Some(HookDecision::Block),
            reason: Some("blocked".into()),
            ..Default::default()
        };
        // continue=false 优先级高于 decision=block
        assert!(matches!(
            sync_response_to_action(&resp),
            HookAction::PreventContinuation { .. }
        ));
    }

    #[test]
    fn test_sync_response_decision_block() {
        let resp = SyncHookResponse {
            decision: Some(HookDecision::Block),
            reason: Some("blocked".into()),
            ..Default::default()
        };
        assert!(matches!(
            sync_response_to_action(&resp),
            HookAction::Block { reason } if reason == "blocked"
        ));
    }

    #[test]
    fn test_sync_response_system_message() {
        let resp = SyncHookResponse {
            system_message: Some("msg".into()),
            ..Default::default()
        };
        assert!(matches!(
            sync_response_to_action(&resp),
            HookAction::SystemMessage { message } if message == "msg"
        ));
    }

    #[test]
    fn test_sync_response_hook_specific_updated_input() {
        let resp = SyncHookResponse {
            hook_specific_output: Some(HookSpecificOutput::PreToolUse {
                updated_input: Some(serde_json::json!({"key": "val"})),
                permission_decision: None,
                permission_decision_reason: None,
                additional_context: None,
            }),
            ..Default::default()
        };
        assert!(matches!(
            sync_response_to_action(&resp),
            HookAction::ModifyInput { new_input } if new_input["key"] == "val"
        ));
    }

    #[test]
    fn test_sync_response_hook_specific_permission_decision() {
        let resp = SyncHookResponse {
            hook_specific_output: Some(HookSpecificOutput::PreToolUse {
                permission_decision: Some(PermissionDecision::Deny),
                permission_decision_reason: Some("not allowed".into()),
                updated_input: None,
                additional_context: None,
            }),
            ..Default::default()
        };
        assert!(matches!(
            sync_response_to_action(&resp),
            HookAction::PermissionOverride { decision, .. } if decision == PermissionDecision::Deny
        ));
    }

    #[test]
    fn test_sync_response_hook_specific_user_prompt_context() {
        let resp = SyncHookResponse {
            hook_specific_output: Some(HookSpecificOutput::UserPromptSubmit {
                additional_context: Some("extra context".into()),
            }),
            ..Default::default()
        };
        assert!(matches!(
            sync_response_to_action(&resp),
            HookAction::AdditionalContext { context } if context == "extra context"
        ));
    }

    #[test]
    fn test_sync_response_hook_specific_session_start_message() {
        let resp = SyncHookResponse {
            hook_specific_output: Some(HookSpecificOutput::SessionStart {
                additional_context: None,
                initial_user_message: Some("start msg".into()),
                watch_paths: None,
            }),
            ..Default::default()
        };
        assert!(matches!(
            sync_response_to_action(&resp),
            HookAction::InitialUserMessage { message } if message == "start msg"
        ));
    }

    #[test]
    fn test_sync_response_default_allow() {
        let resp = SyncHookResponse::default();
        assert!(matches!(sync_response_to_action(&resp), HookAction::Allow));
    }

    #[test]
    fn test_sync_response_decision_approve_is_allow() {
        let resp = SyncHookResponse {
            decision: Some(HookDecision::Approve),
            ..Default::default()
        };
        // Approve is not Block, so falls through to Allow
        assert!(matches!(sync_response_to_action(&resp), HookAction::Allow));
    }
}
