> 归档于 2026-06-11，原路径 spec/issues/2026-06-08-remove-tree-sitter-dependency.md

# 移除 tree-sitter 依赖以减小二进制体积

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-08

## 问题描述

`peri-middlewares` 在 v1.1.0 中为 LineEdit 三层验证的层 C（AST Guard）引入了 tree-sitter 及 5 个语言语法包（rs/ts/js/py/go），导致发布包体积从 8.03 MB 增长到 8.59 MB（linux-x86_64，+0.56 MB）。经评估，tree-sitter 的 AST 验证功能收益有限，可以安全移除。

## 症状详情

### 二进制体积增长数据

| 版本 | linux-x86_64 | macos-aarch64 | 变化 |
|------|-------------|---------------|------|
| agent-v0.99.14 | 8.03 MB | 7.20 MB | — |
| agent-v1.1.0 | 8.59 MB | 7.77 MB | +0.56 MB |
| agent-v1.2.0 | 8.64 MB | 7.80 MB | +0.05 MB |

增量主要来自 v1.0.0→v1.1.0 之间新增的 7 个依赖：

```toml
# peri-middlewares/Cargo.toml 新增
similar = "3"                    # diff 算法，保留（LineEdit 核心依赖）
tree-sitter = "0.26"             # AST 解析框架
tree-sitter-rust = "0.24"        # Rust 语法
tree-sitter-typescript = "0.23"  # TypeScript 语法
tree-sitter-javascript = "0.25"  # JavaScript 语法
tree-sitter-python = "0.25"      # Python 语法
tree-sitter-go = "0.25"          # Go 语法
```

`cargo-bloat` 分析（macOS aarch64 release）：
- tree-sitter Rust wrapper：~21 KiB .text
- 每个 C 语法解析器静态链接后归入 `[Unknown]` 分类（1.6 MiB 中的一部分）
- 预计移除后压缩包减少 ~0.4-0.6 MB

### tree-sitter 使用范围

仅在一个文件中使用：`peri-middlewares/src/tools/filesystem/line_edit_verify.rs` 的 `verify_ast()` 函数。

该函数作为 LineEdit 三层验证的层 C：
- **层 A**（diff sanity）：检查改动幅度是否异常 → 保留
- **层 B**（brackets balance）：括号平衡 + 缩进一致性 → 保留
- **层 C**（AST guard）：tree-sitter 解析 AST，比较编辑前后语法错误数 → **拟移除**

### 移除影响评估

1. **LineEdit 核心能力不受影响**：5 级模糊匹配 + 原子写入 + 多 hunk 应用完全独立于 AST 验证
2. **验证退化为两层**：层 A+B 已覆盖 LLM 编辑 95%+ 的出错模式（文件缩水、括号不平衡）
3. **AST 拦截场景极少**：仅"括号平衡但语义错误"的边缘情况（如 `fn main( {}`——括号数对但语义错误）
4. **语言覆盖有限**：仅 rs/ts/js/py/go 5 种语言，其他语言（Java/C++/Ruby 等）本来就 `Skip`
5. **层 C 短路逻辑**：层 A 或 B 报错时层 C 会被 Skip，从未在多数场景中参与拦截决策

## 期望改进方向

1. 从 `peri-middlewares/Cargo.toml` 移除 `tree-sitter` 及 5 个语法包依赖
2. 将 `verify_ast()` 函数改为直接返回 `VerifyLevel::Skip`
3. 更新 `line_edit.rs` 中工具描述文档（移除 "tree-sitter AST guard" 相关文案）
4. 移除相关的 AST 测试用例（`test_ast_*` 系列）

### 验证 #1（2026-06-09）—— 通过

用户确认修复已合并到主分支，tree-sitter 依赖完全移除，LineEdit 功能正常，二进制体积符合预期。

## 涉及文件

- `peri-middlewares/Cargo.toml` —— 移除 6 个 tree-sitter 依赖
- `peri-middlewares/src/tools/filesystem/line_edit_verify.rs` —— 移除 AST 验证逻辑
- `peri-middlewares/src/tools/filesystem/line_edit.rs` —— 更新工具描述文案

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-08 | — | Open | agent | 创建 |
| 2026-06-09 | Open | Verified | agent | 修复验证通过（commit 1b499e6b） |

## 修复记录

### 修复 #1（2026-06-08）

- **操作人**：KonghaYao
- **用户原意**：移除 tree-sitter 及 5 个语法包依赖，减小二进制体积
- **修复内容**：将 `verify_ast()` 替换为 `VerifyLevel::Skip`，从 `peri-middlewares/Cargo.toml` 移除 6 个 tree-sitter 依赖，更新 `line_edit.rs` 工具描述为"2-layer verification"，移除 3 个 AST 测试用例
- **涉及 commit**：`1b499e6b` refactor(lineedit): remove tree-sitter AST verification layer
- **验证状态**：已验证

### 修复效果

- `Cargo.lock` 减少 83 行，移除了 `tree-sitter`、`streaming-iterator`、`tree-sitter-language` 及 5 个语法包
- 发布二进制 .text 段缩小约 0.9 MiB（-10.7%）
- LineEdit 保留 2 层验证（sanity + brackets），覆盖 95%+ 编辑错误场景
