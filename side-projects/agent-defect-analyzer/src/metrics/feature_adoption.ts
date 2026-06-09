//! 场景四：功能采纳分析。
//!
//! 5 项指标：LineEdit 使用率、LineEdit 成功率、Skill 调用频率、Skill 链深度、工具使用多样性。
//!
//! 用法：bun run src/metrics/feature_adoption.ts [--since 24]

import { DataLoader, type ThreadRow, type MessageRow, type ParsedMessage, type AiContent, type ContentBlock, type ToolCallRequest } from "../data/loader.js";
import { avg, median, p50, p95, pct, formatSize, formatDuration, parseSinceArg, printHeader, printSection, printMetric, printWarning, printTable, printBar, printSeparator } from "../lib/utils.js";

// ── 常量 ──

const SKILL_PATH_RE = /skills\/([\w][-\w]*)\/SKILL\.md/gi;

// ── 入口 ──

const sinceHours = parseSinceArg();
const loader = new DataLoader();
if (sinceHours) printMetric("时间范围", `最近 ${sinceHours} 小时`);
const threads = sinceHours ? loader.loadVisibleThreadsSince(sinceHours) : loader.loadVisibleThreads();

printHeader("场景四：功能采纳");

printMetric("可见会话数", threads.length);

// 1. LineEdit 使用率
analyzeLineEditUsage(loader, threads);

// 2. LineEdit 成功率
analyzeLineEditSuccess(loader, threads);

// 3. Skill 调用频率（两维度）
analyzeSkillFrequency(loader, threads);

// 4. Skill 链深度
analyzeSkillChainDepth(loader, threads);

// 5. 工具使用多样性
analyzeToolDiversity(loader, threads);

loader.close();

// ═══════════════════════════════════════════════════
// 辅助：提取消息文本
// ═══════════════════════════════════════════════════

function extractText(parsed: ParsedMessage): string {
  const content = (parsed as any).content;
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return content
      .filter((b: any) => b.type === "text")
      .map((b: any) => b.text || "")
      .join("");
  }
  return "";
}

// ═══════════════════════════════════════════════════
// 指标 1：LineEdit 使用率
// ═══════════════════════════════════════════════════

function analyzeLineEditUsage(loader: DataLoader, threads: ThreadRow[]): void {
  printSection("1. LineEdit 使用率");

  let lineEditCount = 0;
  let editCount = 0;
  const sessionUsage = new Map<string, { le: number; edit: number }>();

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);
    let le = 0;
    let ed = 0;

    for (const msg of messages) {
      if (msg.role !== "assistant") continue;
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed) continue;
      const toolCalls = DataLoader.extractToolCalls(parsed);
      for (const tc of toolCalls) {
        if (tc.name === "LineEdit") le++;
        if (tc.name === "Edit") ed++;
      }
    }

    if (le > 0 || ed > 0) {
      sessionUsage.set(thread.id, { le, ed });
    }
    lineEditCount += le;
    editCount += ed;
  }

  const total = lineEditCount + editCount;
  printMetric("LineEdit 调用数", lineEditCount);
  printMetric("Edit 调用数", editCount);
  printMetric("LineEdit 占比", total > 0 ? pct(lineEditCount, total) : "-");
  printMetric("使用 LineEdit 的会话数", `${sessionUsage.size} / ${threads.length}`);

  // 按会话分布
  if (sessionUsage.size > 0) {
    const ratios: number[] = [];
    for (const { le, ed } of sessionUsage.values()) {
      const total = le + ed;
      if (total > 0) ratios.push(le / total);
    }
    ratios.sort((a, b) => a - b);
    printMetric("会话级 LineEdit 占比 P50", ratios.length > 0 ? pct(p50(ratios), 1) : "-");
    printMetric("会话级 LineEdit 占比 P95", ratios.length > 0 ? pct(p95(ratios), 1) : "-");

    printBar("总体 LineEdit 使用率", total > 0 ? lineEditCount / total : 0);
  }
}

// ═══════════════════════════════════════════════════
// 指标 2：LineEdit 成功率
// ═══════════════════════════════════════════════════

function analyzeLineEditSuccess(loader: DataLoader, threads: ThreadRow[]): void {
  printSection("2. LineEdit 成功率");

  let totalCalls = 0;
  let errorCount = 0;
  const errorReasons: Record<string, number> = {
    param_parse: 0,
    old_string_not_found: 0,
    other: 0,
  };

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);

    // 收集 assistant 中的 LineEdit tool_use（callId → 存在标记）
    const leCalls = new Set<string>();
    for (const msg of messages) {
      if (msg.role !== "assistant") continue;
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed) continue;
      const toolCalls = DataLoader.extractToolCalls(parsed);
      for (const tc of toolCalls) {
        if (tc.name === "LineEdit") {
          leCalls.add(tc.id);
          totalCalls++;
        }
      }
    }

    // 收集 tool 消息中匹配的 LineEdit 结果
    for (const msg of messages) {
      if (msg.role !== "tool") continue;
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed) continue;
      const toolResult = DataLoader.parseToolError(parsed);
      if (!toolResult || !leCalls.has(toolResult.toolCallId)) continue;

      if (toolResult.isError) {
        errorCount++;
        const reason = classifyLineEditError(toolResult.content);
        errorReasons[reason]++;
      }
    }
  }

  printMetric("LineEdit 总调用数", totalCalls);
  printMetric("is_error 数", errorCount);
  printMetric("成功率", totalCalls > 0 ? pct(totalCalls - errorCount, totalCalls) : "-");
  printMetric("失败率", totalCalls > 0 ? pct(errorCount, totalCalls) : "-");

  // 错误原因分类
  printSection("  错误原因分类");
  if (errorCount > 0) {
    const errRows = Object.entries(errorReasons)
      .filter(([, c]) => c > 0)
      .sort((a, b) => b[1] - a[1])
      .map(([reason, count]) => [
        reason === "param_parse" ? "参数解析失败" :
        reason === "old_string_not_found" ? "old_string 未匹配" :
        "其他",
        String(count),
        pct(count, errorCount),
      ]);
    printTable(["原因", "次数", "占比"], errRows);
  } else {
    console.log("  无错误");
  }
}

function classifyLineEditError(content: string): "param_parse" | "old_string_not_found" | "other" {
  const lower = content.toLowerCase();
  if (/param.*(parse|invalid|malformed)/i.test(lower)) return "param_parse";
  if (/old_string.*not\s*found/i.test(lower) || /unified diff.*invalid/i.test(lower)) return "old_string_not_found";
  return "other";
}

// ═══════════════════════════════════════════════════
// 指标 3：Skill 调用频率（两维度）
// ═══════════════════════════════════════════════════

function analyzeSkillFrequency(loader: DataLoader, threads: ThreadRow[]): void {
  printSection("3. Skill 调用频率");

  // ── 维度一：System 消息中的 skill 加载标记 ──
  const skillLoadCounts: Record<string, number> = {};

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);
    for (const msg of messages) {
      if (msg.role !== "system") continue;
      const content = (() => {
        try {
          const parsed = JSON.parse(msg.content);
          if (typeof parsed.content === "string") return parsed.content;
          if (Array.isArray(parsed.content)) {
            return parsed.content
              .filter((b: any) => b.type === "text")
              .map((b: any) => b.text || "")
              .join("\n");
          }
        } catch { /* ignore */ }
        return "";
      })();

      // 从 SKILL.md 路径中提取 skill 名称
      const pathMatches = content.matchAll(SKILL_PATH_RE);
      const seenInMsg = new Set<string>();
      for (const pm of pathMatches) {
        const skillName = pm[1].toLowerCase();
        if (!seenInMsg.has(skillName)) {
          seenInMsg.add(skillName);
          skillLoadCounts[skillName] = (skillLoadCounts[skillName] || 0) + 1;
        }
      }
    }
  }

  printSection("  维度一：System 消息 Skill 加载频次");
  if (Object.keys(skillLoadCounts).length > 0) {
    const skillRows = Object.entries(skillLoadCounts)
      .sort((a, b) => b[1] - a[1])
      .map(([name, count]) => [name, String(count)]);
    printTable(["Skill 名称", "加载次数"], skillRows);
    printMetric("不同 Skill 种类", Object.keys(skillLoadCounts).length);
  } else {
    console.log("  未在 System 消息中检测到 Skill 加载标记");
  }

  // ── 维度二：Agent 工具调用的 subagent_type 参数 ──
  const subagentTypeCounts: Record<string, number> = {};

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);
    for (const msg of messages) {
      if (msg.role !== "assistant") continue;
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed) continue;
      const toolCalls = DataLoader.extractToolCalls(parsed);
      for (const tc of toolCalls) {
        if (tc.name !== "Agent") continue;
        const st = tc.arguments.subagent_type;
        if (typeof st === "string" && st.length > 0) {
          subagentTypeCounts[st] = (subagentTypeCounts[st] || 0) + 1;
        }
      }
    }
  }

  printSection("  维度二：SubAgent 类型频次");
  if (Object.keys(subagentTypeCounts).length > 0) {
    const saRows = Object.entries(subagentTypeCounts)
      .sort((a, b) => b[1] - a[1])
      .map(([name, count]) => [name, String(count)]);
    printTable(["SubAgent 类型", "调用次数"], saRows);
    printMetric("不同 SubAgent 类型", Object.keys(subagentTypeCounts).length);
  } else {
    console.log("  未发现 Agent 工具调用（含 subagent_type）");
  }
}

// ═══════════════════════════════════════════════════
// 指标 4：Skill 链深度
// ═══════════════════════════════════════════════════

function analyzeSkillChainDepth(loader: DataLoader, threads: ThreadRow[]): void {
  printSection("4. Skill 链深度");

  // 获取主会话（parent_thread_id IS NULL 且可见）
  const mainThreads = sinceHours
    ? threads.filter((t) => t.parent_thread_id === null)
    : loader.loadAllMainThreads();

  const depths: number[] = [];

  for (const mt of mainThreads) {
    const depth = computeChainDepth(loader, mt.id, 1);
    depths.push(depth);
  }

  if (depths.length === 0) {
    console.log("  无主会话数据");
    return;
  }

  printMetric("主会话数", mainThreads.length);
  printMetric("最大链深度", Math.max(...depths));
  printMetric("平均链深度", avg(depths).toFixed(1));
  printMetric("链深度 P50", String(p50(depths)));
  printMetric("链深度 P95", String(p95(depths)));

  // 深度分布
  const buckets: Record<string, number> = {
    "深度 1": 0,
    "深度 2": 0,
    "深度 3": 0,
    "深度 4": 0,
    "深度 5+": 0,
  };

  for (const d of depths) {
    if (d === 1) buckets["深度 1"]++;
    else if (d === 2) buckets["深度 2"]++;
    else if (d === 3) buckets["深度 3"]++;
    else if (d === 4) buckets["深度 4"]++;
    else buckets["深度 5+"]++;
  }

  const distRows = Object.entries(buckets)
    .filter(([, c]) => c > 0)
    .map(([label, count]) => [label, String(count), pct(count, depths.length)]);

  printTable(["深度", "会话数", "占比"], distRows);

  // 打印各深度条
  for (const [label, count] of Object.entries(buckets)) {
    if (count > 0) {
      printBar(label, count / depths.length, 25);
    }
  }
}

function computeChainDepth(loader: DataLoader, threadId: string, currentDepth: number): number {
  const subs = loader.loadSubAgents(threadId);
  if (subs.length === 0) return currentDepth;

  let maxSubDepth = currentDepth;
  for (const sub of subs) {
    const subDepth = computeChainDepth(loader, sub.id, currentDepth + 1);
    maxSubDepth = Math.max(maxSubDepth, subDepth);
  }
  return maxSubDepth;
}

// ═══════════════════════════════════════════════════
// 指标 5：工具使用多样性
// ═══════════════════════════════════════════════════

function analyzeToolDiversity(loader: DataLoader, threads: ThreadRow[]): void {
  printSection("5. 工具使用多样性");

  const sessionDiversity: { threadId: string; uniqueTools: number; totalCalls: number }[] = [];

  for (const thread of threads) {
    const toolSet = new Set<string>();
    let totalCalls = 0;

    const messages = loader.loadMessages(thread.id);
    for (const msg of messages) {
      if (msg.role !== "assistant") continue;
      const parsed = DataLoader.parseContent(msg.content);
      if (!parsed) continue;
      const toolCalls = DataLoader.extractToolCalls(parsed);
      for (const tc of toolCalls) {
        toolSet.add(tc.name);
        totalCalls++;
      }
    }

    if (toolSet.size > 0) {
      sessionDiversity.push({
        threadId: thread.id,
        uniqueTools: toolSet.size,
        totalCalls,
      });
    }
  }

  if (sessionDiversity.length === 0) {
    console.log("  无工具调用数据");
    return;
  }

  const uniqueCounts = sessionDiversity.map((s) => s.uniqueTools);
  printMetric("有工具调用的会话数", sessionDiversity.length);
  printMetric("每会话最多工具种类", Math.max(...uniqueCounts));
  printMetric("每会话 P50 工具种类", String(p50(uniqueCounts)));
  printMetric("每会话 P95 工具种类", String(p95(uniqueCounts)));
  printMetric("每会话平均工具种类", avg(uniqueCounts).toFixed(1));

  // 分布桶
  const buckets: Record<string, number> = {
    "1-3 种": 0,
    "4-6 种": 0,
    "7-10 种": 0,
    "11-15 种": 0,
    "16+ 种": 0,
  };

  for (const { uniqueTools } of sessionDiversity) {
    if (uniqueTools <= 3) buckets["1-3 种"]++;
    else if (uniqueTools <= 6) buckets["4-6 种"]++;
    else if (uniqueTools <= 10) buckets["7-10 种"]++;
    else if (uniqueTools <= 15) buckets["11-15 种"]++;
    else buckets["16+ 种"]++;
  }

  const distRows = Object.entries(buckets)
    .filter(([, c]) => c > 0)
    .map(([label, count]) => [label, String(count), pct(count, sessionDiversity.length)]);

  printTable(["种类范围", "会话数", "占比"], distRows);

  // 打印分布条
  for (const [label, count] of Object.entries(buckets)) {
    if (count > 0) {
      printBar(label, count / sessionDiversity.length, 30);
    }
  }
}
