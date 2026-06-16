# Peri Code: Windows 上跑 Coding Agent，cmd、PowerShell、Windows Terminal 到底用哪个

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。<https://github.com/KonghaYao/peri>

一个朋友在 Windows 上试用 Peri，反馈说 Agent 执行命令经常没反应。我看到截图——命令是正确的，模型输出也是正确的，但 Shell 就是没执行。我让他换了个终端试试。换了之后，同样的问题消失了。问题出在终端层。

Windows 上跑 Coding Agent，终端不只是个窗口。Agent 生成命令文字，终端负责把它交给 Shell 执行，再把 Shell 的输出还给 Agent。这个传递过程在 Unix 上是透明的——你打开任何一个终端，行为基本一致。在 Windows 上，不同的终端对应不同的传递路径、不同的解析规则、不同的坑。我们先后试了 cmd、PowerShell、Windows Terminal 三种方案，每种都在不同层面出了问题。

## cmd 的元字符解析改写 Agent 传入的命令

最早 Peri 在 Windows 上的 Shell 包装器用的是 `cmd /C`。Unix 上是 `bash -c`，Windows 上自然就套了 `cmd /C`——形式上完全对称，一个 `shell_command()` 函数两个分支，干净利落。

然后 Agent 在 Windows 上调 Bash 工具，执行 `echo hello | grep h`。什么都没发生。不是 Agent 没调工具，不是模型输错了命令。是 `cmd /C` 看到管道符 `|`，就把它当成了自己的语法——把 `echo hello | grep h` 拆成两条独立的命令：`echo hello` 正常执行，`grep h` 被当作另一条命令启动（然后在 Windows 上找不到 grep 而报错）。

这跟 Agent 无关。Agent 生成的命令是给操作系统 Shell 的，但 `cmd` 在 Agent 和命令之间做了一轮语法解析——它在你不注意的时候改写了要执行的指令。更糟的是，`cmd` 会解释的元字符不止 `|`——重定向 `>`、变量 `%VAR%`、甚至一些看起来无害的字符组合，都可能被它当作元字符解析，改变命令原意。

Agent 的工作方式天然不适合 `cmd`。Agent 会生成各种各样的命令，难免包含 Shell 看到之后会有想法的字符。你不能指望 Agent 小心一点写命令——Agent 的职责是生成正确的操作指令，不是猜测你的 Shell 会对指令做什么二次加工。

## PowerShell 单引号透传元字符，PSReadLine 只认 \r\n 作为命令终止符

解决方案是把 Shell 从 `cmd` 换成 PowerShell。PowerShell 的 `-NoProfile -NonInteractive -NoLogo` 三个参数可以干净地关掉用户配置、交互提示和启动横幅，而且它的单引号字符串是字面量的——在单引号内只有 `'` 本身需要转义（加倍为 `''`），其余所有字符——包括 `$`、`|`、`(`、`{`、`;`——全部原样透传。

Agent 的命令终于不用被 Shell 二次解释了。一个转义函数（检测参数是否包含元字符，有则单引号包裹，内部 `'` 加倍）就能安全地把任意命令交给 PowerShell 执行。Unix 上的 `bash -c` 也有类似的转义需求（内部单引号用 `'\''` 退出转义再进入），两边的处理逻辑不同但原则一致——别让 Shell 看到元字符。

但 PowerShell 带来了新问题。Peri 的 Web PTY（伪终端，通过 WebSocket 远程连接）中，用户在浏览器敲 Enter，命令不执行。光标移到下一行，Shell 没反应，看上去像连接断了。

根因是换行符。Unix 上 Enter 就是 `\n`，Shell 认它。Windows 上 xterm.js 发来的 Enter 是 `\r`，自动注入的命令用 `\n`，但 PowerShell 内置的 PSReadLine 命令行编辑模块只认 `\r\n` 作为命令终止符。三种换行符，只有一个是对的。用户敲了 Enter，Shell 拿到的是 `\r`——在 PSReadLine 眼里这不是命令结束，只是一个字符。

修复是在 PTY 写入时归一化——裸 `\r` 和裸 `\n` 都转成 `\r\n`。一行预处理解决了整个输入通道的不确定性，但这件事的本质是：即使你选了正确的 Shell，它的内部组件仍然在用一套和你预期不同的规则处理输入。Unix 上没人需要想换行符有几种写法——`\n` 就是 `\n`。Windows 上你需要主动归一化，因为不同的输入源用不同的换行符，而 Shell 只认其中一种。

## ConPTY 对一次滚轮 tick 同时产生 Key 和 MouseScroll 事件

PowerShell + Web PTY 的组合通过了功能测试——Agent 的命令能正确执行了。但 Windows 上的 TUI（终端用户界面）体验仍然有问题。

Peri 的 TUI 主界面分消息区和输入框。鼠标滚轮滚动消息区是高频操作——阅读 Agent 的长回复时你需要不断向下翻。在 macOS 和 Linux 上，滚轮事件直接分发给消息区滚动函数，一切正常。

在 Windows Terminal 上，滚一下滚轮，输入框的内容先跳了。再滚一下，输入框又跳了。输入框跳不动之后，消息区才开始滚动。

原因出在 Windows Terminal 底层的 ConPTY（伪终端 API，Windows 10 1809 引入）。ConPTY 把终端事件翻译给应用时，对一次滚轮 tick 产生了两个事件——一个 `Key(Up/Down)` 和一个 `MouseScroll`。它们交织出现在事件队列里，`Key` 事件先被处理——因为方向键是最常用的操作，天然分发给输入框——输入框先滚动。然后 `MouseScroll` 到来，消息区再滚动。

ConPTY 为什么要产生这个额外的 `Key` 事件，文档没解释。可能是为了兼容不支持鼠标事件的老应用，可能是内部管道实现的副作用。无论如何，作为应用开发者，你面前的事件队列里混入了你不想要的按键。

修复用了两阶段过滤——读到裸方向键事件后，peek 队列看是否有紧随的 MouseScroll（有则丢弃 Key 返回 Scroll），同时检查近期是否处理过 MouseScroll（有堆积则可能是同一批次的残留 Key）。peek 需要等一小段时间让 ConPTY 把配对的 MouseScroll 传过来——首批次等 10ms（没有近期 MouseScroll 做后盾），后续批次等 3ms。真正的方向键操作没有配对的 MouseScroll 跟随，正常通过，不受影响。

## 三个终端方案都需在应用层消化各自的缺陷

cmd 最直接——Windows 自带，不需要额外安装——但它会在 Agent 的指令到达操作系统之前就改写它。不适合 Agent。

PowerShell 解决了 cmd 的元字符问题——单引号字面量透传，`-NoProfile` 三参数关掉干扰——但它需要你主动归一化换行符。这个问题不难修（一个 20 行的 `normalize_crlf` 函数），但它表明了 Windows Shell 层的一个基本事实——同一个操作在不同来源（键盘、WebSocket、自动注入）到达 Shell 时携带不同的格式化约定，而 Shell 只认其中一种。Unix 上不存在这个问题，因为所有输入源都用同一种约定。

Windows Terminal 提供了最好的渲染体验，但它的伪终端层会主动注入你不想要的事件。ConPTY 在转发事件时插入了输入中原本不存在的内容，应用层只能事后过滤，无法要求它不产生这些事件。

我们现在的推荐是 Windows Terminal + PowerShell——功能和渲染都最好，两个已知问题（换行符、伪造事件）已经在 Peri 内部消化。但这不意味着这是完美的答案——它只是一个你知道了所有坑之后还能接受的权衡。在 Windows 上跑 Agent，你选的不是最好的终端，是坑最少的那个。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
