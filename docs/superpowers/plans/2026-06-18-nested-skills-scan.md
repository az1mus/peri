# Skills 嵌套递归扫描 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 把 3 个分散的 skill 扫描入口（`list_skills` / `extract_skills_paths` / `SkillPreloadMiddleware` 内联）统一收口到 `scan_skill_roots(roots: &[SkillRoot])`，支持嵌套递归（深度上限 6、目录数上限 1000/root、symlink 跟随 + canonicalize 防环、叶子语义）。

**Architecture:** 在 `peri-middlewares::skills::loader` 新增 `SkillSource`/`SkillRoot` 类型和 `scan_skill_roots` 收口函数；`plugin_skill_dirs: Vec<PathBuf>` 字段在 peri-middlewares / peri-acp / peri-tui 三层全链路改为 `plugin_skill_roots: Vec<SkillRoot>`，让 plugin_name 信息从 LoadedPlugin 一路传递到 SkillsMiddleware。

**Tech Stack:** Rust 2021 + tokio + tempfile + gray_matter + std::fs（含 symlink 跟随）

**Spec:** `docs/superpowers/specs/2026-06-18-nested-skills-scan-design.md`

---

## 文件结构

| 文件 | 责任 |
|------|------|
| `peri-middlewares/src/skills/loader.rs` | `SkillSource` / `SkillRoot` / `scan_skill_roots` / `scan_dir_recursive` / `insert_skill` / `resolve_skill_roots` / `list_skills`（thin wrapper） |
| `peri-middlewares/src/skills/loader_test.rs` | 10 个新增测试 + 现有 list_skills 测试零改动 + resolve_skill_dirs_* 改名 |
| `peri-middlewares/src/skills/mod.rs` | `SkillsMiddleware.plugin_roots` + `with_plugin_roots` + `resolve_roots` |
| `peri-middlewares/src/skills/mod_test.rs` | 字段改名跟随 |
| `peri-middlewares/src/subagent/skill_preload.rs` | `SkillPreloadMiddleware.plugin_roots` + `with_plugin_roots` |
| `peri-middlewares/src/subagent/skill_preload_test.rs` | 字段改名跟随 |
| `peri-middlewares/src/plugin/loader.rs` | `extract_skills_paths` 返回 `Vec<SkillRoot>`；`LoadedPlugin.skills_roots`；`PluginLoadResult.all_skill_roots` |
| `peri-middlewares/src/plugin/loader_test.rs` | 断言更新跟随 |
| `peri-middlewares/src/plugin/middleware_test.rs` | 字段改名跟随 |
| `peri-acp/src/agent/builder.rs` | `AgentBuildConfig.plugin_skill_roots` + `.with_plugin_roots` 调用 |
| `peri-acp/src/session/executor.rs` | `ExecutorConfig.plugin_skill_roots` + `ExecutorState.plugin_skill_roots` |
| `peri-acp/src/session/frozen.rs` | 函数参数改名 |
| `peri-acp/src/session/mod.rs` | 透传参数改名 |
| `peri-tui/src/acp_server/mod.rs` | `AcpServerConfig.plugin_skill_roots` |
| `peri-tui/src/acp_server/prompt.rs` | 函数参数改名 |
| `peri-tui/src/acp_server/commands.rs` | 函数参数改名 |
| `peri-tui/src/acp_server/requests.rs` | 多处透传改名 |
| `peri-tui/src/acp_server/requests_test.rs` | 测试构造改名 |
| `peri-tui/src/acp_stdio/context.rs` | 字段类型改 |
| `peri-tui/src/acp_stdio/session/create.rs` | 透传改名 |
| `peri-tui/src/acp_stdio/session/prompt_exec.rs` | 字段构造改名 |
| `peri-tui/src/acp_stdio/freeze.rs` | 透传改名 |
| `peri-tui/src/acp_stdio/init.rs` | 透传改名 |
| `peri-tui/src/main.rs` | 多处从 `pd.all_skill_roots` 读 + `scan_skill_roots` |
| `peri-tui/src/app/mod.rs` | `list_skills` → `scan_skill_roots` |
| `peri-tui/src/cli_print.rs` | 同 main.rs |
| `peri-tui/prompts/sections/13_skills.md` | 嵌套子目录说明 |
| `peri-middlewares/CLAUDE.md` | Skills 章节描述更新 |

---

## Task 1: 新增 SkillSource / SkillRoot 类型与常量

**Files:**
- Modify: `peri-middlewares/src/skills/loader.rs:1-12`（顶部 use + 类型定义）

- [ ] **Step 1: 在 loader.rs 顶部新增类型与常量**

打开 `peri-middlewares/src/skills/loader.rs`，在 `use serde::Deserialize;` 之后、`pub struct SkillMetadata` 之前插入：

```rust
/// Skill 来源 scope，用于 metadata 标签与日志诊断
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    /// ~/.claude/skills
    User,
    /// ~/.peri/settings.json::skillsDir
    Global,
    /// {cwd}/.claude/skills
    Project,
    /// 插件 manifest 声明的 skill 目录
    Plugin,
}

/// 带 source 标签的 skill 根目录
#[derive(Debug, Clone)]
pub struct SkillRoot {
    pub path: PathBuf,
    pub source: SkillSource,
    /// 仅 Plugin source 填，用于日志诊断
    pub plugin_name: Option<String>,
}

/// 递归深度上限（相对每个 skill root）
pub const MAX_SCAN_DEPTH: usize = 6;

/// 单 root 目录数上限
pub const MAX_SKILLS_DIRS_PER_ROOT: usize = 1000;
```

- [ ] **Step 2: 验证编译通过**

Run: `cargo build -p peri-middlewares`
Expected: 编译通过（无警告，因为类型已暴露但未使用——如果出现 `dead_code` 警告，将在后续 task 中消费）。

如出现 `dead_code` 警告，添加 `#[allow(dead_code)]` 临时挂到 `SkillRoot` 上，下一 task 移除。

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/src/skills/loader.rs
git commit -m "feat(skills): 新增 SkillSource/SkillRoot 类型与扫描常量

为后续 scan_skill_roots 收口函数准备类型基础。
MAX_SCAN_DEPTH=6 对齐 Codex，MAX_SKILLS_DIRS_PER_ROOT=1000 保守上限。"
```

---

## Task 2: SkillMetadata 加 source/plugin_name 字段

**Files:**
- Modify: `peri-middlewares/src/skills/loader.rs:6-35`（`SkillMetadata` + `load_skill_metadata`）

- [ ] **Step 1: 改 SkillMetadata 结构**

在 `peri-middlewares/src/skills/loader.rs` 找到：

```rust
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
}
```

替换为：

```rust
#[derive(Debug, Clone)]
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    /// skill 来源（由 scan_dir_recursive 注入，load_skill_metadata 内填占位）
    pub source: SkillSource,
    /// 仅 Plugin source 填，其他为 None
    pub plugin_name: Option<String>,
}
```

- [ ] **Step 2: 改 load_skill_metadata 填占位值**

在同一文件找到：

```rust
Some(SkillMetadata {
    name: fm.name,
    description: fm.description,
    path: path.to_path_buf(),
})
```

替换为：

```rust
Some(SkillMetadata {
    name: fm.name,
    description: fm.description,
    path: path.to_path_buf(),
    // 占位值：实际 source/plugin_name 由 scan_dir_recursive 中的 insert_skill 覆盖
    source: SkillSource::Project,
    plugin_name: None,
})
```

- [ ] **Step 3: 验证编译通过**

Run: `cargo build -p peri-middlewares`
Expected: 编译通过。可能出现"field `source` / `plugin_name` never read"警告，本期不处理（Task 7+ 会在 metadata 消费）。

- [ ] **Step 4: 验证现有测试仍通过**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests -- --nocapture`
Expected: 现有 4 个测试全部通过（`test_load_skill_metadata`、`test_list_skills_dedup`、`test_resolve_skill_dirs_*` × 3）。

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/skills/loader.rs
git commit -m "feat(skills): SkillMetadata 加 source/plugin_name 字段

source/plugin_name 由 scan_dir_recursive 在扫描时注入，
load_skill_metadata 内填占位值（Project/None）保持向后兼容。"
```

---

## Task 3: 实现 scan_skill_roots_with_limits + scan_dir_recursive（核心）

**Files:**
- Modify: `peri-middlewares/src/skills/loader.rs`（在 `list_skills` 函数之前插入新函数）
- Modify: `peri-middlewares/src/skills/loader_test.rs`（新增辅助函数 + 2 个测试）

- [ ] **Step 1: 在 loader_test.rs 顶部新增辅助函数 write_skill_file**

打开 `peri-middlewares/src/skills/loader_test.rs`，在现有 `fn write_skill` 之后插入：

```rust
/// 在指定 path 直接写一个 SKILL.md（path 含完整文件名）
fn write_skill_file(path: &Path, name: &str, desc: &str) {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    let content = format!(
        "---\nname: '{}'\ndescription: '{}'\n---\n\n# {}\n\nContent here.\n",
        name, desc, name
    );
    std::fs::write(path, content).unwrap();
}
```

- [ ] **Step 2: 写失败测试 test_scan_skill_roots_nested**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
fn test_scan_skill_roots_nested() {
    let root = tempdir().unwrap();
    // 构造 6 层嵌套：root/a/b/c/d/e/f/SKILL.md（depth=6 在范围内）
    let deep = root.path()
        .join("a").join("b").join("c").join("d").join("e").join("f");
    write_skill_file(&deep.join("SKILL.md"), "deep-skill", "deep nested");

    let roots = vec![SkillRoot {
        path: root.path().to_path_buf(),
        source: SkillSource::Project,
        plugin_name: None,
    }];
    let skills = scan_skill_roots(&roots);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "deep-skill");
    assert_eq!(skills[0].source, SkillSource::Project);
}
```

同时更新 `use super::*;` 后面的导入——确保测试能访问 `SkillRoot`、`SkillSource`、`scan_skill_roots`（这些来自 `super::*`，自动可见）。

- [ ] **Step 3: 运行测试验证它失败**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_nested -- --nocapture`
Expected: 编译失败（`scan_skill_roots` 未定义）。

- [ ] **Step 4: 在 loader.rs 实现核心函数**

打开 `peri-middlewares/src/skills/loader.rs`，在 `pub fn list_skills(dirs: &[PathBuf])` 之前插入：

```rust
use std::collections::{HashMap, HashSet};

/// 统一的 skill 扫描入口。
///
/// 对每个 root 独立递归扫描（深度上限 MAX_SCAN_DEPTH、目录数上限 MAX_SKILLS_DIRS_PER_ROOT、
/// symlink 跟随 + canonicalize 防环、叶子语义：dir 含 SKILL.md 则加载并停止下钻）。
/// 跨 root 同名去重：roots 顺序决定优先级（先到先得）。
pub fn scan_skill_roots(roots: &[SkillRoot]) -> Vec<SkillMetadata> {
    scan_skill_roots_with_limits(roots, MAX_SCAN_DEPTH, MAX_SKILLS_DIRS_PER_ROOT)
}

/// 带参数化上限的扫描入口（仅供测试注入小值，prod 用 scan_skill_roots）
#[cfg(test)]
pub(crate) fn scan_skill_roots_with_limits(
    roots: &[SkillRoot],
    max_depth: usize,
    max_dirs: usize,
) -> Vec<SkillMetadata> {
    let mut seen: HashMap<String, SkillMetadata> = HashMap::new();
    let mut ordered: Vec<String> = Vec::new();

    for root in roots {
        if !root.path.is_dir() {
            continue;
        }
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut dir_count: usize = 0;
        scan_dir_recursive(
            &root.path,
            0,
            max_depth,
            max_dirs,
            root,
            &mut visited,
            &mut dir_count,
            &mut seen,
            &mut ordered,
        );
    }

    ordered.into_iter().filter_map(|n| seen.remove(&n)).collect()
}

/// 重新暴露给非 test 配置的入口（prod 用，固定常量）
#[cfg(not(test))]
fn scan_skill_roots_with_limits(
    roots: &[SkillRoot],
    max_depth: usize,
    max_dirs: usize,
) -> Vec<SkillMetadata> {
    let mut seen: HashMap<String, SkillMetadata> = HashMap::new();
    let mut ordered: Vec<String> = Vec::new();

    for root in roots {
        if !root.path.is_dir() {
            continue;
        }
        let mut visited: HashSet<PathBuf> = HashSet::new();
        let mut dir_count: usize = 0;
        scan_dir_recursive(
            &root.path,
            0,
            max_depth,
            max_dirs,
            root,
            &mut visited,
            &mut dir_count,
            &mut seen,
            &mut ordered,
        );
    }

    ordered.into_iter().filter_map(|n| seen.remove(&n)).collect()
}

fn scan_dir_recursive(
    dir: &Path,
    depth: usize,
    max_depth: usize,
    max_dirs: usize,
    root: &SkillRoot,
    visited: &mut HashSet<PathBuf>,
    dir_count: &mut usize,
    seen: &mut HashMap<String, SkillMetadata>,
    ordered: &mut Vec<String>,
) {
    if depth > max_depth {
        return;
    }
    if *dir_count >= max_dirs {
        return;
    }

    // 防环：canonicalize 后入 visited
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canon) {
        return;
    }
    *dir_count += 1;

    // 叶子语义：dir 自己含 SKILL.md 则加载，不再下钻
    let skill_file = dir.join("SKILL.md");
    if skill_file.is_file() {
        if let Some(meta) = load_skill_metadata(&skill_file) {
            insert_skill(meta, root, seen, ordered);
        }
        return;
    }

    // 容器：递归扫描子目录
    let entries = match std::fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => return,
    };
    let mut subdirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    subdirs.sort();
    for sub in subdirs {
        scan_dir_recursive(
            &sub,
            depth + 1,
            max_depth,
            max_dirs,
            root,
            visited,
            dir_count,
            seen,
            ordered,
        );
    }
}

fn insert_skill(
    mut meta: SkillMetadata,
    root: &SkillRoot,
    seen: &mut HashMap<String, SkillMetadata>,
    ordered: &mut Vec<String>,
) {
    meta.source = root.source;
    meta.plugin_name = root.plugin_name.clone();
    if seen.contains_key(&meta.name) {
        return;
    }
    ordered.push(meta.name.clone());
    seen.insert(meta.name.clone(), meta);
}
```

注意 `use std::collections::{HashMap, HashSet};` 需要加到 loader.rs 顶部的 use 段。当前 loader.rs 只有 `use std::path::{Path, PathBuf};`，需要改为：

```rust
use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
};
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_nested -- --nocapture`
Expected: PASS。

- [ ] **Step 6: 写第二个失败测试 test_scan_skill_roots_depth_limit**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
fn test_scan_skill_roots_depth_limit() {
    let root = tempdir().unwrap();
    // 构造 7 层嵌套：root/a/b/c/d/e/f/g/SKILL.md（depth=7 超出限制）
    let too_deep = root.path()
        .join("a").join("b").join("c").join("d").join("e").join("f").join("g");
    write_skill_file(&too_deep.join("SKILL.md"), "too-deep", "ignored");

    let roots = vec![SkillRoot {
        path: root.path().to_path_buf(),
        source: SkillSource::Project,
        plugin_name: None,
    }];
    let skills = scan_skill_roots(&roots);
    assert!(
        skills.is_empty(),
        "7 层深度的 SKILL.md 应被 MAX_SCAN_DEPTH=6 拒绝"
    );
}
```

- [ ] **Step 7: 运行测试验证通过（实现已支持）**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_depth_limit -- --nocapture`
Expected: PASS。

- [ ] **Step 8: Commit**

```bash
git add peri-middlewares/src/skills/loader.rs peri-middlewares/src/skills/loader_test.rs
git commit -m "feat(skills): scan_skill_roots 递归扫描——深度上限 6 + 叶子语义 + 防环

新增 scan_skill_roots_with_limits/scan_dir_recursive/insert_skill，
统一扫描逻辑：深度上限、目录数上限、canonicalize 防环、叶子下钻停止。"
```

---

## Task 4: 叶子语义 + 目录数上限测试

**Files:**
- Modify: `peri-middlewares/src/skills/loader_test.rs`

- [ ] **Step 1: 写失败测试 test_scan_skill_roots_leaf_semantics**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
fn test_scan_skill_roots_leaf_semantics() {
    let root = tempdir().unwrap();
    // dir 含 SKILL.md，且 dir/sub 也含 SKILL.md
    // 叶子语义：dir/SKILL.md 加载，dir/sub/SKILL.md 不应被扫描
    let dir = root.path().join("my-skill");
    write_skill_file(&dir.join("SKILL.md"), "parent", "parent skill");
    write_skill_file(&dir.join("sub").join("SKILL.md"), "child", "child skill");

    let roots = vec![SkillRoot {
        path: root.path().to_path_buf(),
        source: SkillSource::Project,
        plugin_name: None,
    }];
    let skills = scan_skill_roots(&roots);
    assert_eq!(skills.len(), 1, "叶子语义应停止下钻，子目录 SKILL.md 不被扫描");
    assert_eq!(skills[0].name, "parent");
}
```

- [ ] **Step 2: 运行测试验证通过（实现已支持）**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_leaf_semantics -- --nocapture`
Expected: PASS。

- [ ] **Step 3: 写失败测试 test_scan_skill_roots_dir_count_limit**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
fn test_scan_skill_roots_dir_count_limit() {
    let root = tempdir().unwrap();
    // 构造 5 个子目录，每个含 SKILL.md
    for i in 0..5 {
        let dir = root.path().join(format!("skill-{i}"));
        write_skill_file(&dir.join("SKILL.md"), &format!("s{i}"), "x");
    }

    let roots = vec![SkillRoot {
        path: root.path().to_path_buf(),
        source: SkillSource::Project,
        plugin_name: None,
    }];
    // 注入 max_dirs=3（root 自身算 1，最多再扫 2 个子目录）
    let skills = scan_skill_roots_with_limits(&roots, 6, 3);
    assert!(
        skills.len() <= 2,
        "max_dirs=3 时 root 自身占 1，剩余配额 2，扫描结果应 ≤ 2，实际 {}",
        skills.len()
    );
}
```

- [ ] **Step 4: 运行测试验证通过（实现已支持）**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_dir_count_limit -- --nocapture`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/skills/loader_test.rs
git commit -m "test(skills): 叶子语义与目录数上限测试"
```

---

## Task 5: symlink 防环与跟随测试

**Files:**
- Modify: `peri-middlewares/src/skills/loader_test.rs`

- [ ] **Step 1: 写失败测试 test_scan_skill_roots_symlink_followed**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
#[cfg(unix)] // symlink 在 Windows 需要管理员权限，仅在 unix 测试
fn test_scan_skill_roots_symlink_followed() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    let real_target = tempdir().unwrap();
    // real_target/my-skill/SKILL.md
    write_skill_file(&real_target.path().join("my-skill").join("SKILL.md"), "linked", "via symlink");
    // root/linked → real_target（symlink 应被跟随）
    symlink(real_target.path(), root.path().join("linked")).unwrap();

    let roots = vec![SkillRoot {
        path: root.path().to_path_buf(),
        source: SkillSource::User,
        plugin_name: None,
    }];
    let skills = scan_skill_roots(&roots);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "linked");
}
```

- [ ] **Step 2: 运行测试验证通过（实现已支持，因 is_dir 跟随 symlink）**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_symlink_followed -- --nocapture`
Expected: PASS。

- [ ] **Step 3: 写失败测试 test_scan_skill_roots_symlink_loop**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
#[cfg(unix)]
fn test_scan_skill_roots_symlink_loop() {
    use std::os::unix::fs::symlink;
    let root = tempdir().unwrap();
    // 构造环：root/a/loop → root/a（自指）
    let a_dir = root.path().join("a");
    std::fs::create_dir_all(&a_dir).unwrap();
    symlink(&a_dir, a_dir.join("loop")).unwrap();
    write_skill_file(&a_dir.join("SKILL.md"), "real", "real skill");

    let roots = vec![SkillRoot {
        path: root.path().to_path_buf(),
        source: SkillSource::Project,
        plugin_name: None,
    }];
    // 不应无限递归（防环 canonicalize 命中 visited 后退出）
    let skills = scan_skill_roots(&roots);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].name, "real");
}
```

- [ ] **Step 4: 运行测试验证通过（实现已支持，canonicalize 后 visited 命中）**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_symlink_loop -- --nocapture`
Expected: PASS。

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/skills/loader_test.rs
git commit -m "test(skills): symlink 跟随与防环测试

验证 is_dir 自动跟随 symlink、canonicalize 后 visited 命中防环。"
```

---

## Task 6: 同名去重与 source 标签测试

**Files:**
- Modify: `peri-middlewares/src/skills/loader_test.rs`

- [ ] **Step 1: 写失败测试 test_scan_skill_roots_dedup_across_roots**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
fn test_scan_skill_roots_dedup_across_roots() {
    let user_dir = tempdir().unwrap();
    let project_dir = tempdir().unwrap();
    // 两个 root 都有同名 skill "common"
    write_skill_file(&user_dir.path().join("common").join("SKILL.md"), "common", "from user");
    write_skill_file(&project_dir.path().join("common").join("SKILL.md"), "common", "from project");

    let roots = vec![
        SkillRoot { path: user_dir.path().to_path_buf(), source: SkillSource::User, plugin_name: None },
        SkillRoot { path: project_dir.path().to_path_buf(), source: SkillSource::Project, plugin_name: None },
    ];
    let skills = scan_skill_roots(&roots);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].description, "from user", "User 应先于 Project 胜出");
    assert_eq!(skills[0].source, SkillSource::User);
}
```

- [ ] **Step 2: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_dedup_across_roots -- --nocapture`
Expected: PASS。

- [ ] **Step 3: 写失败测试 test_scan_skill_roots_dedup_within_root**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
fn test_scan_skill_roots_dedup_within_root() {
    let root = tempdir().unwrap();
    // 同一 root 下两个不同子目录都有 "dup" skill
    write_skill_file(&root.path().join("a").join("SKILL.md"), "dup", "from a");
    write_skill_file(&root.path().join("b").join("SKILL.md"), "dup", "from b");

    let roots = vec![SkillRoot {
        path: root.path().to_path_buf(),
        source: SkillSource::Project,
        plugin_name: None,
    }];
    let skills = scan_skill_roots(&roots);
    assert_eq!(skills.len(), 1);
    // subdirs.sort() 后 "a" 排在 "b" 前，应胜出
    assert_eq!(skills[0].description, "from a");
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_dedup_within_root -- --nocapture`
Expected: PASS。

- [ ] **Step 5: 写失败测试 test_scan_skill_roots_source_tag**

在 `loader_test.rs` 末尾追加：

```rust
#[test]
fn test_scan_skill_roots_source_tag() {
    let root = tempdir().unwrap();
    write_skill_file(&root.path().join("p").join("SKILL.md"), "x", "y");

    let roots = vec![SkillRoot {
        path: root.path().to_path_buf(),
        source: SkillSource::Plugin,
        plugin_name: Some("my-plugin".to_string()),
    }];
    let skills = scan_skill_roots(&roots);
    assert_eq!(skills.len(), 1);
    assert_eq!(skills[0].source, SkillSource::Plugin);
    assert_eq!(skills[0].plugin_name.as_deref(), Some("my-plugin"));
}
```

- [ ] **Step 6: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_source_tag -- --nocapture`
Expected: PASS。

- [ ] **Step 7: Commit**

```bash
git add peri-middlewares/src/skills/loader_test.rs
git commit -m "test(skills): 跨 root/同 root 去重 + source 标签测试"
```

---

## Task 7: scan_skill_roots 公共入口（移除 cfg(test) 限制） + list_skills thin wrapper + resolve_skill_roots

**说明**：Task 3 的 `scan_skill_roots` 已是公共入口（委托 `scan_skill_roots_with_limits`）。本 task 改造 `list_skills` 为 thin wrapper，并将 `resolve_skill_dirs` 升级为 `resolve_skill_roots`。

**Files:**
- Modify: `peri-middlewares/src/skills/loader.rs:40-110`（list_skills + resolve_skill_dirs）
- Modify: `peri-middlewares/src/skills/loader_test.rs`（resolve_skill_dirs_* 改名）

- [ ] **Step 1: 改 list_skills 为 thin wrapper**

打开 `peri-middlewares/src/skills/loader.rs`，找到现有 `list_skills` 函数（约 40-84 行），整体替换为：

```rust
/// 已废弃：仅向后兼容旧测试。建议改用 scan_skill_roots + resolve_skill_roots。
///
/// 把传入的 PathBuf 视为 Project source（无来源标签信息）。
pub fn list_skills(dirs: &[PathBuf]) -> Vec<SkillMetadata> {
    let roots: Vec<SkillRoot> = dirs
        .iter()
        .map(|d| SkillRoot {
            path: d.clone(),
            source: SkillSource::Project,
            plugin_name: None,
        })
        .collect();
    scan_skill_roots(&roots)
}
```

- [ ] **Step 2: 把 resolve_skill_dirs 改为 resolve_skill_roots**

在同一文件找到：

```rust
pub fn resolve_skill_dirs(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let user_dir = dirs_next::home_dir()
        .map(|h| h.join(".claude").join("skills"))
        .unwrap_or_default();

    let global_dir = super::load_global_skills_dir();

    let project_dir = PathBuf::from(cwd).join(".claude").join("skills");

    let mut dirs = vec![user_dir];
    if let Some(global) = global_dir {
        dirs.push(global);
    }
    dirs.push(project_dir);
    for dir in extra_dirs {
        if dir.is_dir() {
            dirs.push(dir.clone());
        }
    }
    dirs
}
```

替换为：

```rust
/// 统一解析 skill 根列表，按优先级返回 SkillRoot。
///
/// 顺序即去重优先级：User → Global → Project → Plugin（先到先得）。
pub fn resolve_skill_roots(cwd: &str, plugin_roots: Vec<SkillRoot>) -> Vec<SkillRoot> {
    let mut roots = Vec::new();

    // 1. User
    if let Some(h) = dirs_next::home_dir() {
        roots.push(SkillRoot {
            path: h.join(".claude").join("skills"),
            source: SkillSource::User,
            plugin_name: None,
        });
    }

    // 2. Global（~/.peri/settings.json::skillsDir）
    if let Some(dir) = crate::skills::load_global_skills_dir() {
        roots.push(SkillRoot {
            path: dir,
            source: SkillSource::Global,
            plugin_name: None,
        });
    }

    // 3. Project
    roots.push(SkillRoot {
        path: PathBuf::from(cwd).join(".claude").join("skills"),
        source: SkillSource::Project,
        plugin_name: None,
    });

    // 4. Plugin（来自参数，已带 source/plugin_name）
    for r in plugin_roots {
        if r.path.is_dir() {
            roots.push(r);
        }
    }

    roots
}
```

- [ ] **Step 3: 改 loader_test.rs 中 resolve_skill_dirs_* 测试**

打开 `peri-middlewares/src/skills/loader_test.rs`，找到 `test_resolve_skill_dirs_returns_standard_paths` 和 `test_resolve_skill_dirs_includes_extra_dirs` 和 `test_resolve_skill_dirs_skips_nonexistent_extra_dirs`。

把 `test_resolve_skill_dirs_returns_standard_paths` 替换为：

```rust
#[test]
fn test_resolve_skill_roots_returns_standard_paths() {
    let cwd = "/tmp/test-project";
    let roots = resolve_skill_roots(cwd, vec![]);
    assert!(
        roots.iter().any(|r| r.path.ends_with(".claude/skills") && r.source == SkillSource::User),
        "应包含 ~/.claude/skills 作为 User source"
    );
    assert!(
        roots.iter().any(|r| r.path == PathBuf::from("/tmp/test-project/.claude/skills")
            && r.source == SkillSource::Project),
        "应包含项目 .claude/skills 作为 Project source"
    );
}
```

把 `test_resolve_skill_dirs_includes_extra_dirs` 替换为：

```rust
#[test]
fn test_resolve_skill_roots_includes_plugin_roots() {
    let extra = tempfile::tempdir().unwrap();
    let plugin_root = SkillRoot {
        path: extra.path().to_path_buf(),
        source: SkillSource::Plugin,
        plugin_name: Some("test-plugin".to_string()),
    };
    let roots = resolve_skill_roots("/tmp", vec![plugin_root]);
    assert!(
        roots.iter().any(|r| r.path == extra.path().to_path_buf()
            && r.source == SkillSource::Plugin
            && r.plugin_name.as_deref() == Some("test-plugin")),
        "应包含传入的 plugin root"
    );
}
```

把 `test_resolve_skill_dirs_skips_nonexistent_extra_dirs` 替换为：

```rust
#[test]
fn test_resolve_skill_roots_skips_nonexistent_plugin_roots() {
    let nonexistent = SkillRoot {
        path: PathBuf::from("/nonexistent/path"),
        source: SkillSource::Plugin,
        plugin_name: None,
    };
    let roots = resolve_skill_roots("/tmp", vec![nonexistent]);
    assert!(
        !roots.iter().any(|r| r.path.to_str() == Some("/nonexistent/path")),
        "不存在的 plugin root 应被跳过"
    );
}
```

- [ ] **Step 4: 运行所有 skills::loader 测试**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests -- --nocapture`
Expected: 所有测试 PASS（包括 Task 3-6 新增的 + Task 7 改名的）。

- [ ] **Step 5: 验证整个 peri-middlewares crate 编译通过**

Run: `cargo build -p peri-middlewares`
Expected: 编译通过。可能出现 `resolve_skill_dirs` 未定义的错误——因为下游 `mod.rs` 还在用旧名。先记下，下一个 task 会改。

如果编译失败，**临时**在 loader.rs 末尾添加兼容 wrapper：

```rust
#[deprecated(note = "use resolve_skill_roots instead")]
pub fn resolve_skill_dirs(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let plugin_roots: Vec<SkillRoot> = extra_dirs
        .iter()
        .map(|d| SkillRoot {
            path: d.clone(),
            source: SkillSource::Plugin,
            plugin_name: None,
        })
        .collect();
    resolve_skill_roots(cwd, plugin_roots)
        .into_iter()
        .map(|r| r.path)
        .collect()
}
```

这个 wrapper 让 mod.rs / skill_preload.rs 旧调用点先编译通过，Task 10/11 改完下游后删除。

- [ ] **Step 6: Commit**

```bash
git add peri-middlewares/src/skills/loader.rs peri-middlewares/src/skills/loader_test.rs
git commit -m "feat(skills): list_skills 改 thin wrapper + resolve_skill_roots

resolve_skill_dirs 升级为 resolve_skill_roots 返回 Vec<SkillRoot>。
旧 resolve_skill_dirs 保留为 deprecated wrapper 直到下游改造完成。"
```

---

## Task 8: extract_skills_paths 返回 Vec<SkillRoot>

**Files:**
- Modify: `peri-middlewares/src/plugin/loader.rs:269-310`（extract_skills_paths）

- [ ] **Step 1: 改 extract_skills_paths 签名与实现**

打开 `peri-middlewares/src/plugin/loader.rs`，找到 `extract_skills_paths` 函数（约 269-310 行）。整体替换为：

```rust
/// Extract skill roots from plugin manifest.
///
/// Manifest `skills` entries are treated as paths relative to the plugin root
/// (matching Claude Code convention: `skills: ["./skills/"]` or `skills: ["skills/tdd"]`).
/// Each entry becomes a SkillRoot (source=Plugin, plugin_name=plugin_name),
/// regardless of whether it directly contains SKILL.md or is a container——
/// scan_skill_roots handles both cases via leaf semantics.
///
/// Falls back to `base_dir/skills/` as a single root when no manifest skills are declared.
pub(crate) fn extract_skills_paths(
    manifest: &PluginManifest,
    base_dir: &Path,
    plugin_name: &str,
) -> Vec<SkillRoot> {
    let mut result = Vec::new();

    // 1. manifest 显式声明（每条 entry 是相对于插件根目录的路径）
    if let Some(skills) = &manifest.skills {
        if !skills.is_empty() {
            for entry in skills {
                let skill_path = base_dir.join(entry);
                if !skill_path.is_dir() {
                    debug!(path = %skill_path.display(), "插件 skill 路径不存在，跳过");
                    continue;
                }
                result.push(SkillRoot {
                    path: skill_path,
                    source: SkillSource::Plugin,
                    plugin_name: Some(plugin_name.to_string()),
                });
            }
            return result;
        }
    }

    // 2. fallback：base_dir/skills/ 作为一个 root（由 scan_skill_roots 递归扫描）
    let skills_dir = base_dir.join("skills");
    if skills_dir.is_dir() {
        result.push(SkillRoot {
            path: skills_dir,
            source: SkillSource::Plugin,
            plugin_name: Some(plugin_name.to_string()),
        });
    }

    result
}
```

注意：需要在 plugin/loader.rs 顶部导入 `SkillRoot`/`SkillSource`。找到现有 `use crate::{...};` 块，添加：

```rust
use crate::skills::{SkillRoot, SkillSource};
```

- [ ] **Step 2: 改 load_plugins 内的调用点**

在同一文件找到 `load_plugins` 函数中的调用：

```rust
let skills_dirs = extract_skills_paths(&manifest, &plugin.install_path);
```

替换为（加 plugin_name 参数）：

```rust
let skills_roots = extract_skills_paths(&manifest, &plugin.install_path, &plugin.name);
```

- [ ] **Step 3: 改 LoadedPlugin 结构字段名**

在同一文件找到（约 80-95 行）：

```rust
pub struct LoadedPlugin {
    pub name: String,
    pub version: String,
    pub install_path: PathBuf,
    pub manifest: PluginManifest,
    pub commands: Vec<CommandEntry>,
    pub skills_dirs: Vec<PathBuf>,
    // ...
```

把 `pub skills_dirs: Vec<PathBuf>,` 改为 `pub skills_roots: Vec<SkillRoot>,`。

- [ ] **Step 4: 改 LoadedPlugin 构造点**

在同一文件 `load_plugins` 函数中找到：

```rust
result.push(LoadedPlugin {
    name: plugin.name.clone(),
    version: plugin.version.clone(),
    install_path: plugin.install_path.clone(),
    manifest,
    commands,
    skills_dirs,
    agents_dirs,
    // ...
```

把 `skills_dirs,` 改为 `skills_roots,`。

- [ ] **Step 5: 改 PluginLoadResult 字段名与聚合**

在同一文件找到（约 550-565 行）：

```rust
pub struct PluginLoadResult {
    pub plugins: Vec<LoadedPlugin>,
    pub all_skill_dirs: Vec<PathBuf>,
    // ...
```

把 `pub all_skill_dirs: Vec<PathBuf>,` 改为 `pub all_skill_roots: Vec<SkillRoot>,`。

找到 `load_enabled_plugins_aggregated` 中的：

```rust
let all_skill_dirs: Vec<PathBuf> = plugins.iter().flat_map(|p| p.skills_dirs.clone()).collect();
```

替换为：

```rust
let all_skill_roots: Vec<SkillRoot> = plugins.iter().flat_map(|p| p.skills_roots.clone()).collect();
```

找到 `PluginLoadResult` 的所有构造点（包括错误返回路径的空 `vec![]`），把 `all_skill_dirs:` 改为 `all_skill_roots:`。在 `load_enabled_plugins_aggregated` 失败路径（约 572 行）：

```rust
return PluginLoadResult {
    plugins: vec![],
    all_skill_dirs: vec![],
    // ...
```

改为：

```rust
return PluginLoadResult {
    plugins: vec![],
    all_skill_roots: vec![],
    // ...
```

成功路径（约 660 行）：

```rust
PluginLoadResult {
    plugins,
    all_skill_dirs,
    // ...
```

改为：

```rust
PluginLoadResult {
    plugins,
    all_skill_roots,
    // ...
```

- [ ] **Step 6: 验证编译（预计会失败，下游尚未改）**

Run: `cargo build -p peri-middlewares`
Expected: 编译失败。下游 `loader_test.rs` 和 `middleware_test.rs` 中仍引用 `skills_dirs` / `all_skill_dirs`。先在下一步修测试，再编译。

- [ ] **Step 7: 改 plugin/loader_test.rs**

打开 `peri-middlewares/src/plugin/loader_test.rs`，全局替换：

- `skills_dirs: vec![]` → `skills_roots: vec![]`（约 468、495、726、750 行）
- `result.all_skill_dirs` → `result.all_skill_roots`（约 767 行）
- `result.all_skill_dirs.len()` → `result.all_skill_roots.len()`（约 854 行）
- `result.all_skill_dirs[0]` → `result.all_skill_roots[0].path`（约 855 行）
- `fn test_load_plugin_skill_dirs_aggregated` → `fn test_load_plugin_skill_roots_aggregated`（约 814 行）

具体使用 `sed` 或编辑器全局查找替换。

- [ ] **Step 8: 改 plugin/middleware_test.rs**

打开 `peri-middlewares/src/plugin/middleware_test.rs`，找到：

```rust
skills_dirs: vec![],
```

改为：

```rust
skills_roots: vec![],
```

- [ ] **Step 9: 验证编译与测试通过**

Run: `cargo build -p peri-middlewares && cargo test -p peri-middlewares --lib plugin -- --nocapture`
Expected: 编译通过，plugin 模块测试全部 PASS。

- [ ] **Step 10: Commit**

```bash
git add peri-middlewares/src/plugin/loader.rs peri-middlewares/src/plugin/loader_test.rs peri-middlewares/src/plugin/middleware_test.rs
git commit -m "refactor(plugin): extract_skills_paths 返回 Vec<SkillRoot>

LoadedPlugin.skills_dirs → skills_roots；PluginLoadResult.all_skill_dirs → all_skill_roots。
plugin_name 全程携带，便于日志诊断。"
```

---

## Task 9: SkillsMiddleware 改造

**Files:**
- Modify: `peri-middlewares/src/skills/mod.rs`
- Modify: `peri-middlewares/src/skills/mod_test.rs`

- [ ] **Step 1: 改 SkillsMiddleware 字段**

打开 `peri-middlewares/src/skills/mod.rs`，找到：

```rust
pub struct SkillsMiddleware {
    project_skills_dir: Option<PathBuf>,
    global_skills_dir: Option<PathBuf>,
    user_skills_dir: Option<PathBuf>,
    extra_dirs: Vec<PathBuf>,
    frozen_summary: Option<String>,
}
```

替换为：

```rust
pub struct SkillsMiddleware {
    project_skills_dir: Option<PathBuf>,
    global_skills_dir: Option<PathBuf>,
    user_skills_dir: Option<PathBuf>,
    plugin_roots: Vec<SkillRoot>,
    frozen_summary: Option<String>,
}
```

- [ ] **Step 2: 改 use 导入**

在同一文件顶部找到 `pub use loader::{list_skills, load_skill_metadata, SkillMetadata};`，扩展为：

```rust
pub use loader::{list_skills, load_skill_metadata, scan_skill_roots, SkillMetadata, SkillRoot, SkillSource};
```

- [ ] **Step 3: 改 new() / with_extra_dirs()**

找到 `impl SkillsMiddleware` 中的 `new` 和 `with_extra_dirs`：

```rust
pub fn new() -> Self {
    Self {
        project_skills_dir: None,
        global_skills_dir: None,
        user_skills_dir: None,
        extra_dirs: vec![],
        frozen_summary: None,
    }
}
// ...
pub fn with_extra_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
    self.extra_dirs = dirs;
    self
}
```

替换为：

```rust
pub fn new() -> Self {
    Self {
        project_skills_dir: None,
        global_skills_dir: None,
        user_skills_dir: None,
        plugin_roots: vec![],
        frozen_summary: None,
    }
}
// ...
/// 追加额外 skills 搜索根（来自插件，每个 root 携带 plugin_name）
pub fn with_plugin_roots(mut self, roots: Vec<SkillRoot>) -> Self {
    self.plugin_roots = roots;
    self
}
```

- [ ] **Step 4: 改 resolve_dirs_static 和 resolve_dirs**

找到：

```rust
pub fn resolve_dirs_static(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    loader::resolve_skill_dirs(cwd, extra_dirs)
}
```

替换为：

```rust
pub fn resolve_roots_static(cwd: &str, plugin_roots: Vec<SkillRoot>) -> Vec<SkillRoot> {
    loader::resolve_skill_roots(cwd, plugin_roots)
}

/// 向后兼容：保留旧名让外部调用点平滑迁移
pub fn resolve_dirs_static(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    let plugin_roots: Vec<SkillRoot> = extra_dirs
        .iter()
        .map(|d| SkillRoot {
            path: d.clone(),
            source: SkillSource::Plugin,
            plugin_name: None,
        })
        .collect();
    loader::resolve_skill_roots(cwd, plugin_roots)
        .into_iter()
        .map(|r| r.path)
        .collect()
}
```

注意：`resolve_dirs_static` 保留是为了 Task 13 的 `acp_stdio/commands.rs` 平滑迁移；Task 14 验证后可考虑删除。

找到 `fn resolve_dirs(&self, cwd: &str) -> Vec<PathBuf>`，整体替换为：

```rust
fn resolve_roots(&self, cwd: &str) -> Vec<SkillRoot> {
    // 有 override 字段时走测试隔离路径
    if self.user_skills_dir.is_some()
        || self.global_skills_dir.is_some()
        || self.project_skills_dir.is_some()
    {
        let mut roots = Vec::new();
        // User override
        let user_dir = self.user_skills_dir.clone().unwrap_or_else(|| {
            dirs_next::home_dir()
                .map(|h| h.join(".claude").join("skills"))
                .unwrap_or_default()
        });
        roots.push(SkillRoot {
            path: user_dir,
            source: SkillSource::User,
            plugin_name: None,
        });
        // Global override
        if let Some(global) = &self.global_skills_dir {
            roots.push(SkillRoot {
                path: global.clone(),
                source: SkillSource::Global,
                plugin_name: None,
            });
        }
        // Project override
        let project_dir = self
            .project_skills_dir
            .clone()
            .unwrap_or_else(|| PathBuf::from(cwd).join(".claude").join("skills"));
        roots.push(SkillRoot {
            path: project_dir,
            source: SkillSource::Project,
            plugin_name: None,
        });
        // Plugin roots
        for r in &self.plugin_roots {
            if r.path.is_dir() {
                roots.push(r.clone());
            }
        }
        roots
    } else {
        loader::resolve_skill_roots(cwd, self.plugin_roots.clone())
    }
}
```

- [ ] **Step 5: 改 build_frozen_summary**

找到：

```rust
pub fn build_frozen_summary(cwd: &str, extra_dirs: &[PathBuf]) -> Option<String> {
    let dirs = Self::resolve_dirs_static(cwd, extra_dirs);
    let skills = list_skills(&dirs);
    if skills.is_empty() {
        return None;
    }
    Some(Self::build_summary(&skills))
}
```

替换为：

```rust
pub fn build_frozen_summary(cwd: &str, plugin_roots: Vec<SkillRoot>) -> Option<String> {
    let roots = Self::resolve_roots_static(cwd, plugin_roots);
    let skills = scan_skill_roots(&roots);
    if skills.is_empty() {
        return None;
    }
    Some(Self::build_summary(&skills))
}
```

- [ ] **Step 6: 改 before_agent**

找到 `before_agent` 中的：

```rust
let dirs = self.resolve_dirs(state.cwd());
let skills = tokio::task::spawn_blocking(move || list_skills(&dirs))
    .await
    .map_err(|e| peri_agent::error::AgentError::MiddlewareError {
        middleware: "SkillsMiddleware".to_string(),
        reason: format!("spawn_blocking 失败: {e}"),
    })?;
```

替换为：

```rust
let roots = self.resolve_roots(state.cwd());
let skills = tokio::task::spawn_blocking(move || scan_skill_roots(&roots))
    .await
    .map_err(|e| peri_agent::error::AgentError::MiddlewareError {
        middleware: "SkillsMiddleware".to_string(),
        reason: format!("spawn_blocking 失败: {e}"),
    })?;
```

- [ ] **Step 7: 验证编译**

Run: `cargo build -p peri-middlewares`
Expected: 编译通过。

- [ ] **Step 8: 改 mod_test.rs**

打开 `peri-middlewares/src/skills/mod_test.rs`，全局替换：

- `.with_extra_dirs(` → `.with_plugin_roots(`（约 111、133、156 行）

对于每个调用，把参数从 `vec![extra1.clone(), extra2.clone()]` 改为：

```rust
vec![
    SkillRoot {
        path: extra1.clone(),
        source: SkillSource::Plugin,
        plugin_name: None,
    },
    SkillRoot {
        path: extra2.clone(),
        source: SkillSource::Plugin,
        plugin_name: None,
    },
]
```

对于 `vec![dir.path().join("nonexistent")]` 和 `vec![extra_dir]`，同样改造。

如果测试需要 import，确保 `use super::*;` 已覆盖（SkillRoot/SkillSource 来自父 mod 的 `pub use`）。

- [ ] **Step 9: 验证测试通过**

Run: `cargo test -p peri-middlewares --lib skills -- --nocapture`
Expected: 所有 skills 模块测试 PASS。

- [ ] **Step 10: Commit**

```bash
git add peri-middlewares/src/skills/mod.rs peri-middlewares/src/skills/mod_test.rs
git commit -m "refactor(skills): SkillsMiddleware 用 plugin_roots: Vec<SkillRoot>

字段 extra_dirs → plugin_roots；方法 with_extra_dirs → with_plugin_roots；
resolve_dirs → resolve_roots。before_agent 改用 scan_skill_roots。"
```

---

## Task 10: SkillPreloadMiddleware 改造

**Files:**
- Modify: `peri-middlewares/src/subagent/skill_preload.rs`
- Modify: `peri-middlewares/src/subagent/skill_preload_test.rs`

- [ ] **Step 1: 改字段与构造**

打开 `peri-middlewares/src/subagent/skill_preload.rs`，找到：

```rust
pub struct SkillPreloadMiddleware {
    skill_names: Vec<String>,
    cwd: String,
    extra_dirs: Vec<PathBuf>,
}

impl SkillPreloadMiddleware {
    pub fn new(skill_names: Vec<String>, cwd: &str) -> Self {
        Self {
            skill_names,
            cwd: cwd.to_string(),
            extra_dirs: Vec::new(),
        }
    }

    /// 追加额外 skills 搜索目录（用于插件 skills 路径注入）
    pub fn with_extra_dirs(mut self, dirs: Vec<PathBuf>) -> Self {
        self.extra_dirs = dirs;
        self
    }
}
```

替换为：

```rust
pub struct SkillPreloadMiddleware {
    skill_names: Vec<String>,
    cwd: String,
    plugin_roots: Vec<SkillRoot>,
}

impl SkillPreloadMiddleware {
    pub fn new(skill_names: Vec<String>, cwd: &str) -> Self {
        Self {
            skill_names,
            cwd: cwd.to_string(),
            plugin_roots: Vec::new(),
        }
    }

    /// 追加插件 skills 搜索根（每个 root 携带 plugin_name）
    pub fn with_plugin_roots(mut self, roots: Vec<SkillRoot>) -> Self {
        self.plugin_roots = roots;
        self
    }
}
```

- [ ] **Step 2: 改 use 导入**

找到顶部：

```rust
use crate::skills::{list_skills, loader::resolve_skill_dirs};
```

替换为：

```rust
use crate::skills::{
    loader::resolve_skill_roots, scan_skill_roots, SkillRoot,
};
```

- [ ] **Step 3: 改 before_agent 调用点**

找到：

```rust
let dirs = resolve_skill_dirs(&self.cwd, &self.extra_dirs);
let names_lower: Vec<String> = skill_names.iter().map(|s| s.to_lowercase()).collect();

let skill_contents = tokio::task::spawn_blocking(move || {
    let all_skills = list_skills(&dirs);
    all_skills
        .into_iter()
```

替换为：

```rust
let roots = resolve_skill_roots(&self.cwd, self.plugin_roots.clone());
let names_lower: Vec<String> = skill_names.iter().map(|s| s.to_lowercase()).collect();

let skill_contents = tokio::task::spawn_blocking(move || {
    let all_skills = scan_skill_roots(&roots);
    all_skills
        .into_iter()
```

- [ ] **Step 4: 验证编译**

Run: `cargo build -p peri-middlewares`
Expected: 编译通过。

- [ ] **Step 5: 改 skill_preload_test.rs**

打开 `peri-middlewares/src/subagent/skill_preload_test.rs`，找到（约 338 行）：

```rust
.with_extra_dirs(vec![extra_dir]);
```

替换为：

```rust
.with_plugin_roots(vec![SkillRoot {
    path: extra_dir,
    source: SkillSource::Plugin,
    plugin_name: None,
}]);
```

确保测试模块顶部有 `use crate::skills::{SkillRoot, SkillSource};` 或通过 `use super::*;` 可见。

- [ ] **Step 6: 验证测试通过**

Run: `cargo test -p peri-middlewares --lib subagent::skill_preload -- --nocapture`
Expected: 所有 SkillPreload 测试 PASS。

- [ ] **Step 7: Commit**

```bash
git add peri-middlewares/src/subagent/skill_preload.rs peri-middlewares/src/subagent/skill_preload_test.rs
git commit -m "refactor(skills): SkillPreloadMiddleware 用 plugin_roots

字段 extra_dirs → plugin_roots；with_extra_dirs → with_plugin_roots；
内部调用改用 scan_skill_roots + resolve_skill_roots。"
```

---

## Task 11: peri-acp 链路改造

**Files:**
- Modify: `peri-acp/src/agent/builder.rs`
- Modify: `peri-acp/src/session/executor.rs`
- Modify: `peri-acp/src/session/frozen.rs`
- Modify: `peri-acp/src/session/mod.rs`

**预先准备**：如果 `peri-acp/src/agent/builder.rs` 顶部没有 `use peri_middlewares::skills::SkillRoot;`，在每个需要引用 SkillRoot 的文件顶部添加。或使用全路径 `peri_middlewares::skills::SkillRoot`。本计划示例代码用全路径，实施时根据文件现有风格二选一。

- [ ] **Step 1: 改 AgentBuildConfig 字段**

打开 `peri-acp/src/agent/builder.rs`，找到（约 118 行）：

```rust
pub plugin_skill_dirs: Vec<std::path::PathBuf>,
```

替换为：

```rust
pub plugin_skill_roots: Vec<peri_middlewares::skills::SkillRoot>,
```

找到（约 181 行）构造：

```rust
plugin_skill_dirs,
```

替换为：

```rust
plugin_skill_roots,
```

- [ ] **Step 2: 改 builder.rs 调用 SkillsMiddleware / SkillPreloadMiddleware**

找到（约 464 行）：

```rust
let mut mw = SkillsMiddleware::new().with_extra_dirs(plugin_skill_dirs.clone());
```

替换为：

```rust
let mut mw = SkillsMiddleware::new().with_plugin_roots(plugin_skill_roots.clone());
```

找到（约 472 行）：

```rust
.with_extra_dirs(plugin_skill_dirs.clone()),
```

替换为：

```rust
.with_plugin_roots(plugin_skill_roots.clone()),
```

- [ ] **Step 3: 改 executor.rs 字段与构造**

打开 `peri-acp/src/session/executor.rs`，全局查找替换：

- `plugin_skill_dirs: &[std::path::PathBuf]` → `plugin_skill_roots: &[peri_middlewares::skills::SkillRoot]`（约 103、251 行）
- `plugin_skill_dirs: Vec<std::path::PathBuf>` → `plugin_skill_roots: Vec<peri_middlewares::skills::SkillRoot>`（约 225、779 行）
- `plugin_skill_dirs,` → `plugin_skill_roots,`（所有构造点，约 184、313、477、824、944 行）
- `&cfg.plugin_skill_dirs` → `&cfg.plugin_skill_roots`（如果有）

注意：`SkillsMiddleware::build_frozen_summary(cwd, plugin_skill_dirs)` 调用（约 111 行）替换为 `build_frozen_summary(cwd, plugin_skill_roots.clone())`（新签名接收 `Vec<SkillRoot>` 而非 `&[PathBuf]`）。

- [ ] **Step 4: 改 frozen.rs**

打开 `peri-acp/src/session/frozen.rs`，找到（约 18、25 行）：

```rust
plugin_skill_dirs: &[PathBuf],
// ...
plugin_skill_dirs,
```

替换为：

```rust
plugin_skill_roots: &[peri_middlewares::skills::SkillRoot],
// ...
plugin_skill_roots,
```

- [ ] **Step 5: 改 mod.rs**

打开 `peri-acp/src/session/mod.rs`，找到（约 251、259 行）：

```rust
plugin_skill_dirs: &[PathBuf],
// ...
plugin_skill_dirs,
```

替换为：

```rust
plugin_skill_roots: &[peri_middlewares::skills::SkillRoot],
// ...
plugin_skill_roots,
```

- [ ] **Step 6: 验证 peri-acp 编译**

Run: `cargo build -p peri-acp`
Expected: 编译通过（可能仍有 `peri-tui` 链路未改导致的下游编译失败，但 peri-acp 本身应编译通过——用 `--lib` 限制范围）。

Run: `cargo build -p peri-acp --lib`
Expected: 编译通过。

- [ ] **Step 7: Commit**

```bash
git add peri-acp/src/agent/builder.rs peri-acp/src/session/executor.rs peri-acp/src/session/frozen.rs peri-acp/src/session/mod.rs
git commit -m "refactor(acp): plugin_skill_dirs → plugin_skill_roots

AgentBuildConfig / ExecutorConfig / frozen / mod 全链路字段类型改为
Vec<SkillRoot>，plugin_name 全程携带。"
```

---

## Task 12: peri-tui 链路改造

**Files:**
- Modify: `peri-tui/src/acp_server/mod.rs`, `prompt.rs`, `commands.rs`, `requests.rs`, `requests_test.rs`
- Modify: `peri-tui/src/acp_stdio/context.rs`, `session/create.rs`, `session/prompt_exec.rs`, `freeze.rs`, `init.rs`
- Modify: `peri-tui/src/main.rs`, `app/mod.rs`, `cli_print.rs`

**预先准备**：与 Task 11 同理，引用 SkillRoot 的文件顶部如有需要可加 `use peri_middlewares::skills::SkillRoot;`。本计划示例代码用全路径，实施时根据文件现有风格二选一。

- [ ] **Step 1: 改 acp_server/mod.rs**

打开 `peri-tui/src/acp_server/mod.rs`，找到（约 58 行）：

```rust
pub plugin_skill_dirs: Vec<std::path::PathBuf>,
```

替换为：

```rust
pub plugin_skill_roots: Vec<peri_middlewares::skills::SkillRoot>,
```

找到（约 107、155 行）：

```rust
let plugin_skill_dirs = cfg.plugin_skill_dirs.clone();
// ...
&plugin_skill_dirs,
```

替换为：

```rust
let plugin_skill_roots = cfg.plugin_skill_roots.clone();
// ...
&plugin_skill_roots,
```

- [ ] **Step 2: 改 acp_server/prompt.rs**

打开 `peri-tui/src/acp_server/prompt.rs`，找到（约 32、126 行）：

```rust
plugin_skill_dirs: &[std::path::PathBuf],
// ...
plugin_skill_dirs: plugin_skill_dirs.to_vec(),
```

替换为：

```rust
plugin_skill_roots: &[peri_middlewares::skills::SkillRoot],
// ...
plugin_skill_roots: plugin_skill_roots.to_vec(),
```

- [ ] **Step 3: 改 acp_server/commands.rs**

打开 `peri-tui/src/acp_server/commands.rs`，找到（约 11、16 行）：

```rust
plugin_skill_dirs: &[std::path::PathBuf],
// ...
peri_middlewares::SkillsMiddleware::resolve_dirs_static(cwd, plugin_skill_dirs);
```

替换为：

```rust
plugin_skill_roots: &[peri_middlewares::skills::SkillRoot],
// ...
peri_middlewares::SkillsMiddleware::resolve_roots_static(cwd, plugin_skill_roots.to_vec());
```

- [ ] **Step 4: 改 acp_server/requests.rs**

打开 `peri-tui/src/acp_server/requests.rs`，全局查找替换 `&cfg.plugin_skill_dirs` → `&cfg.plugin_skill_roots`（约 83、102、222、241、331、381 行）。

- [ ] **Step 5: 改 acp_server/requests_test.rs**

打开 `peri-tui/src/acp_server/requests_test.rs`，找到（约 80 行）：

```rust
plugin_skill_dirs: Vec::new(),
```

替换为：

```rust
plugin_skill_roots: Vec::new(),
```

- [ ] **Step 6: 改 acp_stdio/context.rs**

打开 `peri-tui/src/acp_stdio/context.rs`，找到（约 50 行）：

```rust
pub(super) plugin_skill_dirs: Vec<PathBuf>,
```

替换为：

```rust
pub(super) plugin_skill_roots: Vec<peri_middlewares::skills::SkillRoot>,
```

- [ ] **Step 7: 改 acp_stdio/session/create.rs**

打开 `peri-tui/src/acp_stdio/session/create.rs`，找到（约 74、137 行）：

```rust
&ctx.plugin_skill_dirs,
```

替换为：

```rust
&ctx.plugin_skill_roots,
```

- [ ] **Step 8: 改 acp_stdio/session/prompt_exec.rs**

打开 `peri-tui/src/acp_stdio/session/prompt_exec.rs`，找到（约 76 行）：

```rust
plugin_skill_dirs: ctx.plugin_skill_dirs.clone(),
```

替换为：

```rust
plugin_skill_roots: ctx.plugin_skill_roots.clone(),
```

- [ ] **Step 9: 改 acp_stdio/freeze.rs**

打开 `peri-tui/src/acp_stdio/freeze.rs`，找到（约 17 行）：

```rust
.build_frozen_data(cwd, &ctx.plugin_skill_dirs, &ctx.plugin_agent_dirs)
```

替换为：

```rust
.build_frozen_data(cwd, &ctx.plugin_skill_roots, &ctx.plugin_agent_dirs)
```

- [ ] **Step 10: 改 acp_stdio/init.rs**

打开 `peri-tui/src/acp_stdio/init.rs`，找到（约 80、151 行）：

```rust
let plugin_skill_dirs = plugin_data.all_skill_dirs.clone();
// ...
plugin_skill_dirs,
```

替换为：

```rust
let plugin_skill_roots = plugin_data.all_skill_roots.clone();
// ...
plugin_skill_roots,
```

- [ ] **Step 11: 改 main.rs**

打开 `peri-tui/src/main.rs`，找到（约 664-670、700-779 行）：

```rust
let plugin_skill_dirs = app
    // ...
    .map(|pd| pd.all_skill_dirs.clone())
    // ...
let plugin_skills = peri_middlewares::skills::list_skills(&plugin_skill_dirs);
```

替换为：

```rust
let plugin_skill_roots = app
    // ...
    .map(|pd| pd.all_skill_roots.clone())
    // ...
let plugin_skills = peri_middlewares::skills::scan_skill_roots(&plugin_skill_roots);
```

第二处（约 700-779 行）同样改名 `plugin_skill_dirs` → `plugin_skill_roots`，并把 `pd.all_skill_dirs` → `pd.all_skill_roots`。

- [ ] **Step 12: 改 app/mod.rs**

打开 `peri-tui/src/app/mod.rs`，找到（约 327 行）：

```rust
let plugin_skills = peri_middlewares::skills::list_skills(&pd.all_skill_dirs);
```

替换为：

```rust
let plugin_skills = peri_middlewares::skills::scan_skill_roots(&pd.all_skill_roots);
```

- [ ] **Step 13: 改 cli_print.rs**

打开 `peri-tui/src/cli_print.rs`，找到（约 147、167、212 行）：

```rust
let (plugin_skill_dirs, plugin_agent_dirs, hook_groups, plugin_lsp_servers) = if bare {
    // ...
    plugin_data.all_skill_dirs,
    // ...
};
// ...
plugin_skill_dirs,
```

替换为：

```rust
let (plugin_skill_roots, plugin_agent_dirs, hook_groups, plugin_lsp_servers) = if bare {
    // ...
    plugin_data.all_skill_roots,
    // ...
};
// ...
plugin_skill_roots,
```

- [ ] **Step 14: 验证整个 workspace 编译**

Run: `cargo build`
Expected: 全部 crate 编译通过。

如果出现 `resolve_dirs_static` 相关错误（来自 Task 9 Step 4 保留的旧 wrapper），检查调用方是否需要改为 `resolve_roots_static`。

- [ ] **Step 15: 验证测试通过**

Run: `cargo test -p peri-tui --lib`
Expected: 所有 peri-tui 测试 PASS。

- [ ] **Step 16: Commit**

```bash
git add peri-tui/src/acp_server/ peri-tui/src/acp_stdio/ peri-tui/src/main.rs peri-tui/src/app/mod.rs peri-tui/src/cli_print.rs
git commit -m "refactor(tui): plugin_skill_dirs → plugin_skill_roots

AcpServerConfig / StdioContext / main / app / cli_print 全链路改用
Vec<SkillRoot>，list_skills 调用改为 scan_skill_roots。"
```

---

## Task 13: 删除过渡兼容代码 + prompt section + CLAUDE.md 更新

**Files:**
- Modify: `peri-middlewares/src/skills/loader.rs`（删除 deprecated wrapper）
- Modify: `peri-middlewares/src/skills/mod.rs`（删除 resolve_dirs_static 兼容 wrapper）
- Modify: `peri-tui/prompts/sections/13_skills.md`
- Modify: `peri-middlewares/CLAUDE.md`

- [ ] **Step 1: 验证无残留引用**

Run: `grep -rn "resolve_skill_dirs\|with_extra_dirs\|plugin_skill_dirs\b\|skills_dirs\b\|all_skill_dirs\b" --include="*.rs" peri-middlewares/ peri-acp/ peri-tui/`
Expected: 无输出（所有引用都已迁移到新名）。

如果还有残留，先处理掉再继续。

- [ ] **Step 2: 删除 loader.rs 中的 deprecated wrapper**

打开 `peri-middlewares/src/skills/loader.rs`，找到 Task 7 Step 5 添加的：

```rust
#[deprecated(note = "use resolve_skill_roots instead")]
pub fn resolve_skill_dirs(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    // ...
}
```

整体删除。

- [ ] **Step 3: 删除 mod.rs 中的 resolve_dirs_static 兼容 wrapper**

打开 `peri-middlewares/src/skills/mod.rs`，找到 Task 9 Step 4 添加的：

```rust
/// 向后兼容：保留旧名让外部调用点平滑迁移
pub fn resolve_dirs_static(cwd: &str, extra_dirs: &[PathBuf]) -> Vec<PathBuf> {
    // ...
}
```

整体删除。保留 `resolve_roots_static`。

- [ ] **Step 4: 验证编译**

Run: `cargo build`
Expected: 编译通过。

- [ ] **Step 5: 更新 prompt section**

打开 `peri-tui/prompts/sections/13_skills.md`，找到：

```
## Skill discovery

Skills are loaded from the following directories in priority order (first match wins):

1. `~/.claude/skills/` — user-level skills (highest priority)
2. Global `skillsDir` configured in `~/.peri/settings.json`
3. `{cwd}/.claude/skills/` — project-level skills
```

替换为：

```
## Skill discovery

Skills are loaded from the following directories in priority order (first match wins):

1. `~/.claude/skills/` — user-level skills (highest priority)
2. Global `skillsDir` configured in `~/.peri/settings.json`
3. `{cwd}/.claude/skills/` — project-level skills
4. Plugin-contributed skill directories

Each skill root is scanned recursively up to 6 levels deep and 1000 directories per root. A directory containing `SKILL.md` is treated as a leaf — its subdirectories are not scanned further. This lets you organize skills with nested subdirectories (e.g. `frontend/react/hooks/use-state/SKILL.md`).
```

- [ ] **Step 6: 更新 peri-middlewares/CLAUDE.md**

打开 `peri-middlewares/CLAUDE.md`，找到：

```
**Skills**：搜索顺序 `~/.claude/skills/` → `skillsDir` → `./.claude/skills/` → 插件 skills。`SkillsMiddleware.with_extra_dirs()` 是插件扩展点。
```

替换为：

```
**Skills**：搜索顺序 `~/.claude/skills/` → `skillsDir` → `./.claude/skills/` → 插件 skills。所有扫描收口到 `scan_skill_roots(roots: &[SkillRoot])`——递归深度上限 6、单 root 目录数上限 1000、symlink 跟随 + canonicalize 防环、叶子语义（含 SKILL.md 则停止下钻）。`SkillsMiddleware.with_plugin_roots()` 是插件扩展点。
```

找到：

```
插件通过 `plugin_skill_dirs` → `SkillsMiddleware.with_extra_dirs()`、`plugin_hooks` → `HookMiddleware` 注入，无独立 PluginMiddleware。
```

替换为：

```
插件通过 `plugin_skill_roots` → `SkillsMiddleware.with_plugin_roots()`、`plugin_hooks` → `HookMiddleware` 注入，无独立 PluginMiddleware。
```

- [ ] **Step 7: Commit**

```bash
git add peri-middlewares/src/skills/loader.rs peri-middlewares/src/skills/mod.rs peri-tui/prompts/sections/13_skills.md peri-middlewares/CLAUDE.md
git commit -m "chore(skills): 清理过渡 wrapper + 更新 prompt 与 CLAUDE.md

删除 resolve_skill_dirs 和 resolve_dirs_static 的兼容 wrapper；
prompt section 添加嵌套子目录支持说明；
CLAUDE.md 同步描述。"
```

---

## Task 14: 全量集成验证

- [ ] **Step 1: workspace 全量构建**

Run: `cargo build --all`
Expected: 全部 crate 编译通过，无警告。

- [ ] **Step 2: workspace 全量测试**

Run: `cargo test --all`
Expected: 所有测试 PASS。

- [ ] **Step 3: 检查 clippy**

Run: `cargo clippy --all -- -D warnings`
Expected: 无 clippy 警告。

如果有警告，修复后重新运行。

- [ ] **Step 4: 检查格式**

Run: `cargo fmt --all -- --check`
Expected: 无格式问题。

如果有格式问题，运行 `cargo fmt --all` 修复后重新检查。

- [ ] **Step 5: 手动验证 TUI 启动**

Run: `cargo run -p peri-tui -- -p "test" --output-format text 2>&1 | head -20`
Expected: TUI 正常启动，能加载现有 skills（如果有）。

- [ ] **Step 6: 手动验证嵌套 skill 扫描**

构造测试场景：

```bash
mkdir -p ~/.claude/skills/test/nested/deep
cat > ~/.claude/skills/test/nested/deep/SKILL.md <<'EOF'
---
name: 'test-nested-deep'
description: 'A deeply nested test skill'
---
# Test Nested Deep
Content.
EOF
cargo run -p peri-tui -- -p "/test-nested-deep" --output-format text 2>&1 | head -30
```

Expected: skill 被识别并预加载（输出包含 skill 内容）。

清理测试数据：

```bash
rm -rf ~/.claude/skills/test
```

- [ ] **Step 7: 最终提交（如有未提交的修复）**

```bash
git status
# 如果有未提交改动
git add -A
git commit -m "fix(skills): 集成验证后的修复"
```

- [ ] **Step 8: PR 准备**

```bash
git log --oneline main..HEAD
```

确认所有 commit 都在分支上，准备创建 PR。

---

## Self-Review

执行本节检查后才能交付 plan：

### Spec 覆盖

| Spec 章节 | 对应 Task |
|----------|-----------|
| §2 核心数据结构 | Task 1（类型）+ Task 2（SkillMetadata 加字段） |
| §3.1-3.3 scan_skill_roots 算法 | Task 3（实现）+ Task 4-6（测试） |
| §3.4 关键不变量（叶子、per-root 计数、visited、canonicalize） | Task 3 实现 + Task 4-5 测试 |
| §3.5 测试辅助函数 | Task 3 Step 4 实现 `scan_skill_roots_with_limits` |
| §4.1 resolve_skill_roots | Task 7 Step 2 |
| §4.2 list_skills thin wrapper | Task 7 Step 1 |
| §4.3 SkillsMiddleware 改造 | Task 9 |
| §4.4 SkillPreloadMiddleware 改造 | Task 10 |
| §4.5 extract_skills_paths 返回 Vec<SkillRoot> | Task 8 |
| §4.6 LoadedPlugin/PluginLoadResult 字段改名 | Task 8 Step 3-5 |
| §4.7 builder.rs 调用点 | Task 11 Step 2 |
| §4.8 SubAgent 路径 | Task 10（SubAgent 共用 SkillPreloadMiddleware） |
| §5 错误处理 | Task 3 实现（静默跳过 + debug 日志） |
| §6 性能考量 | Task 3 实现（spawn_blocking 保留） |
| §7 测试矩阵（10 个测试） | Task 3-6 |
| §8 影响面汇总（全链路） | Task 8-12 |

### 类型一致性

- `SkillRoot` 在 Task 1 定义，Task 8/9/10/11/12 全部使用 `path: PathBuf, source: SkillSource, plugin_name: Option<String>` 一致
- `scan_skill_roots(roots: &[SkillRoot]) -> Vec<SkillMetadata>` 在 Task 3 定义，后续 task 调用签名一致
- `with_plugin_roots(roots: Vec<SkillRoot>)` 在 Task 9/10 定义，Task 11/12 调用一致
- `resolve_skill_roots(cwd: &str, plugin_roots: Vec<SkillRoot>) -> Vec<SkillRoot>` 在 Task 7 定义，后续一致

### Placeholder 检查

- 所有 Step 都有具体代码或具体命令
- 没有 "TBD"/"TODO"/"类似 Task N"
- 测试代码完整可运行

### 风险点

1. **Task 8 改造 plugin/loader.rs 时**：`PluginLoadResult` 的空构造点（错误返回）容易遗漏——Step 5 已明确指出两处。
2. **Task 11/12 大量机械替换**：建议每个 Step 后立即 `cargo build` 验证，避免错误堆积。
3. **Task 7 Step 5 的临时兼容 wrapper**：Task 13 必须删除，否则 `cargo clippy -D warnings` 会因 `dead_code` 失败。
4. **`resolve_dirs_static` 兼容 wrapper**：Task 9 Step 4 保留，Task 13 删除。如果 Task 12 中有调用方未改为 `resolve_roots_static`，会在 Task 13 编译失败。

---
