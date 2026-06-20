# 一次重构 9000 行代码、2628 测试零回归——Claude Code ultracode 的实战首秀

> **[Claude Code Best](https://github.com/claude-code-best/claude-code)**——Claude Code 的开源复刻，社区维护的多 Provider Agent 终端工具。<https://github.com/claude-code-best/claude-code>

Peri 是一个用 Rust 写的开源 Coding Agent，仓库里有 5 个文件每个 750-860 行——render_state.rs 747 行、hooks/middleware.rs 846 行、panel_plugin.rs 779 行、tracer.rs 761 行、compact.rs 859 行。这类文件行话叫 god file（单个文件承担十几个职责、行数膨胀到改一行怕踩十处坑的大文件），每次改动都要核对散落在注释里的历史约束。

今天用 ultracode——Claude Code 最新引入的 workflow 编排（用脚本描述多 agent 协作流程）特性——把这 5 个 god file 一次性拆分。git diff 是 +222/-3917 行，加上 35 个新增子模块文件共约 5000 行，总变更近 9000 行。跑完全量测试 2628 个全过，零回归。cargo clippy 干净，lefthook 全套 hook 通过，一次 commit 落地。

## 每个 god file 承担 10+ 个职责

Rust 项目里 god file 尤其麻烦——类型系统虽然能挡住一部分错误，但跨模块的不变量、消息顺序、缓存前缀稳定性这些约束没法用类型表达，只能写在注释里。Peri 这 5 个 god file 是典型。

render_state.rs 747 行，承担 15 个职责，表格渲染、列表渲染、内联元素、代码块、CJK 字符宽度计算全集中在一个文件里。hooks/middleware.rs 846 行，分析时识别出 8 个 SRP（单一职责原则，每个模块只该承担一个职责的工程原则）违反点。panel_plugin.rs 779 行，光 open_plugin_panel 一个函数就 345 行。tracer.rs 761 行，16 个职责，trace、llm、tool、subagent、compact 五类事件的追踪逻辑都挤在一起。compact.rs 859 行，是项目 CLAUDE.md 标注 [TRAP]（Peri 项目内部约定的注释标记，记录一个历史踩坑及改动时必须遵守的约束）最密集的文件之一。

比如 compact.rs 有 6 个 contract test（契约测试，固定某个不变量的测试）锁住关键规则——compact 后的消息数组必须以 BaseMessage::human 开头，不能用 system 消息，否则 Anthropic 的 API 会把整个 system 消息 hoist（提升）到顶层 prompt，破坏 frozen system prompt（会话开始时一次性捕获、之后不可变的系统提示词）的缓存。

agent 拆分时只要漏掉一个 [TRAP] 注释，就是一个潜在的生产 bug。god file 重构通常是高风险低收益的工作，行业普遍回避。

## ultracode 把重构拆成四阶段多 agent 流水线

ultracode 在 Claude Code Best 2.7.0 里复刻成完整的 Dynamic Workflow 引擎，通过 /ultracode 触发。引擎核心是四种原语——phase 标记阶段、agent 派发子任务、parallel 并发执行、pipeline 串行流水线。每个原语有明确语义，parallel 是屏障（等所有任务完成才返回），pipeline 不屏障（每个 item 独立走完所有阶段）。

第一阶段 analyze，8 个 agent 并行分析 Peri 里 top 8 的 god file 候选。每个 agent 读一个文件，输出结构化的职责清单、SRP 违反点、推荐设计模式、拆分方案、风险评级。这一阶段只读不写，每个 agent 上下文独立。

第二阶段 synthesize，1 个 agent 拿 8 份分析综合优先级排序，推荐第一批重构哪些文件。综合是必要的——单个 analyzer agent 看自己负责的文件，没法判断哪个最该先动。这一步推荐了 compact、render_state、tracer 三个低风险高收益的文件，加上用户决定扩范围的 hooks 和 panel_plugin，第三阶段实际重构 5 个。

第三阶段 refactor，5 个 agent 按 facade（外观模式，把原大文件改成一个入口模块，对外暴露统一的 pub 接口、内部声明子模块）+ re-export shim（保留原 pub 接口的薄封装层）模式拆分。每个 agent 读分析报告、读原文件、创建子模块、修改入口、跑 cargo check 自我验证。facade 模式的关键是保留原文件的 pub API 签名不变，下游调用方零改动。

第四阶段 verify，1 个 agent 跑 cargo check --workspace 和 cargo test --workspace，看有没有回归。失败的话自己定位修复，最多重试 4 次。verify 用独立 agent 而不是让 refactor agent 自己跑测试，是因为自己改的代码自己测容易确认偏差——独立 agent 拿到完整 workspace 状态做交叉验证更可靠。

四阶段串行，阶段内并行。8 个 analyzer 并发几分钟跑完，5 个 refactor 因为有同 crate 的串行约束花了更长。整个重构 workflow 跑下来不到一小时。

## harness、skill、model 三层的职责边界

Claude Code Best 是 Claude Code 的开源复刻版，提供 harness 层——tools、runtime、permission 这套基础设施。每个 agent 都能用 Read 读文件、Edit 改文件、Bash 跑 cargo、Grep 搜代码。harness 不关心任务是什么，只提供能干事的工具。

ultracode 提供 skill 层——workflow 编排原语。phase、agent、parallel、pipeline 这些原语让你能用脚本表达先并行分析、再综合、再分波重构这种复杂控制流。skill 不直接执行工作，只决定工作怎么组织。

GLM 5.2 提供 model 层——长上下文加上工具调用稳定性。这次重构每个 agent 的上下文压力不小，读整个 800 行 god file、读分析报告、读 CLAUDE.md 相关章节、写新模块、跑 cargo check 看错误、修复，加起来一个 agent 上下文里要装几千行代码加多轮工具调用。GLM 5.2 的长上下文让单 agent 装得下完整任务，工具调用稳定性让 agent 能稳定执行 cargo check、Read、Edit 直到编译通过。

三层缺任何一层重构都做不成。harness 没有的话 agent 连 Read 和 Bash 都没有，只能自己封装 LLM API 加工具协议，和社区方案重复造轮子。skill 没有的话工作没有编排，大重构塞给单个 agent 必然超出上下文，或者手动开多个 session 再人工汇总，协调成本高且容易丢上下文。model 的长上下文是硬约束——单 agent 装不下时只能拆更多 agent，协调成本随数量线性增长，没有替代方案。

## re-export shim 和 contract test 约束 agent 改动边界

agent 重构最隐蔽的风险是改对了一部分但破坏了看不见的不变量。改错代码会被编译器或测试立刻抓住，但破坏不变量往往要到生产环境才暴露。这次靠两个机制把风险压到接近零。

facade + re-export shim 把下游接口固定。原文件比如 render_state.rs 改成入口模块——内部声明子模块 mod table、mod list、mod inline，再用 pub use 把子模块的所有 pub 项 re-export 出来。从下游调用方看，render_state::RenderState 这个路径完全没变，调用代码一行不改。agent 只要不改 pub 签名，下游零影响。

contract test 验证关键不变量。compact.rs 有 6 个 contract test，比如 compact 后的消息数组必须以 BaseMessage::human 开头。这些测试不是为了覆盖代码路径，是为了固定行为契约。agent 拆完跑一遍 contract test，过了就说明关键不变量没破坏。

加上 synthesize 阶段就强调过所有 [TRAP] 注释必须原样迁移到对应新模块，禁止简化——这条约束写在每个 refactor agent 的 prompt 里。最终 35 个新模块文件，每一个 [TRAP] 注释都保留在原位置或迁移到对应子模块，没有一个被删除。

三层约束叠加，把 5 个 agent 的改动范围限制在不变量保护的安全边界内。2628 个测试零回归验证了这种约束机制有效。

重构完跑 cargo clippy，整个 workspace 只有 1 处 redundant closure（冗余闭包，一个本可以直接传函数却多包了一层闭包的写法）warning——某行 `usage.map(|u| build_usage_details(u))` 该写成 `usage.map(build_usage_details)`。修完跑 lefthook 全套 hook 通过，commit 一次成。

这次重构落地说明，行业普遍回避的 god file 拆分，只要编排得当——四阶段流水线串起分析与重构、同 crate 串行避开文件锁、re-export shim 锁住下游接口、contract test 锁住不变量——一次会话就能做完。

项目地址：[github.com/claude-code-best/claude-code](https://github.com/claude-code-best/claude-code)
