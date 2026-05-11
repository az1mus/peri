//! 统一消息渲染管线 (Unified Message Rendering Pipeline)
//!
//! 核心设计：所有 `MessageViewModel` 的产生都经过单一转换函数
//! `messages_to_view_models(base_messages, cwd)`。
//!
//! # 两条路径
//!
//! ```text
//!   流式事件 ──→ 增量更新 BaseMessage[] ──→ reconcile ──→ MessageViewModel[]
//!   历史恢复 ──→ BaseMessage[]            ──→ 直接转换  ──→ MessageViewModel[]
//!                                    ↑
//!                      同一个 messages_to_view_models()
//! ```
//!
//! # 流式 UX 优化
//!
//! `AssistantChunk` 使用 `AppendChunk` 直接操作渲染层（避免每字符重做 markdown），
//! 但在 "finalize 边界"（ToolStart / ToolEnd / Done）会 reconcile 最后的
//! AssistantBubble，确保最终状态与 restore 路径完全一致。

use std::collections::HashMap;
use std::time::{Duration, Instant};

use rust_create_agent::messages::{BaseMessage, ToolCallRequest};

use crate::app::events::AgentEvent;
use crate::app::tool_display;
use crate::ui::markdown::parse_markdown_default;
use crate::ui::message_view::{
    aggregate_tool_groups, tool_color, ContentBlockView, MessageViewModel,
};
use crate::ui::theme;

/// 合并冻结的 SubAgentGroup VM 到 reconcile 重建后的新 VMs 中，防止 Done 后 SubAgent 显示退化。
///
/// `frozen_vms` 是 SubAgentEnd 时构建的完整 SubAgentGroup VM（含 recent_messages、final_result 等），
/// 按出现顺序与新 VMs 中的 SubAgentGroup 占位符按位置匹配替换。
fn merge_frozen_subagents(frozen_vms: &[MessageViewModel], new_vms: &mut [MessageViewModel]) {
    if frozen_vms.is_empty() {
        return;
    }

    // 按位置匹配：新 VMs 中第 N 个 SubAgentGroup 对应 frozen_vms 中第 N 个
    let mut frozen_idx = 0;
    for vm in new_vms.iter_mut() {
        if matches!(vm, MessageViewModel::SubAgentGroup { .. }) && frozen_idx < frozen_vms.len() {
            *vm = frozen_vms[frozen_idx].clone();
            frozen_idx += 1;
        }
    }
}

// ─── 管线事件 ────────────────────────────────────────────────────────────────

/// 管线处理事件后的输出动作
#[derive(Debug)]
pub enum PipelineAction {
    /// 无 UI 变化
    None,
    /// 新增消息（外部通知 + 用户消息）
    AddMessage(MessageViewModel),
    /// 尾部重建（prefix_len 标记不变前缀长度，tail_vms 存储重建尾部）
    RebuildAll {
        prefix_len: usize,
        tail_vms: Vec<MessageViewModel>,
    },
}

// ─── 管线内部状态 ────────────────────────────────────────────────────────────

/// 已开始但未结束的工具调用
struct PendingTool {
    #[allow(dead_code)]
    tool_call_id: String,
    #[allow(dead_code)]
    name: String,
    #[allow(dead_code)]
    input: serde_json::Value,
}

/// ToolEnd 后、StateSnapshot 前的工具结果（用于在 reconcile gap 期间显示）
struct CompletedTool {
    tool_call_id: String,
    name: String,
    input: serde_json::Value,
    output: String,
    is_error: bool,
}

/// 活跃 SubAgent 执行状态
struct SubAgentState {
    agent_id: String,
    task_preview: String,
    total_steps: usize,
    /// 流式期间的内部消息（不持久化）
    recent_messages: Vec<MessageViewModel>,
    is_running: bool,
    /// SubAgentEnd 时固化的完整 VM（含 recent_messages、final_result 等）
    finalized_vm: Option<MessageViewModel>,
}

// ─── MessagePipeline ─────────────────────────────────────────────────────────

/// 统一消息渲染管线。
///
/// 维护规范 `BaseMessage[]` 状态，通过单一转换函数 `messages_to_view_models()`
/// 产生 `MessageViewModel`。流式和恢复共享同一个转换路径。
pub struct MessagePipeline {
    cwd: String,
    /// 已完成的 BaseMessages（规范状态，可用于持久化）
    completed: Vec<BaseMessage>,
    /// 当前正在流式构建的 AI 文本
    current_ai_text: String,
    /// 当前正在流式构建的 AI 推理内容
    current_ai_reasoning: String,
    /// 当前 AI 消息中的 tool_calls（由 ToolStart 事件积累）
    current_ai_tool_calls: Vec<ToolCallRequest>,
    /// 当前 AI 消息是否已 finalize（ToolStart 到达后 finalize）
    current_ai_finalized: bool,
    /// 已开始但未结束的工具调用
    pending_tools: HashMap<String, PendingTool>,
    /// ToolEnd 后、StateSnapshot 前的工具结果（在 reconcile gap 期间显示）
    completed_tools: Vec<CompletedTool>,
    /// SubAgent 栈
    subagent_stack: Vec<SubAgentState>,
    /// 冻结的 SubAgentGroup VMs（SubAgentEnd 时构建，done() 时收集）
    frozen_subagent_vms: Vec<MessageViewModel>,
    // ── 节流状态 ──
    /// 是否有待发射的节流 RebuildAll（有流式 chunk 积累但尚未发射）
    throttle_armed: bool,
    /// 上次节流发射的时间
    throttle_last_fire: Option<Instant>,
    // ── 轮次追踪 ──
    /// 本轮开始时 completed 的长度（用于区分首轮 StateSnapshot 前/后）
    completed_len_at_round_start: usize,
    /// 本轮是否收到过 StateSnapshot
    has_snapshot_this_round: bool,
}

impl MessagePipeline {
    pub fn new(cwd: String) -> Self {
        Self {
            cwd,
            completed: Vec::new(),
            current_ai_text: String::new(),
            current_ai_reasoning: String::new(),
            current_ai_tool_calls: Vec::new(),
            current_ai_finalized: false,
            pending_tools: HashMap::new(),
            completed_tools: Vec::new(),
            subagent_stack: Vec::new(),
            frozen_subagent_vms: Vec::new(),
            throttle_armed: false,
            throttle_last_fire: None,
            completed_len_at_round_start: 0,
            has_snapshot_this_round: false,
        }
    }

    pub fn cwd(&self) -> &str {
        &self.cwd
    }

    /// 统一事件处理入口：将 AgentEvent 转换为 PipelineAction 列表。
    /// 所有事件只更新 pipeline 内部状态，返回 None。
    /// RebuildAll 由 agent_ops 通过 `check_throttle()` 或 `build_rebuild_all()` 显式触发。
    pub fn handle_event(&mut self, event: AgentEvent) -> Vec<PipelineAction> {
        match event {
            AgentEvent::AssistantChunk(chunk) => {
                if !chunk.is_empty() {
                    if self.in_subagent() {
                        self.subagent_push_chunk(&chunk);
                    } else {
                        self.push_chunk(&chunk);
                    }
                    self.throttle_armed = true;
                }
                vec![PipelineAction::None]
            }
            AgentEvent::AiReasoning(text) => {
                if self.in_subagent() {
                    // SubAgent 内部推理：更新 subagent 状态，arm throttle
                    if let Some(_sub) = self.subagent_stack.last_mut() {
                        // 推理内容不直接显示，但需要 arm throttle 以刷新 SubAgentGroup
                    }
                    self.throttle_armed = true;
                } else {
                    self.push_reasoning(&text);
                    self.throttle_armed = true;
                }
                vec![PipelineAction::None]
            }
            AgentEvent::ToolStart {
                tool_call_id,
                name,
                display: _,
                args: _,
                input,
            } => {
                self.throttle_armed = false;
                if self.in_subagent() {
                    self.subagent_tool_start(&tool_call_id, &name, input);
                } else {
                    self.tool_start_internal(&tool_call_id, &name, input);
                }
                vec![PipelineAction::None]
            }
            AgentEvent::ToolEnd {
                tool_call_id,
                name,
                output,
                is_error,
            } => {
                self.throttle_armed = false;
                if self.in_subagent() {
                    // 更新 recent_messages 中对应 ToolBlock 的内容
                    if let Some(sub) = self.subagent_stack.last_mut() {
                        for vm in sub.recent_messages.iter_mut().rev() {
                            if let MessageViewModel::ToolBlock {
                                tool_call_id: tc_id,
                                content,
                                is_error: err,
                                ..
                            } = vm
                            {
                                if tc_id == &tool_call_id {
                                    *content = output.clone();
                                    *err = is_error;
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    self.tool_end_internal(&tool_call_id, &name, &output, is_error);
                }
                vec![PipelineAction::None]
            }
            AgentEvent::SubAgentStart {
                agent_id,
                task_preview,
                is_background: _,
            } => {
                let input =
                    serde_json::json!({"subagent_type": &agent_id, "prompt": &task_preview});
                let tc_id = format!("subagent_{}", agent_id);
                self.tool_start_internal(&tc_id, "Agent", input);
                vec![PipelineAction::None]
            }
            AgentEvent::SubAgentEnd { result, is_error } => {
                let tc_id = self
                    .subagent_stack
                    .last()
                    .map(|s| format!("subagent_{}", s.agent_id))
                    .unwrap_or_else(|| "subagent_end".to_string());
                self.tool_end_internal(&tc_id, "Agent", &result, is_error);
                vec![PipelineAction::None]
            }
            AgentEvent::Done => {
                self.done();
                vec![PipelineAction::None]
            }
            AgentEvent::Interrupted => {
                self.interrupt();
                vec![PipelineAction::None]
            }
            AgentEvent::StateSnapshot(msgs) => {
                self.set_completed(msgs);
                vec![PipelineAction::None]
            }
            // 以下事件由 agent_ops 直接处理，Pipeline 返回 None
            AgentEvent::Error(_)
            | AgentEvent::InteractionRequest { .. }
            | AgentEvent::TodoUpdate(_)
            | AgentEvent::CompactDone { .. }
            | AgentEvent::CompactError(_)
            | AgentEvent::TokenUsageUpdate { .. }
            | AgentEvent::LlmRetrying { .. }
            | AgentEvent::ContextWarning { .. }
            | AgentEvent::OAuthAuthorizationNeeded { .. }
            | AgentEvent::OAuthAuthorizationCompleted { .. }
            | AgentEvent::OAuthAuthorizationFailed { .. }
            | AgentEvent::BackgroundTaskCompleted { .. }
            | AgentEvent::McpActionCompleted { .. }
            | AgentEvent::PluginActionCompleted { .. }
            | AgentEvent::LspDiagnostics { .. } => {
                vec![PipelineAction::None]
            }
        }
    }

    // ─── 流式事件输入 ─────────────────────────────────────────────────────

    /// 追加流式文本 chunk
    pub fn push_chunk(&mut self, chunk: &str) {
        self.current_ai_text.push_str(chunk);
    }

    /// 追加推理 chunk
    pub fn push_reasoning(&mut self, text: &str) {
        self.current_ai_reasoning.push_str(text);
    }

    /// 工具调用开始（内部版本，只更新状态，不返回 PipelineAction）
    fn tool_start_internal(&mut self, tool_call_id: &str, name: &str, input: serde_json::Value) {
        self.finalize_current_ai();
        self.current_ai_tool_calls
            .push(ToolCallRequest::new(tool_call_id, name, input.clone()));

        if name == "Agent" {
            let agent_id = input["subagent_type"]
                .as_str()
                .unwrap_or("Agent")
                .to_string();
            let task_preview: String = input["prompt"]
                .as_str()
                .unwrap_or("")
                .chars()
                .take(40)
                .collect();
            self.subagent_stack.push(SubAgentState {
                agent_id: agent_id.clone(),
                task_preview: task_preview.clone(),
                total_steps: 0,
                recent_messages: Vec::new(),
                is_running: true,
                finalized_vm: None,
            });
        }

        self.pending_tools.insert(
            tool_call_id.to_string(),
            PendingTool {
                tool_call_id: tool_call_id.to_string(),
                name: name.to_string(),
                input,
            },
        );
    }

    /// 工具调用结束（内部版本，只更新状态，不返回 PipelineAction）
    fn tool_end_internal(&mut self, tool_call_id: &str, name: &str, output: &str, is_error: bool) {
        let pending = self.pending_tools.remove(tool_call_id);
        let input = pending
            .as_ref()
            .map(|p| p.input.clone())
            .unwrap_or(serde_json::Value::Null);

        if name == "Agent" {
            if let Some(sub) = self.subagent_stack.last_mut() {
                sub.is_running = false;
                let vm = MessageViewModel::SubAgentGroup {
                    agent_id: sub.agent_id.clone(),
                    task_preview: sub.task_preview.clone(),
                    total_steps: sub.total_steps,
                    recent_messages: std::mem::take(&mut sub.recent_messages),
                    is_running: false,
                    collapsed: false,
                    final_result: Some(output.to_string()),
                    is_error,
                };
                sub.finalized_vm = Some(vm.clone());
                // 立即冻结：RebuildAll 可能在下一个 StateSnapshot 前触发
                self.frozen_subagent_vms.push(vm);
            }
        } else {
            // 非 SubAgent 工具：保存到 completed_tools，在 StateSnapshot 到达前显示
            self.completed_tools.push(CompletedTool {
                tool_call_id: tool_call_id.to_string(),
                name: name.to_string(),
                input,
                output: output.to_string(),
                is_error,
            });
        }
    }

    /// SubAgent 内部工具调用（路由进 SubAgentGroup）
    pub fn subagent_tool_start(
        &mut self,
        tool_call_id: &str,
        name: &str,
        input: serde_json::Value,
    ) {
        if let Some(sub) = self.subagent_stack.last_mut() {
            let display = tool_display::format_tool_name(name);
            let args = tool_display::format_tool_args(name, &input, Some(&self.cwd));
            let vm = MessageViewModel::tool_block_with_id(
                tool_call_id.to_string(),
                name.to_string(),
                display,
                args,
                false,
            );
            sub.total_steps += 1;
            if sub.recent_messages.len() >= 4 {
                sub.recent_messages.remove(0);
            }
            sub.recent_messages.push(vm);
        }
    }

    /// SubAgent 内部 chunk
    pub fn subagent_push_chunk(&mut self, chunk: &str) {
        if let Some(sub) = self.subagent_stack.last_mut() {
            match sub.recent_messages.last_mut() {
                Some(m) if m.is_assistant() => m.append_chunk(chunk),
                _ => {
                    sub.total_steps += 1;
                    if sub.recent_messages.len() >= 4 {
                        sub.recent_messages.remove(0);
                    }
                    let mut bubble = MessageViewModel::assistant();
                    bubble.append_chunk(chunk);
                    sub.recent_messages.push(bubble);
                }
            }
        }
    }

    /// 标记当前 AI 轮次结束
    pub fn done(&mut self) {
        self.finalize_current_ai();
        self.current_ai_finalized = false;
        self.pending_tools.clear();
        self.completed_tools.clear();
        self.throttle_armed = false;
        self.throttle_last_fire = None;
        for sub in self.subagent_stack.drain(..) {
            if let Some(vm) = sub.finalized_vm {
                self.frozen_subagent_vms.push(vm);
            }
        }
    }

    /// 中断：finalize 当前状态并清理残留
    pub fn interrupt(&mut self) {
        self.finalize_current_ai();
        self.current_ai_finalized = false;
        self.pending_tools.clear();
        self.completed_tools.clear();
        self.throttle_armed = false;
        self.throttle_last_fire = None;
        for sub in self.subagent_stack.drain(..) {
            if let Some(vm) = sub.finalized_vm {
                self.frozen_subagent_vms.push(vm);
            }
        }
    }

    /// 清空所有状态
    pub fn clear(&mut self) {
        self.completed.clear();
        self.current_ai_text.clear();
        self.current_ai_reasoning.clear();
        self.current_ai_tool_calls.clear();
        self.current_ai_finalized = false;
        self.pending_tools.clear();
        self.completed_tools.clear();
        self.subagent_stack.clear();
        self.frozen_subagent_vms.clear();
    }

    /// 当前 AI 消息是否有可见内容
    pub fn has_streaming_content(&self) -> bool {
        !self.current_ai_text.trim().is_empty() || !self.current_ai_reasoning.is_empty()
    }

    /// 当前 AI 消息是否有待处理的 tool_calls
    pub fn has_pending_tool_calls(&self) -> bool {
        !self.current_ai_tool_calls.is_empty()
    }

    /// 是否在 SubAgent 执行中
    pub fn in_subagent(&self) -> bool {
        self.subagent_stack.last().is_some_and(|s| s.is_running)
    }

    /// 构建当前流式 AssistantBubble（从 pipeline 流式缓冲区构建完整内容）
    pub fn build_streaming_bubble(&self) -> MessageViewModel {
        let mut blocks: Vec<ContentBlockView> = Vec::new();
        if !self.current_ai_reasoning.is_empty() {
            blocks.push(ContentBlockView::Reasoning {
                char_count: self.current_ai_reasoning.chars().count(),
            });
        }
        if !self.current_ai_text.trim().is_empty() {
            blocks.push(ContentBlockView::Text {
                raw: self.current_ai_text.clone(),
                rendered: parse_markdown_default(&self.current_ai_text),
                dirty: false,
            });
        }
        // 追加当前 AI 消息中已完成的 tool_use blocks（不含 pending tools）
        for tc in &self.current_ai_tool_calls {
            if !self.pending_tools.contains_key(&tc.id) {
                blocks.push(ContentBlockView::ToolUse {
                    name: tc.name.clone(),
                });
            }
        }
        MessageViewModel::AssistantBubble {
            blocks,
            is_streaming: true,
            collapsed: false,
        }
    }

    // ── 轮次管理 ──────────────────────────────────────────────────────────────

    /// 标记新一轮对话开始。由 submit_message() 调用。
    pub fn begin_round(&mut self) {
        self.completed_len_at_round_start = self.completed.len();
        self.has_snapshot_this_round = false;
        self.throttle_armed = false;
        self.throttle_last_fire = None;
    }

    // ── 节流机制 ──────────────────────────────────────────────────────────────

    /// 检查节流计时器，若 100ms 已过则发射 RebuildAll。
    /// 由 poll_agent() 每帧调用。
    pub fn check_throttle(&mut self, prefix_len: usize) -> Option<PipelineAction> {
        if !self.throttle_armed {
            return None;
        }
        let now = Instant::now();
        let should_fire = match self.throttle_last_fire {
            None => true,
            Some(last) => now.duration_since(last) >= Duration::from_millis(100),
        };
        if should_fire {
            self.throttle_last_fire = Some(now);
            self.throttle_armed = false;
            return Some(self.build_rebuild_all(prefix_len));
        }
        None
    }

    // ── RebuildAll 构造 ───────────────────────────────────────────────────────

    /// 构造 RebuildAll action：从 pipeline 规范状态重建尾部 VMs。
    pub fn build_rebuild_all(&self, prefix_len: usize) -> PipelineAction {
        let tail_vms = self.build_tail_vms();
        PipelineAction::RebuildAll {
            prefix_len,
            tail_vms,
        }
    }

    /// 从 pipeline 规范状态构建尾部 VMs。
    ///
    /// 两种情况：
    /// - has_snapshot_this_round == true：从 completed[last_human..] reconcile + streaming + pending tools
    /// - has_snapshot_this_round == false（Case 1）：跳过 reconcile，只输出 streaming + pending tools
    fn build_tail_vms(&self) -> Vec<MessageViewModel> {
        let mut tail_vms = Vec::new();

        if self.has_snapshot_this_round {
            // 从 completed 中本轮的最后一条 Human 消息开始 reconcile
            let round_completed = &self.completed[self.completed_len_at_round_start..];
            let last_human_offset = round_completed
                .iter()
                .rposition(|msg| matches!(msg, BaseMessage::Human { .. }))
                .map(|idx| idx + self.completed_len_at_round_start)
                .unwrap_or(self.completed_len_at_round_start);
            tail_vms =
                Self::messages_to_view_models(&self.completed[last_human_offset..], &self.cwd);
        }

        // 追加流式 AssistantBubble（当前 AI 正在输出的文本）
        // 必须在工具 blocks 之前：AI 先说文本，再调用工具
        if self.has_streaming_content() {
            tail_vms.push(self.build_streaming_bubble());
        }

        // 追加 pending tool blocks（ToolStart 后、下一个 StateSnapshot 前的工具）
        // 跳过 Agent 工具（由 subagent_stack 表示为 SubAgentGroup）
        for tc in &self.current_ai_tool_calls {
            if let Some(pending) = self.pending_tools.get(&tc.id) {
                if pending.name != "Agent" {
                    tail_vms.push(self.build_tool_start_vm(&tc.id, &pending.name, &pending.input));
                }
            }
        }

        // 追加已完成但尚未进入 completed 的工具结果（ToolEnd 后、StateSnapshot 前）
        for ct in &self.completed_tools {
            let display = tool_display::format_tool_name(&ct.name);
            let args = tool_display::format_tool_args(&ct.name, &ct.input, Some(&self.cwd));
            tail_vms.push(MessageViewModel::ToolBlock {
                tool_name: ct.name.clone(),
                tool_call_id: ct.tool_call_id.clone(),
                display_name: display,
                args_display: args,
                content: ct.output.clone(),
                is_error: ct.is_error,
                collapsed: true,
                color: if ct.is_error {
                    theme::ERROR
                } else {
                    tool_color(&ct.name)
                },
            });
        }

        // SubAgentGroup VMs
        if self.has_snapshot_this_round {
            // reconcile 已从 completed 生成 SubAgentGroup 占位符，用冻结版本替换
            // （冻结版本含 recent_messages、final_result 等 richer 信息）
            merge_frozen_subagents(&self.frozen_subagent_vms, &mut tail_vms);
        } else {
            // 无 snapshot 时 reconcile 不执行，直接从 subagent_stack 构建 SubAgentGroup
            for sub in &self.subagent_stack {
                let vm = if let Some(ref finalized) = sub.finalized_vm {
                    finalized.clone()
                } else {
                    MessageViewModel::SubAgentGroup {
                        agent_id: sub.agent_id.clone(),
                        task_preview: sub.task_preview.clone(),
                        total_steps: sub.total_steps,
                        recent_messages: sub.recent_messages.clone(),
                        is_running: sub.is_running,
                        collapsed: false,
                        final_result: None,
                        is_error: false,
                    }
                };
                tail_vms.push(vm);
            }
        }

        // 聚合相邻只读工具
        aggregate_tool_groups(&mut tail_vms);

        tail_vms
    }

    /// 获取已完成的 BaseMessages（用于持久化）
    pub fn completed_messages(&self) -> &[BaseMessage] {
        &self.completed
    }

    /// 追加增量 BaseMessages（StateSnapshot 是增量消息），并清除流式状态防止重复
    pub fn set_completed(&mut self, msgs: Vec<BaseMessage>) {
        self.completed.extend(msgs);
        self.current_ai_text.clear();
        self.current_ai_reasoning.clear();
        self.current_ai_tool_calls.clear();
        self.current_ai_finalized = true;
        self.has_snapshot_this_round = true;
        self.pending_tools.clear();
        self.completed_tools.clear();
    }

    /// 从外部加载全量 BaseMessages（用于历史恢复后覆盖），并清除所有状态
    pub fn restore_completed(&mut self, msgs: Vec<BaseMessage>) {
        self.completed = msgs;
        self.current_ai_text.clear();
        self.current_ai_reasoning.clear();
        self.current_ai_tool_calls.clear();
        self.current_ai_finalized = true;
    }

    // ─── 核心转换函数 ─────────────────────────────────────────────────────

    /// 从规范 BaseMessage[] 构建完整的 MessageViewModel[]。
    ///
    /// **这是唯一的转换入口**——流式 reconcile 和历史恢复都调用此函数。
    pub fn messages_to_view_models(msgs: &[BaseMessage], cwd: &str) -> Vec<MessageViewModel> {
        let mut vms: Vec<MessageViewModel> = Vec::with_capacity(msgs.len());
        let mut prev_ai_tool_calls: Vec<(String, String, serde_json::Value)> = Vec::new();

        for msg in msgs {
            // 维护前一条 Ai 消息的 tool_calls，用于 Tool 消息获取工具名和参数
            if let BaseMessage::Ai { tool_calls, .. } = msg {
                prev_ai_tool_calls = tool_calls
                    .iter()
                    .map(|tc| (tc.id.clone(), tc.name.clone(), tc.arguments.clone()))
                    .collect();
            }

            let vm =
                MessageViewModel::from_base_message_with_cwd(msg, &prev_ai_tool_calls, Some(cwd));

            // 跳过没有可见文本内容的 AssistantBubble（纯 ToolUse 或空文本 + ToolUse）
            if let MessageViewModel::AssistantBubble { blocks, .. } = &vm {
                let has_visible = blocks.iter().any(|b| match b {
                    ContentBlockView::Text { raw, .. } => !raw.trim().is_empty(),
                    ContentBlockView::Reasoning { char_count } => *char_count > 0,
                    ContentBlockView::ToolUse { .. } => false,
                });
                if !has_visible {
                    continue;
                }
            }

            vms.push(vm);
        }

        // 聚合相邻的只读工具调用为 ToolCallGroup
        aggregate_tool_groups(&mut vms);
        vms
    }

    /// Reconcile：从当前 completed 状态重建完整的 view_models。
    ///
    /// 在 "finalize 边界"（ToolStart / Done）调用，确保流式最终状态
    /// 与 restore 路径 `messages_to_view_models()` 完全一致。
    pub fn reconcile(&self) -> Vec<MessageViewModel> {
        Self::messages_to_view_models(&self.completed, &self.cwd)
    }

    // ─── 内部方法 ─────────────────────────────────────────────────────────

    /// Finalize 当前 AI 消息：将流式状态转为 BaseMessage 加入 completed
    fn finalize_current_ai(&mut self) {
        if self.current_ai_finalized {
            return;
        }
        let has_content = !self.current_ai_text.trim().is_empty()
            || !self.current_ai_reasoning.is_empty()
            || !self.current_ai_tool_calls.is_empty();

        if !has_content {
            return;
        }

        // 不清空 current_ai_text/current_ai_reasoning：在 StateSnapshot 到达前，
        // build_tail_vms() 仍需要这些内容来显示 AI 已输出的文本。
        // set_completed() 到达时会清空它们。
        // 保留 tool_calls 信息给后续 reconcile 使用
        self.current_ai_finalized = true;
    }

    /// 构建 ToolStart 的 ToolBlock VM（与 from_base_message_with_cwd 的 Tool 路径一致）
    fn build_tool_start_vm(
        &self,
        tool_call_id: &str,
        name: &str,
        input: &serde_json::Value,
    ) -> MessageViewModel {
        let display_name = tool_display::format_tool_name(name);
        let args_display = tool_display::format_tool_args(name, input, Some(&self.cwd));
        MessageViewModel::ToolBlock {
            tool_name: name.to_string(),
            tool_call_id: tool_call_id.to_string(),
            display_name,
            args_display,
            content: String::new(),
            is_error: false,
            collapsed: true,
            color: tool_color(name),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rust_create_agent::messages::{BaseMessage, ContentBlock, MessageContent, ToolCallRequest};
    use serde_json::json;

    fn _normalize_vms(vms: Vec<MessageViewModel>) -> Vec<String> {
        vms.iter().map(|vm| format!("{:?}", vm)).collect()
    }

    /// 测试：流式路径和恢复路径对简单文本回复产生一致的输出
    #[test]
    fn test_streaming_vs_restore_text_only() {
        let cwd = "/Users/test/project";

        // 恢复路径
        let msgs = vec![BaseMessage::human("hello"), BaseMessage::ai("world")];
        let restore_vms = MessagePipeline::messages_to_view_models(&msgs, cwd);

        // 流式路径：模拟事件序列
        let mut pipeline = MessagePipeline::new(cwd.to_string());
        pipeline.push_chunk("world");
        pipeline.done();
        // 模拟 StateSnapshot 填充 completed
        pipeline.set_completed(vec![BaseMessage::ai("world")]);
        let stream_vms = pipeline.reconcile();

        // 比较非系统消息
        assert_eq!(restore_vms.len(), 2);
        assert_eq!(stream_vms.len(), 1); // 流式路径没有用户消息（由 handle_agent_event 添加）
    }

    /// 测试：工具调用的 cwd 一致性（核心修复验证）
    #[test]
    fn test_tool_args_cwd_consistency() {
        let cwd = "/Users/test/project";

        // 模拟恢复路径：Tool 消息从 BaseMessage 转换
        // Ai 消息带文本 + tool_calls，确保不会被过滤
        let msgs = vec![
            BaseMessage::human("read file"),
            BaseMessage::ai_with_tool_calls(
                MessageContent::text("I'll read the file"),
                vec![ToolCallRequest::new(
                    "tc1",
                    "Read",
                    json!({"file_path": "/Users/test/project/src/main.rs"}),
                )],
            ),
            BaseMessage::Tool {
                id: rust_create_agent::messages::MessageId::new(),
                tool_call_id: "tc1".to_string(),
                content: MessageContent::text("file content here"),
                is_error: false,
            },
        ];
        let restore_vms = MessagePipeline::messages_to_view_models(&msgs, cwd);

        // 找到 ToolBlock 或 ToolCallGroup
        let tool_vm = restore_vms.iter().find(|vm| {
            matches!(vm, MessageViewModel::ToolBlock { .. })
                || matches!(vm, MessageViewModel::ToolCallGroup { .. })
        });
        assert!(
            tool_vm.is_some(),
            "应有 ToolBlock/ToolCallGroup，实际 VMs: {:?}",
            restore_vms
        );

        if let Some(MessageViewModel::ToolBlock { args_display, .. }) = tool_vm {
            // 应该显示相对路径而非绝对路径
            assert!(args_display.is_some(), "args_display 应有值");
            let args = args_display.as_ref().unwrap();
            assert!(
                args.contains("src/main.rs"),
                "应显示相对路径，实际: {}",
                args
            );
            assert!(
                !args.contains("/Users/test/project"),
                "不应包含 cwd 前缀，实际: {}",
                args
            );
        }
    }

    /// 测试：恢复路径的 cwd=None 仍能正常工作（向后兼容）
    #[test]
    fn test_restore_without_cwd() {
        let msgs = vec![BaseMessage::human("hello"), BaseMessage::ai("hi")];
        // cwd=None → fallback 行为
        let vms = MessagePipeline::messages_to_view_models(&msgs, "");
        assert_eq!(vms.len(), 2);
    }

    /// 测试：流式 pipeline 的 finalize 清理流式缓冲（completed 由 StateSnapshot 填充）
    #[test]
    fn test_pipeline_finalize_clears_buffers() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        pipeline.push_reasoning("thinking...");
        pipeline.push_chunk("Hello world");
        pipeline.done();

        // finalize 不再 push 到 completed（StateSnapshot 是唯一数据源）
        assert!(pipeline.completed_messages().is_empty());
        // done() 不再清空流式缓冲（set_completed 到达时才清空），
        // 但 current_ai_finalized 被重置为 false，所以流式状态仍然存在
        assert!(pipeline.has_streaming_content());
        // set_completed 到达后才清空流式缓冲
        pipeline.set_completed(vec![
            BaseMessage::human("hi"),
            BaseMessage::ai("Hello world"),
        ]);
        assert!(!pipeline.has_streaming_content());
    }

    /// 测试：set_completed 是 completed 的唯一数据源
    #[test]
    fn test_pipeline_set_completed_single_source() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        let msgs = vec![BaseMessage::human("hello"), BaseMessage::ai("world")];
        pipeline.set_completed(msgs.clone());

        assert_eq!(pipeline.completed_messages().len(), 2);
    }

    /// 测试：tool_start/tool_end 不直接写入 completed
    #[test]
    fn test_pipeline_tool_end_no_duplicate() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        let _ = pipeline.handle_event(AgentEvent::ToolStart {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            display: "ReadFile".into(),
            args: "test.txt".into(),
            input: json!({"file_path": "/tmp/test.txt"}),
        });
        let _ = pipeline.handle_event(AgentEvent::ToolEnd {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            output: "content here".into(),
            is_error: false,
        });

        // tool_end 不 push 到 completed
        assert!(pipeline.completed_messages().is_empty());

        // 模拟 StateSnapshot 填充
        let snapshot = vec![
            BaseMessage::ai_with_tool_calls(
                MessageContent::text("reading"),
                vec![ToolCallRequest::new(
                    "tc1",
                    "Read",
                    json!({"file_path": "/tmp/test.txt"}),
                )],
            ),
            BaseMessage::Tool {
                id: rust_create_agent::messages::MessageId::new(),
                tool_call_id: "tc1".to_string(),
                content: MessageContent::text("content here"),
                is_error: false,
            },
        ];
        pipeline.set_completed(snapshot);
        assert_eq!(
            pipeline.completed_messages().len(),
            2,
            "StateSnapshot 应无重复地填充 completed"
        );
    }

    /// 测试：from_base_message_with_cwd 与 from_base_message 向后兼容
    #[test]
    fn test_from_base_message_backward_compat() {
        let msg = BaseMessage::ai("hello");
        let vm1 = MessageViewModel::from_base_message(&msg, &[]);
        let vm2 = MessageViewModel::from_base_message_with_cwd(&msg, &[], None);
        // 两者应产生相同结果
        assert_eq!(format!("{:?}", vm1), format!("{:?}", vm2));
    }

    // ─── handle_event 测试 ─────────────────────────────────────────────────

    /// 测试：handle_event AssistantChunk 更新内部状态并 arm throttle
    #[test]
    fn test_handle_event_assistant_chunk() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        let actions = pipeline.handle_event(AgentEvent::AssistantChunk("hello".into()));
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], PipelineAction::None));
        assert_eq!(pipeline.current_ai_text, "hello");
        assert!(pipeline.throttle_armed, "AssistantChunk 应 arm throttle");
    }

    /// 测试：handle_event 空 chunk 不产生 AppendChunk
    #[test]
    fn test_handle_event_empty_chunk() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        let actions = pipeline.handle_event(AgentEvent::AssistantChunk(String::new()));
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], PipelineAction::None));
    }

    /// 测试：handle_event ToolStart + ToolEnd + Done 更新内部状态（所有返回 None）
    #[test]
    fn test_handle_event_tool_lifecycle() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        // ToolStart → None，但内部状态更新
        let actions = pipeline.handle_event(AgentEvent::ToolStart {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            display: "ReadFile".into(),
            args: "src/main.rs".into(),
            input: serde_json::json!({"file_path": "/tmp/src/main.rs"}),
        });
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], PipelineAction::None));
        assert!(
            pipeline.pending_tools.contains_key("tc1"),
            "ToolStart 后 pending_tools 应包含 tc1"
        );
        // ToolEnd → None，pending_tools 移除
        let actions = pipeline.handle_event(AgentEvent::ToolEnd {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            output: "file content".into(),
            is_error: false,
        });
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], PipelineAction::None));
        assert!(
            !pipeline.pending_tools.contains_key("tc1"),
            "ToolEnd 后 pending_tools 应不包含 tc1"
        );
        // Done → None
        let actions = pipeline.handle_event(AgentEvent::Done);
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], PipelineAction::None));
    }

    /// 测试：handle_event StateSnapshot 更新 completed
    #[test]
    fn test_handle_event_state_snapshot() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        let msgs = vec![BaseMessage::human("hello"), BaseMessage::ai("world")];
        let actions = pipeline.handle_event(AgentEvent::StateSnapshot(msgs.clone()));
        assert_eq!(actions.len(), 1);
        assert!(matches!(actions[0], PipelineAction::None));
        assert_eq!(pipeline.completed_messages().len(), 2);
    }

    /// 测试：SubAgent 内部并行相同工具的 tool_call_id 精确匹配
    #[test]
    fn test_subagent_parallel_same_tool_matches_by_call_id() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());

        // 启动 SubAgent
        let _ = pipeline.handle_event(AgentEvent::SubAgentStart {
            agent_id: "test-agent".into(),
            task_preview: "parallel reads".into(),
            is_background: false,
        });

        // SubAgent 内部并行启动两个 read_file
        let _ = pipeline.handle_event(AgentEvent::ToolStart {
            tool_call_id: "tc_a".into(),
            name: "Read".into(),
            display: "ReadFile".into(),
            args: "a.rs".into(),
            input: serde_json::json!({"file_path": "/tmp/a.rs"}),
        });
        let _ = pipeline.handle_event(AgentEvent::ToolStart {
            tool_call_id: "tc_b".into(),
            name: "Read".into(),
            display: "ReadFile".into(),
            args: "b.rs".into(),
            input: serde_json::json!({"file_path": "/tmp/b.rs"}),
        });

        // ToolEnd 按不同顺序到达（tc_b 先完成）
        let _ = pipeline.handle_event(AgentEvent::ToolEnd {
            tool_call_id: "tc_b".into(),
            name: "Read".into(),
            output: "content of b".into(),
            is_error: false,
        });
        let _ = pipeline.handle_event(AgentEvent::ToolEnd {
            tool_call_id: "tc_a".into(),
            name: "Read".into(),
            output: "content of a".into(),
            is_error: false,
        });

        // 验证 recent_messages 中两个 ToolBlock 被正确更新
        let sub = pipeline.subagent_stack.last().unwrap();
        assert_eq!(sub.recent_messages.len(), 2);

        // 找到 tc_a 和 tc_b 对应的 ToolBlock
        let mut found_a = false;
        let mut found_b = false;
        for vm in &sub.recent_messages {
            if let MessageViewModel::ToolBlock {
                tool_call_id,
                content,
                ..
            } = vm
            {
                match tool_call_id.as_str() {
                    "tc_a" => {
                        assert_eq!(content, "content of a", "tc_a 应匹配自己的结果");
                        found_a = true;
                    }
                    "tc_b" => {
                        assert_eq!(content, "content of b", "tc_b 应匹配自己的结果");
                        found_b = true;
                    }
                    _ => {}
                }
            }
        }
        assert!(found_a, "应找到 tc_a 的 ToolBlock");
        assert!(found_b, "应找到 tc_b 的 ToolBlock");
    }

    // ─── build_tail_vms 测试 ──────────────────────────────────────────────────

    /// 场景1: has_snapshot=true, completed 有消息 → 从最后一条 Human 开始 reconcile
    #[test]
    fn test_build_tail_vms_with_snapshot() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        pipeline.completed = vec![
            BaseMessage::human("q1"),
            BaseMessage::ai("a1"),
            BaseMessage::human("q2"),
            BaseMessage::ai("a2"),
        ];
        pipeline.has_snapshot_this_round = true;
        pipeline.completed_len_at_round_start = 0;

        let tail_vms = pipeline.build_tail_vms();
        let expected =
            MessagePipeline::messages_to_view_models(&pipeline.completed[2..], &pipeline.cwd);
        assert_eq!(format!("{:?}", tail_vms), format!("{:?}", expected));
    }

    /// 场景2: has_snapshot=false（Case 1）→ 跳过 reconcile，只输出 streaming + pending tools
    #[test]
    fn test_build_tail_vms_case1_no_snapshot() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        pipeline.completed = vec![BaseMessage::human("old q"), BaseMessage::ai("old a")];
        pipeline.has_snapshot_this_round = false;
        pipeline.completed_len_at_round_start = 2;

        // 有流式内容
        pipeline.push_chunk("streaming text");

        let tail_vms = pipeline.build_tail_vms();
        // Case 1 不应包含 old q / old a
        assert!(
            tail_vms.iter().all(|vm| !matches!(vm, MessageViewModel::UserBubble { content, .. } if content == "old q")),
            "Case 1 不应包含上一轮消息"
        );
        // 应包含 streaming bubble
        assert!(
            tail_vms.iter().any(|vm| matches!(
                vm,
                MessageViewModel::AssistantBubble {
                    is_streaming: true,
                    ..
                }
            )),
            "Case 1 应包含 streaming bubble"
        );
    }

    /// 场景3: 空 completed + 无 streaming → 空 tail
    #[test]
    fn test_build_tail_vms_empty() {
        let pipeline = MessagePipeline::new("/tmp".to_string());
        let tail_vms = pipeline.build_tail_vms();
        assert!(tail_vms.is_empty());
    }

    /// 场景：AssistantChunk → ToolStart 后，build_tail_vms 应包含 AI 文本 + ToolBlock
    #[test]
    fn test_build_tail_vms_text_visible_with_pending_tool() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());

        // 模拟真实事件流：AI 先输出文本，再调用工具
        pipeline.handle_event(AgentEvent::AssistantChunk("I'll read the file".into()));
        pipeline.handle_event(AgentEvent::ToolStart {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            display: "ReadFile".into(),
            args: "src/main.rs".into(),
            input: json!({"file_path": "/tmp/src/main.rs"}),
        });

        let tail_vms = pipeline.build_tail_vms();

        // 应包含 streaming bubble 且有文本内容
        let has_text = tail_vms.iter().any(|vm| {
            if let MessageViewModel::AssistantBubble { blocks, .. } = vm {
                blocks.iter().any(|b| matches!(b, ContentBlockView::Text { raw, .. } if raw.contains("I'll read")))
            } else {
                false
            }
        });
        assert!(
            has_text,
            "ToolStart 后 streaming bubble 应包含 AI 文本，实际 VMs: {:?}",
            tail_vms
        );

        // 应包含 ToolBlock
        let has_tool = tail_vms
            .iter()
            .any(|vm| matches!(vm, MessageViewModel::ToolBlock { .. }));
        assert!(
            has_tool,
            "ToolStart 后应有 ToolBlock，实际 VMs: {:?}",
            tail_vms
        );
    }

    /// 验证尾部重建与全量转换一致性
    #[test]
    fn test_build_tail_vms_consistency() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        pipeline.restore_completed(vec![
            BaseMessage::human("q1"),
            BaseMessage::ai("a1"),
            BaseMessage::human("q2"),
            BaseMessage::ai("a2"),
        ]);
        pipeline.has_snapshot_this_round = true;
        pipeline.completed_len_at_round_start = 0;

        let tail_vms = pipeline.build_tail_vms();

        // tail_vms 应等于从最后一条 Human 消息开始重建的 VMs
        let last_human_idx = pipeline
            .completed_messages()
            .iter()
            .rposition(|msg| matches!(msg, BaseMessage::Human { .. }))
            .unwrap_or(0);
        let expected_tail = MessagePipeline::messages_to_view_models(
            &pipeline.completed_messages()[last_human_idx..],
            &pipeline.cwd,
        );

        assert_eq!(format!("{:?}", tail_vms), format!("{:?}", expected_tail));
    }

    /// 验证工具调用场景的尾部重建
    #[test]
    fn test_build_tail_vms_with_tools() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        pipeline.restore_completed(vec![
            BaseMessage::human("read file"),
            BaseMessage::ai_from_blocks(vec![ContentBlock::ToolUse {
                id: "tc1".to_string(),
                name: "Read".to_string(),
                input: serde_json::json!({"file_path": "/tmp/test.rs"}),
            }]),
            BaseMessage::tool_result("tc1", "file content here"),
        ]);
        pipeline.has_snapshot_this_round = true;
        pipeline.completed_len_at_round_start = 0;

        let tail_vms = pipeline.build_tail_vms();

        // 全量转换对比
        let full_vms =
            MessagePipeline::messages_to_view_models(pipeline.completed_messages(), &pipeline.cwd);

        assert_eq!(format!("{:?}", tail_vms), format!("{:?}", full_vms));
    }

    /// 验证 pending tools 在 build_tail_vms 中生成 ToolBlock VMs
    #[test]
    fn test_build_tail_vms_with_pending_tools() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        pipeline.has_snapshot_this_round = true;
        pipeline.completed_len_at_round_start = 0;
        pipeline.completed = vec![BaseMessage::human("hi")];

        // 模拟 ToolStart（通过 handle_event）
        let _ = pipeline.handle_event(AgentEvent::ToolStart {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            display: "ReadFile".into(),
            args: "src/main.rs".into(),
            input: serde_json::json!({"file_path": "/tmp/test.rs"}),
        });

        let tail_vms = pipeline.build_tail_vms();
        // 应包含 UserBubble + pending ToolBlock（Read 可能被聚合为 ToolCallGroup）
        let has_tool = tail_vms.iter().any(|vm| match vm {
            MessageViewModel::ToolBlock { tool_name, .. } => tool_name == "Read",
            MessageViewModel::ToolCallGroup { tools, .. } => {
                tools.iter().any(|t| t.tool_name == "Read")
            }
            _ => false,
        });
        assert!(
            has_tool,
            "build_tail_vms 应包含 pending tool 的 ToolBlock 或 ToolCallGroup"
        );
    }

    /// 验证 set_completed 清除 pending_tools
    #[test]
    fn test_set_completed_clears_pending_tools() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        let _ = pipeline.handle_event(AgentEvent::ToolStart {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            display: "ReadFile".into(),
            args: "src/main.rs".into(),
            input: serde_json::json!({"file_path": "/tmp/test.rs"}),
        });
        assert!(pipeline.pending_tools.contains_key("tc1"));

        pipeline.set_completed(vec![BaseMessage::human("hi"), BaseMessage::ai("result")]);
        assert!(
            !pipeline.pending_tools.contains_key("tc1"),
            "set_completed 应清除 pending_tools"
        );
        assert!(pipeline.has_snapshot_this_round);
    }

    /// 验证 Interrupted 后 build_tail_vms 产生一致结果（可用于后续 RebuildAll）
    ///
    /// 场景：agent 回复了文本后被中断，Interrupted 处理器调用 build_rebuild_all
    /// 然后 Done 到达，如果重复 build_rebuild_all 并 RebuildAll，会覆盖 Interrupted 添加的通知消息。
    #[test]
    fn test_build_tail_vms_interrupted_then_done_consistency() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        pipeline.has_snapshot_this_round = true;
        pipeline.completed_len_at_round_start = 0;

        // 模拟流式：用户发送消息，agent 回复了文本，然后开始工具调用
        pipeline.push_chunk("I'll read the file");
        let _ = pipeline.handle_event(AgentEvent::ToolStart {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            display: "ReadFile".into(),
            args: "src/main.rs".into(),
            input: serde_json::json!({"file_path": "/tmp/test.rs"}),
        });
        let _ = pipeline.handle_event(AgentEvent::ToolEnd {
            tool_call_id: "tc1".into(),
            name: "Read".into(),
            output: "file content here".into(),
            is_error: false,
        });

        // 模拟 StateSnapshot 填充 completed
        pipeline.set_completed(vec![
            BaseMessage::human("read file"),
            BaseMessage::ai_from_blocks(vec![
                ContentBlock::text("I'll read the file"),
                ContentBlock::tool_use("tc1", "Read", json!({"file_path": "/tmp/test.rs"})),
            ]),
            BaseMessage::tool_result("tc1", "file content here"),
        ]);

        // Interrupted 处理器调用 build_rebuild_all
        let action1 = pipeline.build_rebuild_all(0);
        if let PipelineAction::RebuildAll {
            prefix_len,
            tail_vms,
        } = action1
        {
            assert_eq!(prefix_len, 0);
            assert!(
                tail_vms.len() >= 3,
                "build_tail_vms 应包含 UserBubble + AssistantBubble + ToolBlock/Group"
            );

            // Done 到达时，再次 build_rebuild_all 应产生相同结果
            let action2 = pipeline.build_rebuild_all(0);
            if let PipelineAction::RebuildAll {
                prefix_len: p2,
                tail_vms: tail_vms2,
            } = action2
            {
                assert_eq!(prefix_len, p2);
                assert_eq!(tail_vms.len(), tail_vms2.len());
                for (a, b) in tail_vms.iter().zip(tail_vms2.iter()) {
                    assert_eq!(a, b, "两次 build_rebuild_all 结果应一致");
                }
            } else {
                panic!("Expected RebuildAll");
            }
        } else {
            panic!("Expected RebuildAll");
        }
    }

    /// 验证 Done 后 pipeline.done() 是幂等的（不改变 build_tail_vms 结果）
    #[test]
    fn test_done_idempotent_build_tail_vms() {
        let mut pipeline = MessagePipeline::new("/tmp".to_string());
        pipeline.has_snapshot_this_round = true;
        pipeline.completed_len_at_round_start = 0;

        pipeline.push_chunk("Hello world");
        pipeline.set_completed(vec![
            BaseMessage::human("hi"),
            BaseMessage::ai("Hello world"),
        ]);

        // 第一次 done
        pipeline.done();
        let action1 = pipeline.build_rebuild_all(0);
        let tail_vms1 = match action1 {
            PipelineAction::RebuildAll { tail_vms, .. } => tail_vms,
            _ => panic!("Expected RebuildAll"),
        };

        // 第二次 done（模拟 Interrupted -> Done 双重调用）
        pipeline.done();
        let action2 = pipeline.build_rebuild_all(0);
        let tail_vms2 = match action2 {
            PipelineAction::RebuildAll { tail_vms, .. } => tail_vms,
            _ => panic!("Expected RebuildAll"),
        };

        assert_eq!(tail_vms1.len(), tail_vms2.len());
        for (a, b) in tail_vms1.iter().zip(tail_vms2.iter()) {
            assert_eq!(a, b, "多次 done 后 build_tail_vms 结果应一致");
        }
    }
}
