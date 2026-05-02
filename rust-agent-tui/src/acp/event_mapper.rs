use agent_client_protocol::schema::{
    Content, ContentBlock, ContentChunk, Plan, PlanEntry, PlanEntryPriority, PlanEntryStatus,
    SessionUpdate, TextContent, ToolCall, ToolCallContent, ToolCallStatus, ToolKind, ToolCallUpdate,
    ToolCallUpdateFields,
};
use rust_agent_middlewares::tools::TodoStatus;
use rust_create_agent::agent::events::AgentEvent as ExecutorEvent;

use crate::app::events::AgentEvent;

/// 将 AgentEvent 映射为 ACP SessionUpdate 列表
pub fn map_event_to_updates(event: &AgentEvent) -> Vec<SessionUpdate> {
    match event {
        AgentEvent::AssistantChunk(text) => {
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
            ..
        } => {
            vec![SessionUpdate::ToolCall(
                ToolCall::new(tool_call_id.clone(), name.clone())
                    .kind(infer_tool_kind(name))
                    .status(ToolCallStatus::InProgress)
                    .content(vec![ToolCallContent::Content(Content::new(
                        ContentBlock::Text(TextContent::new(truncate_str(args, 500))),
                    ))]),
            )]
        }
        AgentEvent::ToolEnd {
            tool_call_id,
            output,
            is_error,
            ..
        } => {
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
                    ))]),
            ))]
        }
        AgentEvent::TodoUpdate(todos) => {
            let entries: Vec<PlanEntry> = todos
                .iter()
                .map(|t| PlanEntry::new(
                    t.content.clone(),
                    PlanEntryPriority::Medium,
                    match t.status {
                        TodoStatus::Completed => PlanEntryStatus::Completed,
                        TodoStatus::InProgress => PlanEntryStatus::InProgress,
                        TodoStatus::Pending => PlanEntryStatus::Pending,
                    },
                ))
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
pub fn map_executor_to_updates(event: &ExecutorEvent) -> Vec<SessionUpdate> {
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
                    ))]),
            )]
        }
        ExecutorEvent::ToolEnd {
            tool_call_id,
            output,
            is_error,
            ..
        } => {
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
                    ))]),
            ))]
        }
        // 内部事件、LLM 调用事件等不映射
        _ => vec![],
    }
}
