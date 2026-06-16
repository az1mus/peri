# Peri Code: 一个透明背景色，让 Windows 终端表单出现重影

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。<https://github.com/KonghaYao/peri>

Peri 的 Setup Wizard 有一个很普通的功能——用户在表单里编辑 Provider ID、Base URL、API Key。实现方式很自然——底层用 ratatui（Rust TUI 库）的 Paragraph 渲染静态文字（标签和值的文本），上层用 tui_textarea（Rust TUI 文本输入组件）的 TextArea 组件覆盖在编辑区上方让用户输入。在 macOS 和 Linux 上，用户看到的是干净的编辑体验——聚焦一个字段，光标出现，旧的文本消失，只有 TextArea 的内容在正确的位置闪烁。

在 Windows 上，用户看到的是两行字叠在一起。底层的静态文字还在原位，上层的 TextArea 在旁边偏了两列，两个文字不同步但都清晰可见。输入的内容是对的，但旧文字和新文字同时显示，而且不在同一个位置。

## 默认背景色在 Windows 终端上不等于透明

静态层画好标签和值之后，动态层在同样的屏幕坐标上渲染 TextArea。TextArea 用 `Color::Reset` 作为背景色——等于告诉终端用默认背景。在 macOS/Linux 终端上，`Color::Reset` 等于透明，底下的文字被覆盖后不会显示。

在 Windows Terminal 上，`Color::Reset` 的行为不一样。它仍然是不设背景，但终端的默认背景色不保证会完全遮挡之前渲染的内容块。当 TextArea 的内容因为光标位置触发水平滚动（`top_col > 0`）时，文字起始列偏离了底层静态文字的位置——TextArea 的文字从第 5 列开始显示，底层的静态文字从第 3 列开始，两个文字不在同一列上，但因为背景透明，两个文字都能看到。

修复方法直接——把 TextArea 的背景色从 `Color::Reset` 改成不透明的纯黑（`#000000`）。Color::Reset 在两个平台上的行为不一致——不意识到这一点，就不会把它当 bug 看。

## CJK 标签的显示列宽不等于字符数

背景修好之后，overlay 的文字位置仍然不对。这次是 X 坐标算错了。

表单渲染时，标签列的宽度是硬编码的——`format!("{:<14}", label)`，按 14 个字符填充。这个假设在纯英文环境下成立（1 个字符 = 1 个显示列），但在 CJK 环境下会出问题。中文标签如原始地址只有 5 个字符，但占 10 个显示列——ratatui 的渲染层是按显示列（unicode-width）计算位置的，而 `format!` 的 `:<14` 是按字符数填充的。

标签的实际显示列宽度不等于字符数，但 overlay 的 X 坐标用的是字符数计算出来的列号。结果就是 overlay TextArea 渲染在比底层标签文字偏左的位置，和底层的值文字不完全重叠。

修复是动态计算——在渲染前遍历所有标签，用 `unicode_width::UnicodeWidthStr::width()` 取每个标签的实际显示列宽度，取最大值作为列宽，用这个宽度来填充和对齐。Config Panel、Login Panel、Setup Wizard 三处都需要同样的修复，因为三处的表单渲染代码各自独立演化，用了相似的硬编码逻辑。

## Login Panel 活跃字段改用 overlay textarea 渲染

Config Panel 和 Setup Wizard 在字段聚焦时会把静态值设为空字符串，让 TextArea 独占显示编辑内容。但 Login Panel 的旧实现不一样——它在聚焦时用内联 hack 追加一个 `█` 光标字符——`format!("{}█", value)`。活跃字段的静态值和 TextArea 内容同时渲染在同一个位置，靠人工拼接来模拟编辑状态。这个 hack 在修复 overlay 重影后暴露了出来——TextArea 的背景变成不透明纯黑后，内联的 `█` 光标仍然可见，因为它渲染在静态 Paragraph 中，不归 TextArea 管。修复是把 Login Panel 统一到和其他两个表单一样的方案——活跃字段向静态层传空字符串，TextArea 通过 overlay 在正确的位置渲染。这同时也解决了旧方案中 `█` 光标位置和 tui_textarea 内置光标不一致的问题。

## 事件 stash 延迟消费导致光标跳两格

在调整事件处理代码的过程中，主输入框里按左箭头，光标有时不动、有时跳两格。这个 bug 跨平台（macOS/Windows/Linux 都出），与字符宽度无关（中英文都出），但恰好在这次 Windows 兼容改造中被暴露了出来。

根因是 `next_event` 函数里的 `EVENT_STASH`——一个 thread-local 的单槽事件缓冲区。这个 stash 是本次改造中引入的，用于暂存 `coalesce_mouse_events` 期间遇到的非滚轮事件。但 stash 的消费点（`take`）放在了内层循环的开头，晚于一个关键路径——`poll(50ms)` 超时返回 `Ok(None)`。

用户在消息区滚动鼠标（产生 MouseScroll）的同时按了左箭头（产生 Key(Left)）。`next_event` 读到 MouseScroll，`coalesce_mouse_events` drain 读到 Key(Left)，stash 它，返回 MouseScroll——消息区正常滚动。下一个 `next_event` 调用，队列为空，`poll(50ms)` 超时，`return Ok(None)`——stash 里的 Key(Left) 没有消费。用户看到光标没反应，又按了一次左箭头。再下一个 `next_event`，`poll` 命中，stash take（旧的 Key(Left)），光标左移。再再下一个 `next_event`，`read` 读到新的 Key(Left)，光标再次左移。按一次，光标跳了两格。任何带有鼠标交互的场景都可能触发 MouseScroll 与按键的事件交织，这在三个平台上都会发生。

修复是把 stash take 从内层循环移到 `next_event` 函数的最开头，在所有 early return（包括 poll 超时）之前。stash 有事件时直接用它，跳过 poll；stash 为空时走原来的逻辑。这确保 stash 里的事件永远不会被 poll 超时路径跳过。

## 渲染层跨平台的三种失效模式

这三个 bug（背景透明穿透、CJK 宽度错位、事件 stash 泄漏）分属不同层面，但共享同一个模式——代码在 Unix 上写、在 macOS 上测试、在 Windows 上出问题。背景透明的问题如果在 Windows 上开发、在 macOS 上测试，可能根本不会发生——你会默认用不透明背景，因为在 Windows 上透明不可靠。CJK 宽度如果在测试环境有中文字段，第一天就能发现 14 字符不等于 14 列。事件 stash 的设计是跨平台的，但 bug 在 Windows 上更容易触发——Windows 上的 ConPTY（控制台伪终端层，负责把终端事件翻译给应用）会伪造事件、事件队列时序也不同。

写跨平台终端 UI 的比较可靠的方式——测试用例必须覆盖三个维度——不同语言（CJK vs ASCII）、不同交互设备（纯键盘 vs 鼠标与键盘混用）、不同终端实现（操作系统自带 vs 第三方）。任何一个维度缺失，都有可能在那个组合上产生只在此处出现的 bug。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
