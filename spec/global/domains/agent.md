# Agent 领域

## 领域综述

Agent 领域是整个框架的核心，负责 ReAct 推理循环的执行、消息系统管理、工具抽象与执行、LLM 适配以及线程会话的持久化。

核心职责包括：

- ReAct 执行器：管理推理循环、工具分发、事件发射
- 消息类型系统：`BaseMessage`（Human/Ai/System/Tool，每条含 UUID v7 MessageId）、`ContentBlock` 完整变体、`MessageContent`
- LLM 适配层：OpenAI / Anthropic 双适配，`MessageAdapter` trait 支持双向格式转换，序列化时跳过 id 字段；LLM Factory 签名 `Fn(Option<&str>)` 支持子 Agent 独立模型
- Middleware Chain：横切关注点（Skills、HITL、SubAgent、SkillPreload、Cron 等）通过标准 trait 解耦
- 线程持久化：SQLite WAL 模式，sqlx SqlitePool 连接池（max=5）原生 async，`StateSnapshot` 事件驱动增量写入，message_id 为主键
- 声明式子 Agent：`.claude/agents/*.md` 定义 Explorer/WebResearcher 等专用 Agent，frontmatter 声明工具白名单、skills 预加载和独立模型；Fork 路径（fork: true）继承父 agent 完整上下文
- System Prompt：ReActAgent.with_system_prompt() 固定在 run_before_agent 后 prepend，消除 PrependSystemMiddleware 顺序约束
- 工具接口：ask_user_question（对齐 Claude AskUserQuestion 规范），questions 数组 + header + options.description
- 定时任务：CronMiddleware 提供 cron_register/cron_list/cron_remove 工具，croner 2 解析 cron 表达式
- 后台执行：Agent 工具 `run_in_background` 参数，BackgroundTaskRegistry 最多 3 并发，mpsc unbounded 通道通知，完成后 Human 消息注入，Done 后自动 continuation

## 核心流程

### ReAct 推理循环

```
AgentInput → add_message(Human)
  → chain.collect_tools(cwd)     ← ToolProvider 合并，手动注册优先级最高
  → chain.before_agent(state)    ← AgentsMd → Skills（prepend System）
  → loop(max_iterations=50):
      llm.generate_reasoning(messages, tools)
        stop_reason==ToolUse  → 工具调用分支
        stop_reason==EndTurn  → 最终回答
      state.add_message(Ai{tool_calls})
      for each tool_call:
        chain.before_tool()   ← HITL 在此拦截
        tool.invoke(input)    ← AskUser 在此挂起
        chain.after_tool()    ← Todo 解析结果
        emit(ToolStart/ToolEnd)
        state.add_message(Tool{result})
      emit(TextChunk)
  → chain.after_agent(state, output)
  → AgentOutput
```

### 消息持久化流程

```
StateSnapshot 事件触发
  → 过滤 System 消息（不持久化）
  → append_messages 事务写入 SQLite
  → WAL 模式保证 crash-safe
  → 下次 Agent 执行时 load_messages 恢复
```

### SubAgent 委派流程

```
launch_agent 工具调用
  → 查找 .claude/agents/{id}.md
  → 解析 frontmatter（system_prompt/tools/disallowedTools/maxTurns）
  → 过滤父工具集（无 tools → 全部继承；tools → 白名单；disallowed → 排除）
  → 创建子 ReActAgent（共享事件处理器）
  → 执行 → 返回工具调用摘要 + 最终回答
```

## 技术方案总结

| 维度 | 选型 |
|------|------|
| 持久化 | SQLite WAL，sqlx SqlitePool(max=5) 原生 async，`append_messages` 事务，message_id 为主键 |
| 消息 ID | UUID v7（时间有序，`uuid = "1"` features: v7 + serde），MessageId 封装，构造器自动填充 |
| LLM 适配 | OpenAI（streaming SSE）+ Anthropic（Prompt Cache / Extended Thinking）；序列化时跳过 message id 字段 |
| 消息格式 | `BaseMessage` ↔ `MessageAdapter` trait，`OpenAiAdapter` / `AnthropicAdapter` |
| Middleware | `Middleware<S>` trait（5 个钩子），`MiddlewareChain` 顺序执行 |
| 工具系统 | `BaseTool` trait，`ToolProvider` trait 动态提供，`register_tool` 优先级最高 |
| 错误处理 | LLM 层 `anyhow::Result`，工具层结构化错误信息（`is_error: true`） |
| 测试 | `MockLLM::tool_then_answer()` 脚本回放，无需真实 API |
| 子 Agent 中间件 | AgentsMdMiddleware → SkillsMiddleware → SkillPreloadMiddleware → TodoMiddleware → PrependSystemMiddleware |
| skill 预加载 | SkillPreloadMiddleware：fake read_file 工具调用+ToolResult 消息对注入，frontmatter.skills 声明 |
| System Prompt | ReActAgent.with_system_prompt()：内置字段，execute() 在 run_before_agent 之后固定 prepend；PrependSystemMiddleware 保留用于子 agent 动态 system builder |
| ask_user_question | 工具名对齐 Claude；questions 数组（1-4 个）；header 短标签；options.description；始终允许自定义输入 |
| 事件携带 message_id | TextChunk/ToolStart/ToolEnd 均携带 message_id，Web 前端可 update-in-place |
| LLM Factory | `Arc<dyn Fn(Option<&str>) -> Box<dyn ReactLLM>>`，支持 Option<&str> 参数传递子 Agent 独立模型标识 |
| 子 Agent 模型 | agent.md frontmatter model 字段（sonnet/opus/haiku/inherit），alias 解析在 TUI 层完成 |
| 定时任务 | CronMiddleware 提供 cron_register/cron_list/cron_remove 三个工具；croner 2 解析表达式；内存任务表上限 20 |
| 后台执行 | BackgroundTaskRegistry(max=3) + mpsc unbounded 通道；invoke_background() 不 await；结果注入为 Human 消息；Done 后保持通道存活 + 自动 continuation |
| 工具延迟加载 | 核心工具（12 个）直接加载，非核心工具通过 SearchExtraTools 按需发现、ExecuteExtraTool 代理执行；Prompt 缓存会话级 |
| Web 工具 | WebMiddleware 注入 WebFetch（HTML→Markdown）和 WebSearch（Tavily API），支持域名过滤和实时抓取 |
| trait 清理 | ReactLLM trait 移除废弃方法（generate_reasoning），统一为 generate()；废弃 trait 标记 #[deprecated] |

## Feature 附录

### 20260321_F001_subagents-execution

**摘要:** launch_agent 工具支持子 Agent 委派，防递归，工具过滤
**关键决策:**

- 工具过滤: tools 空→继承全部（除自身）；tools 有值→白名单；disallowedTools→黑名单
- 防递归: launch_agent 始终从子 agent 工具集中排除
- LLM 工厂: `Arc<dyn Fn() -> Box<dyn ReactLLM>>`，每次创建独立实例
- 事件透传: 子 agent 与父共享 `Arc<dyn AgentEventHandler>`
**归档:** [链接](../../archive/feature_20260321_F001_subagents-execution/)
**归档日期:** 2026-03-24

### 20260322_F001_agent-storage-refactor

**摘要:** SQLite WAL 持久化替代 JSONL，MessageAdapter 双向转换
**关键决策:**

- SQLite WAL 模式: journal_mode=WAL, synchronous=NORMAL，并发读写安全
- 串行写: `parking_lot::Mutex<Connection>` 持锁执行所有写操作
- 幂等追加: INSERT OR IGNORE，基于 seq 唯一约束，重复不报错
- MessageAdapter: OpenAI / Anthropic 双实现，BaseMessage ↔ Provider 原生 JSON
**归档:** [链接](../../archive/feature_20260322_F001_agent-storage-refactor/)
**归档日期:** 2026-03-24

### feature_20260325_F001_subagent-middleware-injection

**摘要:** 子 Agent 补全三个缺失中间件使上下文与父 Agent 一致
**关键决策:**

- 注入顺序：AgentsMdMiddleware → SkillsMiddleware → TodoMiddleware → PrependSystemMiddleware
- TodoMiddleware 的 todo_rx 立即丢弃，send 失败静默忽略（子 Agent 不通知 TUI）
- 有意省略：HitlMiddleware（子 Agent 自动执行）、SubAgentMiddleware（防递归）、AskUserTool
**归档:** [链接](../../archive/feature_20260325_F001_subagent-middleware-injection/)
**归档日期:** 2026-03-27

### feature_20260326_F001_specialized-agents

**摘要:** 预置 Explorer 和 WebResearcher 两个声明式专用 Agent
**关键决策:**

- 纯配置文件实现，无 Rust 代码改动
- explorer：只读工具（read_file/glob_files/search_files_rg/bash），disallowedTools 覆盖所有写操作
- web-researcher：bash + write_file + read_file，中间结果落盘 /tmp/research_*.md
- Agent 定义文件：.claude/agents/{id}.md，frontmatter 声明 tools/disallowedTools/maxTurns
**归档:** [链接](../../archive/feature_20260326_F001_specialized-agents/)
**归档日期:** 2026-03-27

### feature_20260326_F005_subagent-skill-preload

**摘要:** Agent 定义 frontmatter 声明 skills，子 Agent 启动时自动全文预加载
**关键决策:**

- AgentFrontmatter.skills: Vec<String>，默认空
- SkillPreloadMiddleware：before_agent 注入 fake read_file ToolUse + ToolResult 消息对
- fake ID 格式：skill_preload_{index}，不依赖 UUID
- 找不到的 skill 静默跳过；不经过 HitlMiddleware（预注入非真实调用）
**归档:** [链接](../../archive/feature_20260326_F005_subagent-skill-preload/)
**归档日期:** 2026-03-27

### feature_20260326_F006_message-uuid-v7

**摘要:** BaseMessage 四变体增加 UUID v7 全局唯一 ID
**关键决策:**

- MessageId(uuid::Uuid)，Default::default() 自动生成新 ID
- 所有构造器（human/ai/system/tool_result 等）自动填充 id
- Provider 适配层序列化时跳过 id 字段（LLM 不需要）
- SQLite Schema 重建：message_id TEXT PRIMARY KEY，移除 seq 列
**归档:** [链接](../../archive/feature_20260326_F006_message-uuid-v7/)
**归档日期:** 2026-03-27

### feature_20260328_F001_ask-user-question-align

**摘要:** ask_user 工具全面对齐 Claude AskUserQuestion 接口规范
**关键决策:**

- 工具名: ask_user → ask_user_question
- 顶层结构改为 questions 数组（1-4 个）；新增 header 字段（≤12字短标签）
- QuestionOption 新增 description 字段；移除 allow_custom_input/placeholder，始终允许自定义输入
- TUI 弹窗 Tab 使用 header；选项下方展示 description；前端 AskUserDialog.js 同步更新
**归档:** [链接](../../archive/feature_20260328_F001_ask-user-question-align/)
**归档日期:** 2026-03-28

### feature_20260327_M3_system-prompt

**摘要:** with_system_prompt() 方法消除 PrependSystemMiddleware 的注册顺序约束
**关键决策:**

- ReActAgent 新增 system_prompt: Option<String> 字段和 with_system_prompt() builder
- execute() 在 run_before_agent() 之后固定 prepend，不受中间件注册顺序影响
- 主 Agent 调用方改用 with_system_prompt()；PrependSystemMiddleware 保留（子 agent 动态场景仍可用）
**归档:** [链接](../../archive/feature_20260327_M3_system-prompt/)
**归档日期:** 2026-03-28

### feature_20260327_H3_interaction-unify

**摘要:** 提取 UserInteractionBroker trait 统一 HITL 和 AskUser 交互机制
**关键决策:**

- 新建 peri-agent/src/interaction/mod.rs：UserInteractionBroker trait + InteractionContext（Approval/Questions）
- HITL 和 AskUser 中间件均通过 broker.request() 等待响应，单 channel 替代两套
- TUI TuiInteractionBroker 实现；relay 协议从 4 条消息合并为 2 条（InteractionRequest/InteractionResponse）
- 两阶段迁移：先新增 broker，再删旧实现（此 feature 归档时尚未完全完成）
**归档:** [链接](../../archive/feature_20260327_H3_interaction-unify/)
**归档日期:** 2026-03-28

### feature_20260326_F009_relay-message-id-propagation

**摘要:** TextChunk/ToolStart/ToolEnd 事件携带 message_id 支持 Web 前端 update-in-place
**关键决策:**

- ExecutorEvent::TextChunk 改为结构体变体 { message_id, chunk }
- ExecutorEvent::ToolStart/ToolEnd 新增 message_id 字段
- TUI agent.rs 用 `..` 解构忽略 message_id，TUI AgentEvent 枚举不变
- Relay Server 无需修改（JSON 自动透传新字段）
**归档:** [链接](../../archive/feature_20260326_F009_relay-message-id-propagation/)
**归档日期:** 2026-03-27

### feature_20260330_F003_cron-loop-command

**摘要:** /loop /cron 定时任务系统，cron 表达式注册管理
**关键决策:**

- CronMiddleware 提供 cron_register/cron_list/cron_remove 三个工具供 AI 使用
- croner 2 crate 解析 cron 表达式并计算下次触发时间
- 内存任务表上限 20 条，TUI 重启后清空
- TUI /loop 命令注册定时任务，/cron 面板管理（导航/删除/切换启用）
- 定时触发时将 prompt 作为 AgentInput 提交到 agent task
**归档:** [链接](../../archive/feature_20260330_F003_cron-loop-command/)
**归档日期:** 2026-04-27

### feature_20260329_F005_legacy-cleanup

**摘要:** Agent trait 层级清理与废弃 API 移除
**关键决策:**

- ReactLLM trait 移除废弃方法（generate_reasoning），统一为 generate() 接口
- 废弃 trait 和方法标记 #[deprecated]，保留编译兼容
- 清理未使用的泛型参数和冗余类型别名
- 同步更新所有 impl 块和测试代码
**归档:** [链接](../../archive/feature_20260329_F005_legacy-cleanup/)
**归档日期:** 2026-04-27

### feature_20260329_F002_subagent-model-switch

**摘要:** 子 Agent 支持独立模型配置，LLM Factory 签名升级
**关键决策:**

- LLM Factory 签名升级为 `Fn(Option<&str>)`，参数传递子 Agent 模型标识
- agent.md frontmatter 新增 model 字段（sonnet/opus/haiku/inherit）
- inherit（默认）继承父 Agent 模型，其他值使用指定别名
- alias 解析在 TUI 层完成（ModelAliasMap），不侵入 core 层
- SkillFrontmatter 同步增加 model 文档字段
**归档:** [链接](../../archive/feature_20260329_F002_subagent-model-switch/)
**归档日期:** 2026-04-27

### feature_20260503_F003_background-agent

**摘要:** Agent 工具支持后台执行，主 agent 不阻塞，完成后通知注入
**关键决策:**

- run_in_background 参数触发 invoke_background()，不 await 子 agent
- BackgroundTaskRegistry 管理最多 3 并发后台任务
- mpsc::unbounded_channel 通知，ReAct 循环末尾 try_recv 消费
- 后台结果注入为 Human 消息（非 ToolResult），原始调用早已返回
- 主 agent Done 后保持通道存活，最后一个后台任务完成时自动 continuation
- BackgroundTaskResult 定义在核心层（保持依赖方向正确）
- 显示样式: ToolBlock 格式，bg:{agent_name} 工具名，超长截断+折叠
**归档:** [链接](../../archive/feature_20260503_F003_background-agent/)
**归档日期:** 2026-05-04

### feature_20260503_F002_multi-agent-design

**摘要:** Fork 路径继承父 agent 上下文 + Agent prompt 指导扩写
**关键决策:**

- fork: true 触发 Fork 路径，子 agent 继承父 agent 完整消息历史 + system prompt + 工具集
- Fork 不读 agent 定义文件（subagent_type 参数被忽略）
- 继承实现：parent_messages.read().clone() 传递给子 agent state
- 结构化输出：子 agent 返回 Scope/Result/Key files/Files changed 四段格式
- Agent 工具 prompt 指导从 21 行扩写为完整指导（使用时机、prompt 写作、示例）
**归档:** [链接](../../archive/feature_20260503_F002_multi-agent-design/)
**归档日期:** 2026-05-04

### feature_20260430_F001_align-claude-code-tools

**摘要:** 10 个工具名称和参数结构完全对齐 Claude Code
**关键决策:**

- 工具名完全对齐：read_file→Read、write_file→Write、edit_file→Edit、glob_files→Glob、search_files_rg→Grep、launch_agent→Agent
- Grep 重大重构：args 数组→结构化字段（pattern/path/glob/type/output_mode/-i/-C/-n 等）
- Agent 对齐：prompt/description 模式，subagent_type/fork/run_in_background 参数
- folder_operations 保留为 Peri 扩展工具
- Read 新增 pages 参数（PDF 页范围）
**归档:** [链接](../../archive/feature_20260430_F001_align-claude-code-tools/)
**归档日期:** 2026-05-04

### feature_20260509_F001_tool-search

**摘要:** 工具延迟加载机制，核心工具直接加载，非核心工具按需发现和代理执行
**关键决策:**

- 核心工具（12 个）始终加载：Read/Write/Edit/Glob/Grep/folder_operations/Bash/WebFetch/WebSearch/Agent/AskUserQuestion/TodoWrite
- 非核心工具通过 SearchExtraTools 按需发现，ExecuteExtraTool 代理执行
- ToolProvider trait 动态提供工具集合，支持运行时扩展
- Prompt 缓存优化：deferred tools 提示词会话级缓存，保证 Anthropic cache 前缀稳定
**归档:** [链接](../../archive/feature_20260509_F001_tool-search/)
**归档日期:** 2026-05-13

### feature_20260505_F001_web-tools

**摘要:** WebFetch 和 WebSearch 工具集成，支持网络请求和搜索功能
**关键决策:**

- WebMiddleware 注入 WebFetch（HTML→Markdown）和 WebSearch（Tavily API）两个工具
- WebFetch 使用 web_reader MCP 工具抓取网页并转换为 Markdown
- WebSearch 使用 Tavily API 进行网络搜索，返回结构化结果
- 支持搜索结果过滤（允许/阻止域名）和实时抓取模式
**归档:** [链接](../../archive/feature_20260505_F001_web-tools/)
**归档日期:** 2026-05-13

---

## Issue 经验附录

### issue_2026-05-11-streaming-text-invisible-with-tools
**摘要:** 流式过程中 AI 文本不可见（工具调用场景）
**状态:** Fixed（待用户验证）
**归档日期:** 2026-05-13
**关键词:** AiReasoning, TextChunk, 事件类型语义, 流式渲染
**问题本质:** 工具前文本通过 AiReasoning 事件发射而非 TextChunk，TUI pipeline 将 AiReasoning 映射为 "Thought for N chars" 推理提示，不显示实际文本
**通用模式:** 核心框架的事件类型决定了 TUI pipeline 的处理路径。新增事件或修改事件语义时，必须同步检查 TUI 侧的事件映射层（agent.rs 的事件映射表）
**技术决策:** 工具前文本改用 TextChunk 发射，与最终回答走同一路径
**涉及文件:** peri-agent/src/agent/executor/tool_dispatch.rs, peri-agent/src/agent/executor/final_answer.rs, peri-tui/src/app/agent.rs, peri-tui/src/app/message_pipeline.rs
**CLAUDE.md 链接:** false

### issue_2026-05-12-background-agent-display-and-continuation-bugs
**摘要:** Background Agent 三个 Bug：显示消失、subagent_type 限制、continuation 不触发
**状态:** Fixed + Verify
**归档日期:** 2026-05-13
**关键词:** frozen_subagent_vms, continuation 竞态, fork+background, pending_bg_continuation
**问题本质:** 三个独立根因：(1) fork 检测优先于 background 导致走错路径且 background_task_count 泄漏；(2) frozen_subagent_vms 跨轮次膨胀导致错位替换；(3) pending_bg_continuation.take() 在 loading=true 时丢失
**通用模式:** 多语义叠加（fork+background）时需要明确的优先级和独立处理路径。跨轮次累积的数据结构（frozen_vms）必须有清理/去重机制。异步 take() + 条件检查应先检查条件再 take()，避免消费后丢弃
**架构影响:** frozen_subagent_vms 的 drain_subagent_stack() 方法规范了异常残留清理；pending_bg_continuation 的修复模式（先检查条件再 take）可作为异步状态消费的通用范式
**涉及文件:** peri-middlewares/src/subagent/tool.rs, peri-tui/src/app/agent_ops.rs, peri-tui/src/app/message_pipeline.rs
**CLAUDE.md 链接:** false

### issue_2026-05-11-background-agent-missing-tools
**摘要:** Background Agent 工具继承缺失——子 agent 仅能使用 TodoWrite
**状态:** Fixed + Verify
**归档日期:** 2026-05-13
**关键词:** SubAgent, 工具继承, register_tool, Arc 共享
**问题本质:** Background agent 的工具完全依赖 parent_tools 通过 register_tool 传递，tokio::spawn 闭包的 Arc 引用在 move 后可能失效
**通用模式:** Background agent 的 middleware 配置与 Normal 路径一致但工具来源不同（register_tool vs middleware 内部构建）。跨 async 边界的工具传递需要确保 Arc 引用的生命周期
**涉及文件:** peri-tui/src/app/agent.rs, peri-middlewares/src/subagent/tool.rs, peri-agent/src/agent/executor/mod.rs
**CLAUDE.md 链接:** false

### issue_2026-05-12-glm-reasoning-field-not-parsed
**摘要:** GLM 模型 reasoning 字段未被解析，thinking 内容跨轮次丢失
**状态:** Fixed
**归档日期:** 2026-05-13
**关键词:** reasoning, reasoning_content, GLM, OpenAI 兼容
**问题本质:** GLM 系列模型使用 `reasoning` 顶层字段而非 `reasoning_content`，代码只检查了后者。附带发现 invariant check 对并行 tool_calls 的合法消息序列产生误报
**通用模式:** OpenAI 兼容 API 的字段名存在 provider 差异。解析时应同时检查多个可能的字段名（or_else 链式），序列化时应同时回传多个字段以保持兼容。invariant check 应基于消息块而非逐条检查
**技术决策:** 解析侧 reasoning_content.or(reasoning) 双字段尝试；序列化侧同时设置两个字段；invariant check 改为连续块检查
**涉及文件:** peri-agent/src/llm/openai.rs, peri-agent/src/messages/adapters/openai.rs
**CLAUDE.md 链接:** true

### issue_2026-05-14-deepseek-anthropic-thinking-block-dropped
**摘要:** SkillPreloadMiddleware 注入的伪 assistant 消息不含 thinking block，DeepSeek API 400
**状态:** Fixed
**归档日期:** 2026-05-15
**关键词:** thinking block, redacted_thinking, SkillPreload, DeepSeek
**问题本质:** DeepSeek 要求 thinking 模式下所有 assistant 消息都必须回传 thinking block（含 signature），但 SkillPreloadMiddleware 构造的伪造消息天然不含 thinking。本地构造的 assistant 消息与 provider 的 thinking 回传约束不兼容。
**通用模式:** 手动构造的 assistant 消息必须考虑 provider 的 thinking 回传约束。在序列化层自动检测并注入 redacted_thinking 比在每个构造点修补更健壮——集中处理比分散修补更可靠。
**架构影响:** 使用 Anthropic 的 redacted_thinking 类型（opaque data 字段）作为"无 thinking 原文但有占位"的通用解决方案
**技术决策:** messages_to_anthropic() 中检测不含 thinking/redacted_thinking 的 assistant 消息自动注入 redacted_thinking
**涉及文件:** peri-middlewares/src/subagent/skill_preload.rs, peri-agent/src/llm/anthropic.rs
**CLAUDE.md 链接:** true

### issue_2026-05-14-orphaned-tool-use-without-tool-result
**摘要:** 并发工具执行中部分路径提前返回导致 tool_result 缺失，Anthropic API 400
**状态:** Fixed
**归档日期:** 2026-05-15
**关键词:** tool_result闭合, 并发工具, deferred_error, 孤儿tool_use
**问题本质:** 并发工具执行的结果处理循环中，P3（run_on_error 错误传播）和 P4（run_after_tool 返回 Err）路径通过 `?` 提前跳出循环，导致后续工具的 tool_result 未写入 state。Anthropic API 要求所有 tool_use 必须在下一条消息中有对应 tool_result（闭合跟随规则），缺失即 400。
**通用模式:** 多工具并发的结果处理必须"尽最大努力收集所有结果，延迟错误传播"（collect-all-before-error 模式）。在循环中收集 deferred_error，循环结束后统一判断是否报错。所有 tool_result 必须始终写入（包括 error tool_result）。
**架构影响:** deferred_error 模式——将所有错误收集到 Option<AgentError>，循环结束后统一返回。这适用于所有需要"处理所有元素后再决定成败"的并发场景
**技术决策:** run_on_error/run_after_tool 失败改为 let _ = 吞掉并收集到 deferred_error；state.add_message(tool_msg) 始终执行
**涉及文件:** peri-agent/src/agent/executor/tool_dispatch.rs
**CLAUDE.md 链接:** true

### issue_2026-05-14-grep-tool-capability-gap
**摘要:** Grep 工具声明参数未实现 + 标准 grep 能力缺失
**状态:** Fixed
**归档日期:** 2026-05-15
**关键词:** Grep工具, 参数声明, 接口契约, 工具标准能力
**问题本质:** 工具暴露给 LLM 的 JSON schema 参数（multiline/-n/whole_word）声称可用但实际未实现，导致 LLM 写出正确的跨行正则却得到错误结果。本质是接口契约不匹配——声明与实现不同步。
**通用模式:** 所有暴露给 LLM 的工具参数必须经过实现验证。工具 schema 是对 LLM 的接口契约，任何声称支持的参数必须有对应的代码路径。新增参数时必须同步实现。
**技术决策:** output_mode 从必填改为默认 "content"，减少 LLM 调用负担
**涉及文件:** peri-middlewares/src/tools/filesystem/grep.rs
**CLAUDE.md 链接:** false

### issue_2026-05-12-thinking-reasoning-dataflow-issues
**摘要:** Thinking/Reasoning数据流：占位thinking缺signature + AiReasoning死代码
**状态:** Fixed
**归档日期:** 2026-05-16
**关键词:** thinking block, reasoning_content, AiReasoning, 死代码清理
**问题本质:** Anthropic extended thinking的占位thinking block缺少signature字段可能导致API拒绝；AiReasoning事件链路是为流式API预留的未使用代码，非流式下reasoning完全依赖StateSnapshot路径
**通用模式:** LLM适配器中多模型兼容字段需按provider条件注入，不可凭空伪造；预留接口若长期未使用应清理或显式标记
**架构影响:** 非流式API下reasoning通过source_message保留而非流式事件，流式路径的预留在当前架构下不必要
**技术决策:** 删除占位thinking注入逻辑；保留AiReasoning事件定义但明确标记为预留
**涉及文件:** peri-agent/src/llm/anthropic.rs, peri-agent/src/llm/openai.rs, peri-agent/src/agent/executor/tool_dispatch.rs, peri-agent/src/agent/executor/final_answer.rs, peri-agent/src/agent/events.rs, peri-tui/src/app/message_pipeline.rs, peri-tui/src/ui/message_view.rs
**CLAUDE.md 链接:** true

### issue_2026-05-14-deepseek-multi-turn-tool-result-duplication
**摘要:** DeepSeek多轮对话中agent_state_messages消息重复导致API 400错误
**状态:** Fixed
**归档日期:** 2026-05-16
**关键词:** prepend_message, StateSnapshot, last_message_count, 消息重复
**问题本质:** prepend_message的insert(0)使last_message_count索引失效，StateSnapshot捕获范围扩大，旧消息被重复extend到agent_state_messages
**通用模式:** 任何插入操作后必须补偿依赖索引的偏移量；基于计数的索引不应在插入/删除操作后继续使用
**架构影响:** StateSnapshot的增量扩展机制对prepend敏感，长期应考虑基于消息ID的标记替代数组索引
**技术决策:** 在prepend_message后补偿last_message_count += 1
**涉及文件:** peri-agent/src/agent/executor/mod.rs, peri-agent/src/agent/state.rs, peri-tui/src/app/agent_ops.rs
**CLAUDE.md 链接:** true

### issue_2026-05-15-glm-anthropic-tool-result-id-attribute-error
**摘要:** GLM Anthropic兼容端口tool_result block缺少id属性导致500错误
**状态:** Fixed
**归档日期:** 2026-05-16
**关键词:** tool_result id, GLM兼容性, Anthropic适配器, 第三方API
**问题本质:** 第三方Anthropic兼容端口对API规范实现不完整，GLM网关要求tool_result有id字段但Anthropic规范无此要求
**通用模式:** 第三方provider的Anthropic兼容端口可能存在属性缺失或额外要求，需客户端兼容策略
**架构影响:** Anthropic适配器需为不同provider准备兼容字段，不能假设所有provider严格遵循规范
**技术决策:** 为tool_result添加可选id字段，BaseMessage::Tool路径用MessageId，ContentBlock路径用UUID v7
**涉及文件:** peri-agent/src/messages/content.rs, peri-agent/src/llm/anthropic/invoke.rs, peri-agent/src/messages/adapters/anthropic.rs
**CLAUDE.md 链接:** false

### issue_2026-05-15-orphaned-tool-use-after-concurrent-tool-error
**摘要:** stop_reason与内容不一致导致孤儿tool_use触发Anthropic API 400
**状态:** Fixed
**归档日期:** 2026-05-16
**关键词:** stop_reason, tool_use, 孤儿tool_use, 延迟写入
**问题本质:** 第三方provider的stop_reason与实际内容不一致（end_turn但含tool_use），仅依赖stop_reason路由导致工具调用误入最终回答路径
**通用模式:** 不能仅信任API元数据字段，必须同时检查实际内容；defense-in-depth通过内容自检兜底
**架构影响:** 延迟写入重构消除了tool_dispatch中的flush路径脆弱性（4次因此bug修复）
**技术决策:** generate_reasoning中增加has_tool_calls()内容检查作为stop_reason的兜底
**涉及文件:** peri-agent/src/llm/anthropic/invoke.rs, peri-agent/src/llm/react_adapter.rs, peri-agent/src/agent/executor/tool_dispatch.rs
**CLAUDE.md 链接:** true

### issue_2026-05-15-tool-execution-error-stops-agent
**摘要:** 工具调用参数错误导致Agent停止而非自动重试
**状态:** Fixed — 部分修复
**归档日期:** 2026-05-16
**关键词:** deferred_error, ToolExecutionFailed, after_tool错误, 工具错误处理
**问题本质:** 工具执行错误和中间件错误被统一设为deferred_error导致Agent停止，应区分错误来源：工具执行错误应反馈给LLM而非终止循环
**通用模式:** 工具执行层面的错误（参数缺失、执行失败）是正常流程的一部分，应创建error ToolResult让LLM自行修正；只有基础设施错误才应终止
**架构影响:** deferred_error机制需要细化分类，区分tool-level error（不终止）和middleware error（可能终止）
**技术决策:** ToolNotFound和ToolExecutionFailed不再设deferred_error；after_tool中间件错误仍设deferred_error（残留问题）
**涉及文件:** peri-agent/src/agent/executor/tool_dispatch.rs
**CLAUDE.md 链接:** true

### issue_2026-05-15-write-tool-missing-filepath-max-tokens
**摘要:** Write工具超长内容触发max_tokens截断导致file_path缺失
**状态:** Fixed
**归档日期:** 2026-05-16
**关键词:** max_tokens, Write工具, JSON截断, file_path缺失
**问题本质:** max_tokens不足导致流式JSON参数截断，关键字段file_path可能因为字段顺序靠后而缺失
**通用模式:** 流式JSON生成中max_tokens截断导致字段缺失是不可恢复的错误；工具定义中关键字段需优先输出
**架构影响:** 工具Schema中字段顺序影响截断时的完整性；超长内容应考虑分块策略
**涉及文件:** peri-middlewares/src/tools/filesystem/write.rs, peri-agent/src/llm/anthropic/invoke.rs, peri-agent/src/llm/openai/invoke.rs
**CLAUDE.md 链接:** false

### issue_2026-05-16-concurrent-subagent-tool-call-routing-and-background

**摘要:** 并发 SubAgent 工具调用路由错误 + 死锁修复
**状态:** Fixed
**归档日期:** 2026-05-17
**关键词:** source_agent_id routing, SubAgent 并发, streaming cancellation, agent_id 匹配, 通道容量
**问题本质:** 并发 SubAgent 场景下的四个系统性缺陷：(1) `subagent_stack.last_mut()` 位置路由将所有内部事件路由到最后一个 SubAgent；(2) LLM 流式期间不检查取消令牌导致 Ctrl+C 死锁；(3) `mpsc::channel(256)` 容量不足以承载 SubAgent 500+ 事件导致静默丢弃；(4) 同名 SubAgent 的 `find(|s| s.agent_id == target)` 永远命中第一个导致 `is_running` 不清零
**通用模式:** 事件路由必须使用唯一标识（agent_id）而非位置索引；流式循环必须通过 `tokio::select!` 竞争取消令牌和 stream.next()；事件通道容量应基于 SubAgent 速率而非主 Agent；同名实体匹配需加状态条件（如 `is_running`）
**架构影响:** `source_agent_id` 字段为所有事件类型添加了精确路由能力，从此 SubAgent 路由不再依赖位置堆栈；`deferred_error` 模式在 tool_dispatch 中成熟
**技术决策:** Agent 工具改为顺序执行消除并发争用（非 Agent 工具保持并发）；通道容量 256→4096；`SourceAgentIdHandler` 包装器注入子 Agent 事件标记
**涉及文件:** peri-agent/src/agent/events.rs, peri-agent/src/agent/executor/tool_dispatch.rs, peri-agent/src/agent/executor/llm_step.rs, peri-agent/src/llm/types.rs, peri-agent/src/llm/anthropic/stream.rs, peri-agent/src/llm/openai/stream.rs, peri-middlewares/src/subagent/tool.rs, peri-tui/src/app/agent.rs, peri-tui/src/app/agent_ops.rs, peri-tui/src/app/agent_submit.rs, peri-tui/src/app/message_pipeline.rs
**CLAUDE.md 链接:** true

### issue_2026-05-14-llm-adapter-modularization

**摘要:** LLM 适配器模块化：anthropic.rs 1983 行、openai.rs 1065 行
**状态:** Fixed
**归档日期:** 2026-05-17
**关键词:** LLM 适配器, 模块化, 大文件拆分, anthropic, openai
**问题本质:** anthropic.rs（1983 行）和 openai.rs（1065 行）承载完整适配器实现：构造器、序列化、缓存策略、API invoke、流式处理、消息转换——职责过重，修改任一环节需阅读整个文件
**通用模式:** 按职责维度拆分大文件（构造器 + 缓存 + invoke + 流式），保留原文件路径 re-export 向后兼容
**技术决策:** 统一子模块结构：anthropic/{mod, cache, invoke, stream}、openai/{mod, invoke, stream}，上游直接 import 新路径
**涉及文件:** peri-agent/src/llm/anthropic.rs, peri-agent/src/llm/openai.rs, peri-agent/src/llm/mod.rs
**CLAUDE.md 链接:** false

### issue_2026-05-13-background-task-completion-race-condition

**摘要:** Background task 完成后未触发 agent continuation（竞态条件）
**状态:** Fixed
**归档日期:** 2026-05-17
**关键词:** background task, 竞态条件, continuation, agent_done_pending_bg, 时序耦合
**问题本质:** BackgroundTaskCompleted 和 Done 通过同一 channel 传递，存在竞态：如果后台任务在 Done 之前完成，BackgroundTaskCompleted 先被消费，此时 agent_done_pending_bg 尚未设置，导致 continuation 永不触发
**通用模式:** 依赖时序耦合的双事件模式（先 A 后 B）必须处理乱序到达。解决方案：在"先到"事件中暂存结果，在"后到"事件中检查暂存区——用空间（pre_done_bg_completions 缓冲）换时间鲁棒性
**技术决策:** 新增 pre_done_bg_completions 字段缓存 Done 前完成的后台任务通知；Done/Error 处理时检查暂存区并设置 pending_bg_continuation
**涉及文件:** peri-tui/src/app/agent_comm.rs, peri-tui/src/app/agent_events_bg.rs, peri-tui/src/app/agent_ops.rs, peri-tui/src/app/agent_submit.rs, peri-tui/src/app/agent_compact.rs, peri-tui/src/ui/headless_test.rs
**CLAUDE.md 链接:** false

### issue_2026-05-18-agent-tool-calls-execute-serially

**摘要:** 多 Agent 工具调用串行执行而非并发
**状态:** Fixed
**归档日期:** 2026-05-18
**关键词:** SubAgent 并发, child_handler_factory, tool_dispatch, join_all
**问题本质:** 为防止并发 SubAgent 死锁而硬编码了 Agent 工具的串行执行循环。但在三个死锁根因（LLM 流式取消 tokio::select!、4096 事件通道缓冲、source_agent_id 精确路由）被独立修复后，串行限制不再必要。
**通用模式:** 临时安全措施（串行化/锁）应有明确的前置条件检查和回退计划。当安全措施引入性能退化时，应追踪其依赖的前置条件修复进度，条件满足后立即移除。
**架构影响:** child_handler_factory 模式使每个 SubAgent 获得独立 event handler，消除共享 Mutex 竞争，是实现多 SubAgent 真正并发的关键抽象。tool_dispatch 的统一 join_all 路径避免了对特定工具类型的特殊处理。
**涉及文件:** peri-agent/src/agent/executor/tool_dispatch.rs, peri-middlewares/src/subagent/tool/define.rs, peri-tui/src/app/agent.rs
**CLAUDE.md 链接:** true

### issue_2026-05-19-concurrent-subagent-duplicate-id

**摘要:** 并发同类型 SubAgent 共享相同 ID，导致事件路由错误到第一个实例
**状态:** Fixed
**归档日期:** 2026-05-20
**关键词:** SubAgent ID 重复, 并发路由, tool_call_id, 身份传播
**问题本质:** 四级链路中每一层都用 subagent_type（类型名）替代唯一实例 ID——LLM 生成的唯一 tool_call_id 被映射层 `..` 丢弃，SourceAgentIdHandler 用 subagent_type 做 source_agent_id，Pipeline 用 subagent_type 做 routing key 和 pending_tools key。并发两个相同类型的 SubAgent 得到完全相同的标识，所有事件路由到第一个实例。
**通用模式:** 任何由 LLM 生成的唯一标识（如 tool_call_id）必须在整条事件链路中保持，不能被中间层丢弃或替换为类型名。并发场景下，类型名不能替代实例 ID。事件路由必须用唯一实例 ID 精确匹配，不能用 `find()` 按类型返回第一个匹配项。
**架构影响:** 四级链路的身份传播失败暴露了事件系统的设计缺陷——每一层都在重新生成或替换标识符，而非透传。修复需将 tool_call_id 贯穿 4 层（define → agent.rs 映射 → events.rs 字段 → Pipeline routing），agent_id 降级为仅用于显示。
**涉及文件:** peri-middlewares/src/subagent/tool/define.rs, peri-middlewares/src/subagent/tool/mod.rs, peri-tui/src/app/agent.rs, peri-tui/src/app/events.rs, peri-tui/src/app/message_pipeline/mod.rs
**CLAUDE.md 链接:** true

### issue_2026-05-24-build-agent-per-turn-arc-transient-fragmentation
**摘要:** build_agent 每轮重建大对象产生瞬态分配碎片
**状态:** Fixed
**归档日期:** 2026-05-24
**关键词:** AgentPool, LLM实例复用, jemalloc碎片, reqwest Client缓存
**问题本质:** 每轮 prompt 都全量重建 ReActAgent + 16 个 middleware + LLM 实例，drop 时产生大量瞬态 malloc/free 导致 jemalloc arena 碎片化
**通用模式:** 高频创建/销毁的重对象（LLM 实例含 reqwest Client + TLS）必须 session 级缓存；用 provider fingerprint 做惰性 invalidation 替代显式 invalidate
**架构影响:** 引入 AgentPool session 级缓存模式，跨 prompt 复用 LLM 实例；为 stateful middleware 添加 reset() 方法支持跨 turn 复用准备
**技术决策:** 惰性 invalidation（fingerprint 检测）优于显式 invalidate（需遍历所有修改路径）
**涉及文件:** peri-acp/src/session/agent_pool.rs, peri-acp/src/session/executor.rs:278, peri-acp/src/agent/builder.rs:94-417, peri-agent/src/agent/executor/mod.rs
**CLAUDE.md 链接:** true

### issue_2026-05-23-background-agent-card-disappears-no-result
**摘要:** Background Agent 完成后 SubAgent 卡片消失且无数据回传
**状态:** Fixed
**归档日期:** 2026-05-24
**关键词:** Background Agent, SubagentStarted, bg_event_sender, 独立通道
**问题本质:** 三层叠加——SubagentStarted 缺 is_background 字段、事件通道随 executor 生命周期销毁、双路径交付导致 revert 后功能退化
**通用模式:** Background task 的生命周期必须独立于发起它的 executor；事件通道需要独立于 executor 存活（unbounded channel）；单路径交付消除重复根因
**涉及文件:** peri-agent/src/agent/events.rs, peri-middlewares/src/subagent/tool/define.rs, peri-acp/src/agent/builder.rs, peri-acp/src/session/executor.rs, peri-tui/src/app/agent.rs
**CLAUDE.md 链接:** false

### issue_2026-05-25-interrupt-undo-last-user-message

**摘要:** Ctrl+C 中断后支持撤回并重发上一条用户消息
**状态:** 已完成（5 层修复，已验证）
**归档日期:** 2026-05-25
**关键词:** Ctrl+C 中断, 消息撤回, 事件路由, 历史回滚, 索引漂移
**问题本质:** 取消事件被错误路由到 Error 处理器（而非 Interrupted），消息撤回路径从未生效；缓存的 `round_start_vm_idx` 在 Pipeline RebuildAll 后失效
**通用模式:** (1) 取消/中断语义必须独立于 Error，事件路由精确匹配 (2) 消息位置查找用 `rposition` 在 `view_messages` 中实时搜索，不依赖缓存索引 (3) 状态回滚（`state.history.truncate`）保证 ACP 层一致性
**架构影响:** 触发了 5 层从 ACP Server → 事件路由 → 行为分叉 → VM 定位 → ephemeral_notes 过滤的纵贯修复
**涉及文件:** acp_server/prompt.rs, agent.rs, agent_ops/lifecycle.rs, mod.rs, agent_render.rs
**CLAUDE.md 链接:** true

### issue_2026-05-24-concurrent-bg-agent-only-one-completion

**摘要:** 并发 Background Agent 只收到一次完成通知，父 Agent 永久等待
**状态:** 完成
**归档日期:** 2026-05-25
**关键词:** 并发 background agent, TOCTOU, 事件丢失, 竞态
**问题本质:** `register()` 的计数检查与 `insert` 分两步执行（TOCTOU 窗口），两个并发 bg agent 可能同时通过检查；`SubagentStarted` 事件在注册前发送，注册失败留下幽灵计数
**通用模式:** (1) 计数器和 map 插入在**同一持锁临界区**内完成 (2) 事件通知必须在状态变更**成功后**发送，注册失败不发事件 (3) 同名 agent 匹配需两遍查找——优先精确匹配（`final_result.is_none()`），兜底回退
**涉及文件:** peri-middlewares/src/subagent/tool/define.rs, background.rs, agent_events_bg.rs
**CLAUDE.md 链接:** true

### issue_2026-05-26-sync-subagent-cancel-fix-attempts-log
**摘要:** 同步 SubAgent Ctrl+C 中断——handle_interrupted() 的 in_subagent() 守卫静默吞掉父 Agent 中断事件
**状态:** Fixed
**归档日期:** 2026-05-26
**关键词:** SubAgent Ctrl+C, handle_interrupted, in_subagent guard, event routing
**问题本质:** `in_subagent()` 守卫设计意图是忽略子 agent 自身的中断，但错误地捕获了父 agent 在 sync SubAgent 执行期间的 Ctrl+C 中断——信号链路全部正确，问题在末端事件处理层静默丢弃
**通用模式:** "UI 卡住"不等于"信号没到"。症状和根因可能在不同层级。二分法追踪比深度假设更高效——从信号链中点开始追踪，而非起点或终点。一次到位的完整诊断 > 多轮逐步追踪。
**架构影响:** in_subagent() 守卫的语义需要区分"子 agent 被取消"和"父 agent 在等待子 agent 时被取消"两种场景。新增事件守卫时必须考虑所有触发路径。
**涉及文件:** peri-tui/src/app/agent_ops/lifecycle.rs, peri-agent/src/agent/tool_dispatch.rs, peri-tui/src/app/agent.rs
**CLAUDE.md 链接:** true

### issue_2026-05-25-fake-read-tool-message-anthropic-400
**摘要:** AtMention/SkillPreload 注入的 fake Read 工具消息导致 Anthropic API 400 错误
**状态:** Fixed
**归档日期:** 2026-05-26
**关键词:** Anthropic 400, fake Read, tool_result, messages_to_anthropic
**问题本质:** middleware 注入 Ai[ToolUse] → Tool[ToolResult] 消息序列时，Anthropic 适配器将 Tool 消息转为 user role 的 tool_result block，如果成为 messages[0] 则违反 Anthropic API 约束
**通用模式:** Anthropic 和 OpenAI 的 Tool 消息格式差异导致消息注入类 middleware 在两个 API 上的行为不同。所有在消息历史中注入 fake tool 交互的 middleware 必须确保消息不会成为 API messages 数组的第一条。
**架构影响:** fake Read 消息注入是跨 middleware 的通用模式（AtMention、SkillPreload），Anthropic 适配器需对此做防御性处理。
**涉及文件:** peri-middlewares/src/at_mention/mod.rs, peri-middlewares/src/subagent/skill_preload.rs, peri-agent/src/llm/anthropic/invoke.rs
**CLAUDE.md 链接:** false

### issue_2026-05-25-skill-preload-no-tool-calls-in-history
**摘要:** 主 Agent SkillPreloadMiddleware preload_skills 硬编码为空，/skill-name 不注入全文
**状态:** Closed/Fixed
**归档日期:** 2026-05-26
**关键词:** SkillPreloadMiddleware, fake Read, preload_skills, middleware self-detection
**问题本质:** executor 构建主 Agent 时 preload_skills 硬编码 Vec::new()，导致 before_agent early return；SubAgent 路径通过 frontmatter skills 字段正确传递
**通用模式:** 主 Agent 与 SubAgent 的 middleware 初始化路径可能不同步。主 Agent 特有功能应优先使用 middleware 自检测模式（从消息内容推断），而非依赖外部传参。
**涉及文件:** peri-acp/src/session/executor.rs, peri-acp/src/agent/builder.rs, peri-middlewares/src/subagent/skill_preload.rs, peri-tui/src/app/agent_submit.rs
**CLAUDE.md 链接:** false

---

### issue_2026-05-26-skillpreload-anthropic-400-tool-result-orphan
**摘要:** SkillPreload 触发 Anthropic 400 Bad Request：tool_result 缺少配对 tool_use
**状态:** Fixed
**归档日期:** 2026-05-27
**关键词:** prepended_ids, add_message vs prepend_message, Anthropic 400, tool_result orphan
**问题本质:** `prepended_ids` 用 `len_after - len_before` 计算 prepend 数量，把 `add_message`（尾部追加）也计入，导致 cleanup 误删头部原始配对消息，产生孤儿 tool_result
**通用模式:** 中间件消息注入有两种语义：`prepend_message`（头部插入 System，需 cleanup）和 `add_message`（尾部追加 Ai/Tool，是正式历史）。cleanup 逻辑必须只追踪 prepend 路径，用 `take_while(|m| m.is_system())` 而非计数差
**涉及文件:** peri-agent/src/agent/executor/mod.rs, peri-middlewares/src/subagent/skill_preload.rs
**CLAUDE.md 链接:** true

### issue_2026-05-26-ctrl-c-interrupt-causes-agent-amnesia
**摘要:** Ctrl+C 中断后继续对话时 agent 丢失当前轮次上下文
**状态:** Fixed
**归档日期:** 2026-05-27
**关键词:** Ctrl+C interrupt, agent amnesia, history truncation, cancelled state
**问题本质:** ACP server 在 result.ok==false 时无条件 truncate history，丢弃了 agent 已写入 state 的当前轮次消息。TUI 显示正常但 agent 无上下文
**通用模式:** 取消操作应保留部分进展——检查 agent 在取消前是否有有效产出，有则保留而非全部回滚。deferred write 模式保证 cancel 后 state 合法性
**涉及文件:** peri-tui/src/acp_server/prompt.rs
**CLAUDE.md 链接:** true

### issue_2026-05-25-ctrl-c-cannot-interrupt-sync-subagent
**摘要:** Ctrl+C 无法中断同步 SubAgent，需等待其自然结束后父 Agent 才被中断
**状态:** Fixed
**归档日期:** 2026-05-27
**关键词:** Ctrl+C interrupt, sync SubAgent, cancel propagation, cancel token
**问题本质:** 父 Agent 的 cancel token 未传播到同步 SubAgent 的执行上下文，SubAgent 独立运行直到完成
**通用模式:** 所有 agent 执行路径（同步/异步/fork）必须共享同一个 cancel token 树，取消信号必须沿调用链传播
**涉及文件:** peri-middlewares/src/subagent/tool/define.rs
**CLAUDE.md 链接:** true

### issue_2026-05-27-language-injection-subagent-drift-cache-isolation
**摘要:** 语言段落注入导致 SubAgent 语言漂移和缓存隔离失效
**状态:** Fixed
**归档日期:** 2026-05-27
**关键词:** SubAgent language drift, frozen_language, cache isolation, last_idx fallback
**问题本质:** (1) SubAgent 从 peri_config.config.language 实时读取语言（非 frozen），/lang 切换后 SubAgent 语言变化而 Main Agent 不变；(2) Anthropic path 的 `i == last_idx` fallback 给动态 block 错误添加 cache_control；(3) session/load/resume/fork 丢失 frozen_language
**通用模式:** session/new 时冻结的所有数据必须通过 AcpAgentConfig 传递到 SubAgent 构建路径；缓存标记必须严格限定在静态前缀 block，不能有 fallback 到动态 block
**架构影响:** frozen data 传播链需显式设计：Main Agent builder → AcpAgentConfig → SubAgent builder，每新增一个 frozen 字段必须检查全链路
**涉及文件:** peri-agent/src/llm/anthropic/invoke.rs, peri-acp/src/agent/builder.rs, peri-acp/src/session/executor.rs, peri-tui/src/acp_server/requests.rs
**CLAUDE.md 链接:** true

---

### issue_2026-05-29-sse-utf8-truncation-mojibake
**摘要:** SSE 流式解析跨 chunk UTF-8 截断产生乱码（U+FFFD）
**状态:** Fixed
**归档日期:** 2026-05-29
**关键词:** SSE UTF-8 截断, from_utf8_lossy, pending_bytes, CJK 乱码
**问题本质:** SseParser 将 pending_line 存为 String，新 chunk 通过 from_utf8_lossy 不可逆替换不完整 UTF-8 序列为 U+FFFD，后续 chunk 到达无法恢复
**通用模式:** 流式协议中跨 chunk 的字节拼接必须在原始字节层完成，仅在行边界处做 UTF-8 解码。from_utf8_lossy 不可逆，不能用于中间状态
**技术决策:** pending_line: String → pending_bytes: Vec&lt;u8&gt;，字节级拼接 + 行边界整体验码
**涉及文件:** peri-agent/src/llm/sse.rs, peri-agent/src/llm/sse_test.rs
**CLAUDE.md 链接:** false

### issue_2026-05-29-immediate-command-missing-push-done
**摘要:** Immediate 命令（/compact、/clear）执行后 TUI 永久卡在 loading 状态
**状态:** Fixed
**归档日期:** 2026-05-29
**关键词:** push_done 缺失, Immediate 命令, 并发 prompt 竞争, AvailableCommandsUpdate
**问题本质:** ACP 命令系统重构后，Immediate 命令路径直接 return PromptResult 绕过了 event pump 的 push_done() 调用。缺少 AgentDone 事件 → TUI 永久 loading。同时 /clear 不发 StateSnapshot 导致旧视图残留。并发 prompt 竞争也需要 per-session Mutex 串行化。
**通用模式:** 任何绕过主循环的快捷路径必须手动补全主循环的清理步骤（push_done、StateSnapshot、loading 状态清理）。并发请求到同一 session 必须串行化。
**架构影响:** executor 中的 Immediate 命令路径、Compact 命令路径、Normal agent 路径需要统一生命周期管理
**涉及文件:** peri-acp/src/session/executor.rs, peri-acp/src/session/command/clear.rs, peri-acp/src/session/command/compact.rs, peri-tui/src/app/agent_ops/lifecycle.rs
**CLAUDE.md 链接:** true

### issue_2026-05-29-available-commands-update-format-mismatch
**摘要:** /compact 显示"未知命令"——AvailableCommandsUpdate 通知 JSON 格式不匹配被静默丢弃
**状态:** Fixed
**归档日期:** 2026-05-29
**关键词:** JSON 格式不一致, SessionNotification, notify.rs vs event_sink.rs, agent_commands HashSet
**问题本质:** 两条 session/update 发送路径（TransportEventSink vs notify.rs）使用不同的 JSON 结构。TUI bridge 统一用 params.get("update") 解析，后者被 warn 丢弃。
**通用模式:** 多个发送方必须统一输出格式，否则接收方无法正确解析。引入新发送方时必须对照已有路径的序列化格式
**涉及文件:** peri-tui/src/acp_server/notify.rs, peri-tui/src/app/agent_ops/acp_bridge.rs, peri-acp/src/session/event_sink.rs
**CLAUDE.md 链接:** false

### issue_2026-05-29-acp-session-update-field-name-mismatch
**摘要:** ACP 大重构后所有流式事件静默丢失——字段名 "type" vs "sessionUpdate" 不匹配
**状态:** Fixed
**归档日期:** 2026-05-29
**关键词:** serde tag 字段名, sessionUpdate vs type, 事件静默丢失, 流式失效
**问题本质:** SessionUpdate 枚举的 serde tag 配置为 #[serde(tag = "sessionUpdate")]，序列化后 JSON 结构为 {"sessionUpdate": "agent_thought_chunk"}，但 TUI bridge 使用 update.get("type") 解析——字段名不匹配导致所有流式事件被静默丢弃
**通用模式:** 枚举序列化的 tag 字段名必须与消费方的解析字段名一致。重构序列化格式时必须同步检查所有消费方
**涉及文件:** peri-tui/src/app/agent_ops/acp_bridge.rs, peri-acp/src/event/mapper.rs, peri-acp/src/session/event_sink.rs
**CLAUDE.md 链接:** false

### issue_2026-05-29-clear-keeps-acp-server-history
**摘要:** /clear 后 ACP Server 端 history 未清理，新会话延续旧上下文
**状态:** Fixed
**归档日期:** 2026-05-29
**关键词:** /clear session 泄漏, reset_session, new_thread, ACP session 状态不一致
**问题本质:** new_thread() 清空 TUI 本地状态但未清除 acp_client.current_session_id，下次 submit 复用旧 session，Agent 看到旧 history
**通用模式:** TUI 层清空本地状态不等于 ACP Server 端状态同步——必须同时通过 ACP 协议通知 Server 侧
**涉及文件:** peri-tui/src/acp_client/client.rs, peri-tui/src/app/thread_ops.rs, peri-tui/src/acp_server/mod.rs
**CLAUDE.md 链接:** true

### issue_2026-05-29-unify-token-usage-prompt-complete
**摘要:** 统一 Token Usage 传递：引入 prompt_complete 事件替代双路径冗余
**状态:** Fixed
**归档日期:** 2026-05-29
**关键词:** prompt_complete, token usage 双路径, UsageUpdate 有损, stopReason
**问题本质:** Token usage 通过两条路径传递（peri/agent_event 完整 + session/update 有损），TUI 和 IDE 各消费不同路径导致数据不一致
**通用模式:** 同一数据不应通过多条路径传递，应统一为单来源。多路径传递导致数据分叉和维护负担
**技术决策:** 引入 prompt_complete SessionUpdate 变体统一携带 stopReason + 完整 usage，废弃双路径模式
**涉及文件:** peri-acp/src/event/mapper.rs, peri-acp/src/session/event_sink.rs, peri-agent/src/agent/events.rs, peri-tui/src/app/agent.rs
**CLAUDE.md 链接:** false

### issue_2026-05-29-tool-end-name-lost-in-acp-bridge
**摘要:** ToolEnd 事件经 ACP bridge 后工具名丢失，显示为空字符串
**状态:** Fixed
**归档日期:** 2026-05-29
**关键词:** ToolEnd 工具名, ToolCallUpdate title, ACP event mapping, 字段遗漏
**问题本质:** ToolEnd 映射为 ToolCallUpdate 时缺少 .title(name) 调用，TUI bridge 硬编码 name: String::new()。双重遗漏导致工具名丢失
**通用模式:** 事件映射（ExecutorEvent → SessionUpdate → AgentEvent）每个环节都必须完整传递所有业务字段。新增映射路径时必须对照源事件的所有字段
**涉及文件:** peri-acp/src/event/mapper.rs, peri-tui/src/app/agent_ops/acp_bridge.rs, peri-acp/src/event/mapper_test.rs
**CLAUDE.md 链接:** false

### issue_2026-05-29-ask-user-tool-auto-complete

**摘要:** AskUserQuestion 弹窗出现后工具调用自行结束，用户操作无效
**状态:** Fixed
**归档日期:** 2026-05-31
**关键词:** AskUserQuestion, MultiplexBroker, 竞速, 空答案, Broker 选择
**问题本质:** MultiplexBroker 中 ChannelBroker 对 Questions 交互立即返回空答案，与 TUI broker 竞速导致空答案被采纳
**通用模式:** Broker/代理模式需为不同交互类型选择正确的后端；不支持特定交互类型的后端不应参与竞速
**架构影响:** MultiplexBroker 的设计需要按交互类型路由，而非简单竞速
**涉及文件:** peri-acp/src/agent/builder.rs, peri-acp/src/broker/transport_broker.rs, peri-tui/src/app/agent_ops_interaction.rs, peri-tui/src/app/ask_user_ops.rs
**CLAUDE.md 链接:** true

### issue_2026-05-27-windows-deepseek-skill-inject-thinking-400

**摘要:** Windows + DeepSeek Anthropic 兼容模式 /skill 注入假 Read 调用触发 thinking 400 错误
**状态:** Fixed
**归档日期:** 2026-05-31
**关键词:** thinking, DeepSeek, SkillPreload, Anthropic 兼容, 400 错误, 假消息
**问题本质:** SkillPreloadMiddleware 注入的假 Read 工具调用消息在 DeepSeek Anthropic 兼容模式下触发 thinking 回传校验失败
**通用模式:** LLM 适配层需考虑不同 provider 的协议变体，假消息注入需符合目标 provider 的约束（如 thinking block 回传要求）
**涉及文件:** peri-middlewares/src/subagent/skill_preload.rs
**CLAUDE.md 链接:** false


### issue_2026-06-05-agent-tool-3-percent-error-rate-subagent-type-missing
**摘要:** Agent 工具调用 3.35% 错误率——93% 源于 subagent_type 参数缺失
**状态:** Fixed
**归档日期:** 2026-06-06
**关键词:** Agent 工具错误率, Ok-error 返回模式, subagent_type 参数缺失, tool_errors 分析器
**问题本质:** Agent 工具将参数校验错误以 `Ok("Error: ...")` 返回而非 `Err()`，导致上层 `tool_errors` 分析器（依赖 `is_error=true` 筛选）完全遗漏这些错误，使 3.35% 错误率在监控系统中不可见。同时 LLM 频繁遗漏 `subagent_type` 参数，说明工具描述中参数要求的传达不够清晰。
**通用模式:** 所有工具的参数校验/执行错误必须以 `Err()` 返回，`Ok("Error: ...")` 是反模式——它绕过 `is_error` 标记和上层错误分析器。新增工具时需显式检查所有错误路径的返回值类型。
**技术决策:** 工具错误返回一致性是工具系统的隐含契约，需通过测试断言（`.unwrap_err()` vs `.unwrap()`）和 code review 双重保障。
**涉及文件:** peri-middlewares/src/subagent/tool/define.rs, peri-middlewares/src/subagent/tool/execute_bg.rs, peri-middlewares/src/subagent/tool/execute_fork.rs, peri-middlewares/src/subagent/tool/tool_test.rs
**CLAUDE.md 链接:** false

### issue_2026-06-06-lineedit-prompt-stress-testing
**摘要:** LineEdit 提示词压力测试方法论
**状态:** Closed
**归档日期:** 2026-06-06
**关键词:** LineEdit 提示词, start_word/end_word 语义, 提示词压力测试, CJK 唯一性
**问题本质:** LineEdit 工具的 `start_word`/`end_word` 语义复杂（替换范围含锚定词、行内必须唯一、缺 end_word 报错），LLM 在高频操作中容易犯参数错误。通过 6 轮迭代测试验证了工具提示词和用户提示词的稳定性。
**通用模式:** 复杂工具描述需要经过 LLM 压力测试（构造陷阱样本、多次迭代、记录成功率）才能收敛到稳定版本。Caution 关键词（如"unique within the line"、"START of start_word to END of end_word"）是减少 LLM 误用的有效手段。CJK 无空格文本中，短词在不重复文本中永远不唯一，需跨越多个重复单元构造长前缀。
**架构影响:** 工具描述质量直接影响 LLM 工具调用成功率，应视为工具实现的一部分而非附属文档。新增复杂工具时应有配套的压力测试流程。
**技术决策:** 5 Caution 格式的工具提示词（Caution: 问题 → 后果）比传统描述性文本更有效
**涉及文件:** peri-middlewares/src/tools/filesystem/line_edit.rs, prompts/lineedit_stress_test.txt
**CLAUDE.md 链接:** false

### issue_2026-06-06-agent-polls-agentresult-repeatedly

- **摘要:** Agent 反复轮询 AgentResult 而非等待后台任务通知
- **状态:** Fixed
- **归档日期:** 2026-06-11
- **关键词:** AgentResult 轮询, 后台任务通知, 系统提示词行为引导
- **问题本质:** Agent 派发后台任务后缺乏"等待"概念，倾向于反复调用 AgentResult 轮询（单次 17+ 次调用）
- **通用模式:** LLM 行为约束需要系统提示词层面（行为准则）+ 工具层面（返回文案强化）双管齐下
- **技术决策:** 混合式提示词策略——引导性措辞 + 关键位置强措辞，工具返回文案删除误导性引导
- **涉及文件:** peri-tui/prompts/sections/11_subagent.md, peri-middlewares/src/subagent/agent_result.rs

### issue_2026-06-06-test-gap-hitl-cancel-race

- **摘要:** HITL 审批与 Cancel 竞态条件缺少测试
- **状态:** Fixed
- **归档日期:** 2026-06-11
- **关键词:** broker timeout, 竞态条件, 无超时等待
- **问题本质:** `broker.request(ctx).await` 是无超时、无 cancel token 的 async 等待，broker 实现永远不返回时 Agent 永久挂起
- **通用模式:** 所有外部交互的 async 等待必须包裹 timeout 保护，超时后返回错误而非永久挂起
- **涉及文件:** peri-middlewares/src/hitl/mod.rs, peri-middlewares/src/hitl/mod_test.rs

### issue_2026-06-06-test-gap-llm-error-cleanup-prepended

- **摘要:** 测试缺口：LLM 错误路径下 system 消息 cleanup 行为无测试
- **状态:** Fixed
- **归档日期:** 2026-06-11
- **关键词:** cleanup_prepended 泄漏, try_break 宏, 循环内错误传播
- **问题本质:** executor 中 `?` 传播会跳过 `cleanup_prepended`，导致 before_agent 注入的 system 消息泄漏到 state
- **通用模式:** 循环内关键 cleanup 逻辑不能依赖 `?` 传播路径——用 try_break 宏将错误捕获到变量，循环后无条件执行 cleanup
- **涉及文件:** peri-agent/src/agent/executor/mod.rs, peri-agent/src/agent/executor/mod_test.rs

### issue_2026-06-12-subagent-missing-web-tools

- **摘要:** SubAgent 缺少 WebFetch 和 WebSearch 工具
- **状态:** Verified
- **归档日期:** 2026-06-14
- **关键词:** SubAgent 工具继承, WebFetch/WebSearch, parent_tools, 子Agent 工具传播
- **问题本质:** 子Agent 构建时 `parent_tools` 构造遗漏了 WebMiddleware 的工具，导致 Fork/Normal/Background 及 /bg 五条路径均缺失 WebFetch/WebSearch。根本原因是工具传递逻辑分散在 `agent/builder.rs` 和 `bg.rs` 两个 builder 路径中，各自手写工具列表，缺乏统一的工具注册表。
- **通用模式:** 新增核心工具到 Agent 时，必须同时排查所有子Agent 构建路径是否需要传递该工具。推荐通过静态函数统一工具构造入口（如 `WebMiddleware::build_tools()`），避免手写工具列表遗漏。
- **架构影响:** 工具传递应采用集中式注册表，而非各 builder 路径独立构造 `parent_tools`。
- **技术决策:** 通过 `WebMiddleware::build_tools()` 静态函数统一工具构造入口，`collect_tools()` 和 `parent_tools` 构造均委托给它。
- **涉及文件:** peri-acp/src/agent/builder.rs, peri-middlewares/src/subagent/tool/build_agent.rs, peri-middlewares/src/subagent/tool/mod.rs, peri-middlewares/src/middleware/web.rs, peri-acp/src/session/command/bg.rs

### issue_2026-06-12-web-researcher-builtin-upgrade

- **摘要:** Web Researcher Agent 升级为 Built-in Agent，支持原生 WebFetch/WebSearch 及复杂研究工作流
- **状态:** Verified
- **归档日期:** 2026-06-14
- **关键词:** Built-in Agent, web-researcher, 子Agent 升级, 原生工具, BUILT_IN_AGENTS
- **问题本质:** 文件级 Agent 定义（`.claude/agents/web-researcher.md`）依赖 Bash 调用外部 npm 工具完成网页抓取，而项目已有原生 WebFetch/WebSearch 工具。升级为 Built-in Agent 后减少外部依赖，使用原生工具提升可靠性。
- **通用模式:** Built-in Agent 应优先使用项目原生工具，而非依赖外部 CLI 工具。文件级 Agent 定义仅用于用户自定义覆盖。
- **架构影响:** 随着 Built-in Agent 数量增长，子Agent 工具传递正确性更加关键（关联 issue_2026-06-12-subagent-missing-web-tools）。
- **技术决策:** `include_str!` 编译期嵌入 Agent 定义，加入 `BUILT_IN_AGENTS` 数组（第 6 个 built-in agent），硬编码计数需同步更新。
- **涉及文件:** peri-middlewares/src/subagent/built_in_agents.rs, peri-middlewares/src/subagent/built-in/web-researcher.md, .claude/agents/web-researcher.md, peri-middlewares/src/subagent/built-in/mod_test.rs, peri-middlewares/src/subagent/built-in/prompt_test.rs

### issue_2026-06-12-large-write-streaming-slow

- **摘要:** Write 工具超长内容流式输出时 LLM Provider 响应极慢
- **状态:** Fixed
- **归档日期:** 2026-06-14
- **关键词:** Write 工具, 流式性能, 超时机制, append 模式, 大文件写入
- **问题本质:** LLM 流式输出超大 JSON（tool_use input 包含完整 content 字段）时，provider 侧 token 生成速度显著下降，非 token 预算不足问题。与历史 issue `2026-05-15-write-tool-missing-filepath-max-tokens` 相关但维度不同——历史 issue 是 max_tokens 截断导致 JSON 不完整，本次是 provider 侧流式性能劣化。
- **通用模式:** 工具实现应考虑极端参数情况（超大输入/输出），通过超时机制或输入校验提前拦截，并引导 LLM 采用更优策略（如 append 分段写入）。
- **技术决策:** 用 `tokio::time::timeout(Duration::from_secs(120), ...)` 包裹 Write 工具 invoke，超时时返回英文错误提示引导 Agent 使用 `append=true` 分段写入。
- **涉及文件:** peri-middlewares/src/tools/filesystem/write.rs

### issue_2026-06-01-skill-prefix-hints-unknown-command
**摘要:** Skill 名在 TUI Hints 浮层重复显示——skills 列表（有 description）和 agent_commands HashSet（无 description）两个数据源无去重
**状态:** Fixed
**归档日期:** 2026-06-14
**关键词:** Hints 去重, agent_commands, 双数据源, ACP 命令分类
**问题本质:** ACP 构建可用命令时将 skill 名同时加入 agent_commands，TUI 从 skills 和 agent_commands 两个独立列表构建候选项时无去重，同一 skill 出现两次
**通用模式:** 多条数据源合并渲染时必须显式去重，不能在渲染层假设数据源互斥。ACP 到 TUI 的命令传递应做好分类（静态命令 vs skill 命令），让消费方无需猜测
**技术决策:** `update_agent_commands()` 中过滤已存在于 skills 列表的条目
**涉及文件:** peri-acp/src/dispatch/commands.rs, peri-tui/src/acp_server/notify.rs, peri-tui/src/app/agent_ops/acp_bridge.rs, peri-tui/src/app/command_system.rs, peri-tui/src/ui/main_ui/popups/hints.rs, peri-tui/src/app/hint_ops.rs
**CLAUDE.md 链接:** false

### issue_2026-06-09-coder-builtin-agent
**摘要:** 新建 coder 内置 Agent 类型，从 general-purpose 拆分代码实现职责，缩减工具集节省上下文
**状态:** Done
**归档日期:** 2026-06-14
**关键词:** Built-in Agent, coder, 工具减量, 上下文优化, 反循环指导
**问题本质:** general-purpose 的 92.3% 任务是 coder 类，但全工具集（含 WebSearch/WebFetch/Agent/AskUserQuestion）占用 system prompt 空间，上下文被工具输出占满后 agent 陷入重复搜索循环（极端案例同一 pattern 562 次 Grep）
**通用模式:** Built-in Agent 应针对使用场景优化工具集和迭代上限。通过数据分析（P95 消息量、重复搜索频率）指导参数设计。system prompt 应嵌入反退化行为指导（"搜索前先确认是否已有结果在上下文中"）
**技术决策:** 工具集从 11 个减至 7 个（移除 WebSearch/WebFetch/Agent/AskUserQuestion）；迭代上限 200（P95=153 + 30% 余量）；`include_str!` 编译期嵌入 Agent 定义，加入 BUILT_IN_AGENTS 数组
**涉及文件:** peri-middlewares/src/subagent/built-in/coder.md, peri-middlewares/src/subagent/built_in_agents.rs
**CLAUDE.md 链接:** false

### issue_2026-06-03-concurrent-bg-agent-next-prompt-hangs
**摘要:** 并发 bg agent 完成后，同一 session 的下一个 prompt 永久卡死——Langfuse flush 阻塞 event pump 导致 prompt_lock 不释放
**状态:** Fixed
**归档日期:** 2026-06-14
**关键词:** Langfuse flush 阻塞, event pump, prompt_lock 死锁, pump_done_tx
**问题本质:** Langfuse flush（HTTP 30s timeout × retry）在 push_done() 之后、pump_done_tx.send() 之前阻塞等待，导致 wait_for_pump() 永久阻塞 → execute_prompt() 不返回 → ACP server 的 prompt_lock 不释放 → 新 prompt 永久等待锁。Ctrl+C 无法恢复因为新 prompt 的 cancel_token 尚未创建
**通用模式:** 外部遥测/I/O 的 flush 操作绝不能阻塞核心事件泵。fire-and-forget 模式 + pump 完成信号提前发送 + 等待端添加超时安全网
**技术决策:** pump_done_tx.send() 移到 Langfuse flush 之前；flush 改为 drop(handle) fire-and-forget；wait_for_pump 添加 10s timeout
**涉及文件:** peri-acp/src/session/executor.rs
**CLAUDE.md 链接:** true（Langfuse 阻塞问题此前未独立成已知陷阱，建议在 CLAUDE.md 中补充）

### issue_2026-06-01-hook-permission-request-fires-in-bypass
**摘要:** PermissionRequest 钩子在 bypass 权限模式下不应触发但仍触发，与 Claude Code 规范不符
**状态:** Fixed
**归档日期:** 2026-06-14
**关键词:** PermissionRequest, bypass 模式, HookMiddleware, 权限门控
**问题本质:** HookMiddleware 不检查 permission_mode，对所有敏感工具无条件触发 PermissionRequest。Claude Code 规范中 bypass/dont-ask 模式下不应展示权限对话框，因此也不应触发 PermissionRequest
**通用模式:** Hook 触发条件需感知全局权限模式，中间件行为与权限系统应正交检查。不能仅依赖工具本身的敏感性标记决定是否触发 hook
**技术决策:** before_tool 中增加 permission_mode != "bypass" 门控；PreToolUse 不受影响（独立于权限系统）
**涉及文件:** peri-middlewares/src/hooks/middleware.rs
**CLAUDE.md 链接:** false

### issue_2026-06-06-global-settings-hooks-not-loaded
**摘要:** ~/.claude/settings.json 全局 Hook 配置未加载生效，所有事件类型 hook 均不触发
**状态:** Fixed
**归档日期:** 2026-06-14
**关键词:** 全局 settings, Hook 加载, 启动初始化, loader
**问题本质:** TUI 启动时 load_global_settings_hooks() 未被正确调用或加载路径错误，启动日志中无 "Loaded N hooks from ~/.claude/settings.json"
**涉及文件:** peri-middlewares/src/hooks/loader.rs, peri-tui/src/main.rs, peri-acp/src/agent/builder.rs, peri-middlewares/src/hooks/middleware.rs
**CLAUDE.md 链接:** false

### issue_2026-05-29-llm-stream-error-causes-amnesia
**摘要:** LLM 流式读取失败后 agent 丢失当前轮次全部上下文——history 保护条件过窄，仅覆盖 Cancelled 路径
**状态:** Fixed
**归档日期:** 2026-06-14
**关键词:** LLM 流式错误, agent amnesia, history truncation, stop_reason 保护不足
**问题本质:** prompt.rs 的 history 保护条件要求 stop_reason == Cancelled，但 LLM 流式错误、LLM 重试耗尽、MaxIterationsExceeded 等路径在有进展时也会 truncate history，丢弃 agent 已写入 state 的当前轮次消息
**通用模式:** 取消/错误后的 history 保护条件应从"是否取消"改为"是否有进展"（检查 result.messages.len()）。任何错误路径在 agent 已有有效产出时都应保留 state
**涉及文件:** peri-tui/src/acp_server/prompt.rs, peri-acp/src/session/executor.rs, peri-agent/src/agent/executor/mod.rs
**CLAUDE.md 链接:** true（与 issue_2026-05-26-ctrl-c-interrupt-causes-agent-amnesia 同根因不同触发条件）

### issue_2026-05-29-new-thread-deadlock-and-update-config-inconsistency
**摘要:** new_thread() 死锁风险（block_in_place + block_on）+ session/update_config 先持久化后验证导致 config 与 provider 状态不一致
**状态:** Fixed
**归档日期:** 2026-06-14
**关键词:** block_in_place 死锁, 配置事务性, 验证顺序, provider 不一致
**问题本质:** (1) block_in_place + block_on 在 tokio runtime 上同步等待 ACP new_session()，可能耗尽所有 worker 线程导致死锁；(2) update_config 先写入 peri_config 再验证 LlmProvider，写入成功但验证失败时 config 与 provider 状态不一致
**通用模式:** 配置更新遵循"先验证再持久化"的事务性顺序。tokio runtime 上不应使用 block_in_place + block_on 等待需要 runtime 协程的异步操作
**涉及文件:** peri-tui/src/app/thread_ops.rs, peri-tui/src/acp_server/requests.rs
**CLAUDE.md 链接:** false

### issue_2026-06-06-write-tool-append-mode
**摘要:** Write 工具新增 append 参数支持增量写入，降低超长文件全量覆写的上下文消耗
**状态:** Fixed
**归档日期:** 2026-06-14
**关键词:** Write append, 上下文消耗, 增量写入, 工具参数扩展
**问题本质:** Write 仅全量覆写，大文件（>200行）content 参数占用 4.2% 上下文且不可压缩，最大观测 71.1KB/2367 行（占 128K 上下文的 14.2%）。61.4% 的 Write 是盲写（不先 Read）
**通用模式:** 工具应提供增量操作选项降低 LLM 上下文消耗。append/overwrite 双模式是文件写入的标准设计。工具描述应引导 LLM 对大文件使用增量模式
**技术决策:** append 可选布尔参数，默认 false；true 时用 O_APPEND 追加，文件不存在自动创建，tool_result 不回传内容（`Appended N lines, file total: M`）
**涉及文件:** peri-middlewares/src/tools/filesystem/write.rs, peri-middlewares/src/tools/filesystem/write_test.rs
**CLAUDE.md 链接:** false

### issue_2026-06-10-prompt-suggestion-not-working
**摘要:** Prompt Suggestion 功能完整实现（17 单元测试通过）但端到端无效果，功能已全部撤回
**状态:** Reverted（代码已完全还原）
**归档日期:** 2026-06-14
**关键词:** 多层数据流, 端到端验证缺失, fire-and-forget, 测试≠可用
**问题本质:** 9 段数据流（executor spawn → LLM generate → filter pipeline → emit SuggestionReady → ACP event mapper → TransportEventSink → TUI acp_client → agent_ops → main_ui placeholder）任一段断裂即功能完全不可见。未逐段运行时验证就声称"功能完成"
**通用模式:** 跨多层（agent → ACP → TUI）的新功能必须在每个数据流环节添加运行时日志逐段验证。单元测试通过 ≠ 端到端可用。fire-and-forget task 的生命周期和错误处理需显式管理，否则静默失败
**涉及文件:** （已全部还原：peri-acp/src/suggestion/* 已删除，peri-agent/src/agent/events.rs、peri-acp/src/session/executor.rs、peri-acp/src/event/mapper.rs、peri-tui/src/app/agent.rs、peri-tui/src/app/events.rs、peri-tui/src/app/agent_ops/mod.rs、peri-tui/src/ui/main_ui/mod.rs、peri-tui/src/event/keyboard/normal_keys.rs 修改已撤回）
**CLAUDE.md 链接:** false

### issue_2026-06-02-read-tool-path-alias-for-file_path
**摘要:** LLM 调用 Read 工具时 1.74%（158/9,058）使用 path 参数名代替 file_path，因 Glob/Grep 的 schema 使用 path 导致参数名混淆
**状态:** Fixed
**归档日期:** 2026-06-14
**关键词:** 参数别名, path vs file_path, tool_dispatch 兜底, 执行层兼容
**问题本质:** Claude Code 工具 schema 有意设计不一致：Glob/Grep 用 path（搜索范围），Read/Write/Edit 用 file_path（操作目标）。LLM 在前后调用不同类别工具时产生参数名混淆
**通用模式:** 工具参数名不一致时，执行层做静默兼容（参数名别名表）而非仅依赖 LLM 遵守 schema。不改 schema 保持规范严格，执行层兜底。可复用已有 TOOL_ALIASES 模式
**技术决策:** 在 tool_dispatch.rs 新增 PARAM_ALIASES 常量 ["path" → "file_path"]，invoke 前静默转换 + tracing::warn 日志监测。不改工具 schema
**涉及文件:** peri-agent/src/agent/executor/tool_dispatch.rs
**CLAUDE.md 链接:** false

---

## 相关 Feature

- → [relay-server.md#feature_20260326_F009_relay-message-id-propagation](./relay-server.md) — message_id 透传到 Web 前端
- → [langfuse.md#feature_20260325_F003_langfuse-observation-types](./langfuse.md#feature_20260325_F003_langfuse-observation-types) — Langfuse 观测依赖 AgentEvent LlmCallStart/End 钩子
- → [tui.md#feature_20260328_F003_test-coverage-improvement](./tui.md#feature_20260328_F003_test-coverage-improvement) — ask_user_tool 10 个单元测试（MockBroker 参数解析和返回格式）
- → [hitl-permissions.md](./hitl-permissions.md) — 5 级权限模式 HITL middleware 集成
- → [llm-retry.md](./llm-retry.md) — RetryableLLM 装饰器包装 ReactLLM
- → [system-prompt.md](./system-prompt.md) — 系统提示词段落化 PromptFeatures 条件注入
- → [file-search.md](./file-search.md) — grep crate 进程内文件搜索
- → [token-tracking.md](./token-tracking.md) — TokenTracker Token 累积追踪
- → [compact.md](./compact.md) — Micro/Full Compact 核心层消息操作
- → [message-pipeline.md](./message-pipeline.md) — MessagePipeline 统一管线
- → [code-architecture.md](./code-architecture.md) — Relay Server 移除
