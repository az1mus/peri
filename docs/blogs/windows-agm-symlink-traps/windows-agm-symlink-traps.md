# Peri Code: Windows 上做包管理器，第一天就撞上了 20 年前的权限设计

> **[Peri Code](https://github.com/konghayao/peri)** — 用 Rust 写的开源 Coding Agent，兼容 Claude Code 生态。<https://github.com/KonghaYao/peri>

AGM（Agent Package Manager）的安装模型是从 pnpm（Node.js 生态的包管理器，用符号链接实现高效安装，避免重复下载）搬过来的——Skills 和 Agents 下载到全局 store 目录，在项目里用符号链接指向 store。不重复下载，不拷贝文件，磁盘占用最小。

在 Unix 上，`std::os::unix::fs::symlink()` 一行代码，零权限要求，任何人都能创建。在 Windows 上，`std::os::windows::fs::symlink_dir()` 第一行调下去就返回了一个错误码——1314。`ERROR_PRIVILEGE_NOT_HELD`。Windows 不让你创建符号链接。

## 用 symlink 而非拷贝实现安装

用户执行 `agm install some-skill`，AGM 从 registry 下载包到 `~/.agm/store/`，然后在项目的 `.agm/skills/` 下创建一个符号链接指向 store 里的包。

用 symlink 而不是直接拷贝——同一个 skill 被多个项目引用时，只有一份磁盘占用。store 里的包更新后，所有链接自动指向新版本。卸载时只需要删除链接，store 和下载缓存不受影响。

`ToolAdapter` trait 里的 `install()` 方法封装了这套逻辑——先在 store 找到包的路径，在项目目录下创建目标文件夹，然后创建符号链接。Unix 上 symlink() 是无特权的系统调用，创建符号链接不需要任何权限配置。Windows 上的 `install()` 一开始也有实现——`symlink_dir` 和 `symlink_file`，和 Unix 的 `symlink` 一起放在平台分支里。代码编译通过，直到第一个 Windows 用户报告安装失败。

## SeCreateSymbolicLinkPrivilege 权限的默认限制

Windows 从 Vista 开始支持符号链接。NTFS（Windows 的默认文件系统）一直有能力做这个，但直到 Vista 才在用户态暴露 API。设计时的安全考量是——符号链接可以绕过目录访问控制，一个低权限用户创建的链接可能指向高权限目录，被其他程序访问时触发权限提升。

微软在 Vista 中加了限制——创建符号链接需要 `SeCreateSymbolicLinkPrivilege` 权限。默认情况下，只有 Administrators 组成员持有这个权限。普通用户即使以管理员身份运行程序，也不一定有这个权限——UAC（用户账户控制，Windows 的权限提升机制）提升后的管理员 token 会剥离部分特权，`SeCreateSymbolicLinkPrivilege` 是否保留取决于 Windows 版本和组策略。

Windows 10 Creators Update 之后新增了一个替代方案——开启开发者模式。在设置中启用后，任何用户都可以创建符号链接，不再需要管理员权限或特权。但大部分 Windows 用户的开发者模式处于关闭状态——这个开关藏得深，很多人不知道它存在。除非已开启开发者模式或使用管理员账号，否则第一次在 Windows 上运行 agm install 大概率直接遇到 1314 错误。

## 捕获 1314 错误码，复制回退加一次性提示

AGM 的修复用一个统一的 `try_symlink` 函数替代双平台分支。函数内部仍然按平台分发，但返回值是 `std::io::Result<()>`，让调用方统一处理错误。匹配到 `raw_os_error() == Some(1314)` 时——这个错误码是 Windows 特有的 `ERROR_PRIVILEGE_NOT_HELD`，标准库没有提供枚举变体，只能通过原始错误码判断——不向上传播错误，进入回退逻辑——目录用 `copy_dir_all` 递归复制，文件用 `std::fs::copy` 直接拷贝。

同时在第一次命中 1314 时打印一条一次性警告（用 `AtomicBool` 保证整个进程生命周期内只打印一次），告诉用户可以开启开发者模式来获得更快的 symlink 安装——让用户在知道有替代方案的同时，现在就能继续工作。

`copy_dir_all` 递归遍历源目录，对每个条目判断类型——目录继续递归、文件直接 copy、符号链接跟随 target。递归深度上限设了 20 层——AGM 的包树最多三四层，20 层足够不会误触发，但能防止符号链接循环导致的栈溢出。遇到悬浮的符号链接（target 不存在）跳过并打印 warning——这种场景在正常的包安装中不会出现，但在用户手动篡改 store 目录后可能发生。

复制的语义和 symlink 有一个关键区别——复制是一次性快照，不会自动跟随 store 更新。用户如果更新了 store 里的包，已安装到项目里的副本需要重新运行 `agm install` 来刷新。这是一个明确的权衡——symlink 是理想的方案，copy 是可工作的回退。降低的体验差异只影响重复安装的速度（symlink 是元数据操作，copy 是磁盘 IO），首次安装的体验是一致的。

## 符号链接权限的二十年安全包袱

Windows 上不能随意创建符号链接，是 2007 年 Vista 的安全策略决定。NTFS 和 syscall（系统调用，程序向操作系统内核请求服务的方式）的能力当时已经到位——正是因为能力完备，微软才加了权限锁来防止符号链接绕过目录访问控制。这个决定在 2026 年还在影响开发者工具的安装流程。

包管理器是开发者工具的核心组件。开发者每天用 npm、pnpm、pip、cargo 安装依赖，所有这些工具在 Windows 上的应对各有不同——npm 和 pnpm 在 `fs.symlink()` 失败时自动回退到 junction（目录联结，不需要特权）或直接复制，cargo 在 Windows 上默认使用拷贝而非硬链接（多个文件名直接指向同一份磁盘数据，和符号链接的路径跳转是两种机制），也是出于同样的原因。

AGM 在这个问题上选择了回退加一次性提示——让用户知道有更好的方案（开启开发者模式），同时不让权限问题阻塞安装流程。在 Windows 上写开发者工具，需要为用户的权限情况准备回退逻辑——没人有义务为了用一个 CLI 工具去啃 2007 年的安全白皮书。

项目地址：[github.com/konghayao/peri](https://github.com/konghayao/peri)
