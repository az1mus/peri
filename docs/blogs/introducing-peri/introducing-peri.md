# Peri：修够了Claude Code，我们自己造了一个

[Peri Code](https://github.com/konghayao/peri) 是我们的新 Coding Agent! 我们团队自从拿到 [Claude Code](https://github.com/claude-code-best/claude-code) 的代码就开始努力维护，到现在已经有 2 个月了。我们也算是从第一天就见识了 Vibe Slop Engineering——AI 生成的代码一层套一层，Vibe Code 就完事了，到处是拼凑的胶水代码，然后 Claude Code 跑着跑着就飙到好一个多 G 内存。我们花了好几个大版本去修复历史的内存泄露，让 Claude Code Best 完成了生产级别的修补。

在积累了足够的经验之后，我们决定自己造一个。Peri 是一个从零开始、用 Rust 写的 Coding Agent 框架，在编写初期就考虑了上面的各种问题， 我们也用实战检验了我们的大型 Agent 工程化及长程开发任务的方法论，后续也会单独写文章分享。

值得一提的是，Peri 本身的代码，99% 是用国产模型生成的——主要用的是 DeepSeek-v4-pro 和 GLM-5.1，把你可以看看 Git 历史记录，大部分都是他们的名称。Peri 现在是自己修自己，做到了比较完善的生产体验。

![image.png](https://p0-xtjj-private.juejin.cn/tos-cn-i-73owjymdk6/4572373c73f04f84a4ff104d7b6042f2~tplv-73owjymdk6-jj-mark-v1:0:0:0:0:5o6Y6YeR5oqA5pyv56S-5Yy6IEAg5rGf5aSP5bCn:q75.awebp?policy=eyJ2bSI6MywidWlkIjoiMjY1NjAzNzk4MzIzODUwNCJ9&rk3s=f64ab15b&x-orig-authkey=f32326d3454f2ac7e96d3d06cdbb035152127018&x-orig-expires=1781345722&x-orig-sign=V3DDEtRVDNvW9b6PTLyS4NvAFUo%3D)

## 快速上手

macOS 和 Linux 用户直接跑这个，

```bash
curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash
```

Windows 用户用 PowerShell，

```powershell
irm https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.ps1 | iex
```

装完终端里输入 `peri` 就能跑起来。第一次打开会让你配模型和 API Key，在界面里填就行，不用去翻配置文件。我们就想让它尽可能简单，安装简单，配置简单，开箱即用。建议配合 [Herdr](https://github.com/ogulcancelik/herdr) 或者 Tmux 使用，在 Mac 端有非常好的终端体验。

## 我们对 Peri 的设计追求

下面是我们粗略汇总的 Peri 特性，每一个都是我们精心设计的，不是 AI Vibe 出来的 Slop 想法。

* 🧠 **内存可控性** — 专为长程任务、多实例开启场景优化的内存。连续跑几个小时、几百轮对话，内存稳定在 200MB 左右，无强力内存碎片与泄露问题。

* ⚡ **98.5% 上下缓存率** — 消息管线全链路受控，写入顺序、工具注册、动态占位符全部在会话开始时冻结，严格复用，大幅降低 token 费用和等待时间，并且消息过程中自动提示你缓存过低。这里强烈推荐 DeepSeek 官方 API，它的缓存是最为优秀的，缓存驱逐非常少见。

* 🌐 **多模型支持** — 统一适配层封装 Anthropic 和 OpenAI 协议差异，DeepSeek、GLM、Qwen 等国产模型均可接入，运行时 `Ctrl+T` 一键切换。

* 🔄 **Agent 自愈能力** — 我们的工具调用的错误信息设计为类似 Rust 编译器的能力，不是简单报错，而是结构化地告诉 Agent 哪里错了、为什么错、该怎么改，引导它回到正确路径。

* 🤖 **多 Agent 并发** — 多智能体架构在我们实践中证实普遍由于单线程执行，并行推进使得 Agent 任务效率更高。默认开箱支持同步、后台、fork 三种模式，使用 superpowers 等 skills，可以让多个 SubAgent 同时执行并发搜索、正交验证或者是交叉开发。

* 🛡️ **编译时安全网** — 工程化 Rust，严格约束每个 feature 的提交和规范，类型错误和并发安全问题编译时拦截，每次提交和更改都是在完整的编译器约束下，保证高度的交付质量。

## 兼容 Claude Code 生态

这个大概是大家最关心的问题。切换到新工具，比如 Codex、Pi 等，之前的配置全白费了，工作流得重新搭，团队成员还得重新适应。所以我们做的第一件事不是加新功能，而是确保 Peri 能直接接住 Claude Code 用户的所有积累。

我们在兼容性上花了非常多的努力，你的 `CLAUDE.md` 不用动，Skills 不用动，MCP 服务器不用动，插件也不用动。你在 Claude Code 里积累的项目配置、技能模板、第三方服务连接，Peri 全部原封不动地识别和使用。

插件系统也搬过来了。`/plugin` 命令照常用，在里面搜索、下载、安装插件，跟 Claude Code 的体验一模一样。之前装过的插件直接生效，不用重新装。

## 分层架构与扩展能力

Peri 的架构核心设计经过了一次大改动，完整把 UI 与 Agent 通过 ACP 协议分离开了，现在 Peri 借助 ACP 规范的能力，可以实现 Headless 的运行，在 Zed、JetBrain 等工具上都可执行。后续再配合 ACP Stdio 转 WS 端口服务，我们有能力将 Peri 带上统一的用户界面，从而不需要改动底层一行代码。

（我们后续开发的端云一体的大型 Agent Cloud 平台也是基于这个能力的）

## 继续翱翔

Peri 已经在我们团队的生产环境中使用了。如果你在用 Claude Code 或者对 AI Agent 框架感兴趣，欢迎试试看，遇到问题直接提 issue，我们会认真看每一条。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
