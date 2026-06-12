# Web Researcher Agent 升级为 Built-in Agent，支持原生 WebFetch/WebSearch 及复杂研究工作流

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-12

## 问题描述

现有 `.claude/agents/web-researcher.md` 是文件级 Agent 定义，通过 Bash 调用 `npx @langgraph-js/web-fetch` 实现网页抓取。项目已有原生 `WebFetch`/`WebSearch` 工具（且刚刚修复了子 Agent 继承 Web 工具的问题），但 web-researcher 未使用。用户期望将其升级为 Built-in Agent（编译期嵌入），使用原生 Web 工具，并支持复杂的多轮研究工作流。

## 症状详情

| 维度 | 现状 | 期望 |
|------|------|------|
| Agent 类型 | 文件级 `.claude/agents/web-researcher.md` | Built-in Agent（`include_str!` 嵌入） |
| Web 访问方式 | Bash 调用 `npx @langgraph-js/web-fetch` | 原生 `WebFetch`/`WebSearch` 工具 |
| 工具白名单 | Bash + Write + Read | WebFetch + WebSearch + Bash + Write + Read |
| 工作流能力 | 单页抓取，简单的搜索-抓取-合成 | 多轮搜索、并行多源分析、深度追踪 |
| 搜索方式 | Bing URL via Bash | WebSearch 工具 |

## 期望改进方向

1. **Built-in Agent**：将 `web-researcher` 加入 `peri-middlewares/src/subagent/built-in/` 和 `BUILT_IN_AGENTS` 数组
2. **原生 Web 工具**：使用 `WebFetch`（替代 Bash + npx web-fetch）、`WebSearch`（替代 Bing URL 搜索）
3. **三种工作流模式**（Agent 根据任务自动选择）：
   - 多轮递进：搜 → 取 → 分析 → 补搜 → 再取 → 合成报告
   - 并行多源：一次搜索取多个 URL，并行分析后合并
   - 深度追踪：搜 → 取 → 发现线索 → 继续深入（最多 2 层）
4. **保留能力**：仍可用 Bash 做辅助处理（如 jq 过滤 JSON）、Write 写中间结果到 `/tmp/`

## 涉及文件

- `peri-middlewares/src/subagent/built_in_agents.rs:29-50` — `BUILT_IN_AGENTS` 数组，需新增 web-researcher 条目
- `peri-middlewares/src/subagent/built-in/` — 需新建 `web-researcher.md`（Built-in Agent 定义文件）
- `.claude/agents/web-researcher.md`（82 行）—— 现有文件级定义，Built-in 版本替代后可移除或保留为覆盖用
- 历史设计文档 `spec/archive/feature_20260326_F001_specialized-agents/`

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-12 | — | Open | agent | 创建 |
| 2026-06-12 | Open | Verified | agent | 修复完成，用户验证通过 |

## 修复记录

### 修复 #1（2026-06-12）

- **操作人**：agent
- **用户原意**：将 web-researcher 升级为 Built-in Agent，使用原生 WebFetch/WebSearch，支持三种复杂研究工作流（多轮递进/并行多源/深度追踪）
- **修复内容**：
  1. 创建 `built-in/web-researcher.md`（138 行）—— 原生工具 + 三种策略 + RESEARCH REPORT 模板
  2. `built_in_agents.rs` 注册为第 6 个 built-in agent（+ 测试更新）
  3. 删除旧 `.claude/agents/web-researcher.md`（Bash+npx 版本不再遮蔽 built-in）
  4. 修复硬编码 built-in count 5→6（`mod_test.rs`、`prompt_test.rs`）
- **涉及 commit**：`7a9ae071`、`d2eb1ef8`、`407a93dc`、`cd40e126`、`9853bde6`
- **验证状态**：已验证

### 验证 #1（2026-06-12）—— 通过

用户确认 web-researcher 作为 built-in agent 正常生效，旧 Bash+npx 版本已移除。
