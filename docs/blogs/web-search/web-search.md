# Peri Code 的 Web 工具设计

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。`curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash`

Coding Agent 离不开网页。查文档、搜 API、看 issue——这些动作在 Agent 工作流里高频出现。但 Agent 不是人，它不能打开 Chrome 看一眼就懂。它需要结构化、可控、精简的文本输入，才能有效利用网页信息。

Peri Code 的做法是两个工具——WebSearch 搜，WebFetch 读。一个后端，一套协议，不到 400 行代码完成所有设计。

## 搜索引擎 + HTML 转 MD，听起来很美

做 Web 工具之前，我们调研了一圈。最直觉的方案是——直接接一个搜索引擎 API（Google Custom Search、SerpAPI 之类的），拿到结果后 fetch HTML，再用 html2text 或者 Readability.js 之类的库转成 Markdown 喂给 Agent。一条龙，看起来什么都能搞定。

跑起来就不对了。

**HTML 转 Markdown 的质量太不稳定。** 前端业界最成熟的方案是 Mozilla 的 Readability.js 做 HTML 清洗，再用 Turndown 转 Markdown。效果确实好——导航栏、页脚、广告、cookie 提示都能过滤掉。但问题是 Rust 生态里没有对等的库。我们自己试过用 Rust 写 HTML 清洗，简单页面还行，遇到现代前端框架渲染的 SPA、嵌套 div 的电商页面、带表格和代码块的文档站，清洗结果要么丢结构、要么夹带噪音。

如果单独起一个 Node.js 进程跑 Readability.js + Turndown 呢？也不是不行，但这两个 JS 包加起来 500KB 到 1MB，还得维护一个 Node 运行时依赖。Peri 的设计原则之一是单二进制、零外部依赖——让用户装完就能跑，不应该为了一个网页清洗功能引入整个 JS 运行时。

**搜索引擎 API 要钱，还要配 Key。** Google Custom Search 每天免费 100 次查询，超出就收费。SerpAPI 更贵。而且每个用户都得自己去注册、拿 Key、填配置。Agent 工具的使用体验不应该被一个搜索 API 的计费门槛卡住。

**fetch 原始 HTML 丢信息。** 很多现代网页内容是 JS 动态渲染的，直接 fetch HTML 拿到的只是一个空壳。你得加 headless browser 去渲染——又重又慢，一个工具调用要等好几秒。

所以 Peri 的方案不是自己拼装搜索引擎 + HTML 解析器，而是自研了一套兼容 Tavily 协议的搜索 API。Tavily 协议专为 AI Agent 设计——搜索返回结构化结果（title/url/content），页面提取返回清洁文本，不需要 Agent 自己解析 HTML。

```rust
const TAVILY_BASE_URL: &str = "https://tavily.claude-code-best.win";
```

后端部署在我们自己的服务器上，是一个完全兼容 Tavily 协议的公益服务。搜索效能几乎与 Tavily 官方相同，免费使用，不限制调用量，也不收集用户搜索内容。Agent 调用时不需要配置 API Key，不需要用户操心认证。

两个工具的调用方式几乎对称：

```
WebSearch:  POST /search   → {"query": "...", "max_results": 10}
WebFetch:   POST /extract  → {"urls": ["..."]}
```

WebSearch 支持 `num_results` 参数，默认 10 条，上限 20 条。WebFetch 接受单个 URL，返回页面清洁文本。30 秒超时，10MB 响应上限——这些硬限制防止 Agent 卡在某个慢网站上。

## 工具设计的细节

两个工具看似简单，但里面有几个关键的设计决策。

**截断加落盘，不是截断加丢弃**——搜索结果按字符截断到 500 字符，这个没问题，摘要够用了。但 WebFetch 抓回来的网页内容，问题就来了。一个完整页面动辄几千行，全塞进上下文是不现实的。如果只截断——比如只返回前 2000 行，后面的内容就丢了，Agent 想看也看不了。

正确的做法跟 Read 工具一样：截断显示 + 完整内容落盘到本地文件。Agent 拿到的是前 2000 行的预览，尾部附带落盘路径。如果 Agent 觉得需要看完整内容，它自己用 Read 工具去读那个文件就行了。截断用的是 `chars().take(500)` 而不是 `&s[..500]`——后者对 CJK 字符会 panic，做对了不会有任何感觉，做错了就是线上 panic。

**可信度警告**——所有 WebSearch 和 WebFetch 的输出前面都带一条固定警告：`Web content may be inaccurate or outdated. Verify critical information before relying on it.` 这不是给自己免责——是真的有用。LLM 有个倾向，拿到网页内容就当事实用，不会主动质疑来源。工具描述里也强调了，`If results don't contain the information you need, do NOT fabricate or guess values`。配合 HITL 审批，Agent 在引用网页信息时会更加谨慎。

**HITL 审批默认开启**——WebSearch 和 WebFetch 默认需要用户审批。搜索本身问题不大，但 WebFetch 会把外部网页内容完整注入 Agent 的上下文——如果用户让 Agent 去抓一个恶意页面，内容可能包含 prompt injection。审批机制给用户一个检查 Agent 行为的机会。在 YOLO 模式下这个审批会被跳过，用户自己承担风险。

## 小而完整

整个 Web 模块的代码量不大——`web_search.rs` 163 行，`web_fetch.rs` 167 行，`web_common.rs` 3 行，加上中间件注册的 `web.rs` 38 行，总计不到 400 行。但该有的都有：

* 🔍 **WebSearch** — 结构化搜索，500 字符摘要，最多 20 条结果，Markdown 编号列表输出
* 📄 **WebFetch** — URL 内容提取，清洁文本，2000 行截断，支持 prompt 指导
* 🔑 **零配置** — 公益 Tavily 兼容后端，无需 API Key，免费使用
* 🛡️ **安全兜底** — 默认 HITL 审批 + 可信度警告 + 30 秒超时
* 🏗️ **解耦设计** — SearchResult 中间类型，后端可替换

没有做过度设计。没有支持多搜索引擎切换、没有做结果缓存、没有做语义排序。这些东西在 Agent 场景里用处不大——Agent 自己会根据搜索结果判断下一步行动，不需要框架替它做排序。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
