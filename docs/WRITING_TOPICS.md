# Peri 博客写作素材库

已有博客覆盖的主题不再列入：compact-mechanism、edit-tool、multi-agent-patterns、perf-optimization、web-search、prompt-cache-optimization、acp-separation、introducing-peri、streaming-render、concurrent-tool-dispatch、domestic-models-adaptation、domestic-models-work、riscv-peri、rewind-design。

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

## LLM 适配

- [x] 统一适配层兼容 10 家模型——Anthropic/OpenAI 之外的国产模型各有哪些不兼容之处 → `domestic-models-adaptation`
- [x] 推理内容如何在多轮对话中正确回传——不同模型用不同字段名，同时兼容两套格式 → `domestic-models-adaptation`
- [x] DeepSeek 每条 assistant 消息都要带回思考块，但注入的伪消息没有——400 的根因和修复 → `domestic-models-adaptation`
- [x] Kimi 的推理参数和推理强度参数不能共存——运行时检测模型名并移除冲突字段 → `domestic-models-adaptation`
- [x] 流式输出的 token 用量统计：Qwen 需要额外请求参数才会返回，其他家不需要 → `domestic-models-adaptation`
- [x] GLM 用两个不同的字段名返回同一个东西——解析端同时检查两个字段兼容历史版本 → `domestic-models-adaptation`

## 中间件与 ReAct 循环

- [ ] 17 个中间件链式组合的设计约束——为什么工具钩子不能在执行过程中读取对话历史
- [ ] Peri 的 ReAct 循环全貌——从用户输入到工具执行再到最终回答的完整流程
- [ ] Agent 执行危险操作前如何询问用户——HITL 审批机制、权限模式动态切换和 LLM 自动分类器
- [ ] 通过 MCP 协议把任意工具接入 Agent——连接池管理、OAuth 回调、断线重连的实现

## 错误处理与踩坑

- [ ] Ctrl+C 取消后 Agent 失忆——中断时无条件截断历史导致已完成的工作被丢弃
- [ ] 中断和完成事件的竞争条件——两个事件都想修改同一个状态，谁先到谁说了算
- [ ] 子 Agent 的输出为什么会在界面上越叠越多——状态没有在正确的时机清空
- [ ] 自定义 Slash 命令的隐蔽陷阱——绕过 Agent 循环的命令必须自己发完成信号，否则前端永远等待
- [ ] 恢复历史对话时，系统内部消息出现在界面上——过滤逻辑漏掉了持久化进数据库的内部消息

## TUI 与渲染

- [ ] 中文鼠标点击偏移：修了三次才真正修好——鼠标坐标是显示列宽，光标位置是字符索引，两者不是同一回事
- [x] 独立渲染线程解析 Markdown + 计算行包装，UI 线程只负责从缓存读取可见行重绘 → `streaming-render`
- [x] 流式输出自适应帧率：短消息 30fps，长消息降到 5fps，减少 CPU 空转 → `streaming-render`
- [x] 鼠标滚动事件合并：连续滚动只保留最后一个，避免单次滚动触发多次重绘 → `streaming-render`
- [ ] Markdown 表格里的中文列被压扁了——从等比缩放改为最小宽度优先的修复过程
- [ ] 双击 ESC 回滚对话：300ms 计时器如何区分"退出输入"和"触发回滚"两种意图

## 命令系统

- [ ] Slash 命令的三种执行模式——立即执行、透传给 LLM、参数转换后执行，各适合什么场景
- [x] /rewind 如何回滚文件变更——截断对话历史的同时，逆向还原磁盘上所有被修改过的文件
- [ ] /bg 命令如何在后台跑独立任务——为什么故意不给后台 Agent 配置 MCP 工具

## 文件与安全

- [ ] 防止路径穿越攻击的三层校验——绝对路径拒绝、目录深度检测、解析后前缀验证，缺一不可
- [ ] Windows 上的路径分隔符问题——一个在 macOS 上完全正常的路径函数，在 Windows 上会悄悄产生反斜杠
- [x] Git 提交自动署名：追踪 Agent 的文件修改，commit 时附上 Co-Authored-By，支持 9 家模型的邮箱映射 → `domestic-models-work`

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

## 系统提示词稳定性

- [x] 冻结的设计：系统提示词不可变性的进化之路——从每轮重建到 frozen_system_prompt 模式，为什么 session 内 system prompt 必须绝对不变 → `spec/global/domains/system-prompt.md`
- [x] Frozen Data 传播链的隐性成本——为什么新增一个 frozen 字段要同步检查 5 个文件，SubAgent 语言漂移暴露了全链路设计盲区 → `spec/global/domains/system-prompt.md#issue_2026-05-27`

## 流式协议与字节级陷阱

- [ ] SSE 流式 UTF-8 截断：from_utf8_lossy 为什么产生不可逆乱码——跨 chunk 边界截断多字节字符时 U+FFFD 替换后无法恢复，修复方案 pending_bytes 缓冲区 → `peri-agent/src/llm/openai.rs`, `spec/global/domains/agent.md#issue_2026-05-29`
- [ ] 流式 JSON 的 max_tokens 截断：字段顺序决定生死——Write 工具超长内容被截断后 file_path 因字段顺序靠后而缺失，关键字段必须排在 Schema 前面 → `spec/global/domains/agent.md#issue_2026-05-15`
- [ ] StopReason 撒谎：当 LLM 返回 end_turn 但内容含 tool_use——DeepSeek 元数据与内容不一致导致路由错误 400，修复用 has_tool_calls() 内容级检查 → `spec/archive-issues/2026-05-15-orphaned-tool-use-after-concurrent-tool-error`

## 编辑工具进化史

- [x] 一个编辑工具的 5 次重写：Edit → Hashline → LineEdit V1/V2/V3——每次迭代都因 LLM 生成的旧字符串与真实文件差异太大而失败，4 个计划文件跨越 1 个月 → `docs/superpowers/plans/2026-06-05-line-edit-tool` 等

## 并发与竞态

- [ ] 一次 Ctrl+C 的五层纵贯修复——从 ACP Server 到 UI 状态的事件路由错误：Cancel 被路由到 Error 处理器、round_start_vm_idx 失效、in_subagent() 静默吞噬父中断、ACP 无条件截断导致失忆、cancel token 未传播到 sync SubAgent → `spec/global/domains/agent.md#issue_2026-05-25` 等

## 跨平台工程

- [x] 一个 bash -c 包装器的三合一——MCP 客户端、Bash 工具、Hook 执行器各自手写平台判断，修复为统一 shell_command() 函数 → `peri-middlewares/src/process/mod.rs`

## TUI 渲染工程

- [ ] TUI 流式 Markdown 表格 holdback 机制——流式渲染中表格字符逐个到达时显示残缺列，需暂缓渲染直到完整行到达 → `peri-widgets/src/markdown/`
- [ ] TUI Markdown LRU 缓存：避免每帧完整重解析——渲染线程中纯计算不一定是瓶颈，内存分配/回收才是 → `peri-tui/src/render/`
- [ ] RenderThread 有界通道 + 自适应流式帧率——无帧率限制时 loading 动画吃满单核 CPU，显式 60fps + batching → `peri-tui/src/render_thread.rs`
- [ ] WrappedLineInfo：CJK 文本在终端中的视觉行→逻辑行映射——逐字符 unicode-width 实现鼠标选区精准定位 → `peri-tui/src/wrap.rs`
- [ ] 19 个手写输入框的统一：FieldTextarea 重构——19+ 个 String + usize 替换为统一 tui_textarea 包装器 → `docs/superpowers/plans/2026-06-07-unified-textarea`

## 插件生态

- [ ] 兼容 Claude Code 插件的完整设计——manifest 格式兼容（skills 字段陷阱、commands 混合数组）、MCP 环境变量 per-plugin 展开、三种安装范围 → `peri-middlewares/src/plugin/`, `spec/global/domains/plugin.md`
- [ ] Hook 系统：4 种执行类型 × 14 种事件——通过 exit code 控制流程，Agent 类型 hook 完整 50 轮循环，防递归 → `peri-middlewares/src/hooks/`

## 质量工程

- [ ] 3.35% 工具调用错误率的根因分析与修复——93% 源于 subagent_type 参数缺失，用数据说话而不是凭感觉优化 → `peri-agent/src/tool_errors.rs`
- [ ] Compact 子系统零测试——最危险的大约 2100 行代码没有测试保护，错误实现可直接损坏对话 → `peri-agent/src/agent/compact/`

## 架构设计

- [ ] 17 个中间件的链式编排：顺序敏感的 5 钩子设计约束——每个中间件在 5 个钩子中的插入点如何影响下游，CLAUDE.md 记录了 6 个顺序依赖 → `peri-middlewares/CLAUDE.md`
- [ ] ACP Event Bridge 三层事件映射的语义坍缩——ExecutorEvent → AcpNotification → AgentEvent，多个 bug 揭示了协议边界处信息的渐进损失 → `peri-tui/src/app/agent.rs`
- [ ] MCP Streamable HTTP + OAuth 2.0 完整协议集成——Stdio/HTTP 双传输、Authorization Code + PKCE、本地回调/手动粘贴混合回退、Token 0600 持久化 → `peri-middlewares/src/mcp/`
- [ ] ServiceRegistry 职责扩散与 GlobalUiState 拆分——跨会话共享服务和纯 UI 临时状态混在一起的代价 → `peri-tui/src/app/service_registry.rs`
