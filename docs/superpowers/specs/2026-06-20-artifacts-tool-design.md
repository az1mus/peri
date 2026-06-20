# Artifacts 工具设计规格

> 让模型把生成的 HTML 上传到公开 URL，7 天后自动消失。服务端复用 CCB，CLI 端作为延迟工具 + `/artifacts` 命令。

## 概述

Artifacts 是一个"小而完整"的功能，横跨：云端服务（复用 CCB）、CLI 工具注册（deferred tool）、TUI 渲染（OSC 8 可点击链接）、会话级状态（`/artifacts` 列表）。三个层面全部在本项目中实现。

**服务端**：复用 CCB 已有的 Cloudflare Worker + R2 服务（`https://cloud-artifacts.claude-code-best.win`），默认 token 内置，环境变量可覆盖自托管。

## 架构

```
模型 Write(HTML 文件)
  → SearchExtraTools("select:artifact")     ← 发现
  → ExecuteExtraTool("artifact", {file_path, ttl})  ← 执行
       │
       ├─ ArtifactTool::invoke()
       │    ├─ 读取本地文件
       │    ├─ HTTP POST → CCB Server
       │    └─ 返回 "{id, url, expiresAt}"
       │
       ▼
  ToolEnd.output (含 OSC 8 escape 的格式化文本)
       │
       ▼
  TUI ToolBlock 渲染 → 终端自动解析 OSC 8 可点击链接
       │
  /artifacts 命令 → ACP Immediate 扫描消息历史 → ArtifactsPanel ↑/↓/Enter/Esc
```

## 组件设计

### 1. ArtifactTool（延迟工具）

**位置**: `peri-middlewares/src/tool_search/tools/artifact_tool.rs`

**类型**: Deferred tool，不在 CORE_TOOLS 中，通过 `SearchExtraTools` → `ExecuteExtraTool` 两步调用。

**参数**:
```json
{
  "file_path": {
    "type": "string",
    "description": "要上传的 HTML 文件路径（相对或绝对）"
  },
  "ttl": {
    "type": "string",
    "enum": ["7d", "30d"],
    "description": "过期时间，默认 7d"
  }
}
```

**invoke() 流程**:
1. 解析 `file_path`（支持相对路径，基于 cwd）
2. 校验文件存在、类型为 HTML、大小（上限 10MB）
3. 调用 `ArtifactClient::upload(file_path, ttl)`
4. 成功：返回带 OSC 8 超链接的格式化文本
5. 失败：返回 `{error: "..."}` 供 LLM 消费

**输出格式**（成功时）:
```
Artifact uploaded: https://cloud-artifacts.claude-code-best.win/7d/abc123.html
Expires: 2026-06-27T12:00:00Z
```

其中 URL 段包裹 OSC 8 escape sequence（通过 `LinkSpan::wrap_osc8()`），终端自动渲染为可点击链接。

**注册**: 在 `ToolSearchMiddleware::collect_tools()` 中实例化并返回。

### 2. ArtifactClient（HTTP 客户端）

**位置**: `peri-middlewares/src/tool_search/tools/artifact_client.rs`

```
POST {ARTIFACTS_URL}/upload
Authorization: Bearer {ARTIFACTS_TOKEN}
Content-Type: text/html

Body: <file_content>
```

**配置**（环境变量）:
| 变量 | 默认值 | 说明 |
|------|--------|------|
| `PERI_ARTIFACTS_URL` | `https://cloud-artifacts.claude-code-best.win` | 上传服务地址 |
| `PERI_ARTIFACTS_TOKEN` | `claude-code-best` | Bearer token |

**错误处理**:
- CCB 的 Deno Deploy 边缘代理会**抹平 HTTP status code 为 200**，必须解析 response body 中的 `{error: "..."}` 字段判断真实状态
- 错误类型：`payload_too_large`、`unauthorized`、`invalid_content_type`、上传失败
- 错误信息直接透传给 LLM

**ID 格式**: CCB 服务端使用 `nanoid(21)` 生成 ID，不检查碰撞。

### 3. TUI 工具结果渲染

**方案**: OSC 8 escape 嵌入输出文本（方案 2A）。

ArtifactTool 的输出字符串直接包含 OSC 8 escape sequence，流经正常的 ToolBlock → ToolCallState → Paragraph 渲染管道。终端接收到 OSC 8 序列后自动将 URL 渲染为可点击链接。

**关键函数**: `wrap_osc8(text, url)` 已实现在 `peri-widgets/src/link.rs:85-92`。

**不做的事情**:
- 不修改 `message_render.rs` 的 ToolBlock 渲染逻辑
- 不修改 `ToolCallState` 结构
- 不添加 ExecuteExtraTool 的委托渲染机制

**局限性**（V1 接受）:
- 链接样式由终端决定，无法自定义颜色（但 LinkSpan 支持 `.style()` 自定义样式，后续可升级）
- URL 不能截断显示（`max_width` 不可用，因为 OSC 8 escape 字节会计入长度计算）
- 不支持复制快捷键（依赖终端自带的右键菜单）

**后续升级路径**（V2）:
- 方案 2B：在 ToolBlock 渲染时检测 artifact 工具结果，用 `LinkSpan` 构建专用 Span，支持颜色和截断。

### 4. `/artifacts` 命令

**ACP Immediate 命令**（`peri-acp/src/session/command/artifacts.rs`）:

```
CommandKind::Immediate

execute():
  1. 遍历 session.history 中的所有消息
  2. 找 assistant 消息中的 tool_use block（name == "artifact"）
  3. 用 tool_use_id 配对 user 消息中的 tool_result block
  4. 从 tool_result 字符串中 regex 提取 url, id, expiresAt
  5. 返回 Vec<ArtifactEntry { id, url, expiresAt }>
```

**为什么解析字符串而非结构化数据**:
- 与 CCB 设计一致——resumed session（从 SQLite 反序列化）中 tool_result 的内容是字符串，结构化数据不一定存在
- Regex pattern:
  ```
  URL:  /https?:\/\/[^\s)"',]+\.html\b/
  ID:   /id:\s*([A-Za-z0-9_-]+)/        （当前未使用，预留）
  DATE: /expires:\s*([0-9T:.Z+-]+)/     （当前未使用，预留）
  ```

**TUI ArtifactsPanel**（`peri-tui/src/command/panel/artifacts.rs`）:

- 实现 `PanelComponent` trait
- 列表渲染：每行显示 URL + 过期时间
- 快捷键：
  - `↑`/`↓`/`j`/`k`：选择
  - `Enter`：`open::that(url)` 浏览器打开
  - `c`：复制 URL 到剪贴板（`arboard` crate）
  - `Esc`：关闭面板
- 空列表时显示 "本会话暂无 artifacts"

**命令调度**（两端协作）:
- ACP 层：`/artifacts` 注册为 `CommandKind::Immediate` 命令。execute 时遍历 session history 提取 artifact 列表，通过事件（如 `ArtifactsList`）回传数据。
- TUI 层：`ArtifactsPanel` 在构造时接收 `Vec<ArtifactEntry>` 数据。面板渲染列表并提供 ↑/↓/Enter/c/Esc 交互。
- V1 简化路径（推荐先走）：TUI 命令直接扫描 `pipeline.completed` 中已有的 `Vec<BaseMessage>`——消息数据已在 TUI 端，无需回 ACP 再传一轮。待后续有跨 session 或远程查询需求时再启用 ACP Immediate 路径。

## 数据流

### 上传流程

```
1. LLM 调用 Write 生成 HTML 文件
2. LLM 调用 SearchExtraTools("select:artifact") → 获取 artifact 工具 schema
3. LLM 调用 ExecuteExtraTool("artifact", {file_path: "dashboard.html", ttl: "7d"})
4. ExecuteExtraTool::invoke() → shared_tools 查找 → ArtifactTool::invoke()
5. ArtifactTool::invoke():
   a. 解析 file_path（基于 cwd 拼接绝对路径）
   b. 读取文件内容
   c. POST → CCB server
   d. 返回格式化文本（含 OSC 8）
6. ToolEnd 事件携带 output 文本
7. TUI 渲染 ToolBlock，终端显示可点击链接
```

### `/artifacts` 列表流程

```
1. 用户输入 /artifacts
2. TUI CommandRegistry 拦截
3. 通过 ACP 通道调用 AgentCommand::execute(artifacts)
4. artifacts 命令扫描 messages:
   - 找 assistant.tool_use[].name == "artifact" 的消息
   - 找对应 user.tool_result[].content 的消息
   - regex 提取 url
5. 返回 Vec<ArtifactEntry>
6. TUI 打开 ArtifactsPanel，渲染列表
```

## 文件清单

| Crate | 文件 | 职责 | 操作 |
|-------|------|------|------|
| `peri-middlewares` | `src/tool_search/tools/artifact_tool.rs` | ArtifactTool 定义 + invoke | **新建** |
| `peri-middlewares` | `src/tool_search/tools/artifact_client.rs` | HTTP client + 错误处理 | **新建** |
| `peri-middlewares` | `src/tool_search/tools/mod.rs` | 模块声明 | **修改** |
| `peri-middlewares` | `src/tool_search/middleware.rs` | collect_tools 注册 artifact | **修改** |
| `peri-acp` | `src/session/command/artifacts.rs` | `/artifacts` Immediate 命令 + 消息扫描 | **新建** |
| `peri-acp` | `src/session/command/mod.rs` | 注册 artifacts 命令 | **修改** |
| `peri-tui` | `src/command/panel/artifacts.rs` | ArtifactsPanel 组件 | **新建** |
| `peri-tui` | `src/command/mod.rs` | 注册面板命令 | **修改** |
| `peri-tui` | `src/app/tool_display.rs` | artifact 工具 display 简称 | **修改** |

## 代码量预估

| 组件 | 预估行数 |
|------|----------|
| ArtifactTool + ArtifactClient | ~150 行 |
| 错误处理 + 测试 | ~80 行 |
| `/artifacts` ACP 命令（扫描 + regex） | ~100 行 |
| ArtifactsPanel（PanelComponent impl） | ~120 行 |
| 注册/模块声明/display 改动 | ~20 行 |
| **合计** | **~470 行** |

## 不变式

- **[服务端解耦]** ArtifactTool 通过环境变量配置服务端 URL/Token，不与 CCB 硬绑定。默认值指向 CCB 生产出口仅用于开箱即用。
- **[不修改 TUI 渲染管道]** V1 通过 OSC 8 escape 嵌入输出文本实现链接渲染，不改 ToolBlock/ToolCallState 结构。
- **[字符串解析兼容 resumed session]** `/artifacts` 从 tool_result 字符串提取 URL，不依赖结构化 toolUseResult。
- **[Immediate 命令 push_done]** `/artifacts` ACP 命令在 executor 的 `intercept_immediate_command()` 中统一调用 `sink.push_done()`。
- **[TTL 默认 7d]** 模型可通过 `ttl` 参数覆盖为 `"30d"`。

## 与 CCB 实现的差异

| 维度 | CCB (TypeScript/Ink) | Perihelion (Rust/ratatui) |
|------|---------------------|--------------------------|
| 工具注册 | `buildTool` 工厂 | `BaseTool` trait + `collect_tools()` |
| TUI 渲染 | Ink React 组件 + `renderToolResultMessage` 委托 | OSC 8 escape 嵌入文本（V1），后续可升级 ToolBlock 自定义渲染 |
| 命令 | Ink 组件内 `<ArtifactsMenu>` | ACP Immediate + TUI PanelComponent |
| 链接渲染 | Ink `<Link>` 组件（OSC 8） | `LinkSpan::wrap_osc8()` 直接注入 escape |
| 复制 | `setClipboard`（跨平台） | `arboard` crate |
| 消息扫描 | scanner.ts 遍历 message 数组 | Rust iterator + regex |
| 服务端 | 同 CCB Worker（复用） | 同 CCB Worker（复用） |
