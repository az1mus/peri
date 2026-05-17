use agent_client_protocol::schema::{
    Content, ContentBlock, ContentChunk, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus,
    SessionInfoUpdate, SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallStatus,
    ToolCallUpdate, ToolCallUpdateFields, ToolKind, UsageUpdate,
};
use peri_agent::agent::events::AgentEvent as ExecutorEvent;
use peri_middlewares::tools::TodoStatus;

use crate::app::events::AgentEvent;

/// 将 AgentEvent 映射为 ACP SessionUpdate 列表
pub fn map_event_to_updates(event: &AgentEvent) -> Vec<SessionUpdate> {
    match event {
        AgentEvent::AssistantChunk {
            chunk: text,
            source_agent_id: _,
        } => {
            vec![SessionUpdate::AgentMessageChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new(text.clone())),
            ))]
        }
        AgentEvent::AiReasoning(text) => {
            vec![SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new(text.clone())),
            ))]
        }
        AgentEvent::ToolStart {
            tool_call_id,
            name,
            args,
            input,
            source_agent_id: _,
            ..
        } => {
            vec![SessionUpdate::ToolCall(
                ToolCall::new(tool_call_id.clone(), name.clone())
                    .kind(infer_tool_kind(name))
                    .status(ToolCallStatus::InProgress)
                    .content(vec![ToolCallContent::Content(Content::new(
                        ContentBlock::Text(TextContent::new(truncate_str(args, 500))),
                    ))])
                    .raw_input(Some(input.clone())),
            )]
        }
        AgentEvent::ToolEnd {
            tool_call_id,
            output,
            is_error,
            source_agent_id: _,
            ..
        } => {
            let raw_output = match serde_json::from_str::<serde_json::Value>(output) {
                Ok(v) => Some(v),
                Err(_) => Some(serde_json::Value::String(output.clone())),
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
                        ContentBlock::Text(TextContent::new(truncate_str(output, 500))),
                    ))])
                    .raw_output(raw_output),
            ))]
        }
        AgentEvent::TodoUpdate(todos) => {
            let entries: Vec<PlanEntry> = todos
                .iter()
                .map(|t| {
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
            vec![SessionUpdate::Plan(Plan::new(entries))]
        }
        // 这些事件没有直接 ACP 映射
        _ => vec![],
    }
}

fn infer_tool_kind(name: &str) -> ToolKind {
    match name {
        "Read" => ToolKind::Read,
        "Write" | "Edit" | "folder_operations" => ToolKind::Edit,
        "Bash" => ToolKind::Execute,
        "Grep" | "Glob" => ToolKind::Search,
        "WebFetch" | "WebSearch" => ToolKind::Fetch,
        _ => ToolKind::Other,
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

/// 直接将 ExecutorEvent 映射为 ACP SessionUpdate（ACP 模式专用，无 TUI 依赖）
///
/// `context_window` 是当前模型的上下文窗口大小（tokens），用于填充 UsageUpdate.size。
pub fn map_executor_to_updates(event: &ExecutorEvent, context_window: u32) -> Vec<SessionUpdate> {
    match event {
        ExecutorEvent::TextChunk { chunk, .. } => {
            vec![SessionUpdate::AgentMessageChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new(chunk.clone())),
            ))]
        }
        ExecutorEvent::AiReasoning(text) => {
            vec![SessionUpdate::AgentThoughtChunk(ContentChunk::new(
                ContentBlock::Text(TextContent::new(text.clone())),
            ))]
        }
        ExecutorEvent::ToolStart {
            tool_call_id,
            name,
            input,
            ..
        } => {
            let args_str = input.to_string();
            vec![SessionUpdate::ToolCall(
                ToolCall::new(tool_call_id.clone(), name.clone())
                    .kind(infer_tool_kind(name))
                    .status(ToolCallStatus::InProgress)
                    .content(vec![ToolCallContent::Content(Content::new(
                        ContentBlock::Text(TextContent::new(truncate_str(&args_str, 500))),
                    ))])
                    .raw_input(Some(input.clone())),
            )]
        }
        ExecutorEvent::ToolEnd {
            tool_call_id,
            output,
            is_error,
            ..
        } => {
            let raw_output = match serde_json::from_str::<serde_json::Value>(output) {
                Ok(v) => Some(v),
                Err(_) => Some(serde_json::Value::String(output.clone())),
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
                        ContentBlock::Text(TextContent::new(truncate_str(output, 500))),
                    ))])
                    .raw_output(raw_output),
            ))]
        }
        ExecutorEvent::LlmCallEnd { usage: Some(u), .. } => {
            vec![SessionUpdate::UsageUpdate(UsageUpdate::new(
                u64::from(u.input_tokens) + u64::from(u.output_tokens),
                u64::from(context_window),
            ))]
        }
        ExecutorEvent::ContextWarning {
            used_tokens,
            total_tokens,
            ..
        } => {
            vec![SessionUpdate::UsageUpdate(UsageUpdate::new(
                *used_tokens,
                *total_tokens,
            ))]
        }
        ExecutorEvent::LlmRetrying {
            attempt,
            max_attempts,
            delay_ms,
            ..
        } => {
            vec![SessionUpdate::SessionInfoUpdate(
                SessionInfoUpdate::new().title(format!(
                    "Retrying LLM call (attempt {}/{}, {}ms delay)",
                    attempt, max_attempts, delay_ms
                )),
            )]
        }
        // 内部事件、LLM 调用事件等不映射
        _ => vec![],
    }
}
