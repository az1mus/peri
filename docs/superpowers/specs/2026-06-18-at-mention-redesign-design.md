# @ Mention 重新设计：Hierarchical Fuzzy Completion

> 状态：Draft | 日期：2026-06-18

## 动机

现有 @ 功能使用"全局 walkdir + fuzzy match + MAX_CANDIDATES=15"策略，存在以下问题：

1. **每次按键全量遍历 cwd**：专用搜索线程 walkdir 整个项目树，项目大时首次搜索慢。
2. **评分退化**：当 query 以 `/` 结尾（如 `@side-projects/`），`file_part` 为空，所有文件 `name_score` 固定为 50，`path_score` 几乎相同（都前缀匹配），排序退化到按路径长度升序。
3. **目录不展开**：`@side-projects/` 把所有深度的文件混排，截断 15 条后可能完全看不到用户目标的顶级子目录。
4. **结果不可预测**：fuzzy 匹配全树，用户无法直观知道哪些文件会被搜到。

## 设计目标

行为类似 terminal Tab 补全 + 模糊匹配：**按目录层级浏览**，每级做 fuzzy。

| 输入 | dir_part | query_part | 行为 |
|------|:--:|:--:|------|
| `@` | `` | `` | 列出 cwd 顶级所有条目 |
| `@sr` | `` | `sr` | fuzzy 匹配 cwd 顶级（如 `src/`、`side-projects/`） |
| `@side-projects/` | `side-projects/` | `` | 列出该目录下一级所有条目 |
| `@side-projects/gi` | `side-projects/` | `gi` | 该目录下 fuzzy 匹配 `gi` |

### 非目标

- 不处理 symlink 循环
- 不优化 50000+ 条目超大目录
- 不区分"只找目录"和"只找文件"

## 架构概览

```
用户输入 @prefix/path/query
        │
        ▼
  parse_at_query(text, cursor)
   → dir = resolve_dir(cwd, dir_part)    (回退到最近存在的目录)
   → query_part                          (剩余文本做 fuzzy)
        │
        ▼
  dir_cache.get(dir)
     ├─ 命中 → 直接取 entries
     └─ 未命中 → read_dir(dir) → 存缓存 → entries
        │
        ▼
  fuzzy_match_entries(entries, query_part)
   → score 降序 → 目录优先 → name 长度升序
        │
        ▼
  render popup (现有多选交互不变)
```

**关键变更**：从"全局 walkdir + fuzzy"改为"单目录 read_dir + fuzzy"。同步调用，不依赖后台线程。

## 组件变更

| 文件 | 操作 | 说明 |
|------|:--:|------|
| `peri-tui/src/app/at_mention/file_search.rs` | **重写** | `read_dir()` + `fuzzy_match_entries()`，`Entry` 替代 `FileCandidate` |
| `peri-tui/src/app/at_mention/mod.rs` | **精简** | 删除搜索线程/mpsc/节流缓存/拉取逻辑，新增 `dir_cache: HashMap<PathBuf, Vec<Entry>>`，同步 `refresh_candidates()` |
| `peri-tui/src/event/keyboard.rs` | **小改** | `update_at_mention_detection` 化简；`inject_at_mention_path` 目录条目追加尾 `/` |
| `peri-tui/src/app/agent_ops/polling.rs` | **小改** | `poll_at_mention` 改为 no-op |
| `peri-tui/src/main.rs` | **小改** | 移除 `poll_at_mention()` 调用（或保留调 no-op 函数） |
| `peri-tui/src/app/at_mention/popup.rs` | **微调** | 空列表显示 "(no matches)" / "(empty directory)" 占位 |
| `peri-middlewares/src/at_mention/` | **不变** | Middleware 层无变更 |

### 删除项

- 专用搜索线程 `search_thread_main`
- `mpsc::channel` 机制（`query_tx` / `result_rx`）
- `search_cache: HashMap<String, Vec<FileCandidate>>`
- 200ms 节流、idle timeout、`should_search_now`
- `MAX_CANDIDATES = 15`（由 popup `MAX_VIEWPORT=10` 自然截断显示）
- `start_search` / `poll_search_result` / `ensure_thread_alive` / `spawn_search_thread` / `kill_thread`

### 净效果

`at_mention/mod.rs` 从 ~438 行降至 ~200 行。

## 数据模型

### Entry（替代 FileCandidate）

```rust
struct Entry {
    name: OsString,    // 仅文件名（非全路径）
    is_dir: bool,
    is_symlink: bool,
}
```

### dir_cache

```rust
dir_cache: HashMap<PathBuf, Vec<Entry>>
```

Key 为 canonicalized 路径。cwd 变更时清空。

### AtMentionState 精简

```
保留: active, query, query_start, candidates, selected, scroll_offset, cwd
删除: query_tx, search_thread, result_rx, search_cache, last_search_query, last_search_time
新增: dir_cache
```

## 查询解析

### parse_at_query(text, cursor_pos) -> Option<(String, String)>

返回 `(dir_part, query_part)`。字符串切片不持有引用以避免生命周期复杂化。

```
输入: "看一下 @side-projects/git-stats/sr"  cursor=末尾
1. 从 cursor 向前找 @
2. @ 之后的部分: "side-projects/git-stats/sr"
3. rfind('/') → dir_part = "side-projects/git-stats/", query_part = "sr"
```

### resolve_dir(cwd, dir_part) -> (PathBuf, String)

路径不存在时向上回退，剩余部分并入 query_part：

```rust
fn resolve_dir(cwd: &Path, dir_part: &str) -> (PathBuf, String) {
    let mut curr = cwd.join(dir_part);
    let mut fallback = String::new();
    while !curr.is_dir() && curr != *cwd {
        if let Some(name) = curr.file_name() {
            let prefix = if fallback.is_empty() { "" } else { "/" };
            fallback = format!("{}{}{}", name.to_string_lossy(), prefix, fallback);
        }
        curr = curr.parent().unwrap_or(cwd).to_path_buf();
    }
    (curr, fallback)
}
```

**示例**：输入 `@side-projects/nonex/sr`，`nonex` 不存在 → 回退到 `side-projects/`，`fallback = "nonex/sr"`，与 `query_part` 合并为完整 fuzzy query。

### detect() 不变

保留现有 `AtMentionState::detect(text, cursor_pos)` 逻辑：`@` 前必须为空白/行首，`@` 后至少一个字符。

## 搜索算法

### read_dir_entries(dir) -> Vec<Entry>

```rust
fn read_dir_entries(dir: &Path) -> Vec<Entry> {
    let rd = fs::read_dir(dir)?;
    rd.flatten()
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter_map(|e| {
            let ft = e.file_type().ok()?;
            Some(Entry {
                name: e.file_name(),
                is_dir: ft.is_dir(),
                is_symlink: ft.is_symlink(),
            })
        })
        .collect()
}
```

- 跳过隐藏条目（`.` 开头）
- `file_type` 自动 resolve symlink
- 权限拒绝返回空 Vec

### fuzzy_match_entries(entries, query) -> Vec<&Entry>

- **query 为空**：返回全部，目录优先，按 name 排序
- **query 非空**：对每个 `entry.name.to_string_lossy()` 做 `SkimMatcherV2::fuzzy_match`，`score > 0` 保留
- **排序**：score 降序 → 目录优先 → name 长度升序
- 无 MAX_CANDIDATES 上限

### 性能

| 操作 | 典型耗时 |
|------|------|
| `fs::read_dir` 500 条目 | <1ms |
| `fs::read_dir` 10,000 条目 | <10ms |
| `fuzzy_match` 500 条目 × 20 字符 | 微秒级 |
| dir_cache 命中 | 零 IO，微秒级 |

全部同步调用，穷尽路径在 60fps（~16ms）内。不需要后台线程。

## TUI 交互

### update_at_mention_detection() 化简

```
旧: detect → activate → start_search → poll → update_candidates
新: detect → resolve_dir → dir_cache.get_or_read() → fuzzy → candidates
```

同步完成，不再跨帧。

### inject_at_mention_path() 调整

```rust
let path_text = format!("{}/{}", at.dir_part, candidate.name.to_string_lossy());
let injected = if candidate.is_dir {
    format!("@{}", path_text) // 尾部自动 / → 触发下级补全
} else {
    format!("@{}", path_text)
};
// 含空格路径: format!("@\"{}\"", path_text)
```

### 键盘事件不变化

Esc（关闭）、Up/Down（导航）、Tab/Enter（选择注入）逻辑不变，只改内部数据源。

### Popup 渲染

空候选时：
- `candidates.is_empty() && query_part.is_empty()` → 显示 "(empty directory)"
- `candidates.is_empty() && !query_part.is_empty()` → 显示 "(no matches)"

## 边界情况

| 场景 | 行为 |
|------|------|
| `@` 无输入 | 列出 cwd 顶级所有条目（目录优先） |
| `@nonexistent` | cwd 下无匹配 → "(no matches)" |
| `@side-projects/nonex/sub` | `nonex/` 不存在 → resolve_dir 回退到 `side-projects/`，整个 `"nonex/sub"` 做 fuzzy query |
| `@` + cwd 空目录 | "(empty directory)" |
| `@path` + 路径不存在 + 所有上级也不存在 | 回退到 cwd，完整文本做 query |
| `@` 前面是单词字符 | `detect()` 不触发（现有逻辑） |
| 目录权限拒绝 | read_dir 返回空，显示 "(permission denied)" |
| symlink 指向目录 | `file_type` 自动 resolve |
| cwd 变更 | 清空 `dir_cache` |
| 选择目录条目 | 注入 `@path/`（尾 `/`），立即触发下级补全 |
| 选择文件条目 | 注入 `@path`，关闭弹窗 |
| 路径含空格 | `read_dir` 的 OsString 原生包含空格；注入时 `@"path"` 包裹 |
| Unicode 文件名 | `OsString::to_string_lossy()` + `SkimMatcherV2`；基础可用，未穷举测试 |

## 测试策略

### 新增测试

| 测试名 | 被测函数 | 场景 |
|------|------|------|
| `test_read_dir_entries` | `read_dir_entries()` | 临时目录 10 条目，验证 Entry 正确 |
| `test_read_dir_skips_hidden` | `read_dir_entries()` | `.git`、`.DS_Store` 不出现在结果 |
| `test_fuzzy_match_entries` | `fuzzy_match_entries()` | 构造 entries，验证 score 排序、目录优先 |
| `test_fuzzy_match_empty_query` | `fuzzy_match_entries()` | query="" 返回全部，目录优先 |
| `test_resolve_dir_exact` | `resolve_dir()` | 路径存在时精确匹配 |
| `test_resolve_dir_fallback` | `resolve_dir()` | 路径不存在时回退，fallback 内容正确 |
| `test_dir_cache_hit` | `AtMentionState::refresh_candidates()` | 同目录第二次不调 read_dir |
| `test_dir_cache_cwd_invalidated` | `AtMentionState` | set_cwd 后清空缓存 |
| `test_detect_at_with_slash` | `AtMentionState::detect()` | `@side-projects/` 正确提取 query |
| `test_refresh_empty_dir` | `AtMentionState::refresh_candidates()` | 空目录显示 "(empty directory)" |
| `test_refresh_no_match` | `AtMentionState::refresh_candidates()` | query 无匹配，显示 "(no matches)" |

### 删除测试

| 测试名 | 原因 |
|------|------|
| `test_search_thread_idle_exit` | 搜索线程已删除 |

### 保留测试

- `AtMentionState::detect()` 现有测试（无 @、仅 @、email 跳过快照）
- 弹窗导航 move_up/move_down 测试
- `AtMentionMiddleware` 测试（不变）
- `extract_at_mentions` 测试（不变）

### 未覆盖

- symlink 循环（OS `read_dir` 行为）
- 50,000+ 条目大目录（场景罕见，不做分页）
- Unicode 文件名模糊匹配准确性（依赖 `SkimMatcherV2` 实现）

## 不变部分

- `peri-middlewares/src/at_mention/`：Parser、file_reader、AtMentionMiddleware
- `peri-acp/src/agent/builder.rs`：Middleware 注册
- `peri-tui/src/app/ui_state.rs`：`AtMentionState` 字段名不变
- `peri-tui/src/ui/main_ui/mod.rs`：渲染调用不变
- 键盘事件结构：Esc / Up / Down / Tab / Enter 逻辑不变

## 参考

- 原始 issue: `spec/global/domains/tui.md#issue_2026-05-31-at-mention-blocking-glob-search`
- 现有设计: `docs/superpowers/specs/2026-05-25-at-mention-design.md`
- 实现计划: `docs/superpowers/plans/2026-05-25-at-mention.md`
- 性能优化计划: `docs/superpowers/plans/2026-05-31-at-mention-process-model.md`
