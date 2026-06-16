# Storage 领域

## 领域综述

Storage 领域负责 Agent 框架中所有数据持久化相关的基础设施选型与实现。当前聚焦于线程（会话）持久化层的异步数据库访问。

核心职责：
- SQLite 数据库连接池管理（sqlx SqlitePool）
- 线程持久化（threads + messages 两表 Schema）
- 异步数据库操作，消除 sync 桥接模式

边界：不涉及消息内容格式（由 agent 领域管理）、不涉及 TUI 层的会话浏览 UI。

## 核心流程

### 线程持久化流程

```
StateSnapshot 事件触发
  → 过滤 System 消息（不持久化）
  → append_messages 事务写入 SQLite（sqlx::query）
  → WAL 模式保证 crash-safe
  → SqlitePool(max=5) 连接池管理并发
  → 下次 Agent 执行时 load_messages 恢复
```

## 技术方案总结

| 维度 | 选型 |
|------|------|
| 数据库驱动 | sqlx 0.8（runtime-tokio + sqlite），原生 async，不含 macros/migrate |
| 连接管理 | SqlitePool(max=5)，替代 Arc\<Mutex\<Connection\>\> + spawn_blocking |
| PRAGMA | journal_mode=WAL, synchronous=NORMAL, foreign_keys=ON |
| Schema | threads(id,title,cwd,created_at,updated_at,message_count) + messages(message_id,thread_id,role,content) |
| 事务 | append_messages 使用 pool.begin() + tx.commit() |
| 初始化 | init_schema 拆分为 3 次 sqlx::query().execute()（sqlx 不支持多语句） |
| 调用方影响 | App::new() / new_headless() 变 async，移除 Default impl |

## Feature 附录

### feature_20260504_F001_sqlx-migration
**摘要:** 线程持久化层从 rusqlite 同步迁移到 sqlx 原生异步
**关键决策:**
- sqlx 0.8（runtime-tokio + sqlite）替代 rusqlite + parking_lot
- SqlitePool(max=5) 替代 Arc\<Mutex\<Connection\>\> + spawn_blocking
- Schema 不变（threads + messages 两表）
- 最小 feature：仅 runtime-tokio + sqlite，不含 macros/migrate
- App::new() / new_headless() 变 async，移除 Default impl
- init_schema 拆分为 3 次独立 execute 调用（sqlx 不支持多语句）
**归档:** [链接](../../archive/feature_20260504_F001_sqlx-migration/)
**归档日期:** 2026-05-04

---

## Issue 经验附录

> 本节记录已归档的 storage 领域 issue，提取通用经验和反模式。

---

### issue_2026-06-01-thread-browser-full-table-scan-high-memory
- **摘要:** ThreadBrowser 全量 SQLite 查询导致高内存占用
- **状态:** Fixed
- **归档日期:** 2026-06-16
- **关键词:** SQLite 全量查询, 内存优化, Lazy Loading
- **问题本质:** `list_threads()` 加载了不需要的 `cached_context` 大字段（~1MB/thread），列表场景不需要完整消息历史 JSON
- **通用模式:** 列表查询与详情查询应使用不同的列集——大字段按需加载，列表仅取元数据列
- **技术决策:** `THREAD_META_COLUMNS` 常量将列表列与详情列分离，列表用 `NULL as cached_context` 占位，`load_context()` 按需单独加载
- **涉及文件:** peri-agent/src/thread/sqlite_store.rs, peri-agent/src/thread/types.rs, peri-tui/src/app/thread_ops.rs

---

## 相关 Feature
- → [agent.md#20260322_F001_agent-storage-refactor](./agent.md#20260322_F001_agent-storage-refactor) — SQLite WAL 持久化替代 JSONL（初始实现）
- → [agent.md#feature_20260326_F006_message-uuid-v7](./agent.md#feature_20260326_F006_message-uuid-v7) — message_id 为主键
