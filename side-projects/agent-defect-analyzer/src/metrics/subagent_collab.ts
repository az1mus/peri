//! 场景六：SubAgent 协作
//!
//! 4 项指标：空转 SubAgent、消息量分布、工具错误率、编辑产出比。
//! 用法：bun run src/metrics/subagent_collab.ts --since 24

import { DataLoader, type ThreadRow, type MessageRow, type AiContent, type ContentBlock } from "../data/loader.js";
import { avg, median, p50, p95, pct, formatSize, parseSinceArg, printHeader, printSection, printMetric, printWarning, printTable, printBar, printSeparator } from "../lib/utils.js";

// ═══════════════════════════════════════════════════
// 常量
// ═══════════════════════════════════════════════════

const EDIT_OUTPUT_TOOLS = new Set(["LineEdit", "Edit", "Write"]);
const EMPTY_RUN_MIN_MESSAGES = 5;

// ═══════════════════════════════════════════════════
// 类型
// ═══════════════════════════════════════════════════

interface SubAgentAnalysis {
  thread: ThreadRow;
  messages: MessageRow[];
  hasEditOutput: boolean;
  toolUseCount: number;
  editToolUseCount: number;
  toolErrorCount: number;
  toolErrorRate: number;
}

// ═══════════════════════════════════════════════════
// 指标 1：空转 SubAgent
// ═══════════════════════════════════════════════════

/** 检查 SubAgent 消息序列中是否有 Write/LineEdit/Edit 的 tool_use */
function hasEditOutput(messages: MessageRow[]): boolean {
  for (const msg of messages) {
    const parsed = DataLoader.parseContent(msg.content);
    if (!parsed || parsed.role !== "assistant") continue;
    const ai = parsed as AiContent;
    const blocks: ContentBlock[] = Array.isArray(ai.content) ? ai.content : [];
    for (const block of blocks) {
      if (block.type === "tool_use" && EDIT_OUTPUT_TOOLS.has(block.name)) {
        return true;
      }
    }
  }
  return false;
}

function analyzeEmptyRun(subAgents: SubAgentAnalysis[]): void {
  printSection("指标 1：空转 SubAgent");

  const emptyRuns = subAgents.filter(
    (sa) => !sa.hasEditOutput && sa.messages.length >= EMPTY_RUN_MIN_MESSAGES,
  );

  printMetric("SubAgent 总数", subAgents.length);
  printMetric("空转 SubAgent 数", emptyRuns.length);
  printMetric("空转占比", pct(emptyRuns.length, subAgents.length));

  if (emptyRuns.length === 0) {
    console.log("  未检测到空转 SubAgent");
    return;
  }

  // 按消息数降序
  emptyRuns.sort((a, b) => b.messages.length - a.messages.length);
  const top20 = emptyRuns.slice(0, 20);

  console.log("");
  printTable(
    ["子Agent ID", "消息数", "父会话 ID", "创建时间"],
    top20.map((sa) => [
      sa.thread.id.slice(0, 14) + "...",
      String(sa.thread.message_count),
      sa.thread.parent_thread_id?.slice(0, 14) + "..." || "-",
      sa.thread.created_at.slice(0, 16).replace("T", " "),
    ]),
  );

  if (emptyRuns.length > 20) {
    console.log(`  ... 及其他 ${emptyRuns.length - 20} 个`);
  }
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

  let totalToolUse = 0;
  let totalEditUse = 0;
  const ratios: number[] = [];

  for (const sa of subAgents) {
    const stats = computeOutputRatio(sa.messages);
    totalToolUse += stats.total;
    totalEditUse += stats.edit;
    if (stats.total > 0) {
      ratios.push(stats.edit / stats.total);
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

  if (ratios.length > 0) {
    printMetric("P50 产出比", pct(p50(ratios), 1));
    printMetric("P95 产出比", pct(p95(ratios), 1));
  }

  // 分布桶
  const buckets: Record<string, number> = {
    "0": 0,
    "0-20%": 0,
    "20-50%": 0,
    "50-80%": 0,
    "80%+": 0,
  };
  for (const r of ratios) {
    if (r === 0) buckets["0"]++;
    else if (r <= 0.2) buckets["0-20%"]++;
    else if (r <= 0.5) buckets["20-50%"]++;
    else if (r <= 0.8) buckets["50-80%"]++;
    else buckets["80%+"]++;
  }

  console.log("");
  printTable(
    ["产出比范围", "数量", "占比", "分布"],
    Object.entries(buckets).map(([label, count]) => [
      label,
      String(count),
      pct(count, ratios.length),
      "█".repeat(Math.round((count / Math.max(1, ratios.length)) * 40)),
    ]),
  );

  printBar("总体编辑产出比", overallRatio);
}

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

  // 批量加载消息（只加载一次，各指标复用）
  const analyses: SubAgentAnalysis[] = [];
  for (const t of filteredSubAgents) {
    const messages = loader.loadMessages(t.id);
    analyses.push({
      thread: t,
      messages,
      hasEditOutput: hasEditOutput(messages),
      toolUseCount: 0,
      editToolUseCount: 0,
      toolErrorCount: 0,
      toolErrorRate: 0,
    });
  }

  // ── 指标 1：空转 SubAgent ──
  analyzeEmptyRun(analyses);

  // ── 指标 2：消息量 ──
  analyzeMessageVolume(filteredSubAgents);

  // ── 指标 3：工具错误率 ──
  analyzeToolErrorRate(analyses);

  // ── 指标 4：产出比 ──
  analyzeOutputRatio(analyses);

  loader.close();
}

main();
