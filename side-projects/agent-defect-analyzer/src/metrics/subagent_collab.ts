//! 场景六：SubAgent 协作
//!
//! 4 项指标：空转 SubAgent、消息量分布、工具错误率、编辑产出比。
//! 用法：bun run src/metrics/subagent_collab.ts --since 24

import { DataLoader, type ThreadRow, type MessageRow, type AiContent, type ContentBlock } from "../data/loader.js";
import { avg, median, p50, p95, pct, formatSize, parseSinceArg, printHeader, printSection, printMetric, printWarning, printTable, printBar, printSeparator } from "../lib/utils.js";
import chalk from "chalk";

// ═══════════════════════════════════════════════════
// 常量
// ═══════════════════════════════════════════════════

const EDIT_OUTPUT_TOOLS = new Set(["LineEdit", "Edit", "Write"]);

/** 非编辑型 SubAgent 类型：这些 agent 的本职工作就不是编辑文件 */
const NON_EDITING_TYPES = new Set([
  "explore",          // 代码探索，只读
  "web-researcher",   // 网页调研，只读
  "hello-agent",      // 打招呼，无操作
  "verification",     // 验证测试，不编辑
  "plan",             // 方案设计，不编辑
]);

// ═══════════════════════════════════════════════════
// 类型
// ═══════════════════════════════════════════════════

interface SubAgentAnalysis {
  thread: ThreadRow;
  messages: MessageRow[];
  subagentType: string;
  toolUseCount: number;
  editToolUseCount: number;
  toolErrorCount: number;
  toolErrorRate: number;
}

// ═══════════════════════════════════════════════════
// 指标 1：内置 Agent 分类分析
// ═══════════════════════════════════════════════════

function analyzeAgentClassification(subAgents: SubAgentAnalysis[]): void {
  printSection("指标 1：内置 Agent 分类分析");

  const editingCount = subAgents.filter(
    (sa) => !NON_EDITING_TYPES.has(sa.subagentType),
  ).length;
  const nonEditingCount = subAgents.filter(
    (sa) => NON_EDITING_TYPES.has(sa.subagentType),
  ).length;

  printMetric("SubAgent 总数", subAgents.length);
  printMetric("编辑型", editingCount);
  printMetric("非编辑型", nonEditingCount, ` (${[...NON_EDITING_TYPES].join(", ")})`);

  // ── 构建类型数据 ──
  const typeProfiles = new Map<
    string,
    {
      agents: SubAgentAnalysis[];
      count: number;
      totalMsg: number;
      totalCall: number;
      toolCounts: Map<string, number>;
      searchPct: number;
      editPct: number;
      execPct: number;
    }
  >();

  for (const sa of subAgents) {
    if (!typeProfiles.has(sa.subagentType)) {
      typeProfiles.set(sa.subagentType, {
        agents: [],
        count: 0,
        totalMsg: 0,
        totalCall: 0,
        toolCounts: new Map(),
        searchPct: 0,
        editPct: 0,
        execPct: 0,
      });
    }
    const p = typeProfiles.get(sa.subagentType)!;
    p.agents.push(sa);
    p.count++;
    p.totalMsg += sa.thread.message_count;
    for (const msg of sa.messages) {
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed || parsed.role !== "assistant") continue;
      const ai = parsed as AiContent;
      const blocks: ContentBlock[] = Array.isArray(ai.content) ? ai.content : [];
      for (const block of blocks) {
        if (block.type === "tool_use") {
          p.toolCounts.set(block.name, (p.toolCounts.get(block.name) || 0) + 1);
          p.totalCall++;
        }
      }
    }
  }

  // 计算各类占比
  for (const [, p] of typeProfiles) {
    const t = p.totalCall;
    p.searchPct = [...p.toolCounts.entries()]
      .filter(([t]) => SEARCH_TOOLS.has(t))
      .reduce((s, [, c]) => s + c, 0) / (t || 1);
    p.editPct = [...p.toolCounts.entries()]
      .filter(([t]) => EDIT_OUTPUT_TOOLS.has(t))
      .reduce((s, [, c]) => s + c, 0) / (t || 1);
    p.execPct = [...p.toolCounts.entries()]
      .filter(([t]) => EXEC_TOOLS.has(t))
      .reduce((s, [, c]) => s + c, 0) / (t || 1);
  }

  // ── 逐一分析各类型 ──
  const sorted = [...typeProfiles.entries()].sort((a, b) => b[1].count - a[1].count);

  for (const [type, p] of sorted) {
    const isNonEditing = NON_EDITING_TYPES.has(type);
    const cat = isNonEditing ? "非编辑型" : "编辑型";
    const direction = isNonEditing
      ? "只读"
      : p.editPct > p.searchPct * 0.3
        ? "编辑"
        : "研究";

    printSection(type);
    console.log(
      chalk.dim(
        `  分类: ${cat}  方向: ${direction}型  数量: ${p.count}  均消息: ${Math.round(p.totalMsg / p.count)}  总调用: ${p.totalCall}`,
      ),
    );

    // 工具占比 Top 8
    console.log("");
    const topTools = [...p.toolCounts.entries()]
      .sort((a, b) => b[1] - a[1])
      .slice(0, 8);
    for (const [tool, count] of topTools) {
      const t = p.totalCall || 1;
      const bar = "█".repeat(Math.round((count / t) * 30));
      console.log(
        `    ${tool.padEnd(18)} ${String(count).padStart(5)}  ${pct(count, t).padStart(5)}  ${bar}`,
      );
    }

    // 模式分布条
    console.log("");
    printBar("搜索", p.searchPct);
    printBar("编辑", p.editPct);
    printBar("执行", p.execPct);
    console.log("");
  }
}

// ═══════════════════════════════════════════════════
// 辅助：SubAgent 类型推断 + 编辑产出检测
// ═══════════════════════════════════════════════════

/** 从父线程的 Agent tool_use 中提取 SubAgent 类型映射 */
function buildSubAgentTypeMap(
  loader: DataLoader,
  subAgents: ThreadRow[],
): Map<string, string> {
  const typeMap = new Map<string, string>();

  // 按父线程分组
  const byParent = new Map<string, ThreadRow[]>();
  for (const sa of subAgents) {
    if (!sa.parent_thread_id) continue;
    if (!byParent.has(sa.parent_thread_id)) byParent.set(sa.parent_thread_id, []);
    byParent.get(sa.parent_thread_id)!.push(sa);
  }

  for (const [parentId, children] of byParent) {
    const messages = loader.loadMessages(parentId);

    // 收集 Agent tool_use 调用
    const agentCalls = new Map<string, { subagentType?: string }>();
    for (const msg of messages) {
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed || parsed.role !== "assistant") continue;
      const ai = parsed as AiContent;
      const blocks: ContentBlock[] = Array.isArray(ai.content) ? ai.content : [];
      for (const block of blocks) {
        if (block.type === "tool_use" && block.name === "Agent") {
          agentCalls.set(block.id, {
            subagentType: (block.input as any)?.subagent_type || (block.input as any)?.type,
          });
        }
      }
    }

    if (agentCalls.size === 0) continue;

    // 匹配 tool_result → child thread
    for (const msg of messages) {
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed || parsed.role !== "tool") continue;
      const tc = parsed as any;
      if (!tc.tool_call_id) continue;

      const agentCall = agentCalls.get(tc.tool_call_id);
      if (!agentCall) continue;

      const resultContent =
        typeof tc.content === "string" ? tc.content : JSON.stringify(tc.content);
      for (const child of children) {
        if (resultContent.includes(child.id)) {
          typeMap.set(child.id, agentCall.subagentType || "unknown");
        }
      }
    }

    // 回退：按顺序匹配未分配的
    const unmatched = children.filter((c) => !typeMap.has(c.id));
    if (unmatched.length > 0) {
      const agentEntries = [...agentCalls.entries()];
      for (let i = 0; i < Math.min(unmatched.length, agentEntries.length); i++) {
        typeMap.set(
          unmatched[i].id,
          agentEntries[i][1].subagentType || "unknown",
        );
      }
    }
  }

  // 最终回退
  for (const sa of subAgents) {
    if (!typeMap.has(sa.id)) {
      typeMap.set(sa.id, "unknown");
    }
  }

  return typeMap;
}

// ═══════════════════════════════════════════════════
// 指标 2：SubAgent 消息量
// ═══════════════════════════════════════════════════

function analyzeMessageVolume(subAgents: ThreadRow[]): void {
  printSection("指标 2：SubAgent 消息量分布");

  const counts = subAgents.map((sa) => sa.message_count).filter((c) => c > 0);

  if (counts.length === 0) {
    printWarning("无数据", "没有可用的 SubAgent 消息量数据");
    return;
  }

  printMetric("P50", p50(counts));
  printMetric("P95", p95(counts));
  printMetric("P99", quantile99(counts));
  printMetric("最大值", Math.max(...counts));
  printMetric("平均值", avg(counts).toFixed(1));
  printMetric("总计消息", counts.reduce((a, b) => a + b, 0));

  // 分布桶
  const buckets: Record<string, number> = {
    "1-5": 0,
    "6-10": 0,
    "11-20": 0,
    "21-50": 0,
    "51-100": 0,
    "101+": 0,
  };
  for (const c of counts) {
    if (c <= 5) buckets["1-5"]++;
    else if (c <= 10) buckets["6-10"]++;
    else if (c <= 20) buckets["11-20"]++;
    else if (c <= 50) buckets["21-50"]++;
    else if (c <= 100) buckets["51-100"]++;
    else buckets["101+"]++;
  }

  console.log("");
  printTable(
    ["消息数范围", "数量", "占比", "分布"],
    Object.entries(buckets).map(([label, count]) => [
      label,
      String(count),
      pct(count, counts.length),
      "█".repeat(Math.round((count / Math.max(1, counts.length)) * 40)),
    ]),
  );
}

// ═══════════════════════════════════════════════════
// 指标 3：SubAgent 工具错误率
// ═══════════════════════════════════════════════════

interface ToolErrorStats {
  toolUseCount: number;
  errorCount: number;
}

/** 统计单个 SubAgent 的工具错误率 */
function computeToolErrorRate(messages: MessageRow[]): ToolErrorStats {
  const toolUseIds = new Set<string>();
  const errorIds = new Set<string>();

  for (const msg of messages) {
    const parsed = DataLoader.parseContent(msg.content);
    if (!parsed) continue;

    if (parsed.role === "assistant") {
      const ai = parsed as AiContent;
      const blocks: ContentBlock[] = Array.isArray(ai.content) ? ai.content : [];
      for (const block of blocks) {
        if (block.type === "tool_use") {
          toolUseIds.add(block.id);
        }
      }
    } else if (parsed.role === "tool") {
      const err = DataLoader.parseToolError(parsed);
      if (err && err.isError) {
        errorIds.add(err.toolCallId);
      }
    }
  }

  return {
    toolUseCount: toolUseIds.size,
    errorCount: errorIds.size,
  };
}

function analyzeToolErrorRate(subAgents: SubAgentAnalysis[]): void {
  printSection("指标 3：SubAgent 工具错误率");

  let totalToolUse = 0;
  let totalErrors = 0;
  const perSubAgent: { id: string; toolUse: number; errors: number; rate: number }[] = [];

  for (const sa of subAgents) {
    const stats = computeToolErrorRate(sa.messages);
    totalToolUse += stats.toolUseCount;
    totalErrors += stats.errorCount;
    if (stats.toolUseCount > 0) {
      perSubAgent.push({
        id: sa.thread.id,
        toolUse: stats.toolUseCount,
        errors: stats.errorCount,
        rate: stats.errorCount / stats.toolUseCount,
      });
    }
  }

  if (totalToolUse === 0) {
    printWarning("无数据", "没有可用的工具调用数据");
    return;
  }

  const overallRate = totalErrors / totalToolUse;
  printMetric("总工具调用数", totalToolUse);
  printMetric("总错误数", totalErrors);
  printMetric("总体错误率", pct(totalErrors, totalToolUse));

  // 每个 SubAgent 的错误率分布
  const rates = perSubAgent.map((s) => s.rate);
  printMetric("P50 错误率", pct(p50(rates), 1));
  printMetric("P95 错误率", pct(p95(rates), 1));

  // Top 10 最高错误率
  perSubAgent.sort((a, b) => b.rate - a.rate);
  const top10 = perSubAgent.slice(0, 10);

  console.log("");
  printTable(
    ["子Agent ID", "工具调用数", "错误数", "错误率"],
    top10.map((s) => [
      s.id.slice(0, 14) + "...",
      String(s.toolUse),
      String(s.errors),
      pct(s.errors, s.toolUse),
    ]),
  );

  printBar("总体错误率", overallRate);
}

// ═══════════════════════════════════════════════════
// 指标 4：SubAgent 产出比
// ═══════════════════════════════════════════════════

function computeOutputRatio(messages: MessageRow[]): { total: number; edit: number } {
  let total = 0;
  let edit = 0;

  for (const msg of messages) {
    const parsed = DataLoader.parseContent(msg.content);
    if (!parsed || parsed.role !== "assistant") continue;
    const ai = parsed as AiContent;
    const blocks: ContentBlock[] = Array.isArray(ai.content) ? ai.content : [];
    for (const block of blocks) {
      if (block.type !== "tool_use") continue;
      total++;
      if (EDIT_OUTPUT_TOOLS.has(block.name)) edit++;
    }
  }

  return { total, edit };
}

function analyzeOutputRatio(subAgents: SubAgentAnalysis[]): void {
  printSection("指标 4：SubAgent 产出比（编辑类工具 / 总 tool_use）");

  // 总体统计
  let totalToolUse = 0;
  let totalEditUse = 0;
  const ratios: { id: string; type: string; ratio: number; total: number; edit: number }[] = [];

  // 按类型统计
  const typeStats = new Map<string, { total: number; edit: number }>();

  for (const sa of subAgents) {
    const stats = computeOutputRatio(sa.messages);
    totalToolUse += stats.total;
    totalEditUse += stats.edit;

    // 按类型聚合
    if (!typeStats.has(sa.subagentType)) {
      typeStats.set(sa.subagentType, { total: 0, edit: 0 });
    }
    const ts = typeStats.get(sa.subagentType)!;
    ts.total += stats.total;
    ts.edit += stats.edit;

    if (stats.total > 0) {
      ratios.push({
        id: sa.thread.id,
        type: sa.subagentType,
        ratio: stats.edit / stats.total,
        total: stats.total,
        edit: stats.edit,
      });
    }
  }

  if (totalToolUse === 0) {
    printWarning("无数据", "没有可用的工具调用数据");
    return;
  }

  const overallRatio = totalEditUse / totalToolUse;
  printMetric("总 tool_use 数", totalToolUse);
  printMetric("编辑类 tool_use 数", totalEditUse);
  printMetric("总体产出比", pct(totalEditUse, totalToolUse));

  // 按类型分层
  printSection("按 SubAgent 类型分层");
  printTable(
    ["类型", "SubAgent 数", "tool_use 总数", "编辑类", "产出比"],
    [...typeStats.entries()]
      .sort((a, b) => b[1].total - a[1].total)
      .map(([t, s]) => [
        t + (NON_EDITING_TYPES.has(t) ? " *" : ""),
        String(subAgents.filter((sa) => sa.subagentType === t).length),
        String(s.total),
        String(s.edit),
        pct(s.edit, s.total || 1),
      ]),
  );
  console.log("  * 非编辑型（本职不含编辑任务）");

  // 编辑型单独统计
  printSection("编辑型 SubAgent 产出比（排除非编辑型）");
  const editingRatios = ratios.filter((r) => !NON_EDITING_TYPES.has(r.type));
  const editingTotal = editingRatios.reduce((s, r) => s + r.total, 0);
  const editingEdit = editingRatios.reduce((s, r) => s + r.edit, 0);

  printMetric("编辑型总 tool_use", editingTotal);
  printMetric("编辑型编辑类产出", editingEdit);
  printMetric("编辑型产出比", pct(editingEdit, editingTotal || 1));

  if (editingRatios.length > 0) {
    printMetric(
      "P50 产出比",
      pct(p50(editingRatios.map((r) => r.ratio)), 1),
    );
    printMetric(
      "P95 产出比",
      pct(p95(editingRatios.map((r) => r.ratio)), 1),
    );

    // 分布桶（仅编辑型）
    const buckets: Record<string, number> = {
      "0": 0,
      "0-20%": 0,
      "20-50%": 0,
      "50-80%": 0,
      "80%+": 0,
    };
    for (const r of editingRatios) {
      if (r.ratio === 0) buckets["0"]++;
      else if (r.ratio <= 0.2) buckets["0-20%"]++;
      else if (r.ratio <= 0.5) buckets["20-50%"]++;
      else if (r.ratio <= 0.8) buckets["50-80%"]++;
      else buckets["80%+"]++;
    }

    console.log("");
    printTable(
      ["产出比范围", "数量", "占比", "分布"],
      Object.entries(buckets).map(([label, count]) => [
        label,
        String(count),
        pct(count, editingRatios.length),
        "█".repeat(Math.round((count / Math.max(1, editingRatios.length)) * 40)),
      ]),
    );
  }

  printBar("编辑型产出比", editingEdit / (editingTotal || 1));
}

// ═══════════════════════════════════════════════════
// 指标 5：SubAgent 类型分布与工具使用模式
// ═══════════════════════════════════════════════════

const SEARCH_TOOLS = new Set(["Read", "Grep", "Glob", "WebFetch", "WebSearch", "folder_operations"]);
const EXEC_TOOLS = new Set(["Bash", "Agent", "AgentResult", "TodoWrite", "AskUserQuestion"]);

// ═══════════════════════════════════════════════════
// 辅助
// ═══════════════════════════════════════════════════

function quantile99(arr: number[]): number {
  if (arr.length === 0) return 0;
  const sorted = [...arr].sort((a, b) => a - b);
  const idx = Math.ceil(sorted.length * 0.99) - 1;
  return sorted[Math.max(0, idx)];
}

// ═══════════════════════════════════════════════════
// 主入口
// ═══════════════════════════════════════════════════

function main(): void {
  const sinceHours = parseSinceArg();
  const loader = new DataLoader();

  printHeader("场景六：SubAgent 协作");

  // 加载 SubAgent 线程
  const subAgentThreads = loader.loadAllSubAgents();

  if (subAgentThreads.length === 0) {
    printWarning("无 SubAgent", "数据库中没有 SubAgent 线程");
    loader.close();
    return;
  }

  printMetric("SubAgent 总数", subAgentThreads.length);

  // 时间过滤：通过父线程判断
  let filteredSubAgents: ThreadRow[];
  if (sinceHours) {
    // SubAgent 没有直接的 updated_at 过滤，通过加载所有主线程再关联
    const mainThreads = loader.loadVisibleThreadsSince(sinceHours);
    const mainIds = new Set(mainThreads.map((t) => t.id));
    filteredSubAgents = subAgentThreads.filter(
      (sa) => sa.parent_thread_id && mainIds.has(sa.parent_thread_id),
    );
    printMetric("时间范围", `最近 ${sinceHours} 小时`);
    printMetric("过滤后 SubAgent 数", filteredSubAgents.length);
  } else {
    filteredSubAgents = subAgentThreads;
  }

  printSeparator();

  // 推断 SubAgent 类型（从父线程的 Agent 调用中提取 subagent_type）
  const typeMap = buildSubAgentTypeMap(loader, filteredSubAgents);
  const unknownCount = [...typeMap.values()].filter((t) => t === "unknown").length;
  if (unknownCount > 0) {
    // 静默处理：类型未知时默认可编辑

  }

  // 批量加载消息（只加载一次，各指标复用）
  const analyses: SubAgentAnalysis[] = [];
  for (const t of filteredSubAgents) {
    const messages = loader.loadMessages(t.id);
    const saType = typeMap.get(t.id) || "unknown";
    analyses.push({
      thread: t,
      messages,
      subagentType: saType,
      toolUseCount: 0,
      editToolUseCount: 0,
      toolErrorCount: 0,
      toolErrorRate: 0,
    });
  }

  // ── 指标 1：内置 Agent 分类分析 ──
  analyzeAgentClassification(analyses);

  // ── 指标 2：消息量 ──
  analyzeMessageVolume(filteredSubAgents);

  // ── 指标 3：工具错误率 ──
  analyzeToolErrorRate(analyses);

  // ── 指标 4：产出比 ──
  analyzeOutputRatio(analyses);

  loader.close();

  loader.close();
}

main();
