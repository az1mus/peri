//! SQLite 只读数据访问层。
//!
//! 使用 bun:sqlite 驱动，所有操作为只读。收拢 DB 行类型、消息解析、
//! 查询方法于一个文件内，不给 metrics 层暴露 SQL 细节。

import { Database } from "bun:sqlite";
import { homedir } from "os";
import { join } from "path";

export const DEFAULT_DB_PATH = join(homedir(), ".peri/threads/threads.db");

// ── DB 行类型 ──

export interface ThreadRow {
  id: string;
  title: string | null;
  cwd: string;
  created_at: string;
  updated_at: string;
  message_count: number;
  parent_thread_id: string | null;
  snapshot_at_message_id: string | null;
  hidden: number;
  cancel_policy: string;
  config: string | null;
  cached_context: string | null;
  agent_status: string;
}

export interface MessageRow {
  message_id: string;
  thread_id: string;
  role: "user" | "assistant" | "system" | "tool";
  content: string; // JSON
}

// ── 解析后的消息类型 ──

export type ContentBlock =
  | { type: "text"; text: string }
  | { type: "tool_use"; id: string; name: string; input: Record<string, unknown> }
  | { type: "tool_result"; tool_use_id: string; content: string; is_error?: boolean }
  | { type: "reasoning"; text: string; signature?: string }
  | { type: "thinking"; text: string; signature?: string }
  | { type: "image"; source: unknown }
  | { type: "document"; title?: string; source: unknown }
  | { type: "unknown"; data: unknown };

export interface ToolCallRequest {
  id: string;
  name: string;
  arguments: Record<string, unknown>;
}

export interface HumanContent {
  role: "user";
  id: string;
  content: string | ContentBlock[];
}

export interface AiContent {
  role: "assistant";
  id: string;
  content: ContentBlock[];
}

export interface SystemContent {
  role: "system";
  id: string;
  content: string | ContentBlock[];
}

export interface ToolContent {
  role: "tool";
  id: string;
  tool_call_id: string;
  content: string;
  is_error: boolean;
}

export type ParsedMessage = HumanContent | AiContent | SystemContent | ToolContent;

// ── DataLoader ──

export class DataLoader {
  private db: Database;

  constructor(dbPath: string = DEFAULT_DB_PATH) {
    this.db = new Database(dbPath, { readonly: true });
  }

  close(): void {
    this.db.close();
  }

  // ── Threads ──

  /** 加载所有可见（非 sub-agent、未隐藏）会话 */
  loadVisibleThreads(): ThreadRow[] {
    return this.db
      .query("SELECT * FROM threads WHERE hidden = 0 ORDER BY created_at ASC")
      .all() as ThreadRow[];
  }

  /** 加载最近 N 小时内更新的可见会话 */
  loadVisibleThreadsSince(sinceHours: number): ThreadRow[] {
    const cutoff = new Date(Date.now() - sinceHours * 3600_000).toISOString();
    return this.db
      .query("SELECT * FROM threads WHERE hidden = 0 AND updated_at >= ? ORDER BY created_at ASC")
      .all(cutoff) as ThreadRow[];
  }

  /** 加载所有会话（含 sub-agent、含隐藏） */
  loadAllThreads(): ThreadRow[] {
    return this.db
      .query("SELECT * FROM threads ORDER BY created_at ASC")
      .all() as ThreadRow[];
  }

  /** 加载指定父会话的所有子 agent */
  loadSubAgents(parentThreadId: string): ThreadRow[] {
    return this.db
      .query("SELECT * FROM threads WHERE parent_thread_id = ? ORDER BY created_at ASC")
      .all(parentThreadId) as ThreadRow[];
  }

  /** 按 ID 批量加载 threads */
  loadThreadsByIds(ids: string[]): ThreadRow[] {
    if (ids.length === 0) return [];
    const placeholders = ids.map(() => "?").join(",");
    return this.db
      .query(`SELECT * FROM threads WHERE id IN (${placeholders})`)
      .all(...ids) as ThreadRow[];
  }

  /** 加载所有 sub-agent 会话 */
  loadAllSubAgents(): ThreadRow[] {
    return this.db
      .query("SELECT * FROM threads WHERE parent_thread_id IS NOT NULL ORDER BY created_at ASC")
      .all() as ThreadRow[];
  }

  /** 加载所有主会话（非 sub-agent） */
  loadAllMainThreads(): ThreadRow[] {
    return this.db
      .query("SELECT * FROM threads WHERE parent_thread_id IS NULL AND hidden = 0 ORDER BY created_at ASC")
      .all() as ThreadRow[];
  }

  // ── Messages ──

  /** 加载指定会话的所有消息（按 rowid 顺序） */
  loadMessages(threadId: string): MessageRow[] {
    return this.db
      .query("SELECT * FROM messages WHERE thread_id = ? ORDER BY rowid ASC")
      .all(threadId) as MessageRow[];
  }

  /** 流式处理指定会话的消息（避免一次性加载大量数据） */
  processMessages(
    threadId: string,
    handler: (msg: MessageRow, idx: number) => void
  ): void {
    const rows = this.db
      .query("SELECT * FROM messages WHERE thread_id = ? ORDER BY rowid ASC")
      .all(threadId) as MessageRow[];
    rows.forEach((row, idx) => handler(row, idx));
  }

  /** 加载所有 is_error=true 的 tool 消息 */
  loadToolErrors(): MessageRow[] {
    return this.db
      .query(`SELECT * FROM messages WHERE role = 'tool' AND content LIKE '%is_error":true%' ORDER BY rowid ASC`)
      .all() as MessageRow[];
  }

  /** 加载指定会话的错误 tool 消息 */
  loadToolErrorsForThread(threadId: string): MessageRow[] {
    return this.db
      .query(`SELECT * FROM messages WHERE thread_id = ? AND role = 'tool' AND content LIKE '%is_error":true%' ORDER BY rowid ASC`)
      .all(threadId) as MessageRow[];
  }

  /** 加载所有 assistant 消息（含 tool_use blocks） */
  loadAssistantMessages(): MessageRow[] {
    return this.db
      .query("SELECT * FROM messages WHERE role = 'assistant' ORDER BY rowid ASC")
      .all() as MessageRow[];
  }

  // ── Statistics ──

  getStats() {
    const threads = this.db
      .query("SELECT COUNT(*) as total, SUM(CASE WHEN hidden=0 THEN 1 ELSE 0 END) as visible FROM threads")
      .get() as any;
    const messages = this.db
      .query("SELECT COUNT(*) as total FROM messages")
      .get() as any;
    const roleDistribution = this.db
      .query("SELECT role, COUNT(*) as count FROM messages GROUP BY role")
      .all() as { role: string; count: number }[];
    const errors = this.db
      .query("SELECT COUNT(*) as total FROM messages WHERE role='tool' AND content LIKE '%is_error\":true%'")
      .get() as any;

    return {
      totalThreads: threads.total as number,
      visibleThreads: threads.visible as number,
      totalMessages: messages.total as number,
      roleDistribution: Object.fromEntries(roleDistribution.map((r) => [r.role, r.count])),
      totalToolErrors: errors.total as number,
    };
  }

  /** 获取时间范围内统计 */
  getFilteredStats(sinceHours: number) {
    const cutoff = new Date(Date.now() - sinceHours * 3600_000).toISOString();
    const threads = this.db
      .query("SELECT COUNT(*) as total, SUM(CASE WHEN hidden=0 THEN 1 ELSE 0 END) as visible FROM threads WHERE updated_at >= ?")
      .get(cutoff) as any;
    const threadIds = (
      this.db.query("SELECT id FROM threads WHERE hidden = 0 AND updated_at >= ?").all(cutoff) as { id: string }[]
    ).map((t) => t.id);

    let totalMessages = 0;
    let totalErrors = 0;
    const roleCounts: Record<string, number> = {};

    if (threadIds.length > 0) {
      const ph = threadIds.map(() => "?").join(",");
      totalMessages = (this.db
        .query(`SELECT COUNT(*) as c FROM messages WHERE thread_id IN (${ph})`)
        .get(...threadIds) as any).c;
      totalErrors = (this.db
        .query(`SELECT COUNT(*) as c FROM messages WHERE role='tool' AND content LIKE '%is_error":true%' AND thread_id IN (${ph})`)
        .get(...threadIds) as any).c;
      const roleRows = this.db
        .query(`SELECT role, COUNT(*) as count FROM messages WHERE thread_id IN (${ph}) GROUP BY role`)
        .all(...threadIds) as { role: string; count: number }[];
      for (const r of roleRows) roleCounts[r.role] = r.count;
    }

    return {
      totalThreads: threads.total as number,
      visibleThreads: threads.visible as number,
      totalMessages,
      roleDistribution: roleCounts,
      totalToolErrors: totalErrors,
    };
  }

  // ── 静态解析辅助 ──

  /** 安全解析消息 content JSON */
  static parseContent(raw: string): ParsedMessage | null {
    try {
      return JSON.parse(raw);
    } catch {
      return null;
    }
  }

  /** 从 AiContent 提取 tool_use 调用 */
  static extractToolCalls(msg: ParsedMessage | null): ToolCallRequest[] {
    if (!msg || msg.role !== "assistant") return [];
    const ai = msg as AiContent;
    const blocks: any[] = Array.isArray(ai.content) ? ai.content : [];
    return blocks
      .filter((b: any) => b.type === "tool_use")
      .map((b: any) => ({
        id: b.id,
        name: b.name,
        arguments: b.input ?? {},
      }));
  }

  /** 获取 assistant 消息中的 tool_use block 列表 */
  static getToolUseBlocks(msg: ParsedMessage | null): ContentBlock[] {
    if (!msg || msg.role !== "assistant") return [];
    const ai = msg as AiContent;
    if (!Array.isArray(ai.content)) return [];
    return ai.content.filter((b) => b.type === "tool_use");
  }

  /** 从 ToolContent 提取错误信息 */
  static parseToolError(msg: ParsedMessage | null): {
    toolCallId: string;
    content: string;
    isError: boolean;
  } | null {
    if (!msg || msg.role !== "tool") return null;
    const tc = msg as ToolContent;
    return {
      toolCallId: tc.tool_call_id ?? "",
      content: typeof tc.content === "string" ? tc.content : JSON.stringify(tc.content),
      isError: !!tc.is_error,
    };
  }
}
