//! 公共工具函数：统计计算 + 终端渲染。
//!
//! 无外部依赖，纯函数。chart.js 等复杂可视化如需引入另建文件。

import chalk from "chalk";

// ═══════════════════════════════════════════════════
// 统计函数
// ═══════════════════════════════════════════════════

/** 算术平均 */
export function avg(arr: number[]): number {
  if (arr.length === 0) return 0;
  return arr.reduce((a, b) => a + b, 0) / arr.length;
}

/** 中位数 */
export function median(arr: number[]): number {
  if (arr.length === 0) return 0;
  const sorted = [...arr].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 === 0
    ? (sorted[mid - 1] + sorted[mid]) / 2
    : sorted[mid];
}

/** 分位数 (0-1) */
export function quantile(arr: number[], q: number): number {
  if (arr.length === 0) return 0;
  const sorted = [...arr].sort((a, b) => a - b);
  const idx = Math.ceil(sorted.length * q) - 1;
  return sorted[Math.max(0, idx)];
}

/** P50 */
export function p50(arr: number[]): number {
  return median(arr);
}

/** P95 */
export function p95(arr: number[]): number {
  return quantile(arr, 0.95);
}

/** 百分比字符串 */
export function pct(n: number, total: number): string {
  if (total === 0) return "0%";
  return `${((n / total) * 100).toFixed(1)}%`;
}

/** 字节格式化 */
export function formatSize(bytes: number): string {
  if (bytes < 1024) return `${bytes}B`;
  if (bytes < 1024 * 1024) return `${(bytes / 1024).toFixed(1)}KB`;
  return `${(bytes / 1024 / 1024).toFixed(1)}MB`;
}

/** 持续时间格式化 */
export function formatDuration(minutes: number): string {
  if (minutes < 1) return "<1m";
  if (minutes < 60) return `${Math.round(minutes)}m`;
  const h = Math.floor(minutes / 60);
  const m = Math.round(minutes % 60);
  return m > 0 ? `${h}h${m}m` : `${h}h`;
}

// ═══════════════════════════════════════════════════
// CLI 参数解析
// ═══════════════════════════════════════════════════

/** 从 process.argv 提取 --since 参数 */
export function parseSinceArg(): number | undefined {
  const idx = process.argv.indexOf("--since");
  if (idx < 0) return undefined;
  const val = parseFloat(process.argv[idx + 1]);
  return val > 0 ? val : undefined;
}

// ═══════════════════════════════════════════════════
// 终端渲染
// ═══════════════════════════════════════════════════

const SEP = "─".repeat(80);

/** 主标题 */
export function printHeader(title: string): void {
  console.log("\n" + chalk.bold.cyan(`═${SEP}═`));
  console.log(chalk.bold.cyan(`  ${title}`));
  console.log(chalk.bold.cyan(`═${SEP}═\n`));
}

/** 段落标题 */
export function printSection(title: string): void {
  console.log(chalk.bold.yellow(`\n▸ ${title}`));
  console.log(chalk.yellow(`  ${"─".repeat(60)}`));
}

/** 单行指标 */
export function printMetric(label: string, value: string | number, unit?: string): void {
  const val = typeof value === "number" ? value.toLocaleString() : value;
  const suffix = unit ? chalk.gray(unit) : "";
  console.log(`  ${chalk.gray("•")} ${chalk.white(label)}: ${chalk.bold.green(val)}${suffix}`);
}

/** 警告信息 */
export function printWarning(label: string, detail: string): void {
  console.log(`  ${chalk.yellow("⚠")} ${chalk.yellow(label)}: ${detail}`);
}

/** 表格 */
export function printTable(headers: string[], rows: string[][]): void {
  // 轻量表格：手动计算列宽
  const colWidths = headers.map((h, i) => {
    const maxRow = rows.reduce((m, r) => Math.max(m, (r[i] || "").length), 0);
    return Math.max(h.length, maxRow) + 2;
  });

  const drawLine = () => {
    const parts = colWidths.map((w) => "─".repeat(w));
    console.log("  ┌" + parts.join("┬") + "┐");
  };

  drawLine();
  const headerCells = headers.map((h, i) => chalk.bold.cyan(h.padEnd(colWidths[i])));
  console.log("  │" + headerCells.join("│") + "│");
  drawLine();
  for (const row of rows) {
    const cells = row.map((c, i) => (c || "").padEnd(colWidths[i]));
    console.log("  │" + cells.join("│") + "│");
  }
  drawLine();
}

/** 进度条 */
export function printBar(label: string, ratio: number, width = 30): void {
  const filled = Math.round(Math.min(ratio, 1) * width);
  const empty = width - filled;
  const barColor = ratio > 0.7 ? chalk.red : ratio > 0.4 ? chalk.yellow : chalk.green;
  console.log(
    `  ${label} ${barColor("█".repeat(filled))}${chalk.gray("░".repeat(empty))} ${(ratio * 100).toFixed(1)}%`
  );
}

/** 代码块 */
export function printCodeBlock(code: string): void {
  console.log(chalk.gray("  ┌" + "─".repeat(40)));
  for (const line of code.split("\n")) {
    console.log(chalk.gray("  │ ") + chalk.white(line));
  }
  console.log(chalk.gray("  └" + "─".repeat(40)));
}

/** 水平分隔线 */
export function printSeparator(): void {
  console.log(chalk.gray(`  ${SEP}`));
}
