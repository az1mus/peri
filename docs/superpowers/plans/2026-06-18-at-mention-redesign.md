# @ Mention 重新设计：Hierarchical Fuzzy Completion 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 @ 文件补全从"全局 walkdir + fuzzy"改为"按目录层级 read_dir + fuzzy"，行为类似 terminal Tab 补全。

**Architecture:** 每次按键触发单目录 `fs::read_dir()` + `SkimMatcherV2` 模糊匹配，结果同步返回（<1ms 典型）。`HashMap<PathBuf, Vec<Entry>>` 缓存目录条目，cwd 变更时清空。删除搜索线程/mpsc/节流逻辑。

**Tech Stack:** `fuzzy-matcher` (SkimMatcherV2)、`std::fs::read_dir`、`HashMap`。**移除 `walkdir` 依赖**。

---

## 文件结构

| 文件 | 操作 | 职责 |
|------|:--:|------|
| `peri-tui/src/app/at_mention/file_search.rs` | **重写** | `Entry` 类型、`read_dir_entries()`、`fuzzy_match_entries()`、`parse_at_query()`、`resolve_dir()` |
| `peri-tui/src/app/at_mention/mod.rs` | **重写** | 精简 `AtMentionState`：删除线程/mpsc/节流逻辑，新增 `dir_cache`，同步 `refresh_candidates()` |
| `peri-tui/src/event/keyboard.rs` | **修改** | `update_at_mention_detection` 化简为同步调用；`inject_at_mention_path` 目录条目追加尾 `/` |
| `peri-tui/src/app/agent_ops/polling.rs` | **修改** | `poll_at_mention` 改为 no-op |
| `peri-tui/src/main.rs` | **修改** | 移除 `app.poll_at_mention()` 调用 |
| `peri-tui/src/app/at_mention/popup.rs` | **修改** | 空状态显示 "(empty directory)" / "(no matches)" |
| `peri-tui/Cargo.toml` | **修改** | 移除 `walkdir` 依赖 |
| `peri-middlewares/src/at_mention/` | **不变** | Middleware 层无变更 |

---

### Task 1: 重写 file_search.rs

**Files:**
- Modify: `peri-tui/src/app/at_mention/file_search.rs`
- Remove dep: `peri-tui/Cargo.toml` (walkdir)

- [ ] **Step 1: 移除 walkdir 依赖**

在 `peri-tui/Cargo.toml` 中删除 `walkdir = "2.5"` 行。

- [ ] **Step 2: 编写测试 — read_dir_entries**

在 `file_search.rs` 底部 `#[cfg(test)] mod tests {` 中先用代码块分离出新增测试（整个 `file_search.rs` 作为单独文件替换时有测试）：

```rust
#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::tempdir;
    use super::*;

    #[test]
    fn test_read_dir_entries_basic() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::write(base.join("main.rs"), "").unwrap();
        fs::write(base.join("lib.rs"), "").unwrap();
        fs::create_dir(base.join("src")).unwrap();

        let entries = read_dir_entries(base);
        let names: Vec<&str> = entries.iter().map(|e| e.name.to_str().unwrap()).collect();
        // 目录优先排序
        assert!(names.contains(&"src"));
        assert!(names.contains(&"main.rs"));
        assert!(names.contains(&"lib.rs"));
        // src 应在 main.rs 前面（is_dir 优先）
        let src_pos = entries.iter().position(|e| e.name == "src").unwrap();
        let main_pos = entries.iter().position(|e| e.name == "main.rs").unwrap();
        assert!(src_pos < main_pos, "目录应排在文件前面");
    }

    #[test]
    fn test_read_dir_skips_hidden() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "").unwrap();
        fs::write(dir.path().join("visible.rs"), "").unwrap();

        let entries = read_dir_entries(dir.path());
        let names: Vec<&str> = entries.iter().map(|e| e.name.to_str().unwrap()).collect();
        assert!(!names.iter().any(|n| n.starts_with('.')), "应跳过隐藏文件");
        assert!(names.contains(&"visible.rs"));
    }

    #[test]
    fn test_fuzzy_match_entries_with_query() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main_logic.rs"), "").unwrap();
        fs::write(dir.path().join("util_test.rs"), "").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let entries = read_dir_entries(dir.path());

        let results = fuzzy_match_entries(&entries, "main");
        assert!(!results.is_empty(), "应匹配到 main_logic.rs");
        assert!(results.iter().any(|e| e.name == "main_logic.rs"));
    }

    #[test]
    fn test_fuzzy_match_entries_empty_query() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("readme.md"), "").unwrap();
        let entries = read_dir_entries(dir.path());

        let results = fuzzy_match_entries(&entries, "");
        assert_eq!(results.len(), 2);
        // 目录优先
        assert!(results[0].is_dir, "第一个条目应为目录");
    }

    #[test]
    fn test_resolve_dir_exact_match() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::create_dir_all(base.join("side-projects/git-stats/src")).unwrap();

        let (resolved, fallback) = resolve_dir(base, "side-projects/git-stats/");
        assert!(resolved.ends_with("side-projects/git-stats"));
        assert_eq!(fallback, "");
    }

    #[test]
    fn test_resolve_dir_fallback() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::create_dir_all(base.join("side-projects")).unwrap();

        let (resolved, fallback) = resolve_dir(base, "side-projects/nonex/sr");
        assert!(resolved.ends_with("side-projects"));
        assert_eq!(fallback, "nonex/sr");
    }

    #[test]
    fn test_resolve_dir_all_nonexistent() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        let (resolved, fallback) = resolve_dir(base, "nonex/sub");
        assert_eq!(resolved, base);
        assert_eq!(fallback, "nonex/sub");
    }

    #[test]
    fn test_parse_at_query_simple() {
        let (dir_part, query_part) = parse_at_query("side-projects/git-stats/sr").unwrap();
        assert_eq!(dir_part, "side-projects/git-stats/");
        assert_eq!(query_part, "sr");
    }

    #[test]
    fn test_parse_at_query_no_slash() {
        let (dir_part, query_part) = parse_at_query("sr").unwrap();
        assert_eq!(dir_part, "");
        assert_eq!(query_part, "sr");
    }

    #[test]
    fn test_parse_at_query_trailing_slash() {
        let (dir_part, query_part) = parse_at_query("side-projects/").unwrap();
        assert_eq!(dir_part, "side-projects/");
        assert_eq!(query_part, "");
    }
}
```

- [ ] **Step 3: 运行测试验证失败**

```bash
cargo test -p peri-tui --lib -- at_mention::file_search 2>&1 | tail -20
```
Expected: 编译错误 / 测试 FAIL（`read_dir_entries`、`fuzzy_match_entries` 等函数不存在）

- [ ] **Step 4: 写入 file_search.rs 完整实现**

**完整替换** `peri-tui/src/app/at_mention/file_search.rs`：

```rust
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use fuzzy_matcher::{skim::SkimMatcherV2, FuzzyMatcher};

/// 文件系统条目
#[derive(Clone, Debug)]
pub struct Entry {
    pub name: OsString,
    pub is_dir: bool,
    pub is_symlink: bool,
}

/// 查询当前目录下的条目（跳过隐藏文件，目录优先排序）
pub fn read_dir_entries(dir: &Path) -> Vec<Entry> {
    let rd = match std::fs::read_dir(dir) {
        Ok(rd) => rd,
        Err(_) => return Vec::new(),
    };

    let mut entries: Vec<Entry> = rd
        .flatten()
        .filter(|e| {
            let name = e.file_name();
            let name_str = name.to_string_lossy();
            !name_str.starts_with('.')
        })
        .filter_map(|e| {
            let ft = e.file_type().ok()?;
            Some(Entry {
                name: e.file_name(),
                is_dir: ft.is_dir(),
                is_symlink: ft.is_symlink(),
            })
        })
        .collect();

    // 目录优先，其次按名字排序
    entries.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then_with(|| a.name.cmp(&b.name))
    });

    entries
}

/// 对条目列表做模糊匹配，返回匹配到的条目引用（保持排序）
pub fn fuzzy_match_entries<'a>(entries: &'a [Entry], query: &str) -> Vec<&'a Entry> {
    if query.is_empty() {
        return entries.iter().collect();
    }

    let matcher = SkimMatcherV2::default();
    let mut scored: Vec<(&Entry, i64)> = entries
        .iter()
        .filter_map(|e| {
            let name_str = e.name.to_string_lossy();
            let score = matcher.fuzzy_match(&name_str, query).unwrap_or(0);
            if score > 0 {
                Some((e, score))
            } else {
                None
            }
        })
        .collect();

    // score 降序 → 目录优先 → name 长度升序
    scored.sort_by(|(a, sa), (b, sb)| {
        sb.cmp(sa)
            .then_with(|| b.is_dir.cmp(&a.is_dir))
            .then_with(|| a.name.len().cmp(&b.name.len()))
    });

    scored.into_iter().map(|(e, _)| e).collect()
}

/// 解析 @ 后的 query，拆分为目录部分和搜索部分
/// 返回 (dir_part, query_part)。不处理回退逻辑。
pub fn parse_at_query(query: &str) -> Option<(String, String)> {
    if query.is_empty() {
        return None;
    }
    if let Some(slash_pos) = query.rfind('/') {
        let dir_part = query[..=slash_pos].to_string();
        let query_part = query[slash_pos + 1..].to_string();
        Some((dir_part, query_part))
    } else {
        Some((String::new(), query.to_string()))
    }
}

/// 解析目录路径，不存在时向上回退到最近存在的目录
/// 返回 (resolved_dir, fallback_query)。
/// fallback_query 是回退掉的不存在部分，需与 query_part 合并做 fuzzy。
pub fn resolve_dir(cwd: &Path, dir_part: &str) -> (PathBuf, String) {
    let mut curr = cwd.join(dir_part);
    let mut fallback = String::new();

    while !curr.is_dir() {
        if let Some(name) = curr.file_name() {
            let name_str = name.to_string_lossy();
            if fallback.is_empty() {
                fallback = name_str;
            } else {
                fallback = format!("{}/{}", name_str, fallback);
            }
        }
        if let Some(parent) = curr.parent() {
            curr = parent.to_path_buf();
        } else {
            // 回退到 cwd
            let cwd_owned = cwd.to_path_buf();
            if !cwd_owned.is_dir() {
                return (cwd_owned, fallback);
            }
            curr = cwd_owned;
            break;
        }
    }

    (curr, fallback)
}

#[cfg(test)]
mod tests {
    use std::fs;
    use tempfile::tempdir;
    use super::*;

    #[test]
    fn test_read_dir_entries_basic() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::write(base.join("main.rs"), "").unwrap();
        fs::write(base.join("lib.rs"), "").unwrap();
        fs::create_dir(base.join("src")).unwrap();

        let entries = read_dir_entries(base);
        let names: Vec<&str> = entries.iter().map(|e| e.name.to_str().unwrap()).collect();
        assert!(names.contains(&"src"));
        assert!(names.contains(&"main.rs"));
        assert!(names.contains(&"lib.rs"));
        // src 应在 main.rs 前面（is_dir 优先）
        let src_pos = entries.iter().position(|e| e.name == "src").unwrap();
        let main_pos = entries.iter().position(|e| e.name == "main.rs").unwrap();
        assert!(src_pos < main_pos, "目录应排在文件前面");
    }

    #[test]
    fn test_read_dir_skips_hidden() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join(".gitignore"), "").unwrap();
        fs::write(dir.path().join("visible.rs"), "").unwrap();

        let entries = read_dir_entries(dir.path());
        let names: Vec<&str> = entries.iter().map(|e| e.name.to_str().unwrap()).collect();
        assert!(!names.iter().any(|n| n.starts_with('.')), "应跳过隐藏文件");
        assert!(names.contains(&"visible.rs"));
    }

    #[test]
    fn test_fuzzy_match_entries_with_query() {
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("main_logic.rs"), "").unwrap();
        fs::write(dir.path().join("util_test.rs"), "").unwrap();
        fs::write(dir.path().join("Cargo.toml"), "").unwrap();
        let entries = read_dir_entries(dir.path());

        let results = fuzzy_match_entries(&entries, "main");
        assert!(!results.is_empty(), "应匹配到 main_logic.rs");
        assert!(results.iter().any(|e| e.name == "main_logic.rs"));
    }

    #[test]
    fn test_fuzzy_match_entries_empty_query() {
        let dir = tempdir().unwrap();
        fs::create_dir(dir.path().join("src")).unwrap();
        fs::write(dir.path().join("readme.md"), "").unwrap();
        let entries = read_dir_entries(dir.path());

        let results = fuzzy_match_entries(&entries, "");
        assert_eq!(results.len(), 2);
        assert!(results[0].is_dir, "第一个条目应为目录");
    }

    #[test]
    fn test_resolve_dir_exact_match() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::create_dir_all(base.join("side-projects/git-stats/src")).unwrap();

        let (resolved, fallback) = resolve_dir(base, "side-projects/git-stats/");
        assert!(resolved.ends_with("side-projects/git-stats"));
        assert_eq!(fallback, "");
    }

    #[test]
    fn test_resolve_dir_fallback() {
        let dir = tempdir().unwrap();
        let base = dir.path();
        fs::create_dir_all(base.join("side-projects")).unwrap();

        let (resolved, fallback) = resolve_dir(base, "side-projects/nonex/sr");
        assert!(resolved.ends_with("side-projects"));
        assert_eq!(fallback, "nonex/sr");
    }

    #[test]
    fn test_resolve_dir_all_nonexistent() {
        let dir = tempdir().unwrap();
        let base = dir.path();

        let (resolved, fallback) = resolve_dir(base, "nonex/sub");
        assert_eq!(resolved, base);
        assert_eq!(fallback, "nonex/sub");
    }

    #[test]
    fn test_parse_at_query_simple() {
        let (dir_part, query_part) = parse_at_query("side-projects/git-stats/sr").unwrap();
        assert_eq!(dir_part, "side-projects/git-stats/");
        assert_eq!(query_part, "sr");
    }

    #[test]
    fn test_parse_at_query_no_slash() {
        let (dir_part, query_part) = parse_at_query("sr").unwrap();
        assert_eq!(dir_part, "");
        assert_eq!(query_part, "sr");
    }

    #[test]
    fn test_parse_at_query_trailing_slash() {
        let (dir_part, query_part) = parse_at_query("side-projects/").unwrap();
        assert_eq!(dir_part, "side-projects/");
        assert_eq!(query_part, "");
    }
}
```

- [ ] **Step 5: 运行测试验证通过**

```bash
cargo test -p peri-tui --lib -- at_mention::file_search 2>&1 | tail -20
```
Expected: all tests PASS

- [ ] **Step 6: Commit**

```bash
git add peri-tui/src/app/at_mention/file_search.rs peri-tui/Cargo.toml
git commit -m "refactor(at-mention): rewrite file_search with hierarchical read_dir + fuzzy, remove walkdir"
```

---

### Task 2: 重写 mod.rs — 精简 AtMentionState

**Files:**
- Modify: `peri-tui/src/app/at_mention/mod.rs`

- [ ] **Step 1: 编写测试 — dir_cache 行为**

先在 `mod.rs` 底部 `#[cfg(test)] mod tests {` 中追加新测试（后续完整替换文件时保留这些测试）：

```rust
#[test]
fn test_dir_cache_hit() {
    let mut state = AtMentionState::new();
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::write(base.join("test.rs"), "").unwrap();
    state.set_cwd(base.to_string_lossy().to_string());

    // 首次刷新：应该 load
    state.refresh_candidates();
    assert!(!state.candidates.is_empty());
    assert!(state.dir_cache.contains_key(base));

    let first_count = state.candidates.len();

    // 新增文件后再次刷新：缓存命中，不应该看到新文件
    std::fs::write(base.join("new_file.rs"), "").unwrap();
    state.refresh_candidates();
    assert_eq!(state.candidates.len(), first_count, "缓存命中，不应看到新文件");
}

#[test]
fn test_dir_cache_cwd_invalidated() {
    let mut state = AtMentionState::new();
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::write(base.join("a.rs"), "").unwrap();
    state.set_cwd(base.to_string_lossy().to_string());
    state.refresh_candidates();
    assert!(state.dir_cache.contains_key(base));

    let dir2 = tempfile::tempdir().unwrap();
    state.set_cwd(dir2.path().to_string_lossy().to_string());
    assert!(state.dir_cache.is_empty(), "cwd 变更后缓存应清空");
}

#[test]
fn test_refresh_empty_dir() {
    let mut state = AtMentionState::new();
    let dir = tempfile::tempdir().unwrap();
    state.set_cwd(dir.path().to_string_lossy().to_string());
    state.activate("@".to_string(), 0);
    state.refresh_candidates();
    assert!(state.candidates.is_empty());
    assert_eq!(state.empty_message, Some("(empty directory)".to_string()));
}

#[test]
fn test_refresh_no_match() {
    let mut state = AtMentionState::new();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("readme.md"), "").unwrap();
    state.set_cwd(dir.path().to_string_lossy().to_string());
    state.activate("@nonexistent".to_string(), 0);
    state.refresh_candidates();
    assert!(state.candidates.is_empty());
    assert_eq!(state.empty_message, Some("(no matches)".to_string()));
}
```

- [ ] **Step 2: 运行测试验证失败**

```bash
cargo test -p peri-tui --lib -- at_mention 2>&1 | tail -30
```
Expected: 新增测试编译错误（`refresh_candidates`、`dir_cache`、`empty_message` 不存在）

- [ ] **Step 3: 写入 mod.rs 完整替换**

**完整替换** `peri-tui/src/app/at_mention/mod.rs`（保留原有 detect / move_up / move_down / adjust_scroll / selected_candidate 方法及其测试）：

```rust
pub mod file_search;
pub mod popup;

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use file_search::Entry;

/// @ 提及状态：管理文件搜索候选、选择和弹窗
pub struct AtMentionState {
    pub active: bool,
    pub query: String,
    /// @ 符号在文本中的字符位置
    pub query_start: usize,
    pub candidates: Vec<Entry>,
    pub selected: usize,
    pub scroll_offset: usize,
    /// 当前解析出的目录部分
    dir_part: String,
    /// 工作目录
    cwd: PathBuf,
    /// 目录条目缓存：PathBuf → Vec<Entry>
    dir_cache: HashMap<PathBuf, Vec<Entry>>,
    /// 空列表占位消息："(empty directory)" / "(no matches)" / None
    pub empty_message: Option<String>,
}

impl Default for AtMentionState {
    fn default() -> Self {
        Self::new()
    }
}

impl AtMentionState {
    pub fn new() -> Self {
        Self {
            active: false,
            query: String::new(),
            query_start: 0,
            candidates: Vec::new(),
            selected: 0,
            scroll_offset: 0,
            dir_part: String::new(),
            cwd: PathBuf::new(),
            dir_cache: HashMap::new(),
            empty_message: None,
        }
    }

    /// 惰性设置 cwd（仅在变更时重新设置）
    pub fn set_cwd(&mut self, cwd: String) {
        let new_path = PathBuf::from(&cwd);
        if self.cwd != new_path {
            self.dir_cache.clear();
            self.cwd = new_path;
        }
    }

    /// 确保 cwd 已设置（惰性初始化，仅设置一次）
    pub fn ensure_cwd(&mut self, cwd: String) {
        let new_path = PathBuf::from(&cwd);
        if self.cwd.as_os_str().is_empty() {
            self.set_cwd(cwd);
        }
    }

    /// detect 保持不变：从文本和光标位置检测 @ 提及
    pub fn detect(text: &str, cursor_pos: usize) -> Option<(String, usize)> {
        if cursor_pos == 0 || cursor_pos > text.len() {
            return None;
        }
        let before_cursor = &text[..cursor_pos];
        let at_pos = before_cursor.rfind('@')?;
        let query = &before_cursor[at_pos + '@'.len_utf8()..];
        if query.is_empty() {
            return None;
        }
        if at_pos > 0 {
            let char_before = before_cursor[..at_pos].chars().next_back().unwrap();
            if !char_before.is_whitespace() && char_before != '\n' {
                return None;
            }
        }
        Some((query.to_string(), at_pos))
    }

    pub fn activate(&mut self, query: String, query_start: usize) {
        self.active = true;
        self.query = query;
        self.query_start = query_start;
        self.selected = 0;
        self.scroll_offset = 0;
        self.empty_message = None;
    }

    pub fn close(&mut self) {
        self.active = false;
        self.query.clear();
        self.candidates.clear();
        self.selected = 0;
        self.scroll_offset = 0;
        self.dir_part.clear();
        self.empty_message = None;
    }

    /// 同步刷新候选列表：解析 query → resolve_dir → read_dir (cached) → fuzzy_match
    pub fn refresh_candidates(&mut self) {
        self.empty_message = None;

        if self.query.is_empty() {
            return;
        }

        let (dir_part, query_part) = match file_search::parse_at_query(&self.query) {
            Some(v) => v,
            None => return,
        };

        // 解析目录，不存在时回退
        let (resolved_dir, fallback) = file_search::resolve_dir(&self.cwd, &dir_part);

        // 存储解析后的相对目录路径（resolved → relative to cwd），回退场景下用 resolved 路径
        let rel_resolved = resolved_dir
            .strip_prefix(&self.cwd)
            .unwrap_or(&resolved_dir);
        let mut resolved_prefix = rel_resolved.to_string_lossy().to_string();
        if !resolved_prefix.is_empty() && !resolved_prefix.ends_with('/') {
            resolved_prefix.push('/');
        }
        self.dir_part = resolved_prefix;

        // 合并 fallback 和 query_part 作为 fuzzy query
        let effective_query = if fallback.is_empty() {
            query_part.as_str()
        } else if query_part.is_empty() {
            fallback.as_str()
        } else {
            return self.refresh_candidates_with_query(&resolved_dir, &format!("{}/{}", fallback, query_part));
        };

        self.refresh_candidates_with_query(&resolved_dir, effective_query);
    }

    fn refresh_candidates_with_query(&mut self, dir: &Path, query: &str) {
        let entries = self.get_or_read_dir(dir);

        if entries.is_empty() {
            self.candidates.clear();
            self.empty_message = Some("(empty directory)".to_string());
            return;
        }

        let matched = file_search::fuzzy_match_entries(entries, query);

        if matched.is_empty() && !query.is_empty() {
            self.candidates.clear();
            self.empty_message = Some("(no matches)".to_string());
            return;
        }

        self.candidates = matched.into_iter().cloned().collect();
        if self.selected >= self.candidates.len() && !self.candidates.is_empty() {
            self.selected = self.candidates.len() - 1;
        }
    }

    fn get_or_read_dir(&mut self, dir: &Path) -> &Vec<Entry> {
        if !self.dir_cache.contains_key(dir) {
            let entries = file_search::read_dir_entries(dir);
            self.dir_cache.insert(dir.to_path_buf(), entries);
        }
        self.dir_cache.get(dir).unwrap()
    }

    pub fn move_up(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        if self.selected > 0 {
            self.selected -= 1;
        } else {
            self.selected = self.candidates.len() - 1;
        }
        self.adjust_scroll();
    }

    pub fn move_down(&mut self) {
        if self.candidates.is_empty() {
            return;
        }
        if self.selected < self.candidates.len() - 1 {
            self.selected += 1;
        } else {
            self.selected = 0;
        }
        self.adjust_scroll();
    }

    pub fn adjust_scroll(&mut self) {
        let viewport = popup::MAX_VIEWPORT.min(self.candidates.len());
        if viewport == 0 {
            self.scroll_offset = 0;
            return;
        }
        if self.selected < self.scroll_offset {
            self.scroll_offset = self.selected;
        } else if self.selected >= self.scroll_offset + viewport {
            self.scroll_offset = self.selected - viewport + 1;
        }
    }

    pub fn selected_candidate(&self) -> Option<&Entry> {
        self.candidates.get(self.selected)
    }

    /// 获取当前选中的完整路径字符串（用于注入）
    pub fn selected_path(&self) -> Option<String> {
        let entry = self.selected_candidate()?;
        let name = entry.name.to_string_lossy();
        if self.dir_part.is_empty() {
            Some(name.to_string())
        } else {
            Some(format!("{}{}", self.dir_part, name))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;
    use std::fs;

    use super::*;

    // ===== 保留的原有测试 =====

    #[test]
    fn test_detect_at_sign_with_text() {
        let text = "请看 @main";
        let result = AtMentionState::detect(text, text.len());
        assert!(result.is_some(), "应检测到 @ 提及");
        let (query, pos) = result.unwrap();
        assert_eq!(query, "main");
        assert_eq!(pos, "请看 ".len());
    }

    #[test]
    fn test_detect_no_at_sign() {
        let result = AtMentionState::detect("hello world", "hello world".len());
        assert!(result.is_none(), "无 @ 应返回 None");
    }

    #[test]
    fn test_detect_at_sign_only() {
        let result = AtMentionState::detect("看 @", "看 @".len());
        assert!(result.is_none(), "@ 后无内容应返回 None");
    }

    #[test]
    fn test_detect_path_with_slash() {
        let text = "看 @src/main";
        let result = AtMentionState::detect(text, text.len());
        assert!(result.is_some());
        let (query, _) = result.unwrap();
        assert_eq!(query, "src/main");
    }

    #[test]
    fn test_detect_not_at_line_start() {
        let result = AtMentionState::detect("user@example", "user@example".len());
        assert!(result.is_none(), "非空白前导的 @ 不应触发");
    }

    #[test]
    fn test_move_up_down() {
        let mut state = AtMentionState::new();
        state.active = true;
        state.candidates = vec![
            Entry { name: "a.rs".into(), is_dir: false, is_symlink: false },
            Entry { name: "b.rs".into(), is_dir: false, is_symlink: false },
            Entry { name: "c.rs".into(), is_dir: false, is_symlink: false },
        ];
        assert_eq!(state.selected, 0);
        state.move_down();
        assert_eq!(state.selected, 1);
        state.move_down();
        assert_eq!(state.selected, 2);
        state.move_down();
        assert_eq!(state.selected, 0);
        state.move_up();
        assert_eq!(state.selected, 2);
    }

    #[test]
    fn test_ensure_cwd_sets_once() {
        let mut state = AtMentionState::new();
        assert!(state.cwd.as_os_str().is_empty());
        state.ensure_cwd("/tmp".to_string());
        assert_eq!(state.cwd, PathBuf::from("/tmp"));
        state.ensure_cwd("/other".to_string());
        assert_eq!(state.cwd, PathBuf::from("/tmp"), "ensure_cwd 只设置一次");
    }

    // ===== 新增 dir_cache 测试 =====

    #[test]
    fn test_dir_cache_hit() {
        let mut state = AtMentionState::new();
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        fs::write(base.join("test.rs"), "").unwrap();
        state.set_cwd(base.to_string_lossy().to_string());
        state.activate("@".to_string(), 0);

        // 首次刷新：应该 load
        state.refresh_candidates();
        assert!(!state.candidates.is_empty());
        assert!(state.dir_cache.contains_key(base));

        let first_count = state.candidates.len();

        // 新增文件后再次刷新：缓存命中，不应该看到新文件
        fs::write(base.join("new_file.rs"), "").unwrap();
        state.refresh_candidates();
        assert_eq!(state.candidates.len(), first_count, "缓存命中，不应看到新文件");
    }

    #[test]
    fn test_dir_cache_cwd_invalidated() {
        let mut state = AtMentionState::new();
        let dir = tempfile::tempdir().unwrap();
        let base = dir.path();
        fs::write(base.join("a.rs"), "").unwrap();
        state.set_cwd(base.to_string_lossy().to_string());
        state.activate("@".to_string(), 0);
        state.refresh_candidates();
        assert!(state.dir_cache.contains_key(base));

        let dir2 = tempfile::tempdir().unwrap();
        state.set_cwd(dir2.path().to_string_lossy().to_string());
        assert!(state.dir_cache.is_empty(), "cwd 变更后缓存应清空");
    }

    #[test]
    fn test_refresh_empty_dir() {
        let mut state = AtMentionState::new();
        let dir = tempfile::tempdir().unwrap();
        state.set_cwd(dir.path().to_string_lossy().to_string());
        state.activate("@".to_string(), 0);
        state.refresh_candidates();
        assert!(state.candidates.is_empty());
        assert_eq!(state.empty_message, Some("(empty directory)".to_string()));
    }

    #[test]
    fn test_refresh_no_match() {
        let mut state = AtMentionState::new();
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("readme.md"), "").unwrap();
        state.set_cwd(dir.path().to_string_lossy().to_string());
        state.activate("@nonexistent".to_string(), 0);
        state.refresh_candidates();
        assert!(state.candidates.is_empty());
        assert_eq!(state.empty_message, Some("(no matches)".to_string()));
    }
}
```

- [ ] **Step 4: 运行测试验证通过**

```bash
cargo test -p peri-tui --lib -- at_mention 2>&1 | tail -30
```
Expected: 所有测试 PASS

- [ ] **Step 5: Commit**

```bash
git add peri-tui/src/app/at_mention/mod.rs
git commit -m "refactor(at-mention): simplify AtMentionState — remove search thread, mpsc, throttle; add dir_cache + sync refresh"
```

---

### Task 3: 更新 keyboard.rs — update_at_mention_detection + inject_at_mention_path

**Files:**
- Modify: `peri-tui/src/event/keyboard.rs:164-286`

- [ ] **Step 1: 替换 update_at_mention_detection**

将 `peri-tui/src/event/keyboard.rs` 第 164-204 行替换为：

```rust
pub(super) fn update_at_mention_detection(app: &mut App) {
    let textarea = &app.session_mgr.current_mut().ui.textarea;
    let text = textarea.lines().join("\n");
    let (row, col) = textarea.cursor();
    let mut pos = 0usize;
    for (i, line) in textarea.lines().iter().enumerate() {
        if i == row {
            pos += line.chars().take(col).map(|c| c.len_utf8()).sum::<usize>();
            break;
        }
        pos += line.len() + 1;
    }

    let at = &mut app.session_mgr.current_mut().ui.at_mention;
    at.ensure_cwd(app.services.cwd.clone());

    if let Some((query, start)) = crate::app::AtMentionState::detect(&text, pos) {
        if at.active && at.query == query {
            return; // 未变化
        }
        at.activate(query.clone(), start);
        // 同步刷新候选列表
        at.refresh_candidates();
    } else if at.active {
        at.close();
    }
}
```

- [ ] **Step 2: 替换 inject_at_mention_path**

将 `peri-tui/src/event/keyboard.rs` 第 246-287 行替换为：

```rust
pub(super) fn inject_at_mention_path(app: &mut App) {
    let at = &app.session_mgr.current_mut().ui.at_mention;
    let path = match at.selected_path() {
        Some(p) => p,
        None => return,
    };
    let is_dir = at.selected_candidate().is_some_and(|e| e.is_dir);
    let query_start = at.query_start;
    let query_len = at.query.len();

    let textarea = &app.session_mgr.current_mut().ui.textarea;
    let full_text: String = textarea.lines().join("\n");

    let needs_quotes = path.contains(' ');
    let replacement = if needs_quotes {
        format!("@\"{}\"", path)
    } else {
        format!("@{}", path)
    };

    let mut new_text = String::with_capacity(full_text.len() + replacement.len());
    new_text.push_str(&full_text[..query_start]);
    new_text.push_str(&replacement);
    let after_end = query_start + 1 + query_len;
    if after_end < full_text.len() {
        new_text.push_str(&full_text[after_end..]);
    }

    let mut new_ta = crate::app::build_textarea(false);
    new_ta.insert_str(&new_text);
    app.session_mgr.current_mut().ui.textarea = new_ta;

    if is_dir {
        app.session_mgr.current_mut().ui.textarea.insert_str("/");
        update_at_mention_detection(app);
    } else {
        app.session_mgr.current_mut().ui.textarea.insert_str(" ");
        app.session_mgr.current_mut().ui.at_mention.close();
    }
}
```

- [ ] **Step 3: 编译验证**

```bash
cargo build -p peri-tui 2>&1 | tail -20
```
Expected: 编译成功

- [ ] **Step 4: Commit**

```bash
git add peri-tui/src/event/keyboard.rs
git commit -m "refactor(at-mention): simplify keyboard handlers — sync refresh, directory path injection"
```

---

### Task 4: 更新 polling.rs — poll_at_mention 改为 no-op

**Files:**
- Modify: `peri-tui/src/app/agent_ops/polling.rs:252-259`

- [ ] **Step 1: 替换 poll_at_mention**

将 `peri-tui/src/app/agent_ops/polling.rs` 第 252-259 行替换为：

```rust
    /// 每帧调用：@ mention 已改为同步刷新，此处为 no-op（保留接口兼容）
    pub fn poll_at_mention(&mut self) -> bool {
        false
    }
```

- [ ] **Step 2: 编译验证**

```bash
cargo build -p peri-tui 2>&1 | tail -10
```
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/app/agent_ops/polling.rs
git commit -m "refactor(at-mention): make poll_at_mention a no-op"
```

---

### Task 5: 更新 main.rs — 移除 poll_at_mention 调用

**Files:**
- Modify: `peri-tui/src/main.rs:833`

- [ ] **Step 1: 删除 poll_at_mention 调用**

删除 `peri-tui/src/main.rs` 第 833 行：

```rust
        agent_updated |= app.poll_at_mention();
```

- [ ] **Step 2: 编译验证**

```bash
cargo build -p peri-tui 2>&1 | tail -10
```
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/main.rs
git commit -m "refactor(at-mention): remove poll_at_mention call from main loop"
```

---

### Task 6: 更新 popup.rs — 空状态占位显示

**Files:**
- Modify: `peri-tui/src/app/at_mention/popup.rs:18-21`

- [ ] **Step 1: 修改渲染逻辑**

将 `render_at_mention_popup` 函数开头的保护子句（第 19-21 行）：

```rust
    if !state.active || state.candidates.is_empty() {
        return;
    }
```

替换为：

```rust
    if !state.active {
        return;
    }

    // 空目录 / 无匹配时显示占位消息
    if state.candidates.is_empty() {
        if let Some(ref msg) = state.empty_message {
            let popup_height = 3u16; // 内容 1 行 + 边框上下
            let y = input_area.y.saturating_sub(popup_height);
            let popup_area = Rect {
                x: input_area.x,
                y,
                width: input_area.width.min(30), // 占位消息窄即可
                height: popup_height,
            };
            let inner = BorderedPanel::new(Span::styled("", Style::default()))
                .border_style(Style::default().fg(theme::BORDER))
                .render(f, popup_area);
            let text = Span::styled(
                msg.as_str(),
                Style::default().fg(theme::TEXT).add_modifier(Modifier::DIM),
            );
            f.render_widget(Paragraph::new(text), inner);
        }
        return;
    }
```

- [ ] **Step 2: 编译验证**

```bash
cargo build -p peri-tui 2>&1 | tail -10
```
Expected: 编译成功

- [ ] **Step 3: Commit**

```bash
git add peri-tui/src/app/at_mention/popup.rs
git commit -m "feat(at-mention): show empty directory/no matches placeholder in popup"
```

---

### Task 7: 全量测试 & 编译验证

- [ ] **Step 1: 运行 @ mention 全量测试**

```bash
cargo test -p peri-tui --lib -- at_mention 2>&1
```
Expected: 所有测试 PASS

- [ ] **Step 2: 运行 peri-tui 全量测试**

```bash
cargo test -p peri-tui --lib 2>&1 | tail -30
```
Expected: 全量 PASS（之前通过的不应回归）

- [ ] **Step 3: 编译全 workspace**

```bash
cargo build 2>&1 | tail -10
```
Expected: 全 workspace 编译成功

- [ ] **Step 4: 删除 walkdir 残留引用**

```bash
grep -r "walkdir" peri-tui/ --include="*.rs" --include="Cargo.toml"
```
Expected: 无输出（peri-tui 下不再引用 walkdir）

- [ ] **Step 5: Commit**

```bash
git add -A
git commit -m "chore(at-mention): verify all tests pass, remove walkdir references"
```

---

## 实施后检查清单

- [ ] `peri-tui` 不再依赖 `walkdir` crate
- [ ] `AtMentionState` 无 `unsafe`、无后台线程、无 `mpsc`
- [ ] `refresh_candidates()` 同步调用，耗时 <1ms（仅 `fs::read_dir`）
- [ ] 目录选择后注入 `@path/`（尾 `/`），自动触发下级补全
- [ ] `dir_cache` 在 `set_cwd` 时正确清空
- [ ] Middleware 层未触碰（`peri-middlewares/src/at_mention/` 零改动）
- [ ] 空目录 → "(empty directory)"，无匹配 → "(no matches)"
- [ ] `detect()` 行为不变（email 不触发、前缀空白检查）
- [ ] `move_up` / `move_down` 行为不变
- [ ] popup `MAX_VIEWPORT=10` 不变
