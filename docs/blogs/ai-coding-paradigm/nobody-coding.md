# Nobody Coding


## Issue Loop

整个项目跑在一条闭环上。四步，Agent 自己触发自己。

**issue-create** 接手人的描述，补全细节、查重复、判优先级，把模糊的感觉变成可追踪的任务单。人只描述现象，不诊断。根因是 fix 环节的事。

**fix-issue** 动笔之前先读 CLAUDE.md。里面全是 TRAP——Agent 以前踩过的坑，标注为不可行路径。方案空间被切过，Agent 不自由发挥，在被切过的空间里找路。

**issue-verify** 修完自己验证。改了哪些文件、做了什么改动、commit hash，全写进 issue。人看一眼结果对不对。对就过，不对退回去。

**issue-archive** 问题关闭后做三件事。提炼领域认知写进 spec，有新约束回写 CLAUDE.md，更新全局索引。CLAUDE.md 里几十条 TRAP，没有一条是我写的——全是 Agent 在 archive 阶段自己写进去的。

新 TRAP 写进去之后，Agent 下次遇到同类问题，它已经知道了。不是大概知道，是确定——因为 TRAP 是硬约束。

## 约束是怎么长出来的

Prompt Cache 那件事很说明问题。

第一次出问题是在用了动态占位符之后。缓存命中率掉下来了。Agent 修完回写规则——动态内容放 boundary 标记之后。第二次是中间件注入的消息破坏了前缀，修完回写——frozen_system_prompt 会话级冻结。第三次是 MCP 工具注册顺序不稳定，修完回写——工具列表确定性排序。

三个独立的问题，三次独立的修复，三条独立的规则。合在一起，命中率从 20% 涨到 98.5%。这不是前期设计的，是修出来的。

模型兼容也是这么过来的。DeepSeek 不认识 thinking block，OpenAI 要求 reasoning_content 原样回传。修好这边坏那边，反复横跳了好几轮。后来 Agent 直接把每个 provider 的处理规则列进 CLAUDE.md，不再猜。

并发工具调用也是。P3/P4 错误路径提前 return 会把后面的 tool_result 写漏。Agent 改成先收集所有错误，循环结束统一判断。这条模式写进 TRAP 后，以后再写并发代码，early return 这条路径直接被跳过了。

还有一个小的——Alt+Enter 在 Windows 上被终端截获。修完回写规则：新增快捷键优先用 Ctrl。后面加新功能自动遵守。

这些东西的特点是一样的：不是人设计出来的规范，是 Agent 在事故现场自己提炼的。人只审查精确性，不动手写。

## 还没闭环的地方

TRAP 数量在涨。二十几条审得过来，五十条以上会打架、冗余、有盲区。让 Agent 自己检查 TRAP 质量——这个任务本身也需要 TRAP 来约束。自举问题，还没解。

不是每个 issue 都值得回写 TRAP。有些是纯实现细节，有些很小但揭示了边界条件。这个判断还靠人对项目的感觉，机器没法做。

TRAP 长得再快也追不上代码库变复杂的速度。总有覆盖不到的地方。到时候治理体系本身可能也需要被治理。

这就是 Issue Loop 继续转的原因。今天的盲区，明天的 issue，后天的 TRAP。

## 试一下

下次 Agent 犯了同样的错，别打开编辑器。建一个 issue，把现象写清楚，关掉。Agent 会自己跑完闭环。

修完去翻 CLAUDE.md，新的约束应该已经在了。从那之后，这个坑不会再踩。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
