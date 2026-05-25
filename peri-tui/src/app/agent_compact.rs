use super::message_pipeline::PipelineAction;
use super::*;
use peri_agent::agent::events::CompactFileInfo;
use peri_agent::messages::BaseMessage;

impl App {
    pub(crate) fn handle_compact_started(&mut self) -> (bool, bool, bool) {
        // 显示 loading 状态（spinner + 禁用输入）
        self.set_loading(true);
        let vm = MessageViewModel::system(self.services.lc.tr("app-compact-started"));
        self.apply_pipeline_action(PipelineAction::AddMessage(vm));
        (true, false, false)
    }

    pub(crate) fn handle_compact_completed(
        &mut self,
        _summary: String,
        files: Vec<CompactFileInfo>,
        skills: Vec<String>,
        micro_cleared: usize,
        messages: Vec<BaseMessage>,
    ) -> (bool, bool, bool) {
        if micro_cleared > 0 {
            // Micro-compact: 更新内部状态，保留 pipeline 显示
            self.session_mgr.sessions[self.session_mgr.active]
                .agent
                .agent_state_messages = messages;
            let vm = MessageViewModel::system(self.services.lc.tr_args(
                "app-compact-auto-cleared",
                &[("count".into(), (micro_cleared as i64).into())],
            ));
            self.apply_pipeline_action(PipelineAction::AddMessage(vm));
            return (true, false, false);
        }

        // Full compact: 清理 pipeline + 更新内部状态
        // 结束 loading 状态
        self.set_loading(false);
        let mut label_lines = vec![format!("✻ {}", self.services.lc.tr("app-compact-done"))];
        for f in &files {
            label_lines.push(format!("  ⎿  Read {} ({} lines)", f.path, f.lines));
        }
        if !skills.is_empty() {
            label_lines.push(format!("  ⎿  Skill: {}", skills.join(", ")));
        }
        let compact_label = label_lines.join("\n");

        // 更新内部状态消息（供下一次 prompt 使用）
        self.session_mgr.sessions[self.session_mgr.active]
            .agent
            .agent_state_messages = messages.clone();

        // 清空 pipeline 内部状态 + 用压缩后消息恢复
        self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .pipeline
            .clear();
        self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .pipeline
            .restore_completed(messages);

        // 清除 ephemeral_notes，防止 compact 前的系统通知残留
        self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .ephemeral_notes
            .clear();

        // 清空 view_messages，只显示 compact 通知
        let view_msgs = vec![MessageViewModel::system(compact_label)];
        // resubmit 后 view_messages 被清空重建，round_start_vm_idx 必须重置
        // 否则第二轮 agent 事件的 request_rebuild() 使用旧值会越界并导致 VM 累积
        self.session_mgr.sessions[self.session_mgr.active]
            .messages
            .round_start_vm_idx = 0;
        self.apply_pipeline_action(PipelineAction::RebuildAll {
            prefix_len: 0,
            tail_vms: view_msgs,
        });

        // Full compact 释放了大量消息数据，归���内存页给 OS
        super::thread_ops::allocator_decay();

        (true, false, false)
    }

    pub(crate) fn handle_compact_error(&mut self, msg: String) -> (bool, bool, bool) {
        self.set_loading(false);
        let vm = MessageViewModel::system(
            self.services
                .lc
                .tr_args("app-compact-failed", &[("error".into(), msg.into())]),
        );
        self.apply_pipeline_action(PipelineAction::AddMessage(vm));

        (true, false, false)
    }
}
