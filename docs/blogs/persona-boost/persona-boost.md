# Claude 的人格能提升别的模型智商——SWE-bench 实测涨 11 个点

> **[CC_Pure (CCP)](https://github.com/GhostDragon124/CC_Pure)** — 基于 [Claude Code Best (CCB)](https://github.com/claude-code-best/claude-code) 的开源增强实现，支持自定义 Persona、权限控制和多模型调度。<https://github.com/GhostDragon124/CC_Pure>

DeepSeek V4 Pro 加 CCP 工具链，90 个 SWE-bench Lite 实例分两组跑，结果差了 11 个百分点。

唯一的变量是一段 3KB 的 system prompt。模型、工具、参数全部不变——只换了段「性格」，模型多解了 9 道题。

这是我们的同僚 James Feng 做的实验。实验设计干净利落，核心发现足够反直觉——Claude 的角色特质能迁移到别的模型上，越难的题效果越猛。

## 90 个实例对照跑，只换 system prompt

从 SWE-bench Lite 的 300 个实例里取 90 个分层子集，分两组跑。Default 模式 system prompt 为空，模型完全按 base training 行为运作。Claude Persona 模式注入 3KB 的 Claude 角色文档。

两组共用 DeepSeek V4 Pro，bypassPermissions 让 agent 自动跑 pytest，600 秒超时，独立工作目录并行跑。

如果结果出现差异，原因只可能是那段 system prompt。

## 7 条性格特质写进 system prompt

注入的内容来自 Anthropic 2025 年 5 月公开的 Claude 4.5 Opus 角色文档。James 把它提炼成 7 条性格——求知欲、温暖但不奉承、直接自信、对错误开放、诚实准则、有分寸的帮助、协作者姿态。

「先读再改」「追踪调用链」「注意边界情况」——这些是技术指令，直接规定操作步骤。注入的 7 条性格完全不同，它们改变的是决策偏好，由模型自己判断。claude.md 的核心片段长这样。

```markdown
## Core traits
- **Intellectually curious.** You genuinely enjoy learning about and
  discussing ideas across every domain.
- **Direct and confident.** You share your genuine perspective. You
  disagree when you have good reason to.
- **Open to being wrong.** Confidence and openness aren't opposites.
  You hold your views firmly but revise them readily.
```

没有一行提到代码、测试或 bug。

## 59 比 50，增益集中在 Hard 实例

| | Default | Claude Persona |
|---|---|---|
| Resolved | 50 | 59 |
| 通过率 | 57.5% | 68.6% |

交集更有说服力——47 个两种模式都解了，25 个都没解。剩下 15 个产生分歧，Persona 独占 12 个，Default 独占 3 个。净赚 9 个，丢失 3 个，这不是零和博弈。

按难度拆，增益随难度递增。

| 难度 | Default | Claude Persona | 增益 |
|---|---|---|---|
| Hard | 56.1% | 68.3% | +12.2pp |
| Medium | 55.6% | 66.7% | +11.1pp |
| Easy | 60.7% | 67.9% | +7.1pp |

题越难，人格注入的收益越大。Hard 实例需要跨多文件追踪调用链，恰好是「求知欲」和「协作者姿态」发挥作用的场景。

## 注入后模型更愿意追踪多文件调用链

seaborn-2848 是最典型的对比。Default 模式在这个实例上连 patch 都没生成——模型望而却步，没敢动。Persona 模式解了。「默认帮助」和不过度谨慎的性格，推着模型先做了充分的代码分析，最终定位到一个跨库的 matplotlib 后端兼容性问题。

django 上的差距更系统化。37 个实例，Default 解了 56.8%，Persona 解了 70.3%，涨了 13.5 个点。django 的 bug 通常涉及跨 ORM、中间件、URL 路由的多文件修改。Default 模式倾向局部 patch，报错的那一行改掉就收工。Persona 模式在「求知欲」的驱动下更愿意追踪完整调用链再动手。

两组案例指向同一个结论——注入人格后，模型面对困难的第一反应从退缩转向深挖。

25 个两种模式都没解的实例说明这不是银弹。有些 bug——隐式环境依赖、C 扩展交互——超出了当前模型加工具链的能力边界。flask、requests、xarray 全军覆没。这些要靠更强的模型或更好的工具，性格解决不了。

---

回头看——只换了 3KB 性格文档，就多解 9 道题，其中 7 道是 Hard 级别。Claude 的人格确实能提升别的模型智商。

完整报告和 Claude Persona 定义都在 CC_Pure 仓库。

CCP 基于 Claude Code Best：<https://github.com/claude-code-best/claude-code>
项目地址：[github.com/GhostDragon124/CC_Pure](https://github.com/GhostDragon124/CC_Pure)
完整报告：[ccp-claude-persona-swebench-report-v2.md](https://github.com/GhostDragon124/CC_Pure/blob/main/docs/ccp-claude-persona-swebench-report-v2.md)
