# Peri Code 的多智能体——如何让一群 Agent 协作完成一个任务

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。`curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash`

一个人写代码，遇到不确定的地方会先查文档、再写方案、然后动手、最后跑测试。这个过程是串行的——查完才能写，写完才能测。

但如果你有一个团队，可以让一个人查文档的同时另一个人跑测试，写代码的人拿着前面查到的结果直接动手，测试的人跑完自动通知。这就是 Peri Code 多智能体要解决的问题——不是让 Agent 跑得更快，而是让多个 Agent 协作完成一个单 Agent 做不好的任务。

Peri Code 内置了 5 个专用 agent（explore/coder/plan/verification/general-purpose），支持三种协作模式（Sync/Background/Fork），每种模式解决不同的协作场景。这篇文章讲清楚这些设计。

## 协作的基础——专用 Agent 分工

多智能体协作的前提是分工。一个人什么都做效率不高，一群什么都能做的人一起做也一样。Peri Code 的做法是给每个 agent 一个明确的职责边界——只读的不写，规划的不动手，验证的不改代码。

**explore** 用 Haiku 模型的快速只读探索 agent。只能用 Glob、Grep、Read，不写不改。适合「帮我看一下这个函数在哪」「这个模块的结构是什么」。Haiku 模型成本低、速度快，几十秒就能回来。

**coder** 专注代码实现。能用 Read/Grep/Glob/Bash/Edit/Write/TodoWrite，不能调子 agent（防递归）。maxTurns 设到 200，专门处理长程实现任务。coder 的设计思路是——一个只管写代码的 agent 不需要搜索能力以外的东西，也不需要再派子任务。

**plan** 只读规划 agent。分析需求、输出架构方案和关键文件列表，不动一行代码。plan 和 explore 的区别在于输出——explore 回答「是什么」，plan 回答「怎么做」。

**verification** 实现后验证。跑构建、测试、lint，输出 PASS/FAIL/PARTIAL 判定。默认后台运行——主 Agent 改完代码，派 verification 去跑测试，自己继续干活。

**general-purpose** 兜底 agent。继承所有工具，当其他专用 agent 不够用时的 fallback。

每个 agent 的定义就是一个 Markdown 文件，YAML frontmatter 声明工具、模型、权限，正文是系统提示词。用户可以在 `.claude/agents/` 下自定义 agent，同名覆盖内置。插件也能注册自己的 agent。

## 三种协作模式

有了专用 agent，怎么让它们协作？Peri Code 支持三种模式，核心区别是**主 Agent 和子 Agent 的时序关系和上下文隔离程度**。

**同步模式（Sync）**——主 Agent 阻塞等待子 agent 完成，拿到结构化结果。子 agent 从零开始，只拿到任务描述，不继承父 agent 的对话历史。适合串行流水线——前一个 agent 的输出是后一个的输入。

**后台模式（Background）**——主 Agent 派发任务后立即继续，不阻塞。子 agent 在后台跑，完成后通过系统消息通知主 Agent。最多 3 个并发后台任务。适合不需要等结果的长任务。

**Fork 模式**——继承主 Agent 的完整对话历史和工具集，像是从主 Agent「分叉」出来一个并行实例。设计上选择完整继承而不是选择性继承——因为「选择性继承哪些历史」本身就是一个很难自动做出的决策，不如全给，让子 agent 自己判断。适合需要对话上下文的并行任务。

```
Sync:       主 Agent → [等待] → 子 agent 完成 → 拿结果
Fork:       主 Agent → [分叉] → 子 agent 带完整上下文并行跑
Background: 主 Agent → [派发] → 继续工作 → 后台完成后收到通知
```

每轮对话中 SubAgent 的构建完全独立于父 agent——AgentState、中间件链、LLM 实例都是全新构造的，不共享状态。Cancel Token 跟着模式走——Sync 和 Fork 用 `Cascade` 策略（父取消时子跟着取消），Background 用 `Independent`（父取消不影响后台任务）。

## 场景 1：串行流水线——重构一个模块

用户说「把 `tool_dispatch.rs` 里的错误处理拆成独立模块」。这是一个典型的串行流水线——需要先理解现状，再规划方案，最后动手改。

```
主 Agent 收到任务
  → 派 Sync(explore)：分析 tool_dispatch.rs 的依赖关系，找到所有调用错误处理函数的地方
  → 等 explore 回来，拿到 12 个调用点的文件列表
  → 派 Sync(plan)：基于 explore 的结果，输出拆分方案——新模块的函数签名、迁移步骤
  → 等 plan 回来，拿到方案
  → 派 Sync(coder)：按方案迁移代码
  → 等 coder 完成
  → 汇总结果返回给用户
```

主 Agent 全程等待，但不做重活。探索交给 explore（Haiku 模型，快且便宜），规划交给 plan（继承模型，能分析复杂依赖），实现交给 coder（专注写代码，maxTurns=200 能跑长任务）。

为什么不用一个通用 agent 从头做到尾？因为专用 agent 的系统提示词聚焦——explore 的提示词告诉它「只找信息，不要改文件」，plan 的提示词告诉它「只输出方案，不要动手」。一个通用 agent 容易在探索阶段就开始改代码，改到一半发现方向错了还得回退。分工不是额外开销，是减少错误的手段。

## 场景 2：并行验证——改完代码后同时跑三项检查

用户说「我改了消息管线的三个文件，帮我跑一下测试」。三项检查互不依赖——测试不需要等 clippy，clippy 不需要等 import 检查。并行派发。

```
主 Agent
  → 派 Background(verification)：跑 cargo test
  → 派 Background(verification)：跑 cargo clippy
  → 派 Background(verification)：检查变更文件的 import 是否完整
  → 主 Agent 自己继续回答用户的其他问题
  → 后台任务逐个完成，系统消息通知主 Agent 结果
```

三个 verification agent 并发跑，主 Agent 不阻塞。Background 的 `Independent` Cancel Token 在这里体现价值——用户如果中途改主意、Ctrl+C 取消了当前对话，后台的测试还是会跑完，不会白费。

为什么用 Background 而不是 Fork？因为 verification 不需要对话上下文——它只需要知道「检查哪些文件」，任务描述里已经给了。用 Fork 继承完整对话历史是浪费 token。Background 从零开始，轻量且不阻塞。

## 场景 3：并行实现——大范围迁移

用户说「把这三个模块的错误处理全部迁移到新方案」。三个模块相对独立，不需要串行等。但子 agent 需要理解「新的错误处理方案是什么」——这个信息在主 Agent 的对话历史里。

```
主 Agent（持有完整对话历史，理解了迁移方案）
  → Fork 子 Agent 1：迁移 module_a
  → Fork 子 Agent 2：迁移 module_b
  → Fork 子 Agent 3：迁移 module_c
  → 三个 Fork 实例并发跑，各自拿到完整的对话历史
  → 主 Agent 等三个 Fork 全部完成，汇总结果
```

Fork 的关键优势——三个子 agent 都知道「新方案是什么」。如果用 Sync，每个子 agent 从零开始，你得把迁移方案在每个 agent 的任务描述里重复传一遍。一个迁移方案可能有几十行细节，传三遍就是三倍的 token 开销。Fork 继承完整历史，子 agent 上来就知道上下文，不需要重复沟通。

为什么不用 Background？因为主 Agent 需要等三个 fork 都完成才能汇总结果。Background 是「跑了就走，完事通知我」，Fork 是「分叉出去但最终回到我这里」。需要汇总结果的任务用 Fork，不需要汇总的用 Background。

## 场景 4：混合编排——完整的工程流程

真实任务很少只用一种模式。一个完整的「重构 + 验证」流程会混合使用三种模式。

```
主 Agent 收到「重构消息管线」任务

  阶段 1：理解现状
  → 派 Sync(explore)：分析消息管线的模块结构和依赖关系
  → 等结果

  阶段 2：并行规划
  → 派 Sync(plan)：输出重构方案
  → 等方案

  阶段 3：并行实现（Fork）
  → Fork 子 Agent 1：重构 message_pipeline.rs
  → Fork 子 Agent 2：重构 message_view_model.rs
  → 等两个 Fork 完成

  阶段 4：并行验证（Background）
  → 派 Background(verification)：跑全量测试
  → 派 Background(verification)：跑 clippy
  → 主 Agent 自己继续回答用户，后台验证跑完自动通知
```

阶段 1 和 2 用 Sync（串行依赖，前一步的输出是后一步的输入）。阶段 3 用 Fork（并行实现，需要对话上下文）。阶段 4 用 Background（不需要等，跑完通知就行）。

这个编排不是用户手动指定的——主 Agent 的 LLM 自己决定用什么模式、派什么 agent。用户只说「重构消息管线」，编排是自动的。但这要求框架提供足够好的工具设计，让 LLM 能做出正确的分派决策。这就是为什么专用 agent 的职责边界要清晰——如果 explore 和 plan 的能力重叠，LLM 就不知道该派谁。

## 选择逻辑

三种模式的选择归结为两个判断：

1. **主 Agent 需不需要等结果？** 不需要 → Background。需要 → 继续判断。
2. **子 Agent 需不需要对话上下文？** 不需要 → Sync（轻量，从零开始）。需要 → Fork（继承完整历史）。

```
                    需要等结果吗？
                   /              \
                 否                是
                 |                  |
            Background        需要上下文吗？
                            /              \
                          否                是
                          |                  |
                        Sync              Fork
```

这个决策树足够简单，LLM 在实际调用中几乎不会选错。

## 工具继承和防递归

子 agent 的工具集不是简单的「全部继承」。`tools` 为空时继承父 agent 所有工具但排除 Agent（防递归——子 agent 不能再调子 agent，避免无限嵌套）。`tools` 为 `*` 时继承全部包括 Agent。`tools` 为具体列表时走白名单。`disallowedTools` 在白名单基础上额外排除。

```
explore:     继承全部 - {Agent, Write, Edit, Bash} = 只读探索
plan:        继承全部 - {Agent, Write, Edit, Bash} = 只读规划
coder:       白名单 [Read, Grep, Glob, Bash, Edit, Write, TodoWrite]
verification: 继承全部 - {Agent, Write, Edit} = 只读验证 + Bash 跑测试
general:     * = 全部工具
```

防递归是最关键的安全约束。如果 coder 能调子 agent，它可能派一个 coder 去做子任务，子 coder 再派一个 coder——无限递归直到 token 用尽。Agent 工具默认从子 agent 的工具集中排除，只有显式声明 `tools: *` 的 agent（general-purpose）或者 Fork 模式才能继续派发。

## 继续迭代

多智能体协作的核心不是让每个 agent 变强，而是让它们各司其职、减少重叠。explore 只探索不改代码，plan 只规划不动手，coder 只实现不设计，verification 只验证不改文件。职责越清晰，LLM 的分派决策越准确，协作效率越高。

如果你在用 Peri Code 的多 agent 能力，遇到问题直接提 issue。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
