# Peri 跑在 RISC-V 上了

Claude Code 没有 RISC-V 的版本。Bun 也没有。

Peri 有。

你在 GitHub Release 页面能找到 `peri-linux-riscv64.tar.gz`，跟 x86_64、aarch64、macOS、Windows 的包放在一起。安装脚本自动识别架构，`uname -m` 返回 `riscv64` 就直接拉对应包。

```bash
curl -fsSL https://raw.githubusercontent.com/konghayao/peri/main/scripts/install.sh | bash
```

我在昉星光的板子上跑的，8G 内存，装完直接 `peri` 回车进 TUI。

## 跑起来什么样

内存——稳定在 70MB。跑了很久，不涨。对比 Claude Code 随随便便好一个多 G 内存，70MB 在 8G 的 RISC-V 板子上完全不叫事。

TUI 功能全在。鼠标滚动翻聊天记录、浏览代码输出，跟在 x86 上没区别。`crossterm` + `ratatui` 纯 Rust 实现，架构无关。流式输出也正常，模型吐字终端立刻显示，延迟瓶颈在服务端不在本地。

## 为什么别人没有

Claude Code 没有 RISC-V 版本——Node.js 生态的二进制依赖（特别是原生模块）在 RISC-V 上编译不过去，上游也没动力支持。Bun 同样，目前没有 `linux-riscv64` 的构建产物。

Peri 能做到，说到底是因为 Rust 的交叉编译工具链足够成熟。`rustup target add riscv64gc-unknown-linux-gnu` 一行命令就能准备好编译环境，项目里没有内联汇编、没有 SIMD、没有架构专属的原生代码。剩下几个 C 依赖——`libsqlite3-sys`（纯 C89 源码编译）和 `crossterm`/`ratatui`（纯 Rust）——对 CPU 架构无感知。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
