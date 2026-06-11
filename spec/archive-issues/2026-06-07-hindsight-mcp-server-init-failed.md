> 归档于 2026-06-11，原路径 spec/issues/2026-06-07-hindsight-mcp-server-init-failed.md

# 插件 MCP 子进程缺少 CLAUDE_PLUGIN_ROOT/DATA 环境变量注入

**状态**：Fixed
**优先级**：高
**创建日期**：2026-06-07

## 问题描述

插件 MCP server 启动时，`spawn_stdio_transport` 只注入 `.mcp.json` 中的 `env` 字段，不会注入 `CLAUDE_PLUGIN_ROOT` 和 `CLAUDE_PLUGIN_DATA` 环境变量。这导致依赖这些变量的插件 MCP 启动脚本（如 Hindsight 的 `run_mcp.sh`）因无法定位 venv 和 requirements 而在 MCP 握手阶段直接退出。Claude Code 原生会在启动插件 MCP 子进程时注入这些变量，Peri 缺少此行为。

## 症状详情

| 维度 | 表现 |
|------|------|
| 错误日志 | `MCP 连接失败 server=plugin:hindsight-memory:hindsight error=connection closed: initialize response` |
| 出现频率 | 每次启动必现（所有依赖 `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` 的插件 MCP 均受影响） |
| stderr | 无输出——子进程在初始化阶段即退出 |
| 对比 | Context7（`npx` 启动，不依赖插件环境变量）正常工作 |

### 日志证据

```
2026-06-07T01:40:09.926695Z  WARN peri_middlewares::mcp::initialize: MCP 连接失败 server=plugin:hindsight-memory:hindsight error=connection closed: initialize response
```

### 根因分析

1. Hindsight 的 `.mcp.json` 定义了 `"command": "bash", "args": ["${CLAUDE_PLUGIN_ROOT}/scripts/run_mcp.sh"]`
2. Peri 的 `expand_server_config_with_context` 正确展开了 args 字符串中的 `${CLAUDE_PLUGIN_ROOT}` 占位符为实际路径
3. 但 `run_mcp.sh` 内部（L6-L8）**再次使用 `$CLAUDE_PLUGIN_ROOT` 和 `$CLAUDE_PLUGIN_DATA`** 来定位 venv 和 requirements 文件
4. `spawn_stdio_transport`（`client.rs:401-438`）只注入 `config.env`（来自 `.mcp.json` 的 env 字段——Hindsight 没有此字段）
5. **Claude Code 原生行为**：在 `hooks/executor.rs:69-70` 中通过 `.env("CLAUDE_PLUGIN_ROOT", &plugin_root_str)` 注入，Peri 的 MCP 启动路径缺少等效逻辑

### 数据流对比

| 步骤 | Claude Code | Peri |
|------|-------------|------|
| `${CLAUDE_PLUGIN_ROOT}` 展开 | ✅ args 字符串 + 环境变量注入 | ✅ args 字符串展开 |
| `CLAUDE_PLUGIN_ROOT` 作为子进程 env | ✅ `.env()` 注入 | ❌ 缺失 |
| `CLAUDE_PLUGIN_DATA` 作为子进程 env | ✅ `.env()` 注入 | ❌ 缺失 |
| `run_mcp.sh` 内部 `$CLAUDE_PLUGIN_ROOT` | ✅ 可解析 | ❌ 空值 → venv 路径错误 |

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 安装任何使用 `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` 环境变量的插件 MCP（如 Hindsight Memory Plugin）
  2. 启动 Peri TUI
  3. 观察日志中的 MCP 连接失败
- **环境**：macOS，Hindsight v0.7.1，标准 marketplace 安装

## 涉及文件

- `peri-middlewares/src/mcp/client.rs:401-438` — `spawn_stdio_transport`：缺少插件环境变量注入
- `peri-middlewares/src/mcp/initialize.rs:89-94` — MCP 初始化：调用 `spawn_stdio_transport` 时未传递插件上下文信息
- `peri-middlewares/src/mcp/config.rs:284-396` — `load_merged_config_full`：展开了 args 但未在 config.env 中追加插件变量
- `peri-middlewares/src/hooks/executor.rs:69-70` — 参考实现：hooks 路径正确注入了 `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA`

## 修复方向

`spawn_stdio_transport`（或其调用方）需要为 `ConfigSource::Plugin` 来源的 MCP server 自动注入 `CLAUDE_PLUGIN_ROOT` 和 `CLAUDE_PLUGIN_DATA` 环境变量。两种实现路径：

1. **在 `load_merged_config_full` Step 2 展开时追加到 config.env**：展开完成后将 `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` 写入 `McpServerConfig.env`，这样 `spawn_stdio_transport` 已有的 `cmd.envs(env)` 就能自动注入
2. **在 `spawn_stdio_transport` 或 `initialize.rs` 中检测 Plugin source 并注入**：需要传递 source 信息到 spawn 层

方案 1 更简单，不需要改 `spawn_stdio_transport` 的签名。

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-07 | — | Open | agent | 创建 |
| 2026-06-07 | Open | Fixed | agent | 修复：在 load_merged_config_full Step 2 注入 CLAUDE_PLUGIN_ROOT/DATA |

## 修复记录

### 修复 #1（2026-06-07）
- **操作人**：agent
- **用户原意**：插件 MCP 子进程需要 CLAUDE_PLUGIN_ROOT/DATA 环境变量才能正常启动，如 Hindsight 的 run_mcp.sh
- **修复内容**：在 `load_merged_config_full` Step 2 展开插件 MCP config 后，将 `CLAUDE_PLUGIN_ROOT`/`CLAUDE_PLUGIN_DATA` 注入到 `McpServerConfig.env` 字段
- **涉及 commit**：`21b1bdb1` + `97573705`
- **验证状态**：待验证
