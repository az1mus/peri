# 问题索引

按关键词索引已归档 issue，遇到相似问题时快速定位历史经验。

## 关键词索引

### HashMap 顺序
- [多处 HashMap 非确定性顺序导致 Anthropic Prompt Cache 前缀不稳定](domains/message-pipeline.md#issue_2026-05-12-deferred-tool-list-nondeterministic-order) — message-pipeline

### Prompt Cache
- [多处 HashMap 非确定性顺序导致 Anthropic Prompt Cache 前缀不稳定](domains/message-pipeline.md#issue_2026-05-12-deferred-tool-list-nondeterministic-order) — message-pipeline
- [Skill Preload 注入消息到历史最前面导致首轮 Prompt Cache 失效](domains/message-pipeline.md#issue_2026-05-12-skill-preload-invalidates-prompt-cache) — message-pipeline
- [System prompt 动态内容导致 Anthropic prompt cache 频繁失效，边界标记拆分修复](domains/system-prompt.md#issue_2026-05-13-system-prompt-dynamic-cache-invalidation) — system-prompt
- [AskUserQuestion 导致缓存命中率极速下降](domains/system-prompt.md#issue_2026-05-13-askuserquestion-cache-hit-rate-drop) — system-prompt
- [82% system 未缓存 + 断点在 tool_result-only 消息上静默失效](domains/message-pipeline.md#issue_2026-05-14-cache-breakpoint-structural-inefficiency) — message-pipeline

### 缓存前缀
- [多处 HashMap 非确定性顺序导致 Anthropic Prompt Cache 前缀不稳定](domains/message-pipeline.md#issue_2026-05-12-deferred-tool-list-nondeterministic-order) — message-pipeline

### ToolSearchIndex
- [多处 HashMap 非确定性顺序导致 Anthropic Prompt Cache 前缀不稳定](domains/message-pipeline.md#issue_2026-05-12-deferred-tool-list-nondeterministic-order) — message-pipeline

### prepend_message
- [Skill Preload 注入消息到历史最前面导致首轮 Prompt Cache 失效](domains/message-pipeline.md#issue_2026-05-12-skill-preload-invalidates-prompt-cache) — message-pipeline
- [prepend_message 的 insert(0) 右移导致 StateSnapshot 包含 System 消息](domains/system-prompt.md#issue_2026-05-13-system-prompt-dynamic-parts-duplicated-in-consecutive-calls) — system-prompt

### add_message
- [Skill Preload 注入消息到历史最前面导致首轮 Prompt Cache 失效](domains/message-pipeline.md#issue_2026-05-12-skill-preload-invalidates-prompt-cache) — message-pipeline

### cache_control
- [Skill Preload 注入消息到历史最前面导致首轮 Prompt Cache 失效](domains/message-pipeline.md#issue_2026-05-12-skill-preload-invalidates-prompt-cache) — message-pipeline
- [82% system 未缓存 + 断点在 tool_result-only 消息上静默失效](domains/message-pipeline.md#issue_2026-05-14-cache-breakpoint-structural-inefficiency) — message-pipeline

### SystemNote
- [SystemNote 在 RebuildAll 后堆积到消息列表末尾](domains/message-pipeline.md#issue_2026-05-12-systemnote-position-drift-on-rebuild) — message-pipeline

### RebuildAll
- [SystemNote 在 RebuildAll 后堆积到消息列表末尾](domains/message-pipeline.md#issue_2026-05-12-systemnote-position-drift-on-rebuild) — message-pipeline
- [CacheWarning 消息被 RebuildAll 立即丢弃，用户无法看到](domains/message-pipeline.md#issue_2026-05-12-cache-warning-discarded-by-rebuild) — message-pipeline
- [Compact 完成后残留 "正在压缩上下文…" 系统通知](domains/message-pipeline.md#issue_2026-05-12-compact-ephemeral-notes-not-cleared) — message-pipeline

### ephemeral_notes
- [SystemNote 在 RebuildAll 后堆积到消息列表末尾](domains/message-pipeline.md#issue_2026-05-12-systemnote-position-drift-on-rebuild) — message-pipeline
- [Compact 完成后残留 "正在压缩上下文…" 系统通知](domains/message-pipeline.md#issue_2026-05-12-compact-ephemeral-notes-not-cleared) — message-pipeline

### 锚点机制
- [SystemNote 在 RebuildAll 后堆积到消息列表末尾](domains/message-pipeline.md#issue_2026-05-12-systemnote-position-drift-on-rebuild) — message-pipeline

### CacheWarning
- [CacheWarning 消息被 RebuildAll 立即丢弃，用户无法看到](domains/message-pipeline.md#issue_2026-05-12-cache-warning-discarded-by-rebuild) — message-pipeline

### AiReasoning
- [流式过程中 AI 文本不可见（工具调用场景）](domains/agent.md#issue_2026-05-11-streaming-text-invisible-with-tools) — agent

### TextChunk
- [流式过程中 AI 文本不可见（工具调用场景）](domains/agent.md#issue_2026-05-11-streaming-text-invisible-with-tools) — agent

### 事件类型语义
- [流式过程中 AI 文本不可见（工具调用场景）](domains/agent.md#issue_2026-05-11-streaming-text-invisible-with-tools) — agent

### frozen_subagent_vms
- [Background Agent 三个 Bug：显示消失、subagent_type 限制、continuation 不触发](domains/agent.md#issue_2026-05-12-background-agent-display-and-continuation-bugs) — agent

### continuation 竞态
- [Background Agent 三个 Bug：显示消失、subagent_type 限制、continuation 不触发](domains/agent.md#issue_2026-05-12-background-agent-display-and-continuation-bugs) — agent

### fork+background
- [Background Agent 三个 Bug：显示消失、subagent_type 限制、continuation 不触发](domains/agent.md#issue_2026-05-12-background-agent-display-and-continuation-bugs) — agent

### SubAgent
- [Background Agent 工具继承缺失——子 agent 仅能使用 TodoWrite](domains/agent.md#issue_2026-05-11-background-agent-missing-tools) — agent
- [同步子 Agent（Normal/Fork）事件溢出到主 Agent 消息流](domains/agent.md#issue_2026-05-13-sync-subagent-events-leak-to-parent) — agent

### in_subagent
- [同步子 Agent（Normal/Fork）事件溢出到主 Agent 消息流](domains/agent.md#issue_2026-05-13-sync-subagent-events-leak-to-parent) — agent

### StateSnapshot 守卫
- [同步子 Agent（Normal/Fork）事件溢出到主 Agent 消息流](domains/agent.md#issue_2026-05-13-sync-subagent-events-leak-to-parent) — agent
- [流式渲染中 map_executor_event 丢弃中间 StateSnapshot](domains/message-pipeline.md#issue_2026-05-13-streaming-text-tool-aggregation-visual-issues) — message-pipeline

### 事件溢出
- [同步子 Agent（Normal/Fork）事件溢出到主 Agent 消息流](domains/agent.md#issue_2026-05-13-sync-subagent-events-leak-to-parent) — agent
- [并发前台SubAgent调用时UI感知延迟，SubAgentGroup卡片不可见](domains/tui.md#issue_2026-05-15-concurrent-subagent-display-delay) — tui

### map_executor_event
- [流式渲染中 map_executor_event 丢弃中间 StateSnapshot](domains/message-pipeline.md#issue_2026-05-13-streaming-text-tool-aggregation-visual-issues) — message-pipeline

### 双写路径
- [后台 Agent 完成后 input_history 消息重复导致 Prompt Cache 失效](domains/agent.md#issue_2026-05-13-input-history-message-duplication-after-background-tasks) — agent

### agent_state_messages
- [后台 Agent 完成后 input_history 消息重复导致 Prompt Cache 失效](domains/agent.md#issue_2026-05-13-input-history-message-duplication-after-background-tasks) — agent
- [prepend_message 的 insert(0) 右移导致 StateSnapshot 包含 System 消息](domains/system-prompt.md#issue_2026-05-13-system-prompt-dynamic-parts-duplicated-in-consecutive-calls) — system-prompt
- [DeepSeek多轮对话中agent_state_messages消息重复导致API 400错误](domains/agent.md#issue_2026-05-14-deepseek-multi-turn-tool-result-duplication) — agent

### tool_call_id 重复
- [后台 Agent 完成后 input_history 消息重复导致 Prompt Cache 失效](domains/agent.md#issue_2026-05-13-input-history-message-duplication-after-background-tasks) — agent

### 流式渲染
- [多轮对话中 AI message 和 thinking 在进行时不可见](domains/message-pipeline.md#issue_2026-05-13-ai-message-thinking-invisible-during-multi-turn) — message-pipeline
- [流式渲染中 map_executor_event 丢弃中间 StateSnapshot](domains/message-pipeline.md#issue_2026-05-13-streaming-text-tool-aggregation-visual-issues) — message-pipeline

### has_snapshot_this_round
- [多轮对话中 AI message 和 thinking 在进行时不可见](domains/message-pipeline.md#issue_2026-05-13-ai-message-thinking-invisible-during-multi-turn) — message-pipeline
- [并发前台SubAgent调用时UI感知延迟，SubAgentGroup卡片不可见](domains/tui.md#issue_2026-05-15-concurrent-subagent-display-delay) — tui

### 边界标记
- [System prompt 动态内容导致 Anthropic prompt cache 频繁失效，边界标记拆分修复](domains/system-prompt.md#issue_2026-05-13-system-prompt-dynamic-cache-invalidation) — system-prompt

### __SYSTEM_PROMPT_DYNAMIC_BOUNDARY__
- [System prompt 动态内容导致 Anthropic prompt cache 频繁失效，边界标记拆分修复](domains/system-prompt.md#issue_2026-05-13-system-prompt-dynamic-cache-invalidation) — system-prompt

### split_system_blocks
- [System prompt 动态内容导致 Anthropic prompt cache 频繁失效，边界标记拆分修复](domains/system-prompt.md#issue_2026-05-13-system-prompt-dynamic-cache-invalidation) — system-prompt

### SkillPreloadMiddleware
- [主 Agent 中间件链缺少 SkillPreloadMiddleware，预加载失效](domains/system-prompt.md#issue_2026-05-13-missing-skillpreload-in-main-agent) — system-prompt

### 中间件链缺失
- [主 Agent 中间件链缺少 SkillPreloadMiddleware，预加载失效](domains/system-prompt.md#issue_2026-05-13-missing-skillpreload-in-main-agent) — system-prompt

### 工具继承
- [Background Agent 工具继承缺失——子 agent 仅能使用 TodoWrite](domains/agent.md#issue_2026-05-11-background-agent-missing-tools) — agent

### register_tool
- [Background Agent 工具继承缺失——子 agent 仅能使用 TodoWrite](domains/agent.md#issue_2026-05-11-background-agent-missing-tools) — agent

### merge_frozen_subagents
- [并发前台SubAgent调用时UI感知延迟，SubAgentGroup卡片不可见](domains/tui.md#issue_2026-05-15-concurrent-subagent-display-delay) — tui

### reasoning
- [GLM 模型 reasoning 字段未被解析，thinking 内容跨轮次丢失](domains/agent.md#issue_2026-05-12-glm-reasoning-field-not-parsed) — agent

### reasoning_content
- [GLM 模型 reasoning 字段未被解析，thinking 内容跨轮次丢失](domains/agent.md#issue_2026-05-12-glm-reasoning-field-not-parsed) — agent

### GLM
- [GLM 模型 reasoning 字段未被解析，thinking 内容跨轮次丢失](domains/agent.md#issue_2026-05-12-glm-reasoning-field-not-parsed) — agent

### context_window
- [OpenAI 兼容第三方 Provider 上下文用量计算不准确](domains/token-tracking.md#issue_2026-05-11-context-usage-miscalculation-openai-compatible) — token-tracking

### 缓存命中率
- [OpenAI 兼容第三方 Provider 上下文用量计算不准确](domains/token-tracking.md#issue_2026-05-11-context-usage-miscalculation-openai-compatible) — token-tracking
- [状态栏缓存百分比在对话停止后消失](domains/token-tracking.md#issue_2026-05-12-cache-percentage-disappears-after-done) — token-tracking

### last_user_input
- [Auto Compact 后 Agent 未自动 Resubmit 继续执行](domains/compact.md#issue_2026-05-11-auto-compact-no-resubmit) — compact

### auto-compact
- [Auto Compact 后 Agent 未自动 Resubmit 继续执行](domains/compact.md#issue_2026-05-11-auto-compact-no-resubmit) — compact

### CJK 宽度
- [输入框鼠标点击光标定位不准](domains/tui.md#issue_2026-05-12-textarea-mouse-click-cursor-misposition-cjk) — tui

### unicode-width
- [输入框鼠标点击光标定位不准](domains/tui.md#issue_2026-05-12-textarea-mouse-click-cursor-misposition-cjk) — tui

### 鼠标定位
- [输入框鼠标点击光标定位不准](domains/tui.md#issue_2026-05-12-textarea-mouse-click-cursor-misposition-cjk) — tui

### on_error 回调
- [LSP transport 层错误处理缺陷（进程退出未更新状态 + 崩溃后无自动重连）](domains/lsp.md#issue_2026-05-12-lsp-transport-no-fast-fail-on-process-exit) — lsp

### LSP 重连
- [LSP transport 层错误处理缺陷（进程退出未更新状态 + 崩溃后无自动重连）](domains/lsp.md#issue_2026-05-12-lsp-transport-no-fast-fail-on-process-exit) — lsp

### parking_lot::MutexGuard !Send
- [LSP transport 层错误处理缺陷（进程退出未更新状态 + 崩溃后无自动重连）](domains/lsp.md#issue_2026-05-12-lsp-transport-no-fast-fail-on-process-exit) — lsp

### transport 断开
- [LSP transport 层错误处理缺陷（进程退出未更新状态 + 崩溃后无自动重连）](domains/lsp.md#issue_2026-05-12-lsp-transport-no-fast-fail-on-process-exit) — lsp

### Grep工具
- [Grep 工具声明参数未实现 + 标准 grep 能力缺失](domains/agent.md#issue_2026-05-14-grep-tool-capability-gap) — agent

### 参数声明
- [Grep 工具声明参数未实现 + 标准 grep 能力缺失](domains/agent.md#issue_2026-05-14-grep-tool-capability-gap) — agent

### 接口契约
- [Grep 工具声明参数未实现 + 标准 grep 能力缺失](domains/agent.md#issue_2026-05-14-grep-tool-capability-gap) — agent

### 工具标准能力
- [Grep 工具声明参数未实现 + 标准 grep 能力缺失](domains/agent.md#issue_2026-05-14-grep-tool-capability-gap) — agent

### thinking block
- [SkillPreloadMiddleware 注入的伪 assistant 消息不含 thinking block，DeepSeek API 400](domains/agent.md#issue_2026-05-14-deepseek-anthropic-thinking-block-dropped) — agent
- [Thinking/Reasoning数据流：占位thinking缺signature + AiReasoning死代码](domains/agent.md#issue_2026-05-12-thinking-reasoning-dataflow-issues) — agent

### redacted_thinking
- [SkillPreloadMiddleware 注入的伪 assistant 消息不含 thinking block，DeepSeek API 400](domains/agent.md#issue_2026-05-14-deepseek-anthropic-thinking-block-dropped) — agent

### SkillPreload
- [SkillPreloadMiddleware 注入的伪 assistant 消息不含 thinking block，DeepSeek API 400](domains/agent.md#issue_2026-05-14-deepseek-anthropic-thinking-block-dropped) — agent

### DeepSeek
- [SkillPreloadMiddleware 注入的伪 assistant 消息不含 thinking block，DeepSeek API 400](domains/agent.md#issue_2026-05-14-deepseek-anthropic-thinking-block-dropped) — agent

### tool_result闭合
- [并发工具执行中部分路径提前返回导致 tool_result 缺失](domains/agent.md#issue_2026-05-14-orphaned-tool-use-without-tool-result) — agent

### 并发工具
- [并发工具执行中部分路径提前返回导致 tool_result 缺失](domains/agent.md#issue_2026-05-14-orphaned-tool-use-without-tool-result) — agent

### 工具错误处理
- [工具调用参数错误导致Agent停止而非自动重试](domains/agent.md#issue_2026-05-15-tool-execution-error-stops-agent) — agent

### deferred_error
- [并发工具执行中部分路径提前返回导致 tool_result 缺失](domains/agent.md#issue_2026-05-14-orphaned-tool-use-without-tool-result) — agent

### 延迟写入
- [stop_reason与内容不一致导致孤儿tool_use触发Anthropic API 400](domains/agent.md#issue_2026-05-15-orphaned-tool-use-after-concurrent-tool-error) — agent

### stop_reason
- [stop_reason与内容不一致导致孤儿tool_use触发Anthropic API 400](domains/agent.md#issue_2026-05-15-orphaned-tool-use-after-concurrent-tool-error) — agent

### 孤儿tool_use
- [并发工具执行中部分路径提前返回导致 tool_result 缺失](domains/agent.md#issue_2026-05-14-orphaned-tool-use-without-tool-result) — agent
- [stop_reason与内容不一致导致孤儿tool_use触发Anthropic API 400](domains/agent.md#issue_2026-05-15-orphaned-tool-use-after-concurrent-tool-error) — agent

### tool_result id
- [GLM Anthropic兼容端口tool_result block缺少id属性导致500错误](domains/agent.md#issue_2026-05-15-glm-anthropic-tool-result-id-attribute-error) — agent

### 第三方API
- [GLM Anthropic兼容端口tool_result block缺少id属性导致500错误](domains/agent.md#issue_2026-05-15-glm-anthropic-tool-result-id-attribute-error) — agent
- [stop_reason与内容不一致导致孤儿tool_use触发Anthropic API 400](domains/agent.md#issue_2026-05-15-orphaned-tool-use-after-concurrent-tool-error) — agent

### max_tokens
- [Write工具超长内容触发max_tokens截断导致file_path缺失](domains/agent.md#issue_2026-05-15-write-tool-missing-filepath-max-tokens) — agent

### 消息重复
- [DeepSeek多轮对话中agent_state_messages消息重复导致API 400错误](domains/agent.md#issue_2026-05-14-deepseek-multi-turn-tool-result-duplication) — agent

### last_message_count
- [DeepSeek多轮对话中agent_state_messages消息重复导致API 400错误](domains/agent.md#issue_2026-05-14-deepseek-multi-turn-tool-result-duplication) — agent

### 死代码
- [24 处 #[allow(dead_code/unused)] 抑制了真正的死代码和未完成功能](domains/code-architecture.md#issue_2026-05-14-dead-code-unfinished-features-cleanup) — code-architecture

### allow注解
- [24 处 #[allow(dead_code/unused)] 抑制了真正的死代码和未完成功能](domains/code-architecture.md#issue_2026-05-14-dead-code-unfinished-features-cleanup) — code-architecture

### 代码清理
- [24 处 #[allow(dead_code/unused)] 抑制了真正的死代码和未完成功能](domains/code-architecture.md#issue_2026-05-14-dead-code-unfinished-features-cleanup) — code-architecture

### 编译器警告
- [24 处 #[allow(dead_code/unused)] 抑制了真正的死代码和未完成功能](domains/code-architecture.md#issue_2026-05-14-dead-code-unfinished-features-cleanup) — code-architecture

### 测试分离
- [89.8% 源文件内联测试违反规范，两轮分离后 152 个文件外部化](domains/code-architecture.md#issue_2026-05-14-test-separation-convention-debt) — code-architecture

### include!
- [89.8% 源文件内联测试违反规范，两轮分离后 152 个文件外部化](domains/code-architecture.md#issue_2026-05-14-test-separation-convention-debt) — code-architecture

### #[path]
- [89.8% 源文件内联测试违反规范，两轮分离后 152 个文件外部化](domains/code-architecture.md#issue_2026-05-14-test-separation-convention-debt) — code-architecture

### 模块可见性
- [89.8% 源文件内联测试违反规范，两轮分离后 152 个文件外部化](domains/code-architecture.md#issue_2026-05-14-test-separation-convention-debt) — code-architecture

### Reasoning渲染
- [最后一条 AI 消息无正文时展示思考最后 1 行](domains/message-pipeline.md#issue_2026-05-15-thinking-tail-preview) — message-pipeline

### tail_lines
- [最后一条 AI 消息无正文时展示思考最后 1 行](domains/message-pipeline.md#issue_2026-05-15-thinking-tail-preview) — message-pipeline

### ContentBlockView
- [最后一条 AI 消息无正文时展示思考最后 1 行](domains/message-pipeline.md#issue_2026-05-15-thinking-tail-preview) — message-pipeline

### Hash设计
- [最后一条 AI 消息无正文时展示思考最后 1 行](domains/message-pipeline.md#issue_2026-05-15-thinking-tail-preview) — message-pipeline

### 断点回退
- [82% system 未缓存 + 断点在 tool_result-only 消息上静默失效](domains/message-pipeline.md#issue_2026-05-14-cache-breakpoint-structural-inefficiency) — message-pipeline

### 缓存驱逐
- [82% system 未缓存 + 断点在 tool_result-only 消息上静默失效](domains/message-pipeline.md#issue_2026-05-14-cache-breakpoint-structural-inefficiency) — message-pipeline

### system缓存
- [82% system 未缓存 + 断点在 tool_result-only 消息上静默失效](domains/message-pipeline.md#issue_2026-05-14-cache-breakpoint-structural-inefficiency) — message-pipeline

### Resize事件
- [流式加载期间拖动窗口宽度，Resize 事件无节流导致 CPU 暴涨](domains/tui.md#issue_2026-05-14-streaming-resize-cpu-spike) — tui

### 去抖/节流
- [流式加载期间拖动窗口宽度，Resize 事件无节流导致 CPU 暴涨](domains/tui.md#issue_2026-05-14-streaming-resize-cpu-spike) — tui

### 渲染线程
- [流式加载期间拖动窗口宽度，Resize 事件无节流导致 CPU 暴涨](domains/tui.md#issue_2026-05-14-streaming-resize-cpu-spike) — tui

### CPU暴涨
- [流式加载期间拖动窗口宽度，Resize 事件无节流导致 CPU 暴涨](domains/tui.md#issue_2026-05-14-streaming-resize-cpu-spike) — tui

## 更新记录

- 2026-05-13: 首次创建，归档 22 个 issue，提取 14 条领域认知
- 2026-05-14: 第二次归档，归档 12 个 issue，提取 8 条领域认知（agent 2 + message-pipeline 2 + system-prompt 4）
- 2026-05-15: 第三次归档，归档 8 个 issue，提取 7 条领域认知（agent 3 + code-architecture 2 + message-pipeline 2 + tui 1）
- 2026-05-16: 第四次归档，归档 11 个 issue，提取 7 条领域认知（agent 6 + tui 1）
