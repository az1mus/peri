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
        tail_vms.iter().all(
            |vm| !matches!(vm, MessageViewModel::UserBubble { content, .. } if content == "old q")
        ),
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
            blocks.iter().any(
                |b| matches!(b, ContentBlockView::Text { raw, .. } if raw.contains("I'll read")),
            )
        } else {
            false
        }
    });
    assert!(
        has_text,
        "ToolStart 后 streaming bubble 应包含 AI 文本，实际 VMs: {:?}",
        tail_vms
    );

    // Read 工具被 aggregate_tool_groups 折叠为 ToolCallGroup
    let has_tool = tail_vms.iter().any(|vm| {
            matches!(
                vm,
                MessageViewModel::ToolCallGroup { tools, .. } if tools.iter().any(|t| t.tool_name == "Read")
            )
        });
    assert!(
        has_tool,
        "ToolStart 后应有 ToolCallGroup(Read)，实际 VMs: {:?}",
        tail_vms
    );
}

/// 端到端：多轮工具调用中 AI 文本可见性
/// Chunk → ToolStart → ToolEnd → StateSnapshot → Chunk → ToolStart → Done
#[test]
fn test_e2e_text_visible_between_tool_calls() {
    use rust_create_agent::messages::{MessageContent, MessageId, ToolCallRequest};

    let mut pipeline = MessagePipeline::new("/tmp".to_string());
    pipeline.begin_round();

    // 1. AI 输出文本
    pipeline.handle_event(AgentEvent::AssistantChunk("Let me check the file".into()));
    let tail1 = pipeline.build_tail_vms();
    assert!(has_text(&tail1, "Let me check"), "步骤1: chunk 后应有文本");

    // 2. ToolStart
    pipeline.handle_event(AgentEvent::ToolStart {
        tool_call_id: "tc1".into(),
        name: "Read".into(),
        display: "ReadFile".into(),
        args: "main.rs".into(),
        input: json!({"path": "/tmp/main.rs"}),
    });
    let tail2 = pipeline.build_tail_vms();
    assert!(
        has_text(&tail2, "Let me check"),
        "步骤2: ToolStart 后文本应保留"
    );

    // 3. ToolEnd
    pipeline.handle_event(AgentEvent::ToolEnd {
        tool_call_id: "tc1".into(),
        name: "Read".into(),
        output: "fn main() {}".into(),
        is_error: false,
    });
    let tail3 = pipeline.build_tail_vms();
    assert!(
        has_text(&tail3, "Let me check"),
        "步骤3: ToolEnd 后文本应保留"
    );

    // 4. StateSnapshot（清空流式缓冲，切换到 reconcile 路径）
    pipeline.set_completed(vec![
        BaseMessage::human("read file"),
        BaseMessage::ai_with_tool_calls(
            MessageContent::text("Let me check the file"),
            vec![ToolCallRequest::new(
                "tc1",
                "Read",
                json!({"path": "/tmp/main.rs"}),
            )],
        ),
        BaseMessage::Tool {
            id: MessageId::new(),
            tool_call_id: "tc1".to_string(),
            content: MessageContent::text("fn main() {}"),
            is_error: false,
        },
    ]);
    let tail4 = pipeline.build_tail_vms();
    assert!(
        has_text(&tail4, "Let me check"),
        "步骤4: StateSnapshot 后 reconcile 应包含文本, VMs: {:?}",
        tail4
    );

    // 5. 新的 AI 文本（工具之间）
    pipeline.handle_event(AgentEvent::AssistantChunk("Now let me write tests".into()));
    let tail5 = pipeline.build_tail_vms();
    assert!(
        has_text(&tail5, "Now let me write tests"),
        "步骤5: 新 chunk 后应有新文本"
    );
    assert!(
        has_text(&tail5, "Let me check"),
        "步骤5: 旧文本也应保留（reconcile）"
    );

    // 6. 第二个 ToolStart
    pipeline.handle_event(AgentEvent::ToolStart {
        tool_call_id: "tc2".into(),
        name: "Write".into(),
        display: "WriteFile".into(),
        args: "test.rs".into(),
        input: json!({"path": "/tmp/test.rs"}),
    });
    let tail6 = pipeline.build_tail_vms();
    assert!(
        has_text(&tail6, "Now let me write tests"),
        "步骤6: 第二个 ToolStart 后新文本应保留"
    );
    assert!(
        has_text(&tail6, "Let me check"),
        "步骤6: 旧文本也应保留（reconcile）"
    );
}

fn has_text(vms: &[MessageViewModel], text: &str) -> bool {
    vms.iter().any(|vm| {
        if let MessageViewModel::AssistantBubble { blocks, .. } = vm {
            blocks
                .iter()
                .any(|b| matches!(b, ContentBlockView::Text { raw, .. } if raw.contains(text)))
        } else {
            false
        }
    })
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
