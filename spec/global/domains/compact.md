# 上下文压缩增强 领域

## 领域综述

上下文压缩增强领域负责 Micro-compact 和 Full Compact 策略的全面增强，包括可压缩工具白名单、9 段结构化摘要模板和压缩后重新注入。

核心职责：
- Micro-compact 可压缩工具白名单 + 时间衰减清除策略
- Full Compact 9 段结构化摘要模板对齐 Claude Code
- 压缩后重新注入最近读取文件和激活 Skills
- 工具对完整性保护确保 tool_use + tool_result 不被拆开
- CompactConfig 通过 settings.json 配置，环境变量可覆盖

## 核心流程

### Micro-compact 流程

```
触发条件: context_usage 70%-85%
  → 白名单工具结果可压缩（bash/read/glob/search/write/edit）
  → 时间衰减: 超过 micro_compact_stale_steps(5) 步的旧结果
  → 图片替换: [image] 或 [compacted: image ~{tokens} tokens]
  → 文档替换: [document] 或 [compacted: document ~{tokens} tokens]
  → 工具对保护: adjust_index_to_preserve_invariants() 确保 tool_use + tool_result 不拆开
```

### Full Compact 流程

```
触发条件: context_usage > 85%
  → 9 段结构化摘要模板:
      Primary Request → Technical Concepts → Files → Errors & Fixes →
      Problem Solving → User Messages → Pending Tasks → Current Work → Next Step
  → 调用 LLM 生成摘要
  → 移除 <analysis> 块，保留 <summary>
  → PTL 降级重试: 按消息步数组逐步删除最旧组，最多重试 3 次
  → re_inject: 提取最近文件路径 + Skills → System 消息重新注入
```

## 技术方案总结

| 维度 | 选型 |
|------|------|
| Micro-compact | 可压缩白名单 + 时间衰减 + 图片/文档替换 + 工具对保护 |
| Full Compact | 9 段摘要模板 + LLM 调用 + PTL 降级重试 |
| 重新注入 | extract_recent_files() + extract_skills_paths() → System 消息 |
| 配置 | CompactConfig 支持环境变量覆盖 |
| 核心层分离 | 纯消息操作在核心层，TUI 层仅触发和展示 |

## Feature 附录

### feature_20260428_F001_compact-redesign
**摘要:** 全面增强 Micro/Full Compact 策略与压缩后重新注入
**关键决策:**
- Micro-compact 引入可压缩工具白名单 + 时间衰减清除策略
- Full Compact 采用 9 段结构化摘要模板对齐 Claude Code
- 压缩后重新注入最近读取文件和激活 Skills（System 消息形式）
- 工具对完整性保护：确保 tool_use 和 tool_result 不被拆开
- PTL 降级重试：按消息步数组逐步删除最旧组，最多重试 3 次
- CompactConfig 通过 settings.json 配置，环境变量可覆盖
- 核心层实现纯消息操作，TUI 层仅负责触发和 UI 展示
**归档:** [链接](../../archive/feature_20260428_F001_compact-redesign/)
**归档日期:** 2026-04-30

---

## Issue 经验附录

### issue_2026-05-11-auto-compact-no-resubmit
**摘要:** Auto Compact 后 Agent 未自动 Resubmit 继续执行
**状态:** Fixed + Verify
**归档日期:** 2026-05-13
**关键词:** last_user_input, auto-compact, resubmit, 状态保留
**问题本质:** last_user_input 在 compact 异步执行期间可能为 None 或被覆盖，导致 handle_compact_done 的 resubmit 被静默跳过，无任何日志或用户提示
**通用模式:** 跨异步操作的状态依赖（如 compact 后需要原始输入 resubmit）应在操作开始时保存到独立字段，防止异步执行期间被清理。静默跳过关键操作（如 resubmit）是危险的，应至少记录 warn 日志
**技术决策:** compact 开始时保存 last_user_input 到独立字段，防止异步期间被清理
**涉及文件:** peri-tui/src/app/agent_compact.rs, peri-tui/src/app/agent_submit.rs, peri-tui/src/app/agent_ops.rs, peri-tui/src/app/agent_comm.rs
**CLAUDE.md 链接:** false

### issue_2026-05-12-compact-auto-continue-scenarios
**摘要:** Compact 自动继续功能在不应触发的场景（手动 /compact、Done 后 auto-compact）下仍然 resubmit
**状态:** Fixed
**归档日期:** 2026-05-20
**关键词:** auto-continue, compact 触发来源, resubmit 控制, instructions 参数
**问题本质:** handle_compact_done 的 resubmit 逻辑不区分 compact 触发来源——手动 /compact 和 Done 后 auto-compact 也被错误地 resubmit。用户手动压缩后期望停下来查看结果，agent 完成任务后 compact 再用原始输入重新执行没有意义。
**通用模式:** 异步操作的触发来源（auto vs manual）需要作为上下文传递到完成后处理逻辑。用 instructions 参数区分来源，通过独立 flag（compact_should_resubmit）控制后续行为。两个合理的 resubmit 场景（auto-compact 在 agent 执行中、后台任务完成后）和两个不合理的场景（手动 compact、Done 后 compact）需要精确区分。
**涉及文件:** peri-tui/src/app/agent_compact.rs, peri-tui/src/app/agent_ops.rs, peri-tui/src/app/agent_comm.rs
**CLAUDE.md 链接:** false

### issue_2026-05-20-compact-command-not-triggering
**摘要:** /compact 命令作为普通文本发给 LLM，未触发压缩
**状态:** Fixed
**归档日期:** 2026-05-20
**关键词:** /compact 命令, ACP compact 通道, loading spinner, session 同步
**问题本质:** /compact 命令处理器未接入 ACP compact 管道，将命令文本当作普通用户消息发送给 LLM；compact 期间缺少 loading 状态和用户可见错误反馈
**通用模式:** 所有 TUI 命令必须通过正确的 ACP 协议通道（compact/set_model/set_mode 等）触发操作，不能将命令文本作为普通消息提交；compact 这类异步操作需要完整的 UI 状态管理（loading spinner + 错误反馈）
**架构影响:** Compact 触发路径统一收敛到 ACP compact 通道（acp_client.compact() → ACP Server → compact_runner），命令处理器和 auto-compact 虽触发点不同但最终汇合
**技术决策:** TUI 命令 → ACP client → ACP server → compact runner 的分层架构，命令处理器不直接操作 compact 逻辑
**涉及文件:** peri-tui/src/command/session/compact.rs, peri-tui/src/app/agent_compact.rs, peri-tui/src/app/agent_ops/polling.rs, peri-tui/src/app/agent_comm.rs, peri-tui/src/app/thread_ops.rs
**CLAUDE.md 链接:** true

### issue_2026-05-20-auto-compact-empty-messages-400
**摘要:** Auto compact 后 LLM 请求 messages 为空导致 400 错误
**状态:** Fixed
**归档日期:** 2026-05-20
**关键词:** compact messages 为空, BaseMessage::system vs human, LLM 适配器提取, DeepSeek 400
**问题本质:** Compact 摘要被放入 BaseMessage::system()，LLM 适配器（messages_to_json/messages_to_anthropic）将 System 消息提取到 system 字段不进入 messages 数组，导致发给 API 的 messages 数组为空
**通用模式:** 发给 LLM API 的 messages 数组必须始终包含至少一条非 System 消息（Human 或 Ai）；任何向消息列表插入的内容如果可能被 LLM 适配器提取到顶层字段（system、tools 等），必须验证剩余 messages 数组非空
**架构影响:** Compact 架构从「外层 loop + resubmit」改为「CompactMiddleware 作为 before_model 钩子在 ReAct 循环内原地处理」，消除了 compact 后独立 LLM 调用的脆弱性
**技术决策:** CompactMiddleware 替代 compact_runner 的 before_model 钩子模式，摘要始终使用 BaseMessage::human() 确保 LLM 适配器提取 System 后 messages 数组有效
**涉及文件:** peri-middlewares/src/compact_middleware.rs, peri-acp/src/session/compact_runner.rs, peri-acp/src/session/executor.rs, peri-tui/src/acp_server/compact.rs
**CLAUDE.md 链接:** true

### issue_2026-05-26-manual-compact-long-loading-skeleton
**摘要:** 手动 /compact 后聊天区域长时间显示 loading 骨架屏（30s+）
**状态:** Fixed
**归档日期:** 2026-05-26
**关键词:** compact loading, set_loading, manual vs auto compact
**问题本质:** handle_compact_completed() 在 full compact 路径故意不调 set_loading(false)——设计对 auto-compact 正确（executor 循环继续→Done 清除），对手动 compact 错误（独立操作无 Done 事件）
**通用模式:** 同一处理函数服务于两条执行路径时，必须区分路径语义。auto-compact 嵌套在 ReAct 循环内（有后续 Done/Error），manual compact 是独立操作（需自行清理 loading）。缺少路径标志导致状态泄漏。
**涉及文件:** peri-tui/src/app/agent_compact.rs, peri-tui/src/acp_server/compact.rs, peri-tui/src/app/agent_comm.rs, peri-tui/src/command/session/compact.rs
**CLAUDE.md 链接:** false

### issue_2026-05-23-micro-compact-repeated-triggering
**摘要:** Micro Compact 重复触发，每轮工具调用后都显示"自动清理"通知
**状态:** verify
**归档日期:** 2026-05-27
**关键词:** micro compact, repeated triggering, once-per-prompt guard, AtomicBool
**问题本质:** CompactMiddleware 缺少 once-per-prompt 守卫。micro compact 压缩量 < 新增量，永远降不到 70% 阈值以下，每轮都重复触发
**通用模式:** 有副作用的 per-prompt 操作（如 compact、通知）必须加 once-per-prompt 守卫。同一 execute_prompt 内只应触发一次 micro compact，之后由 full compact 接管
**技术决策:** 用 `AtomicBool` 做守卫——每次 execute_prompt 创建新 CompactMiddleware 实例，标志天然 per-prompt 作用域
**涉及文件:** peri-middlewares/src/compact_middleware.rs, peri-middlewares/src/compact_middleware_test.rs
**CLAUDE.md 链接:** true

### issue_2026-06-07-full-compact-loses-project-path-context

- **摘要:** Full Compact 后 Agent 使用错误的项目路径前缀
- **状态:** Fixed
- **归档日期:** 2026-06-11
- **关键词:** preprocess_messages, 工具参数丢失, cwd 注入, compact 摘要质量
- **问题本质:** `preprocess_messages` 处理 Ai 消息时只保留工具调用名称，完全丢弃参数（含 file_path 等路径信息），摘要 LLM 无法知道操作的是哪个文件
- **通用模式:** compact 摘要必须保留路径关键参数——工具名称不够，参数中的 file_path/path/command 是路径感知的核心信息
- **架构影响:** `full_compact()` 增加 `cwd` 参数为摘要 LLM 提供路径锚点；`extract_recent_files` 兼容 file_path 和 path 两种参数名
- **涉及文件:** peri-agent/src/agent/compact/full.rs, peri-agent/src/agent/compact/re_inject.rs, peri-middlewares/src/compact_middleware.rs

### issue_2026-06-02-system-reminder-compact-summary

- **摘要:** Compact 摘要包裹 `<system-reminder>` 标签，TUI 折叠展示，减少显示干扰
- **状态:** Fixed
- **归档日期:** 2026-06-14
- **关键词:** compact summary, system-reminder, TUI 折叠, UserBubble 渲染
- **问题本质:** Compact 后的摘要文本以完整 Human/UserBubble 渲染，占用大片显示空间；用户无法区分"真实用户输入"和"系统注入的上下文摘要"
- **通用模式:** 系统注入的上下文信息（如 compact 摘要）应与普通用户输入区分渲染——通过标签包裹让 TUI 识别并折叠为简略提示行，同时 LLM 侧已有 `14_system_reminder.md` 指导静默处理
- **技术决策:** Human 摘要消息包裹 `<system-reminder>` 标签；re_inject 产生的 System 消息保持原样不纳入标签；TUI 检测标签后默认折叠为单行 `📋 上下文已压缩（N 个文件，M 个技能）`，可展开查看详情
- **涉及文件:** peri-middlewares/src/compact_middleware.rs, peri-tui/src/ui/message_view/mod.rs, peri-tui/src/ui/message_render.rs, peri-tui/src/app/agent_ops/mod.rs
- **CLAUDE.md 链接:** false

### issue_2026-06-02-session-restore-compact-message-duplication

- **摘要:** Session 恢复后 compact 前后的消息同时存在，导致对话重复
- **状态:** Fixed
- **归档日期:** 2026-06-14
- **关键词:** session restore, 消息重复, compact 持久化, ThreadStore
- **问题本质:** 会话触发 compact 后关闭 TUI 再恢复，compact 前被移除的旧消息和 compact 后新消息同时出现。ThreadStore 持久化时不应把已被 compact 移除的旧消息也写入存储
- **通用模式:** 持久化消息时必须反映内存中 compact 后的实际消息状态——compact 已在内存中移除旧消息，持久化层应保存当前 state.messages() 的快照，而非追加/累积历史
- **涉及文件:** peri-tui/src/acp_server/prompt.rs, peri-middlewares/src/compact_middleware.rs
- **CLAUDE.md 链接:** false

### issue_2026-06-07-compact-breaks-rendering-selection-loading

- **摘要:** Compact 后文字拖选蓝色高亮消失 + Auto-compact 后 Loading spinner 丢失
- **状态:** Fixed
- **归档日期:** 2026-06-14
- **关键词:** compact, 文字选区, SELECTION_BG, loading spinner, compact_manual 标志, text_selection, wrap_map
- **问题本质:** 两个独立根因——(1) `handle_compact_started()` 对所有 compact（含 auto）无条件设 `compact_manual=true`，导致 auto-compact 完成后 `handle_compact_completed()` 错误清除 loading；(2) compact 触发的 `RebuildAll` 完全替换 view_messages 和 RenderCache 后，`text_selection` 中的旧 visual 坐标与新 `wrap_map` 错位，导致选区高亮消失和复制内容错乱
- **通用模式:** 同一处理函数服务多条执行路径（auto vs manual compact）时，必须区分路径语义设置标志；UI 状态（text_selection）与 RenderCache 存在隐式坐标依赖，RebuildAll 重建 RenderCache 后必须清除/重建 text_selection
- **技术决策:** 移除 `handle_compact_started` 中的 `compact_manual=true`，loading 统一由 `Done` 事件结束（manual compact 是 `CommandKind::Immediate`，executor 调用 `push_done()`）；compact 开始/完成时调用 `text_selection.clear()` 防止旧坐标与新 wrap_map 错位
- **涉及文件:** peri-tui/src/app/agent_compact.rs, peri-tui/src/ui/main_ui/message_area.rs, peri-tui/src/app/text_selection.rs, peri-tui/src/ui/render_thread.rs
- **CLAUDE.md 链接:** false

---

## 相关 Feature
- → [token-tracking.md](./token-tracking.md) — Token 追踪触发压缩
- → [tui.md](./tui.md) — TUI /compact 命令
