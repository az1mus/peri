# Peri 博客写作素材库

已有博客覆盖的主题不再列入：compact-mechanism、edit-tool、multi-agent-patterns、perf-optimization、web-search、prompt-cache-optimization、acp-separation、introducing-peri、streaming-render、concurrent-tool-dispatch、domestic-models-adaptation、domestic-models-work、riscv-peri、rewind-design。

> 筛选标准（2026-06-11）：优先通用技术读者，只留有戏剧性冲突 / 反直觉设计 / 可迁移工程教训 / 共鸣痛点的强故事。删除纯架构描述、琐碎实现细节、与已有选题重复、Peri 特有无法迁移的条目。

## Prompt Cache

- [x] Prompt Cache 命中率从 20% 爬到 98.5%——为什么 cache_control 的位置比内容更重要 → `prompt-cache-optimization`
- [x] 重启 Agent 后 Prompt Cache 全部失效？根因是 HashMap 迭代顺序在进程间不稳定 → `prompt-cache-optimization`
- [x] 一个中间件让所有对话首轮缓存全部失效——头部插入消息会悄悄移动缓存标记 → `prompt-cache-optimization`
- [x] MCP 工具列表一变，缓存就全失效——用边界标记把静态段（约 8K tokens）和动态段隔离开 → `prompt-cache-optimization`

## 内存管理

- [x] 内存分配器换了三次才找到正确答案——jemalloc、mimalloc 都没用，真正的问题是大对象每轮重建 → `perf-optimization`
- [x] Agent 每跑一轮多 40MB 内存——追踪到每轮 68 万次瞬态分配，根源是 HTTP 客户端每轮重建 → `perf-optimization`
- [x] session 级缓存复用如何让内存增长停下来——不是调分配器，是减少不必要的重建 → `perf-optimization`

## 工具调度

- [x] 并发工具调用的完整设计——批量审批、并发执行、延迟错误收集、统一写入；一个失败其他结果不丢，取消路径也不产生孤儿工具调用；连续 5 次同错误自动注入纠正消息防止 LLM 原地打转 → `concurrent-tool-dispatch`
- [ ] LLM 调工具时的名称和参数问题——三层工具名匹配（精确→大小写无关→语义别名 task/shell/reading）+ 参数名归一化（path→file_path），来自 1344 次调用的真实错误数据
- [ ] `Box<dyn BaseTool>` 不能直接转 `Arc<dyn BaseTool>`——标准库缺失的转换需要 ToolWrapper(ManuallyDrop) 透传，绝不能用 `Box::into_raw` + `Arc::from_raw`（布局不同导致 UB） → `peri-agent/src/agent/executor/tool_setup.rs`, `peri-middlewares/src/tools/mod.rs`
- [ ] 工具结果为什么要延迟写入 state——collect_tool_results 先并发收集所有结果不写 state，dispatch_tools 最后统一写入，防止部分路径提前返回产生孤儿 tool_use 触发 Anthropic 400 → `peri-agent/src/agent/executor/tool_dispatch.rs`, `spec/archive-issues/2026-05-14-orphaned-tool-use-without-tool-result.md`

## LLM 适配

- [x] 统一适配层兼容 10 家模型——Anthropic/OpenAI 之外的国产模型各有哪些不兼容之处 → `domestic-models-adaptation`
- [x] 推理内容如何在多轮对话中正确回传——不同模型用不同字段名，同时兼容两套格式 → `domestic-models-adaptation`
- [x] DeepSeek 每条 assistant 消息都要带回思考块，但注入的伪消息没有——400 的根因和修复 → `domestic-models-adaptation`
- [x] Kimi 的推理参数和推理强度参数不能共存——运行时检测模型名并移除冲突字段 → `domestic-models-adaptation`
- [x] 流式输出的 token 用量统计：Qwen 需要额外请求参数才会返回，其他家不需要 → `domestic-models-adaptation`
- [x] GLM 用两个不同的字段名返回同一个东西——解析端同时检查两个字段兼容历史版本 → `domestic-models-adaptation`
- [ ] RetryableLLM 重试的"首透传重试禁"——指数退避 + 25% 抖动，但重试路径走非流式传 None，防止同一 message_id 流式事件双重发射 → `peri-agent/src/llm/retry.rs`

## 中间件与 ReAct 循环

- [ ] 17 个中间件链式组合的设计约束——为什么工具钩子不能在执行过程中读取对话历史
- [ ] Agent 执行危险操作前如何询问用户——HITL 审批机制、权限模式动态切换和 LLM 自动分类器
- [ ] 通过 MCP 协议把任意工具接入 Agent——连接池管理、OAuth 回调、断线重连的实现
- [ ] max_iterations 默认 10 但 TUI 覆盖为 500——核心层保守值防止无限循环，UI 层 50 倍放大满足复杂任务，同一参数在架构不同层的优先级反转 → `peri-agent/src/agent/executor/mod.rs`, `peri-tui/src/app/agent.rs`
- [ ] prepended_ids 为什么用 take_while 而不是长度差——旧逻辑用长度差计算把 SkillPreload 的 add_message 也计入，导致 cleanup 误删头部配对消息；新逻辑只收集头部连续 System 消息 → `peri-agent/src/agent/executor/mod.rs`, `spec/archive-issues/2026-05-26-skillpreload-anthropic-400-tool-result-orphan.md`
- [ ] try_break 宏的错误捕获设计——把 `?` 传播替换为捕获到 loop_error 变量，确保 cleanup_prepended 无论成功/失败/循环耗尽都执行，防止 before_agent 注入的 system 消息泄漏 → `peri-agent/src/agent/executor/mod.rs`, `spec/global/domains/agent.md#issue_2026-06-06`

## 错误处理与踩坑

- [ ] Ctrl+C 取消后 Agent 失忆——中断时无条件截断历史导致已完成的工作被丢弃
- [ ] 中断和完成事件的竞争条件——两个事件都想修改同一个状态，谁先到谁说了算
- [ ] 子 Agent 的输出为什么会在界面上越叠越多——状态没有在正确的时机清空
- [ ] 自定义 Slash 命令的隐蔽陷阱——绕过 Agent 循环的命令必须自己发完成信号，否则前端永远等待
- [ ] 恢复历史对话时，系统内部消息出现在界面上——过滤逻辑漏掉了持久化进数据库的内部消息
- [ ] 并发工具调用中的孤儿 tool_use 危机——一个 `?` 提前返回导致后续工具的 tool_result 全部丢失，Anthropic 400 根因是"部分路径提前跳出"，修复用 deferred_error 延迟传播 → `peri-agent/src/agent/executor/tool_dispatch.rs`, `spec/global/domains/agent.md#issue_2026-05-14`
- [ ] 工具执行错误本该反馈给 LLM 让它修正参数，却被当作 MiddlewareError 终止循环——区分 tool-level error（不终止）与 middleware error（可能终止）→ `spec/global/domains/agent.md#issue_2026-05-15`, `peri-agent/src/agent/executor/tool_dispatch.rs`
- [ ] HITL 审批与 Cancel 的竞态——`broker.request(ctx).await` 是无超时、无 cancel token 的等待，broker 不返回时 Agent 永久挂起，外部交互等待必须包裹 timeout → `spec/archive-issues/2026-06-06-test-gap-hitl-cancel-race.md`, `peri-middlewares/src/hitl/mod.rs`

## TUI 与渲染

- [ ] 中文鼠标点击偏移：修了三次才真正修好——鼠标坐标是显示列宽，光标位置是字符索引，两者不是同一回事
- [x] 独立渲染线程解析 Markdown + 计算行包装，UI 线程只负责从缓存读取可见行重绘 → `streaming-render`
- [x] 流式输出自适应帧率：短消息 30fps，长消息降到 5fps，减少 CPU 空转 → `streaming-render`
- [x] 鼠标滚动事件合并：连续滚动只保留最后一个，避免单次滚动触发多次重绘 → `streaming-render`
- [ ] Markdown 表格里的中文列被压扁了——从等比缩放改为最小宽度优先的修复过程
- [ ] 双击 ESC 回滚对话：300ms 计时器如何区分"退出输入"和"触发回滚"两种意图
- [ ] Event::Paste 是独立事件链——绕过 key event 拦截路径单独处理，支持 bracketed paste + Windows 模拟粘贴检测 → `peri-tui/src/event/mod.rs`, `peri-tui/src/event/keyboard.rs`
- [ ] 字符串截断的字符级操作——CJK 文本用 `&s[..N]` 会 panic，必须用 `chars().take(N)` 或 `char_indices().nth(N)` → `CLAUDE.md 编码规范`, `peri-tui/src/ui/render_thread.rs`

## 命令系统

- [ ] Slash 命令的三种执行模式——立即执行、透传给 LLM、参数转换后执行，各适合什么场景
- [x] /rewind 如何回滚文件变更——截断对话历史的同时，逆向还原磁盘上所有被修改过的文件
- [ ] /bg 命令如何在后台跑独立任务——为什么故意不给后台 Agent 配置 MCP 工具

## 文件与安全

- [ ] 防止路径穿越攻击的三层校验——绝对路径拒绝、目录深度检测、解析后前缀验证，缺一不可
- [ ] Windows 上的路径分隔符问题——一个在 macOS 上完全正常的路径函数，在 Windows 上会悄悄产生反斜杠
- [x] Git 提交自动署名：追踪 Agent 的文件修改，commit 时附上 Co-Authored-By，支持 9 家模型的邮箱映射 → `domestic-models-work`
- [ ] Sync 协议的端到端加密——配对码即 PBKDF2 密钥，64KB 分片 + SHA-256 校验和保证大文件完整性 → `peri-tui/src/sync/crypto.rs`, `peri-tui/src/sync/packer.rs`
- [ ] 原子写入 + 自动备份策略——settings.json 写入前先 .bak 备份，.tmp 文件 rename 原子性保证配置永不错坏 → `peri-tui/src/sync/writer.rs`

## 模式与运维

- [x] -p 非交互模式：不启动界面，执行完直接输出结果，支持 text/json/stream-json 三种格式 → `acp-separation`
- [x] 推理增强模式：Anthropic 和 OpenAI 的推理参数不同，如何统一控制思考深度 → `domestic-models-adaptation`
- [x] 一键安装脚本：自动检测平台和架构，绕过 GitHub API 限速，支持代理下载 → `riscv-peri`
- [ ] 对话历史如何持久化——SQLite 管理多会话、子线程和取消操作，断点续跑的实现
- [ ] 插件的三种作用域：用户级、项目级、本地级——安装、卸载和工具资源聚合的设计
- [ ] 给 Agent 加一个定时器——用 Cron 表达式注册定时任务，到点自动触发新一轮对话
- [ ] 把 LSP 语言服务接入 Agent——让 Agent 能调用代码补全、定义跳转、实时诊断

## Side Project

- [ ] git-graph：在终端里可视化 Git 分支历史——拓扑排序布局、分支着色、三栏视图展示 stash/remote/status
- [ ] Lane 布局的收敛策略——优先保留低 index lane（视觉上更接近主线），ColorPool 按释放行号复用颜色避免分支跳动 → `side-projects/git-graph/src/graph/layout.rs`
- [ ] 增强字符的语义选择规则——收敛用 Merge（╰/╯）= 路径从上方来转向，分叉用 Branch（╭/╮）= 新分支向下走，靠 TOP/BOTTOM + 边的方向推导 → `side-projects/git-graph/src/graph/layout.rs`
- [ ] FNV-1a Hash 保证颜色稳定性——同一分支名永远获得同一颜色，HashMap values_mut().sort() 防迭代顺序不稳定导致视觉跳动 → `side-projects/git-graph/src/graph/color.rs`, `side-projects/git-graph/src/git/repo.rs`
- [ ] Git 数据的懒加载策略——scan_topology 一次性生成轻量 TopoNode（仅 oid/parent/time/message），按需 commit_detail 加载完整信息 + diff 统计 → `side-projects/git-graph/src/git/repo.rs`

## 系统提示词稳定性

- [x] 冻结的设计：系统提示词不可变性的进化之路——从每轮重建到 frozen_system_prompt 模式，为什么 session 内 system prompt 必须绝对不变 → `spec/global/domains/system-prompt.md`
- [x] Frozen Data 传播链的隐性成本——为什么新增一个 frozen 字段要同步检查 5 个文件，SubAgent 语言漂移暴露了全链路设计盲区 → `spec/global/domains/system-prompt.md#issue_2026-05-27`
- [ ] 系统提示词 15 个段落文件的静/动分区——01-06 静态始终 include，07-15 动态条件注入，6 个静态段约 8K tokens 占 80%，最大化缓存前缀命中率 → `peri-tui/prompts/sections/`, `peri-agent/src/llm/anthropic/cache.rs`

## 流式协议与字节级陷阱

- [x] SSE 流式 UTF-8 截断：from_utf8_lossy 为什么产生不可逆乱码——跨 chunk 边界截断多字节字符时 U+FFFD 替换后无法恢复，修复方案 pending_bytes 缓冲区 → `peri-agent/src/llm/openai.rs`, `spec/global/domains/agent.md#issue_2026-05-29`
- [x] 流式 JSON 的 max_tokens 截断：字段顺序决定生死——Write 工具超长内容被截断后 file_path 因字段顺序靠后而缺失，关键字段必须排在 Schema 前面 → `spec/global/domains/agent.md#issue_2026-05-15`
- [x] StopReason 撒谎：当 LLM 返回 end_turn 但内容含 tool_use——DeepSeek 元数据与内容不一致导致路由错误 400，修复用 has_tool_calls() 内容级检查 → `spec/archive-issues/2026-05-15-orphaned-tool-use-after-concurrent-tool-error`

## 编辑工具进化史

- [x] 一个编辑工具的 5 次重写：Edit → Hashline → LineEdit V1/V2/V3——每次迭代都因 LLM 生成的旧字符串与真实文件差异太大而失败，4 个计划文件跨越 1 个月 → `docs/superpowers/plans/2026-06-05-line-edit-tool` 等

## 并发与竞态

- [ ] 一次 Ctrl+C 的五层纵贯修复——从 ACP Server 到 UI 状态的事件路由错误：Cancel 被路由到 Error 处理器、round_start_vm_idx 失效、in_subagent() 静默吞噬父中断、ACP 无条件截断导致失忆、cancel token 未传播到 sync SubAgent → `spec/global/domains/agent.md#issue_2026-05-25` 等
- [ ] 并发同类型 SubAgent 共享相同 ID 导致事件路由错误——4 层链路都用 subagent_type 替代唯一实例 ID，所有事件路由到第一个实例，修复把 tool_call_id 贯穿整条链路 → `spec/archive-issues/2026-05-19-concurrent-subagent-duplicate-id.md`, `peri-middlewares/src/subagent/tool/define.rs`
- [ ] Background task 完成后未触发 agent continuation——BackgroundTaskCompleted 和 Done 走同一 channel，后台在 Done 之前完成时 agent_done_pending_bg 尚未设置，修复用 pre_done_bg_completions 缓冲区暂存 → `spec/archive-issues/2026-05-13-background-task-completion-race-condition.md`, `peri-tui/src/app/agent_events_bg.rs`
- [ ] Agent 工具从并发到串行再回到并发——为修并发 SubAgent 死锁引入串行，三个根因（流式取消 select!、4096 通道缓冲、source_agent_id 精确路由）独立修复后串行限制不再必要 → `spec/archive-issues/2026-05-18-agent-tool-calls-execute-serially.md`, `peri-agent/src/agent/executor/tool_dispatch.rs`
- [ ] 并发 Background Agent 只收到一次完成通知——TOCTOU 竞态让两个 invoke_background 同时通过计数检查，幽灵计数因注册失败留下永不递减的计数，修复持锁临界区 + 事件移到注册成功后发送 → `spec/archive-issues/2026-05-24-concurrent-bg-agent-only-one-completion.md`, `peri-middlewares/src/subagent/tool/define.rs`

## 跨平台工程

- [x] 一个 bash -c 包装器的三合一——MCP 客户端、Bash 工具、Hook 执行器各自手写平台判断，修复为统一 shell_command() 函数 → `peri-middlewares/src/process/mod.rs`
- [ ] shell_command() 的引号转义细节——Unix 参数内部单引号用 `'\''` 模式退出转义再进入，容易被忽略导致带引号参数执行失败 → `peri-middlewares/src/process/mod.rs`

## TUI 渲染工程

- [ ] TUI 流式 Markdown 表格 holdback 机制——流式渲染中表格字符逐个到达时显示残缺列，需暂缓渲染直到完整行到达 → `peri-widgets/src/markdown/`
- [ ] TUI Markdown LRU 缓存：避免每帧完整重解析——渲染线程中纯计算不一定是瓶颈，内存分配/回收才是 → `peri-tui/src/render/`
- [ ] RenderThread 有界通道 + 自适应流式帧率——无帧率限制时 loading 动画吃满单核 CPU，显式 60fps + batching → `peri-tui/src/render_thread.rs`
- [ ] WrappedLineInfo：CJK 文本在终端中的视觉行→逻辑行映射——逐字符 unicode-width 实现鼠标选区精准定位 → `peri-tui/src/wrap.rs`
- [ ] 19 个手写输入框的统一：FieldTextarea 重构——19+ 个 String + usize 替换为统一 tui_textarea 包装器 → `docs/superpowers/plans/2026-06-07-unified-textarea`
- [ ] wrap_map 增量计算——复用稳定前缀 + 修正偏移，resize 时只重算变化部分，节省 60-80% 重渲染开销 → `peri-tui/src/ui/render_thread.rs`
- [ ] 快捷键跨平台兼容陷阱——Alt+Enter/Alt+M 在 Windows 终端被截获，macOS Option 键发送字符而非修饰键，优先 Ctrl+字母 + 禁止 Shift+字母（编辑态等同大写） → `peri-tui/src/event/keyboard.rs`, `CLAUDE.md 编码规范`

## 插件生态

- [ ] 兼容 Claude Code 插件的完整设计——manifest 格式兼容（skills 字段陷阱、commands 混合数组）、MCP 环境变量 per-plugin 展开、三种安装范围 → `peri-middlewares/src/plugin/`, `spec/global/domains/plugin.md`
- [ ] Hook 系统：4 种执行类型 × 14 种事件——通过 exit code 控制流程，Agent 类型 hook 完整 50 轮循环，防递归 → `peri-middlewares/src/hooks/`

## 质量工程

- [ ] 3.35% 工具调用错误率的根因分析与修复——93% 源于 subagent_type 参数缺失，用数据说话而不是凭感觉优化 → `peri-agent/src/tool_errors.rs`
- [ ] Compact 子系统零测试——最危险的大约 2100 行代码没有测试保护，错误实现可直接损坏对话 → `peri-agent/src/agent/compact/`
- [ ] Full Compact 的路径上下文丢失——摘要 LLM 只看到工具名不知道操作哪个文件，注入 cwd 参数为摘要提供路径锚点 → `peri-agent/src/agent/compact/full.rs`, `spec/global/domains/compact.md#issue_2026-06-07`

## 架构设计

- [ ] ACP Event Bridge 三层事件映射的语义坍缩——ExecutorEvent → AcpNotification → AgentEvent，多个 bug 揭示了协议边界处信息的渐进损失 → `peri-tui/src/app/agent.rs`

## Goal Steering（长程目标跟踪）

- [ ] Agent 做到一半停了怎么办——Goal Steering 让它自己续跑到完成：6 态状态机（Active/Paused/Blocked/UsageLimited/BudgetLimited/Complete）+ SQL CASE 守护防非法翻转 + token 预算计费（`input - cache_read + output`）+ continuation 自动续跑循环放在 ACP 层而非 ReAct 层 + `expected_goal_id` 乐观锁防 stale update；2800 行代码、13 个 TRAP → `peri-agent/src/goal/`, `peri-acp/src/session/goal_state/`, `peri-acp/src/session/continuation.rs`

## Tool Search 延迟加载

- [ ] LLM 工具列表超过 20 个就开始变蠢——三层延迟加载架构：Core 12 工具始终可见 + Meta 2 工具（SearchExtraTools/ExecuteExtraTool）桥接 + Deferred N 工具（Cron/MCP/LSP）按需发现；LLM 不直接看到 Deferred 工具，通过 Meta 工具两步调用；`box_to_arc()` 透过 `ToolWrapper(ManuallyDrop)` 解决 `Box<dyn> → Arc<dyn>` 标准库缺失转换 → `peri-middlewares/src/tool_search/`

## Skills 热加载与冻结

- [ ] 加载一个技能文件就让 Prompt Cache 全部失效——Skills 冻结优化的设计：多目录发现（用户级 > 全局 > 项目级 > 插件）+ `session/new` 时一次性扫描生成 `frozen_summary` 避免每轮磁盘 I/O + `before_agent` 用 `prepend_message(system(summary))` 注入动态区域不破坏缓存前缀 → `peri-middlewares/src/skills/loader.rs`, `peri-middlewares/src/skills/mod.rs`

## 遥测与可观测性

- [ ] Agent 在想什么——自建 Langfuse 客户端给 ReAct 循环加 X 光：batcher 异步聚合 + OTLP 格式转换 + trace/span/generation 三层观测，与 ReAct 每个阶段集成（before_model/before_tool/after_tool/after_model），缺一公钥密钥则安全降级禁用 → `langfuse-client/`

## 权限与安全模式

- [ ] 让 Agent 自由但不危险——5 种权限模式的设计哲学：bypass（全跳过）/ default（逐个审批）/ dont-ask（不问但记录）/ accept-edit（自动批准编辑）/ auto-mode；运行时 Shift+Tab 动态切换 + LLM 自动判断操作危险等级 + HITL broker 无超时等待陷阱 → `peri-middlewares/src/hitl/`

## Widget 组件库

- [ ] 终端 UI 组件库怎么设计才能零业务依赖——49 个文件的 peri-widgets：仅依赖 ratatui + pulldown-cmark，包含 Markdown 渲染（语法高亮/表格/代码块）、Diff 可视化、FileTree、Spinner 动词动画、Form/List/RadioGroup 等通用组件 → `peri-widgets/`

## 上下文预算管理

- [ ] Agent 怎么知道自己的"记忆"快满了——预算计算比压缩策略更关键：ContextBudget 实时估算 token 占比（system prompt + tools + messages / context_window），双阈值触发 0.70 micro-compact / 0.85 full-compact，`COMPACT_THRESHOLD` 环境变量覆盖 → `peri-agent/src/agent/token.rs`, `peri-middlewares/src/compact_middleware.rs`

## 状态同步与快照

- [ ] 为什么不把完整对话发给前端——StateSnapshot 增量快照设计：Agent 每轮推送快照（迭代轮数 + 消息范围 + 工具状态），`prepend_message` 的 `insert(0)` 右移会泄露 System 消息必须 `.filter(|m| !m.is_system())`，TUI 通过快照 + 流式 TextChunk 增量维护 UI → `peri-agent/src/agent/executor/`, `peri-tui/src/app/agent.rs`

## Multi-Provider 运行时切换

- [ ] 同一对话中途从 Claude 切到 GLM——运行时 Provider 切换如何不破坏缓存：`session/new` 一次性捕获 provider 无关的 `frozen_system_prompt`，每轮从 `Arc<RwLock<>>` 克隆 Provider Snapshot；Ctrl+T 切模型、Ctrl+Shift+T 切 Provider，`SessionConfigOptions` 按优先级覆盖 → `peri-acp/src/session/executor.rs`, `peri-acp/src/dispatch/config_update.rs`

## 消息模型设计

- [ ] 一套消息类型打通 API + SQLite + JSON-RPC 三个世界——BaseMessage（Human/Ai/System/Tool 四角色）+ ContentBlock（Text/Image/Document/ToolUse/ToolResult/Reasoning/Unknown 七内容块）统一建模，同时满足 Anthropic/OpenAI API 序列化、SQLite JSON 持久化、JSON-RPC 传输；Unknown 变体吸收未知 provider 块实现前向兼容 → `peri-agent/src/message/`
