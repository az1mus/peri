//! 场景七：搜索卫生（Search Hygiene）。
//!
//! 检测 Glob/Grep 巨型调用——上下文窗口的"沉默杀手"。
//! 常见爆炸指纹：.claude/** 全扫（含 worktree 副本）、pattern: **/*.{md,ts} 全递归、
//! head_limit:0 关掉结果上限、path 不限定走全局。
//!
//! 4 项主指标：
//!   1. 巨型调用率（按工具细分 >20KB / >50KB 阈值占比）
//!   2. 爆炸 pattern Top-N（>50KB 的 pattern/path 聚合）
//!   3. 危险路径命中率（.claude/** / node_modules / worktrees 等）
//!   4. head_limit 配置缺陷（=0 或缺失的 Grep 调用）
//!
//! 追查场景切片：
//!   5. 单会话堆积（同一 thread 内巨型调用次数 Top-N）
//!   6. 高危 cwd（按 thread.cwd 聚合巨型调用量）
//!
//! 用法：bun run src/metrics/search_hygiene.ts [--since 24]

import {
  DataLoader,
  type ThreadRow,
  type ParsedMessage,
} from "../data/loader.js";
import {
  avg,
  p50,
  p95,
  pct,
  formatSize,
  parseSinceArg,
  printHeader,
  printSection,
  printMetric,
  printWarning,
  printTable,
  printBar,
} from "../lib/utils.js";

// ── 常量 ──

const SEARCH_TOOLS = new Set(["Glob", "Grep"]);
const GIANT_THRESHOLD = 20 * 1024; // 20KB：明显过大
const EXPLOSION_THRESHOLD = 50 * 1024; // 50KB：严重爆炸

/** 已知危险路径片段——这些目录的 glob 几乎必然产生巨型结果 */
const DANGEROUS_PATH_PATTERNS: { re: RegExp; label: string }[] = [
  { re: /\/\.claude\/(?!skills\/[^/]+\/SKILL\.md)/, label: ".claude/ (含 worktrees + plugins/cache)" },
  { re: /\/node_modules\//, label: "node_modules/" },
  { re: /\/\.git\//, label: ".git/" },
  { re: /\/worktrees?\//, label: "worktrees/" },
  { re: /\/plugins\/cache\//, label: "plugins/cache/" },
  { re: /\/target\/(debug|release)\//, label: "target/(debug|release)/" },
  { re: /\/dist\//, label: "dist/" },
  { re: /\/build\//, label: "build/" },
];

/** 已知危险 glob 模式——这些 pattern 本身就是宽泛递归 */
const DANGEROUS_GLOBS: { re: RegExp; label: string }[] = [
  { re: /\*\*\/\*/, label: "**/* (全递归)" },
  { re: /\*\*\/\*\.\w+/, label: "**/*.<ext> (全递归按扩展名)" },
  { re: /^\*$/, label: "* (单层全匹配)" },
  { re: /\*\.\*\*?$/, label: "*.<ext>* (前缀模糊)" },
];

// ── 内部数据结构 ──

interface SearchCall {
  threadId: string;
  cwd: string;
  tool: "Glob" | "Grep";
  input: Record<string, unknown>;
  /** Grep 的 pattern，或 Glob 的 pattern */
  pattern: string;
  /** Grep 的 path 参数，或 Glob 的 path 参数 */
  path: string | undefined;
  resultSize: number;
  isError: boolean;
}

// ── 入口 ──

const sinceHours = parseSinceArg();
const loader = new DataLoader();

printHeader("场景七：搜索卫生（巨型 Glob/Grep 检测）");
if (sinceHours) printMetric("时间范围", `最近 ${sinceHours} 小时`);
const threads = sinceHours ? loader.loadVisibleThreadsSince(sinceHours) : loader.loadVisibleThreads();
printMetric("可见会话数", threads.length);

const calls = collectSearchCalls(loader, threads);
printMetric("Glob/Grep 调用总数", calls.length);

// 主指标
analyzeGiantRate(calls);
analyzeExplosionPatterns(calls);
analyzeDangerousPaths(calls);
analyzeHeadLimitDefects(calls);

// 追查场景
analyzeSessionAccumulation(calls);
analyzeHighRiskCwd(calls);

loader.close();

// ═══════════════════════════════════════════════════
// 数据收集
// ═══════════════════════════════════════════════════

function collectSearchCalls(loader: DataLoader, threads: ThreadRow[]): SearchCall[] {
  const out: SearchCall[] = [];

  for (const thread of threads) {
    const messages = loader.loadMessages(thread.id);

    // callId → resultSize + isError
    const resultMap = new Map<string, { size: number; isError: boolean }>();
    for (const m of messages) {
      if (m.role !== "tool") continue;
      const parsed = DataLoader.parseContent(m.content);
      if (!parsed || parsed.role !== "tool") continue;
      const tc = parsed as any;
      const content = typeof tc.content === "string" ? tc.content : JSON.stringify(tc.content ?? "");
      resultMap.set(tc.tool_call_id, {
        size: Buffer.byteLength(content, "utf8"),
        isError: !!tc.is_error,
      });
    }

    for (const m of messages) {
      if (m.role !== "assistant") continue;
      const parsed = DataLoader.parseContent(m.content);
      const toolCalls = DataLoader.extractToolCalls(parsed);
      for (const tc of toolCalls) {
        if (!SEARCH_TOOLS.has(tc.name)) continue;
        const result = resultMap.get(tc.id) ?? { size: 0, isError: false };
        const tool = tc.name as "Glob" | "Grep";
        out.push({
          threadId: thread.id,
          cwd: thread.cwd,
          tool,
          input: tc.arguments,
          pattern: extractPattern(tool, tc.arguments),
          path: extractPath(tool, tc.arguments),
          resultSize: result.size,
          isError: result.isError,
        });
      }
    }
  }

  return out;
}

function extractPattern(tool: "Glob" | "Grep", input: Record<string, unknown>): string {
  if (tool === "Glob") {
    return typeof input.pattern === "string" ? input.pattern : "";
  }
  // Grep
  return typeof input.pattern === "string" ? input.pattern : "";
}

function extractPath(tool: "Glob" | "Grep", input: Record<string, unknown>): string | undefined {
  const p = input.path ?? input.cwd;
  return typeof p === "string" ? p : undefined;
}

// ═══════════════════════════════════════════════════
// 指标 1：巨型调用率
// ═══════════════════════════════════════════════════

function analyzeGiantRate(calls: SearchCall[]): void {
  printSection("1. 巨型调用率");

  const validCalls = calls.filter((c) => c.resultSize > 0);
  if (validCalls.length === 0) {
    console.log("  无有效结果数据");
    return;
  }

  for (const tool of ["Glob", "Grep"] as const) {
    const group = validCalls.filter((c) => c.tool === tool);
    if (group.length === 0) continue;

    const sizes = group.map((c) => c.resultSize).sort((a, b) => a - b);
    const giantCount = sizes.filter((s) => s > GIANT_THRESHOLD).length;
    const explosionCount = sizes.filter((s) => s > EXPLOSION_THRESHOLD).length;

    printSection(`  ${tool}（n=${group.length}）`);
    printMetric("结果平均", formatSize(Math.round(avg(sizes))));
    printMetric("结果 P50", formatSize(p50(sizes)));
    printMetric("结果 P95", formatSize(p95(sizes)));
    printMetric("结果最大", formatSize(sizes[sizes.length - 1]));
    printMetric(">20KB 巨型调用", `${giantCount} (${pct(giantCount, group.length)})`);
    printMetric(">50KB 严重爆炸", `${explosionCount} (${pct(explosionCount, group.length)})`);
  }

  // 总览
  const totalGiant = validCalls.filter((c) => c.resultSize > GIANT_THRESHOLD).length;
  const totalExplosion = validCalls.filter((c) => c.resultSize > EXPLOSION_THRESHOLD).length;
  printSection("  合计");
  printMetric("巨型调用 (>20KB)", `${totalGiant} / ${validCalls.length} (${pct(totalGiant, validCalls.length)})`);
  printMetric("严重爆炸 (>50KB)", `${totalExplosion} / ${validCalls.length} (${pct(totalExplosion, validCalls.length)})`);
}

// ═══════════════════════════════════════════════════
// 指标 2：爆炸 pattern Top-N
// ═══════════════════════════════════════════════════

function analyzeExplosionPatterns(calls: SearchCall[]): void {
  printSection("2. 爆炸 pattern Top-N");

  const explosions = calls.filter((c) => c.resultSize > EXPLOSION_THRESHOLD);
  if (explosions.length === 0) {
    console.log("  未发现 >50KB 的爆炸调用");
    return;
  }

  // 按 (tool, pattern) 聚合——发现反复出现的危险 pattern
  const groups = new Map<string, { tool: string; pattern: string; count: number; totalBytes: number; maxSize: number }>();
  for (const c of explosions) {
    const key = `${c.tool}::${c.pattern}`;
    const g = groups.get(key) ?? { tool: c.tool, pattern: c.pattern, count: 0, totalBytes: 0, maxSize: 0 };
    g.count++;
    g.totalBytes += c.resultSize;
    g.maxSize = Math.max(g.maxSize, c.resultSize);
    groups.set(key, g);
  }

  const sorted = [...groups.values()].sort((a, b) => b.count - a.count || b.totalBytes - a.totalBytes);

  printMetric("爆炸调用总数", explosions.length);
  printMetric("独立 (tool, pattern) 组合", groups.size);

  const rows = sorted.slice(0, 15).map((g) => [
    g.tool,
    truncate(g.pattern, 30),
    String(g.count),
    formatSize(Math.round(g.totalBytes / g.count)),
    formatSize(g.maxSize),
  ]);

  printTable(["工具", "pattern", "次数", "平均", "最大"], rows);
}

// ═══════════════════════════════════════════════════
// 指标 3：危险路径命中率
// ═══════════════════════════════════════════════════

function analyzeDangerousPaths(calls: SearchCall[]): void {
  printSection("3. 危险路径 / 危险 pattern 命中率");

  const validCalls = calls.filter((c) => c.resultSize > 0);

  // 危险路径分析
  printSection("  3.1 危险路径（path 参数含已知风险目录）");
  for (const { label, re } of DANGEROUS_PATH_PATTERNS) {
    const hits = validCalls.filter((c) => {
      const full = c.path ? `${c.path}/${c.pattern}` : c.pattern;
      return re.test(full);
    });
    if (hits.length === 0) continue;

    const giantHits = hits.filter((c) => c.resultSize > GIANT_THRESHOLD);
    const explosionHits = hits.filter((c) => c.resultSize > EXPLOSION_THRESHOLD);
    const avgSize = avg(hits.map((c) => c.resultSize));

    printMetric(label, `${hits.length} 次调用`);
    if (giantHits.length > 0) {
      const ratio = pct(giantHits.length, hits.length);
      const warn = giantHits.length / hits.length > 0.3 ? "  ⚠ 高危" : "";
      console.log(`      └ 巨型 ${giantHits.length} (${ratio})${warn}`);
    }
    if (explosionHits.length > 0) {
      console.log(`      └ 严重爆炸 ${explosionHits.length} (${pct(explosionHits.length, hits.length)})`);
    }
    console.log(`      └ 平均 ${formatSize(Math.round(avgSize))}`);
  }

  // 危险 glob pattern 分析
  printSection("  3.2 危险 glob（宽泛递归 pattern）");
  for (const { label, re } of DANGEROUS_GLOBS) {
    const hits = validCalls.filter((c) => re.test(c.pattern));
    if (hits.length === 0) continue;

    const giantHits = hits.filter((c) => c.resultSize > GIANT_THRESHOLD);
    const explosionHits = hits.filter((c) => c.resultSize > EXPLOSION_THRESHOLD);
    const avgSize = avg(hits.map((c) => c.resultSize));

    printMetric(label, `${hits.length} 次调用`);
    if (giantHits.length > 0) {
      console.log(`      └ 巨型 ${giantHits.length} (${pct(giantHits.length, hits.length)})`);
    }
    if (explosionHits.length > 0) {
      console.log(`      └ 严重爆炸 ${explosionHits.length} (${pct(explosionHits.length, hits.length)})`);
    }
    console.log(`      └ 平均 ${formatSize(Math.round(avgSize))}`);
  }
}

// ═══════════════════════════════════════════════════
// 指标 4：head_limit 配置缺陷（仅 Grep）
// ═══════════════════════════════════════════════════

function analyzeHeadLimitDefects(calls: SearchCall[]): void {
  printSection("4. Grep head_limit 配置缺陷");

  const greps = calls.filter((c) => c.tool === "Grep");
  if (greps.length === 0) {
    console.log("  无 Grep 调用");
    return;
  }

  // head_limit 状态分类
  const buckets = {
    "head_limit=0 (关闭上限)": 0,
    "head_limit 缺失 (用默认)": 0,
    "head_limit<=50 (合理)": 0,
    "head_limit 51-250 (偏大)": 0,
    "head_limit>250 (过大)": 0,
  };
  const bucketsExplosion = { ...buckets }; // 每个桶里 >20KB 的数量

  for (const c of greps) {
    const hl = (c.input as any).head_limit;
    let bucket: keyof typeof buckets;
    if (hl === 0) bucket = "head_limit=0 (关闭上限)";
    else if (hl === undefined) bucket = "head_limit 缺失 (用默认)";
    else if (hl <= 50) bucket = "head_limit<=50 (合理)";
    else if (hl <= 250) bucket = "head_limit 51-250 (偏大)";
    else bucket = "head_limit>250 (过大)";

    buckets[bucket]++;
    if (c.resultSize > GIANT_THRESHOLD) bucketsExplosion[bucket]++;
  }

  printMetric("Grep 调用总数", greps.length);

  const rows = (Object.keys(buckets) as (keyof typeof buckets)[]).map((k) => {
    const total = buckets[k];
    const giant = bucketsExplosion[k];
    return [k, String(total), pct(total, greps.length), `${giant} (${total > 0 ? pct(giant, total) : "0%"})`];
  });

  printTable(["head_limit 状态", "次数", "占比", "其中 >20KB"], rows);
}

// ═══════════════════════════════════════════════════
// 追查场景 5：单会话堆积
// ═══════════════════════════════════════════════════

function analyzeSessionAccumulation(calls: SearchCall[]): void {
  printSection("5. 单会话堆积（巨型调用最密集的会话）");

  const giants = calls.filter((c) => c.resultSize > GIANT_THRESHOLD);
  if (giants.length === 0) {
    console.log("  无巨型调用");
    return;
  }

  const byThread = new Map<string, { count: number; totalBytes: number; maxSize: number; cwd: string }>();
  for (const c of giants) {
    const g = byThread.get(c.threadId) ?? { count: 0, totalBytes: 0, maxSize: 0, cwd: c.cwd };
    g.count++;
    g.totalBytes += c.resultSize;
    g.maxSize = Math.max(g.maxSize, c.resultSize);
    byThread.set(c.threadId, g);
  }

  const sorted = [...byThread.entries()].sort((a, b) => b[1].totalBytes - a[1].totalBytes);

  printMetric("涉及会话数", `${byThread.size} / ${new Set(calls.map((c) => c.threadId)).size}`);
  printMetric("单会话最多巨型调用次数", sorted[0][1].count);

  const rows = sorted.slice(0, 10).map(([tid, g]) => [
    tid.slice(0, 12) + "...",
    String(g.count),
    formatSize(g.totalBytes),
    formatSize(g.maxSize),
    truncate(basename(g.cwd), 24),
  ]);

  printTable(["会话ID", "巨型次数", "累积大小", "单次最大", "项目"], rows);
}

// ═══════════════════════════════════════════════════
// 追查场景 6：高危 cwd
// ═══════════════════════════════════════════════════

function analyzeHighRiskCwd(calls: SearchCall[]): void {
  printSection("6. 高危项目（按 cwd 聚合巨型调用量）");

  const giants = calls.filter((c) => c.resultSize > GIANT_THRESHOLD);
  if (giants.length === 0) {
    console.log("  无巨型调用");
    return;
  }

  const byCwd = new Map<string, { count: number; totalBytes: number; threads: Set<string> }>();
  for (const c of giants) {
    const g = byCwd.get(c.cwd) ?? { count: 0, totalBytes: 0, threads: new Set<string>() };
    g.count++;
    g.totalBytes += c.resultSize;
    g.threads.add(c.threadId);
    byCwd.set(c.cwd, g);
  }

  const sorted = [...byCwd.entries()].sort((a, b) => b[1].count - a[1].count);

  printMetric("涉及项目数", byCwd.size);

  const rows = sorted.slice(0, 10).map(([cwd, g]) => [
    truncate(basename(cwd), 32),
    String(g.count),
    formatSize(g.totalBytes),
    String(g.threads.size),
  ]);

  printTable(["项目 (cwd basename)", "巨型次数", "累积大小", "会话数"], rows);
}

// ═══════════════════════════════════════════════════
// 辅助
// ═══════════════════════════════════════════════════

function truncate(s: string, max: number): string {
  if (s.length <= max) return s;
  if (max < 5) return s.slice(0, max);
  const head = Math.ceil((max - 3) / 2);
  const tail = max - 3 - head;
  return `${s.slice(0, head)}...${s.slice(-tail)}`;
}

function basename(p: string): string {
  if (!p) return "(unknown)";
  const clean = p.replace(/\/+$/, "");
  const i = clean.lastIndexOf("/");
  return i >= 0 ? clean.slice(i + 1) : clean;
}
