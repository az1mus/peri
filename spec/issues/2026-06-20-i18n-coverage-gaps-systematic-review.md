# 界面国际化不完整——切换语言后大量 UI 文字不变

**状态**：Open
**优先级**：中
**创建日期**：2026-06-20

## 问题描述

对 TUI 界面做了三视角交叉审查（终端用户视角 + 技术判断视角 + 代码现状视角），发现国际化覆盖存在系统性缺口。切换到 zh-CN 后，以下 UI 区域的文字仍显示英文或直接硬编码中文不走 Fluent：

1. **13 个 FTL key 已定义但代码从未调用**（翻译工作已完成，只差接入）
2. **7 个 UI 面板完全没有 `lc.tr()` 调用**（model、status、agent、hooks、thread_browser、rewind、oauth）
3. **4 个命令的 `execute()` 输出硬编码中文**（channel、bg、loop、plugin）
4. **Login 面板 Edit 模式的 7 个字段标签硬编码英文**

工具输出（Read/Write/Edit/Glob/Grep 发给 LLM 的错误消息）、开发者诊断（`/gc`）、技术缩写（N/A/ON/OFF/MEM）和统计图表图例（Input Tokens 等）**不需要翻译**，已明确排除。

## 症状详情

### 现象：切换语言不生效的 UI 区域

| 语言 | 区域 | 预期 | 实际 |
|------|------|------|------|
| zh-CN | 模型选择面板 | 显示"选择模型" | 显示 " Select model " |
| zh-CN | 状态面板 | 显示"状态" / Tab "费用""上下文" | 显示 " Status " / "Cost""Context" |
| zh-CN | 历史浏览器 | 显示"搜索…"/"刚刚" | 显示 "Search…"/"just now" |
| zh-CN | Rewind 弹窗 | 中文提示 | 标题 "Rewind" + 中英混排 |
| zh-CN | Hook 面板 | 显示"已配置 3 个 hook" | 显示 "3 hooks configured" |
| zh-CN | Login 编辑表单 | 中文字段名 | "Name""Type""Base URL" 等 |
| zh-CN | 帮助命令 | (已有 FTL key, 但 execute() 不用) | 硬编码中文 |

### 排除项（不需要翻译）

以下已确认不需要翻译，三视角达成共识：
- LLM 工具输出（Read/Write/Edit/Glob/Grep/Bash/WebFetch/WebSearch 的错误消息）
- 专有名词（Opus/Sonnet/Haiku、Anthropic/OpenAI）
- 技术缩写（N/A、MEM、ON/OFF、KB/MB/GB）
- 统计图表图例（Input Tokens、cache_read、Cache Hit Rate）
- `/gc` 诊断输出（开发者工具）
- thiserror 定义（内部错误）
- 权限模式标签（Don't Ask 等，zh-CN FTL 有意保持英文）

## 涉及文件

### Phase 1 — FTL 已定义但代码未用（13 个 key，零成本）

| FTL Key (已翻译) | 代码文件 | 硬编码行 |
|-------------------|---------|---------|
| `help-available-commands` | `command/core/help.rs` | :19 `"可用命令："` |
| `help-alias-prefix` | `command/core/help.rs` | :24 `format!(" (别名: /{})")` |
| `help-skills-count` | `command/core/help.rs` | :33-34 |
| `help-skills-empty` | `command/core/help.rs` | :38-39 |
| `help-shortcuts` | `command/core/help.rs` | :44-47 |
| `rename-no-session` | `command/session/rename.rs` | :19 |
| `rename-current-title` | `command/session/rename.rs` | :39 |
| `rename-untitled` | `command/session/rename.rs` | :38 |
| `rename-updated` | `command/session/rename.rs` | :54 |
| `rename-failed` | `command/session/rename.rs` | :62 |
| `effort-set` | `command/session/effort.rs` | :42 |
| `effort-current` | `command/session/effort.rs` | :69 |
| `effort-usage` | `command/session/effort.rs` | :69 |
| `history-agent-running` | `command/core/history.rs` | :27-28 |
| `config-save-failed` | `event/keyboard/shortcuts.rs` | :58,106 |
| — | `event/keyboard/setup_wizard.rs` | :49 |
| — | `command/panel/model.rs` | :30 |
| — | `command/session/effort.rs` | :34 |

### Phase 2 — 完全未接入 i18n 的 UI 面板（7 个）

| 文件 | `lc.tr()` 调用 | 主要缺口 |
|------|---------------|---------|
| `ui/main_ui/panels/model.rs` | **0** | 标题、描述、MaxToken/Effort/1MContext 字段、Effort 等级名 |
| `ui/main_ui/panels/status.rs` | **0** | 标题、Tab 标签（Cost/Context）、图表图例 |
| `ui/main_ui/panels/agent.rs` | **0** | 标题、"无 Agent（默认）"、引导语 |
| `ui/main_ui/panels/hooks.rs` | **0** | 标题、统计行、详情字段（matcher/plugin） |
| `ui/main_ui/panels/thread_browser.rs` | **0** | 标题、搜索占位符、相对时间、空状态、默认标题 |
| `ui/main_ui/popups/rewind.rs` | **0** | 标题、消息计数、操作描述、确认提示 |
| `ui/main_ui/popups/oauth.rs` | **0** | 标题、提示、输入标签 |

### Phase 3 — 命令输出的硬编码中文（4 个命令）

| 文件 | 数量 |
|------|------|
| `command/session/channel.rs` | ~10 处（命令描述、用法、状态反馈） |
| `command/session/bg.rs` | 1 处（用法帮助） |
| `command/session/loop_cmd.rs` | 1 处（用法帮助） |
| `command/panel/plugin_command.rs` | ~4 处（错误反馈、帮助文本） |

### Phase 4 — 可选增强

| 文件 | 内容 |
|------|------|
| `ui/message_render.rs` | 批次摘要（`"{} agents failed"`）、状态标签、推理摘要 |
| `ui/main_ui/panels/config.rs:278,317` | streaming/proactiveness 值显示名 |
| `ui/message_view/build.rs:143-156` | `[Image]`/`[Document]` 回退占位符 |
| `cli_print.rs:41` | `-p` 模式错误提示 |
| `thread/browser.rs:159-161` | 删除对话反馈 |
| `command/agent.rs:21-29` | Agent 切换反馈 |

### 已正确 i18n 的模块（标杆参考）

- `ui/welcome.rs` ★★★★★
- `ui/main_ui/panels/config.rs` ★★★★★
- `ui/main_ui/panels/mcp.rs` ★★★★☆
- `ui/main_ui/popups/hitl.rs` ★★★★★
- `ui/main_ui/popups/setup_wizard.rs` ★★★★★
- `ui/main_ui/status_bar.rs` ★★★★☆（核心指示器已 i18n，权限名和 Rewind 未接入）

## 相关历史 Issue

- 2026-05-16 `setup-language-step-hardcoded-no-i18n.md`（Fixed）— Setup 语言步骤
- 2026-05-16 `setup-form-edit-labels-hardcoded.md`（Fixed）— Setup 表单标签
- 2026-05-16 `i18n-language-not-in-setup.md`（Fixed）— 语言选择缺失
- 2026-05-26 `login-panel-hardcoded-chinese-no-i18n.md`（Fixed）— Login 面板部分硬编码

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-20 | — | Open | claude | 三视角审查创建 |

## 修复记录

### 修复 #1（2026-06-20）

- **操作人**：claude
- **用户原意**：Phase 1 — 接入 13 个已有 FTL key，零成本消除漏网之鱼
- **修复内容**：
  - `command/core/help.rs` — execute() 全部改用 `lc.tr()`/`lc.tr_args()`（5 个 key）
  - `command/session/rename.rs` — execute() 全部改用 i18n（5 个 key）
  - `command/session/effort.rs` — execute() 全部改用 i18n（3 个 key）+ config-save-failed
  - `command/core/history.rs` — execute() 改用 `lc.tr("history-agent-running")`
  - `event/keyboard/shortcuts.rs` — 2 处 config-save-failed 改用 i18n
  - `event/keyboard/setup_wizard.rs` — config-save-failed 改用 i18n
  - `command/panel/model.rs` — config-save-failed 改用 i18n
  - `locales/{en,zh-CN}/main.ftl` — `help-shortcuts` 添加 `{ $model_key }` 参数支持跨平台快捷键名
- **验证状态**：待验证
