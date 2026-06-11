> 归档于 2026-06-11，原路径 spec/issues/2026-06-07-full-compact-loses-project-path-context.md

# Full Compact 后 Agent 使用错误的项目路径前缀

**状态**：Fixed
**优先级**：中
**创建日期**：2026-06-07

## 问题描述

在单项目 session 中，Full compact（context_usage > 85%）后，agent 尝试 Read 文件时使用了错误的项目路径前缀。例如，session 一直在项目 A 上工作，compact 后 agent 把文件路径拼成了项目 B 的路径前缀，导致 Read 失败。这表明 Full compact 的摘要和 re_inject 机制未能有效保留 agent 对当前项目路径的感知。

## 症状详情

| 阶段 | Agent 行为 | 结果 |
|------|-----------|------|
| Compact 前 | 正确使用绝对路径如 `/Users/xxx/project-a/src/foo.rs` | Read 成功 |
| Full compact 触发 | 9 段摘要模板压缩历史 + re_inject 注入最近读取文件 | — |
| Compact 后 | 使用了错误的项目路径前缀如 `/Users/xxx/project-b/src/foo.rs` | Read 失败 |

**关键观察**：
- 问题发生在**单项目 session** 中（不是跨项目操作）
- 是 **Full compact** 触发后出现，Micro compact 无此问题
- Agent 记住了文件名和目录层级，但拼错了项目根路径前缀

## 根因分析

**根因**：`full.rs:82-90` 的 `preprocess_messages` 函数在处理 Ai 消息时，只保留了工具调用名称（`Read`, `Edit` 等），**完全丢弃了工具调用参数**（包含 `file_path`, `path`, `command` 等关键路径信息）。

修复前的输出格式：
```
[助手] Let me read the file（调用了工具: Read, Edit）
```

摘要 LLM 只看到工具名称，无法知道 Read 的是哪个文件、Edit 的是哪一行。生成 "Files and Code Sections" 段落时只能从 assistant 文本和工具结果猜测路径，导致路径不精确。

## 复现条件

- **复现频率**：偶发，Full compact 后出现
- **触发步骤**：
  1. 在某个项目上长时间工作
  2. context_usage 超过 85% 触发 Full compact
  3. Compact 后 agent 继续工作，尝试 Read 文件
  4. 路径前缀错误，Read 失败
- **环境**：所有模型均可复现

## 涉及文件

- `peri-agent/src/agent/compact/full.rs` —— `preprocess_messages` 中 Ai 消息处理丢弃了 `tool_calls[].arguments`
- `peri-agent/src/agent/compact/re_inject.rs` —— `extract_recent_files()` 正确从 `arguments` 提取路径（说明数据存在，只是 `preprocess_messages` 没用）

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-07 | — | Open | agent | 创建 |
| 2026-06-07 | Open | Fixed | agent | 修复 preprocess_messages 保留工具参数 |
| 2026-06-07 | Fixed | Pending | agent | 等待用户验证 |
| 2026-06-10 | Pending | Reopen | agent | 同类问题复现 |
| 2026-06-10 | Reopen | Fixed | agent | 修复 #2：cwd 注入 + 提示词英文化 + file_path 参数兼容 |

## 修复记录

### 修复 #1（2026-06-07）

- **操作人**：agent
- **用户原意**：compact 后 agent 不应丢失文件路径上下文
- **修复内容**：在 `preprocess_messages` 中新增 `format_tool_call_summary` 函数，提取工具调用中的路径相关参数（`file_path`, `path`, `folder_path`, `command`, `pattern`），格式化为 `ToolName(field="value")` 形式纳入摘要 LLM 的输入
- **涉及文件**：`peri-agent/src/agent/compact/full.rs`（+30 行），`peri-agent/src/agent/compact/full_test.rs`（+40 行）
- **验证状态**：验证失败——同类问题复现

### 修复 #2（2026-06-10）

- **操作人**：agent
- **用户原意**：compact 后 agent 不应丢失文件路径上下文
- **修复内容**：
  1. `full_compact()` 增加 `cwd` 参数，在摘要 LLM prompt 中注入 `Current working directory: {cwd}`，为摘要 LLM 提供路径锚点
  2. SYSTEM_PROMPT / USER_PROMPT_TEMPLATE 改为英文，对齐 Claude Code 风格
  3. `re_inject.rs` 的 `extract_recent_files()` 和 `extract_skills_paths()` 同时检查 `file_path` 和 `path` 两种参数名，兼容不同 LLM provider 的参数风格
  4. 更新两条调用路径（auto-compact + manual `/compact`）传入 cwd
- **涉及文件**：`full.rs`、`re_inject.rs`、`full_test.rs`、`re_inject_test.rs`、`compact_middleware.rs`、`compact.rs`
- **验证状态**：待验证
