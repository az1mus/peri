use super::message_pipeline::PipelineAction;
use super::*;
use crate::ui::message_view::MessageViewModel;

/// 构建后台任务完成的显示通知文本（截断版，供 UI 展示）
fn build_bg_display_notification(
    task_id: &str,
    agent_name: &str,
    success: bool,
    output: &str,
    tool_calls_count: usize,
    duration_ms: u64,
    lc: &crate::i18n::LcRegistry,
) -> String {
    let short_id = &task_id[..8.min(task_id.len())];
    if success {
        let output_preview: String = output
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(80)
            .collect();
        let _preview = if output.chars().count() > 80 || output.lines().count() > 1 {
            format!("{}...", output_preview)
        } else {
            output_preview
        };
        lc.tr_args(
            "app-bg-task-done",
            &[
                ("id".into(), short_id.into()),
                ("agent".into(), agent_name.into()),
                ("tools".into(), (tool_calls_count as i64).into()),
                ("duration".into(), (duration_ms as i64).into()),
            ],
        )
    } else {
        let err_preview: String = output.chars().take(80).collect();
        lc.tr_args(
            "app-bg-task-failed",
            &[
                ("id".into(), short_id.into()),
                ("agent".into(), agent_name.into()),
                ("error".into(), err_preview.into()),
            ],
        )
    }
}

impl App {
    pub(crate) fn handle_background_task_completed(
        &mut self,
        task_id: String,
        agent_name: String,
        success: bool,
        output: String,
        tool_calls_count: usize,
        duration_ms: u64,
    ) -> (bool, bool, bool) {
        // 检查被移除的 agent 是否���当前聚焦的
        let was_focused = self.session_mgr.sessions[self.session_mgr.active]
            .focused_instance_id
            .as_deref()
            .map(|id| {
                self.session_mgr.sessions[self.session_mgr.active]
                    .background_agents
                    .iter()
                    .any(|a| a.agent_name == agent_name && a.instance_id == id)
            })
            .unwrap_or(false);

        // 按 agent_name 移除第一个匹配项
        if let Some(pos) = self.session_mgr.sessions[self.session_mgr.active]
            .background_agents
            .iter()
            .position(|a| a.agent_name == agent_name)
        {
            self.session_mgr.sessions[self.session_mgr.active]
                .background_agents
                .remove(pos);
        }

        // 聚焦检查：如果被移除的是当前聚焦的 agent，退出聚焦
        if was_focused {
            self.session_mgr.sessions[self.session_mgr.active].focused_instance_id = None;
            self.session_mgr.sessions[self.session_mgr.active].ui.bg_bar_cursor = None;
            self.request_rebuild();
        }

        tracing::info!(
            task_id = %task_id,
            agent_name = %agent_name,
            success = success,
            bg_count_before = self.session_mgr.sessions[self.session_mgr.active].background_agents.len() + 1,
            bg_count_after = self.session_mgr.sessions[self.session_mgr.active].background_agents.len(),
            agent_done_pending = self.session_mgr.sessions[self.session_mgr.active].agent.agent_done_pending_bg,
            "[bg-diag] TUI: handle_background_task_completed called"
        );

        // 用于 LLM 上下文的纯文本通知
        let short_id = &task_id[..8.min(task_id.len())];
        let state_notification = if success {
            self.services.lc.tr_args(
                "app-bg-task-done-with-result",
                &[
                    ("id".into(), short_id.into()),
                    ("agent".into(), agent_name.clone().into()),
                    ("tools".into(), (tool_calls_count as i64).into()),
                    ("duration".into(), (duration_ms as i64).into()),
                    ("result".into(), output.clone().into()),
                ],
            )
        } else {
            self.services.lc.tr_args(
                "app-bg-task-failed-with-error",
                &[
                    ("id".into(), short_id.into()),
                    ("agent".into(), agent_name.clone().into()),
                    ("error".into(), output.clone().into()),
                ],
            )
        };

        // 将通知加入 agent_state_messages，使下一轮 agent 执行可见。
        // 仅在 executor 已结束（agent_done_pending_bg）时直接 push 作为兜底；
        // executor 运行期间的通知由 drain_notifications → StateSnapshot 路径写入，
        // 此处 push 会导致 agent_state_messages 中出现重复消息。
        if self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_done_pending_bg
        {
            self.session_mgr.sessions[self.session_mgr.active]
                .agent
                .agent_state_messages
                .push(peri_agent::messages::BaseMessage::human(
                    state_notification.as_str(),
                ));
        }

        // 尝试在 view_messages 中找到匹配的 SubAgentGroup 并更新。
        // 对于同名并发 bg agent（同一 agent_name 的多个实例），优先选择
        // final_result.is_none() 的组（尚未被之前的完成事件更新过），
        // 避免第一个完成事件更新了第一个组后，第二个完成事件 break 在
        // 已标记 is_running=false 的第一个组上而找不到第二个组。
        let short_id = &task_id[..8.min(task_id.len())];
        let mut found_and_updated = false;
        let session = &mut self.session_mgr.sessions[self.session_mgr.active];

        // 第一遍：精确匹配——找 is_running && agent_id 匹配 && final_result 为空的目标
        for vm in &mut session.messages.view_messages {
            if let MessageViewModel::SubAgentGroup {
                agent_id,
                is_running,
                is_background,
                bg_hash: _,
                final_result,
                is_error,
                ..
            } = vm
            {
                if *is_background
                    && *is_running
                    && agent_id == &agent_name
                    && final_result.is_none()
                {
                    *is_running = false;
                    *final_result = Some(output.clone());
                    *is_error = !success;
                    found_and_updated = true;
                    break;
                }
            }
        }

        // 第二遍（兜底）：如果没有找到 final_result 为空的匹配项，
        // 回退到原始逻辑——接受第一个 is_running && agent_id 匹配的组
        if !found_and_updated {
            for vm in &mut session.messages.view_messages {
                if let MessageViewModel::SubAgentGroup {
                    agent_id,
                    is_running,
                    is_background,
                    bg_hash: _,
                    final_result,
                    is_error,
                    ..
                } = vm
                {
                    if *is_background && *is_running && agent_id == &agent_name {
                        *is_running = false;
                        *final_result = Some(output.clone());
                        *is_error = !success;
                        found_and_updated = true;
                        break;
                    }
                }
            }
        }

        if found_and_updated {
            // 成功更新 SubAgentGroup，触发 RebuildAll
            self.request_rebuild();
        } else {
            // 未找到匹配的 SubAgentGroup，回退到创建 ToolBlock（兼容现有行为）
            let display_name = format!("bg:{}", agent_name);
            // 输出截断为单行（取第一行，再截取前 80 字符）
            let first_line = output.lines().next().unwrap_or("");
            let one_line = if first_line.chars().count() > 80 {
                let truncated: String = first_line.chars().take(80).collect();
                format!("{}...", truncated)
            } else if first_line.is_empty() && !output.is_empty() {
                String::from("(empty)")
            } else {
                first_line.to_string()
            };
            let header_info = if success {
                format!(
                    "{} completed ({} calls, {}ms): {}",
                    short_id, tool_calls_count, duration_ms, one_line
                )
            } else {
                format!("{} failed: {}", short_id, one_line)
            };
            let mut vm =
                MessageViewModel::tool_block(display_name.clone(), header_info, None, !success);
            if let MessageViewModel::ToolBlock { collapsed, .. } = &mut vm {
                *collapsed = true; // 始终折叠，摘要已在 header 中
            }
            self.apply_pipeline_action(PipelineAction::AddMessage(vm));
        }

        // 诊断日志：记录 BackgroundTaskCompleted 处理后的 view_messages 中 SubAgentGroup 数量
        {
            let subagent_count = self.session_mgr.sessions[self.session_mgr.active]
                .messages
                .view_messages
                .iter()
                .filter(|vm| vm.is_subagent_group())
                .count();
            let frozen_count = self.session_mgr.sessions[self.session_mgr.active]
                .messages
                .pipeline
                .frozen_subagent_vms_count();
            tracing::debug!(
                task_id = %&task_id[..8.min(task_id.len())],
                agent_name = %agent_name,
                subagent_count_in_view = subagent_count,
                frozen_count,
                background_agents_count = self.session_mgr.sessions[self.session_mgr.active].background_agents.len(),
                agent_done_pending_bg = self.session_mgr.sessions[self.session_mgr.active].agent.agent_done_pending_bg,
                "[bg-diag] after BackgroundTaskCompleted"
            );
        }

        // 如果 agent 已完成（Done）且所有后台任务都已完成，关闭通道并自动提交 continuation
        if self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_done_pending_bg
            && self.session_mgr.sessions[self.session_mgr.active].background_agents.is_empty()
        {
            tracing::info!("all background tasks completed, auto-submitting continuation");
            self.session_mgr.sessions[self.session_mgr.active]
                .agent
                .agent_done_pending_bg = false;
            self.session_mgr.sessions[self.session_mgr.active]
                .agent
                .agent_rx = None;
            let display_notification = build_bg_display_notification(
                &task_id,
                &agent_name,
                success,
                &output,
                tool_calls_count,
                duration_ms,
                &self.services.lc,
            );
            self.session_mgr.sessions[self.session_mgr.active]
                .agent
                .pending_bg_continuation = Some(display_notification);

            return (true, false, true);
        } else if !self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_done_pending_bg
            && self.session_mgr.sessions[self.session_mgr.active].background_agents.is_empty()
        {
            // 竞态修复：agent 尚未 Done，但所有后台任务已完成。
            // 暂存通知，待 Done 处理时检查此字段并设置 pending_bg_continuation。
            tracing::info!(
                "background task completed before Done, buffering notification for deferred continuation"
            );
            let display_notification = build_bg_display_notification(
                &task_id,
                &agent_name,
                success,
                &output,
                tool_calls_count,
                duration_ms,
                &self.services.lc,
            );
            self.session_mgr.sessions[self.session_mgr.active]
                .agent
                .pre_done_bg_completions
                .push(display_notification);
        }

        (true, false, false)
    }
}
