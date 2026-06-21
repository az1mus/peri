# Peri Code × CCB 2.8.0——八小时复刻 Anthropic Artifacts，双端客户端开源就绪

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。<https://github.com/KonghaYao/peri>

Agent 写 HTML 已经够好了——看板、报告、原型、交互页面，模型几分钟就能出活。真正卡住的是写完以后：想分享就得自己起 HTTP server，太折腾；拷贝文件发给别人，太原始。缺的就是一个上传按钮——点一下生成公开 URL，发链接就能打开。

Anthropic 的 Artifacts 做的正是这件事，但它闭源、绑 Claude 账号——开源 Agent 用不了。我们花了 8 小时，在 CCB 2.8.0 里把它完整复刻了出来：服务端、工具链、UI 渲染全部开源。更妙的是，Peri Code 拿了同一套后端和接口，一小时就用 Rust 移植完了客户端。两个开源 Agent 共享一个免费的云端 Artifacts 底座——这是以前没有过的事。

Artifacts 选择只支持 HTML，出发点不是一个格式偏好，而是一个约束：让 Agent 产出"适合看而不是适合读"的东西。终端文本天然是线性阅读的介质，当 Agent 产出一段带注释的 diff、一组设计方案的并排对比、一个可拖拽的排序面板时，终端完全装不下。Artifacts 补的就是这个缺口——它是终端文本的反面，把 Agent 的工作成果变成可直接浏览、可交互、可分享的页面。

这个约束反过来让 Agent 不用纠结输出格式——就是 HTML，没有第二个选项。模型的 HTML 生成能力已经足够成熟，Tailwind、Chart.js 直接 CDN 引用就能做出可交互图表。PDF 容易生成出错，SVG 缺乏交互能力，图片需要额外渲染步骤。HTML 是最低摩擦的选择。

## 不只是看板——Artifact 的典型场景

数据看板是最直观的用法，但不是唯一的用法。Artifact 的覆盖面比看板宽得多。只要 Agent 产出的东西"看比读快"，就该上页面。

**PR 走查带注释 diff。** 让 Agent 对一个 PR 做 code review，把 diff 渲染成页面，在对应行旁边标注发现和建议，按严重程度着色。Reviewer 打开链接就能在 diff 上下文里直接看分析，不用在终端文本里找对应行号。

**多方案对比网格。** 让 Agent 生成某个模块的几种不同实现方案，API 形状、数据流、性能取舍——排成网格，每个方案下面一行 tradeoff 说明。比来回问"换一种呢"高效得多。

**交互式调参。** HTML 里内联一点点 JS 就能做出滑块、开关、输入框。给 easing 曲线加个滑块，CSS transition 的参数实时预览——这在终端里完全做不到，但在 artifact 里就是几行 JS 的事。

**结果回流到 session。** 比纯看更进一步——artifact 可以加一个"Copy as prompt"按钮，用户在页面上操作完（比如拖拽排序了 issue 优先级），一键把结果转成 prompt 粘贴回终端，Agent 接着干活。页面不只是展示，还是轻量交互入口。

**进度追踪 checklist。** 让 Agent 执行长任务时同步维护一个 artifact checklist，每完成一项就勾掉并附注。团队其他人打开链接就能看到进度，不用守着终端。

这些场景的核心逻辑是一样的：Agent 产出 HTML，上传，给链接。差别只在 HTML 里写了什么。约束越少，Agent 越知道该怎么做。

## Deno Deploy 代理国内访问，Cloudflare Workers 处理存储

服务端只有两层——一层边缘代理，一层业务逻辑加存储。Deno Deploy 放在最外层，应对的是国内用户直连 Cloudflare Workers 延迟高、丢包严重的问题。Deno Deploy 在国内的连通性比 Cloudflare 好一个量级，放在前面做透传代理，请求和响应 body 完整转发，不加工。唯一的副作用是 Deno Deploy 会把上游的 HTTP status code 抹平为 200，所以客户端必须解析 response body 里的 error 字段来判断真实状态——这个约束贯穿了整个客户端的错误处理路径。

Cloudflare Worker 处理三件事——Bearer token 鉴权，MIME 类型和文件大小校验（只收 text/html，上限 10MB），文件写入 R2 bucket（Cloudflare 的对象存储服务，兼容 S3 API）。Worker 只做请求级别的处理，不跑 cron、不扫表、不维护任何状态。整个 Worker 只有 119 行代码。

过期逻辑完全交给 R2 bucket 的 lifecycle rule。文件 key 带前缀——`7d/` 或 `30d/`，R2 配两条 rule 分别在 7 天和 30 天后自动删除对应前缀的文件。Worker 完全不参与过期处理，119 行能做完服务端，核心减负就在这里。

文件 ID 用 nanoid(21) 生成，126 bit 熵足够大，不做碰撞检查。迭代同一个 artifact 时传 `hash` 参数就能让 URL 保持稳定——Worker 内部先删旧 key（R2 delete 不存在的 key 不报错），再写新 key。URL 即秘密，拿到链接的人都能访问，没有额外的权限层。

## Agent 怎么知道该上传？——把工具藏起来，让提示词教它用

artifact 不在 Agent 的默认工具列表里。这不是疏忽，是有意为之。CCB 现在有几十个工具，全部列给模型，每轮请求凭空多出几千 token。大部分工具 99% 的时间根本不碰，没必要次次都亮出来。

做法是把工具分成两类：一直可见的核心工具（Read、Write、Bash 这些），和按需发现的延迟工具。artifact 属于后者——Agent 平时看不见它，直到它想起来"该搜一下了"。

那 Agent 什么时候会想起来？靠的是提示词里的一句引导。artifact 的 description 不是干巴巴写"上传 HTML 文件的工具"，而是直接告诉 Agent 使用时机——"Use this after generating HTML content that you want to share"。就这么一句话，Agent 就知道了：写完 HTML、想分享的时候，先去搜 artifact。搜到了，调 ExecuteExtraTool 执行。多了一步搜索，但对 Agent 来说没认知门槛——跟直接调工具没区别。

关键是，提示词不只告诉了 Agent "有这个工具"，而是告诉了它"什么时候用"。system prompt 里甚至把整个工作流串了起来——Write HTML → Upload → Share Link——Agent 理解 artifact 不是孤立的操作，而是写 HTML 之后理所当然的最后一步。

## 终端里怎么展示上传结果？——OSC 8 的超链接魔法

接下来是客户端的事。上传成功，后端返回一个 URL。这个 URL 要在终端里显示成可点击的链接，不能是一行裸文本让用户手动复制。

ExecuteExtraTool 有个通用委托机制——执行完延迟工具后，把结果和工具名一起传回下游，下游根据工具名匹配对应的渲染逻辑。Artifact 是第一个需要自定义 UI 渲染的延迟工具。

CCB 走的是 React 组件委托——相当于给 artifact 单独写了一个渲染组件。但这里踩了一个并发坑。每轮 API 请求前，CCB 会把消息里的 toolUseResult 字段（工具调用返回结果的原始数据）删掉，防止大文件内存膨胀。问题是删的是原始引用，不是副本。下一轮请求可能抢在 UI 渲染完成前启动，UI 还没来得及读 toolUseResult，值已经被清空了。Artifact 是第一个重度依赖 toolUseResult 渲染的工具，这个 5ms 的并发窗口才第一次暴露出来。修复不复杂——shallow copy，只在传给 API 的副本上剥离字段，原始消息留着给 UI。

Peri 的方案更简单——OSC 8。这是终端内联超链接的 ANSI 转义码，绝大多数现代终端都支持。上传成功后，直接把 URL 包进 OSC 8 序列塞进 tool_result 的输出文本里。终端自己解析渲染成可点击链接——不需要委托层，不需要额外组件，不需要 toolUseResult 字段。那个并发坑直接绕过去了。

双端共用同一个 POST /upload 接口，体验完全一致。差别只在渲染那一层。

| 维度 | CCB (TypeScript + Ink) | Peri (Rust + ratatui) |
|------|----------------------|------------------------|
| 工具实现 | ~180 行 | ~130 行 |
| HTTP 客户端 | ~60 行 | ~110 行 |
| UI 渲染 | React 组件委托 | OSC 8 终端内联链接 |
| 总代码 | ~280 行 | ~200 行 |
| 核心复杂度 | 委托链 + race 修复 | 零 UI 代码 |

## 用 `/use-artifacts` 发布 HTML

Artifacts 工具本身已经能让 Agent 上传 HTML，但普通用户未必知道怎么让 Agent 主动用起来。直接说帮我上传这个 HTML 当然可以——Agent 在提示词的引导下会自己走 SearchExtraTools ExecuteExtraTool 流程。但如果想让 Agent 更主动地判断哪些产出值得上传，需要前置的引导。

`/use-artifacts` 就是一键引导命令。输入回车之后，会话里注入一段提示——告诉 Agent 当前已启用 Artifacts 功能，生成 HTML 看板、报告、原型等内容后应主动上传并分享链接，不需要等用户每轮手动说。这个命令本质上是把 artifact 从被动工具升级为主动行为——Agent 写完 HTML 之后自己判断这个产出适合分享就上传。

用法很简单。`/use-artifacts` 回车，然后正常让 Agent 做事。Agent 会在合适的时机自己调用 artifact 工具，产出就是一个公开链接。

CCB 2.8.0 和 Peri 都内置了这个命令。服务端跑在博客主自己的 Cloudflare 账号下，token 就写在源码里，不需要申请、注册、配任何东西。开箱即用。

Artifacts 是 Agent 世界里一个不大但很扎实的功能——生成 HTML 的最后一步有了着落。两个开源 Agent 用同一套后端，客户端代码加起来不到 500 行。下次你在 Peri 或 CCB 里让 Agent 生成一个看板，发个链接就能打开——Artifacts 把这个缺失的最后一步补上了。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
