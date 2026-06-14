> 归档于 2026-06-14，原路径 spec/issues/2026-06-12-subagent-missing-web-tools.md

# SubAgent 缺少 WebFetch 和 WebSearch 工具

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-12

## 问题描述

子 Agent（Fork/Normal/Background 三种路径均受影响）无法使用 WebFetch 和 WebSearch 工具。例如 `web-researcher` agent 原本设计用于上网搜索资料并写入文件，但因缺少 Web 工具只能使用文件操作和 Bash。用户期望子 Agent 继承父 Agent 所有核心工具，但实际上 Web 类工具未传入子 Agent。

## 症状详情

| 路径 | 工具是否可用 |
|------|------------|
| Read/Write/Edit/Glob/Grep/folder_operations | ✅ 正常 |
| Bash | ✅ 正常 |
| WebFetch | ❌ 缺失 |
| WebSearch | ❌ 缺失 |
| MCP 工具 | ✅ 正常（如有 MCP pool）|
| TodoWrite | ✅ 正常（由子 agent 中间件链提供）|

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 主 Agent 调用 Agent 工具委托子 agent（如 `subagent_type: "web-researcher"`）
  2. 子 Agent 启动后，其工具列表中不包含 WebFetch/WebSearch
  3. 子 Agent 无法完成需要联网的任务
- **环境**：任意

## 涉及文件

- `peri-acp/src/agent/builder.rs:238-251` —— `parent_tools` 构造，仅包含 FilesystemMiddleware + TerminalMiddleware + MCP 工具，未包含 WebMiddleware 的 WebFetch/WebSearch
- `peri-middlewares/src/subagent/tool/build_agent.rs:75-78,129-131` —— `filter_tools()` 过滤后通过 `register_tool` 注入子 agent，WebFetch/WebSearch 因不在 `parent_tools` 中而无法通过
- `peri-middlewares/src/subagent/tool/mod.rs:77-94` —— `build_subagent_middlewares()` 子 agent 中间件链，不包含 WebMiddleware
- `peri-middlewares/src/middleware/web.rs:22-32` —— `WebMiddleware::collect_tools()` 提供 WebFetch/WebSearch，但子 agent 不走此路径

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-12 | — | Open | agent | 创建 |
| 2026-06-12 | Open | Verified | agent | 修复完成，用户验证通过 |

## 修复记录

### 修复 #1（2026-06-12）

- **操作人**：agent
- **用户原意**：子 Agent（Fork/Normal/Background）应有 WebFetch/WebSearch 工具可用
- **修复内容**：
  1. 添加 `WebMiddleware::build_tools()` 静态函数（`web.rs`）
  2. `collect_tools()` 委托给 `build_tools()` 消除重复（`web.rs`）
  3. `agent/builder.rs` 的 `parent_tools` 追加 `WebMiddleware::build_tools()`
  4. `bg.rs` 的 `/bg` 路径 `parent_tools` 同样追加（补充修复）
- **涉及 commit**：`e7eca285`、`883531e9`、`7c808efe`、`0354c70a`
- **验证状态**：已验证

### 验证 #1（2026-06-12）—— 通过

用户确认子 Agent（Fork/Normal/Background 及 `/bg` 命令五条路径）WebFetch/WebSearch 工具均已可用。
