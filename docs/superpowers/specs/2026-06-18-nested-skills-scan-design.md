# Skills 嵌套递归扫描

**日期**：2026-06-18
**状态**：Approved
**作者**：KonghaYao
**相关**：`2026-06-10-unify-resolve-skill-dirs-design.md`（resolve_skill_dirs 公共化的先例）

---

## 1. 问题与动机

当前 `peri-middlewares/src/skills/loader.rs` 的 `list_skills(dirs)` **只扫一层**：对每个根目录，仅遍历其直接子项（子目录或直接 SKILL.md 文件），不递归。这意味着用户在 `~/.claude/skills/` 下用嵌套子目录组织 skill 仓库时（如 `frontend/react/hooks/use-state/SKILL.md`），深层 skill 永远不会被加载。

同样的限制存在于：
- `plugin/loader.rs::extract_skills_paths`（一层扫描，fallback 到 `base_dir/skills/` 一层）
- `skill_preload.rs::SkillPreloadMiddleware` 内联调用 `list_skills`（一层）

三个调用点各自维护一份"扫一层"逻辑，缺少递归能力，且彼此之间有语义漂移风险。

**核心洞察**：参考 Codex 的 skill loader 设计——每个 skill root 独立递归扫描，配合深度上限（6）、目录数上限（1000/root）、符号链接防环、叶子语义（含 SKILL.md 则停止下钻），即可让用户用任意嵌套目录组织 skill 仓库，同时保持扫描成本有界。

**目标**：
1. 把三个分散的 skill 扫描入口**统一收口**到一个核心函数 `scan_skill_roots(roots: &[SkillRoot])`。
2. 收口函数应用统一的递归规则（深度 6、目录数 1000/root、symlink 跟随 + 防环、叶子语义）。
3. 引入 `SkillRoot`/`SkillSource` 类型，让 metadata 携带来源标签（仅供日志/诊断，不进 prompt）。

**非目标**：
- 不引入 Codex 的 System/Admin scope（保留现有 User/Global/Project/Plugin 4 路径模型）
- 不改 `(scope_rank, name, path)` 排序键（保持 roots 顺序 + dir 内 sort 的先到先得）
- 不让 `MAX_SCAN_DEPTH` / `MAX_SKILLS_DIRS_PER_ROOT` 可配置（保持常量，未来如需再加 env）
- 不改 prompt 文案大段说明（仅新增"嵌套子目录支持"一行）
- 不引入并发扫描或缓存（1000/root 上限已足够保护）

---

## 2. 核心数据结构

新增于 `peri-middlewares/src/skills/loader.rs`：

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    User,    // ~/.claude/skills
    Global,  // ~/.peri/settings.json::skillsDir
    Project, // {cwd}/.claude/skills
    Plugin,  // 插件 manifest 声明的 skill 目录
}

#[derive(Debug, Clone)]
pub struct SkillRoot {
    pub path: PathBuf,
    pub source: SkillSource,
    pub plugin_name: Option<String>, // 仅 Plugin 填，用于日志
}

pub const MAX_SCAN_DEPTH: usize = 6;
pub const MAX_SKILLS_DIRS_PER_ROOT: usize = 1000;
```

`SkillMetadata` 增字段：

```rust
pub struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub path: PathBuf,
    pub source: SkillSource,         // 新增
    pub plugin_name: Option<String>, // 新增
}
```

**关键约束**：`source` / `plugin_name` **不进入 prompt 文本**——`build_summary` 函数签名与输出格式不变，仅在日志和未来诊断工具中使用。

---

## 3. 核心算法：`scan_skill_roots`

### 3.1 入口函数

```rust
pub fn scan_skill_roots(roots: &[SkillRoot]) -> Vec<SkillMetadata> {
    let mut seen: HashMap<String, SkillMetadata> = HashMap::new();
    let mut ordered: Vec<String> = Vec::new(); // 保持首次出现顺序

    for root in roots {
        if !root.path.is_dir() { continue; }
        let mut visited: HashSet<PathBuf> = HashSet::new(); // 每 root 独立 visited
        let mut dir_count: usize = 0;
        scan_dir_recursive(
            &root.path, 0, root, &mut visited, &mut dir_count,
            &mut seen, &mut ordered,
        );
    }

    ordered.into_iter().filter_map(|n| seen.remove(&n)).collect()
}
```

### 3.2 内部递归 `scan_dir_recursive`

```rust
fn scan_dir_recursive(
    dir: &Path, depth: usize, root: &SkillRoot,
    visited: &mut HashSet<PathBuf>, dir_count: &mut usize,
    seen: &mut HashMap<String, SkillMetadata>, ordered: &mut Vec<String>,
) {
    if depth > MAX_SCAN_DEPTH { return; }
    if *dir_count >= MAX_SKILLS_DIRS_PER_ROOT { return; }

    // 防环：canonicalize 后入 visited
    let canon = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());
    if !visited.insert(canon) { return; }
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
    let entries = match std::fs::read_dir(dir) { Ok(e) => e, Err(_) => return };
    let mut subdirs: Vec<PathBuf> = entries
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_dir()) // is_dir 自动跟随 symlink
        .collect();
    subdirs.sort(); // 同层目录名排序保证遍历稳定
    for sub in subdirs {
        scan_dir_recursive(&sub, depth + 1, root, visited, dir_count, seen, ordered);
    }
}
```

### 3.3 同名去重 `insert_skill`

```rust
fn insert_skill(
    meta: SkillMetadata,
    root: &SkillRoot,
    seen: &mut HashMap<String, SkillMetadata>,
    ordered: &mut Vec<String>,
) {
    let meta = SkillMetadata {
        source: root.source,
        plugin_name: root.plugin_name.clone(),
        ..meta
    };
    if seen.contains_key(&meta.name) { return; } // 先到先得
    ordered.push(meta.name.clone());
    seen.insert(meta.name.clone(), meta);
}
```

### 3.4 关键不变量

1. **叶子优先**：dir 含 SKILL.md 即停止下钻，避免嵌套 skill 歧义
2. **每 root 独立计数**：dir_count 在每个 root 入口重置，避免一个 root 把别的 root 的配额吃掉
3. **visited per root**：每 root 独立 visited set，避免不同 root 共享 symlink 目标被误判为环
4. **canonicalize 防环**：A → B → A 在第二次访问 A 的 canonical path 时命中 visited 退出
5. **顶层 SKILL.md 文件被忽略**：旧 `list_skills` 允许 `dir/SKILL.md` 直接作为顶层文件 skill，新算法只递归子目录——这是**唯一的行为变化**。如需保留顶层文件 skill，未来可在 `scan_skill_roots` 入口对每个 root 自身做一次预检

### 3.5 测试辅助函数

为避免污染 prod 常量，暴露内部函数供测试注入小上限：

```rust
#[cfg(test)]
pub(crate) fn scan_skill_roots_with_limits(
    roots: &[SkillRoot],
    max_depth: usize,
    max_dirs: usize,
) -> Vec<SkillMetadata> { /* 同算法，参数化 */ }
```

对外只暴露 `scan_skill_roots(roots)` 使用默认常量（6 / 1000）。

---

## 4. 调用点改造

### 4.1 `resolve_skill_dirs` → `resolve_skill_roots`

`loader.rs`：

```rust
pub fn resolve_skill_roots(
    cwd: &str,
    plugin_roots: Vec<SkillRoot>,
) -> Vec<SkillRoot> {
    let mut roots = Vec::new();

    // 1. User
    if let Some(h) = dirs_next::home_dir() {
        roots.push(SkillRoot {
            path: h.join(".claude").join("skills"),
            source: SkillSource::User,
            plugin_name: None,
        });
    }

    // 2. Global
    if let Some(dir) = crate::skills::load_global_skills_dir() {
        roots.push(SkillRoot { path: dir, source: SkillSource::Global, plugin_name: None });
    }

    // 3. Project
    roots.push(SkillRoot {
        path: PathBuf::from(cwd).join(".claude").join("skills"),
        source: SkillSource::Project,
        plugin_name: None,
    });

    // 4. Plugin
    for r in plugin_roots {
        if r.path.is_dir() {
            roots.push(r);
        }
    }

    roots
}
```

### 4.2 `list_skills` 保留为 thin wrapper

```rust
/// 已废弃：仅向后兼容旧测试。建议改用 scan_skill_roots + resolve_skill_roots。
/// 把传入的 PathBuf 视为 Project source（无来源标签信息）。
pub fn list_skills(dirs: &[PathBuf]) -> Vec<SkillMetadata> {
    let roots: Vec<SkillRoot> = dirs.iter()
        .map(|d| SkillRoot {
            path: d.clone(),
            source: SkillSource::Project,
            plugin_name: None,
        })
        .collect();
    scan_skill_roots(&roots)
}
```

**理由**：现有 `test_list_skills_dedup` 等用 `list_skills(&[dir1, dir2])` 调用，重写全部测试工作量大且偏离本次目标。

### 4.3 `SkillsMiddleware` 改造

`mod.rs`：

- 字段 `extra_dirs: Vec<PathBuf>` → `plugin_roots: Vec<SkillRoot>`
- builder `with_extra_dirs()` → `with_plugin_roots()`
- 方法 `resolve_dirs()` → `resolve_roots()`（返回 `Vec<SkillRoot>`）
- override 分支内部构造对应 source 的 SkillRoot（测试隔离）
- `build_frozen_summary` 委托 `resolve_skill_roots`
- `before_agent` 中 `tokio::task::spawn_blocking(move || scan_skill_roots(&roots))`

### 4.4 `SkillPreloadMiddleware` 改造

`skill_preload.rs`：

- 字段 `extra_dirs` → `plugin_roots: Vec<SkillRoot>`
- `with_extra_dirs()` → `with_plugin_roots()`
- `before_agent` 中 `resolve_skill_dirs` + `list_skills` → `resolve_skill_roots` + `scan_skill_roots`

### 4.5 `extract_skills_paths` 改造（关键变化）

`plugin/loader.rs`：

**当前**返回 `Vec<PathBuf>`（已识别为"含 SKILL.md 的目录"）。
**改造后**返回 `Vec<SkillRoot>`，每个 root 的 `source=Plugin`、`plugin_name=插件名`。

语义调整：
- 旧：先扫一层子目录找 SKILL.md，找不到则 fallback 扫 `base_dir/skills/` 一层
- 新：直接返回 manifest 声明的路径作为 root（fallback 改为返回 `base_dir/skills/` 作为 root），由统一扫描函数递归处理

新算法对两种情况都兼容（叶子语义保证：含 SKILL.md 则加载并停止）。

### 4.6 `LoadedPlugin` 字段改名

```rust
pub struct LoadedPlugin {
    // ...
    pub skills_roots: Vec<SkillRoot>, // 原 skills_dirs: Vec<PathBuf>
    // ...
}
```

`PluginLoadResult.all_skill_dirs` → `all_skill_roots: Vec<SkillRoot>`。

下游所有引用此字段的代码（`builder.rs`、`load_enabled_plugins_aggregated`）同步改名。

### 4.7 `builder.rs` 调用点

```rust
// 原
.add_middleware(Box::new(
    SkillsMiddleware::new()
        .with_global_config()
        .with_extra_dirs(plugin_skill_dirs.clone())
))
// 改造后
.add_middleware(Box::new(
    SkillsMiddleware::new()
        .with_global_config()
        .with_plugin_roots(plugin_skill_roots.clone())
))
```

`plugin_skill_dirs: Vec<PathBuf>` 变量改名为 `plugin_skill_roots: Vec<SkillRoot>`，直接从 `PluginLoadResult.all_skill_roots` 传递，全程无类型转换。

### 4.8 SubAgent 路径

`build_subagent_middlewares` 当前调用 `SkillsMiddleware::new().with_global_config()`——SubAgent 无插件 roots。改造后行为不变（仅 `resolve_dirs` → `resolve_roots` 改名）。SubAgent 在递归扫描下自动受益。

---

## 5. 错误处理

| 场景 | 策略 |
|------|------|
| `read_dir` 失败 | 静默跳过该 dir |
| `canonicalize` 失败 | 用原 path 作为 visited key，继续 |
| `load_skill_metadata` 解析失败 | 静默跳过该 SKILL.md |
| `std::fs::read_to_string` 失败（preload 路径） | 静默跳过 |
| `depth > MAX_SCAN_DEPTH` | 返回，不再下钻 |
| `dir_count >= MAX_SKILLS_DIRS_PER_ROOT` | 返回，不再下钻 |
| symlink 环接 | canonicalize 后 visited 命中，正常退出 |

**超限场景日志**：超限时 `tracing::debug!` 记录一次"root X 达到上限，截断扫描"。配置异常时（如误把 node_modules 软链到 skills 下）用户能从调试日志看到原因。常规错误（IO/解析）不输出 warn，避免日志噪声。

---

## 6. 性能考量

| 关注点 | 现状 | 改造后 | 影响 |
|--------|------|--------|------|
| 扫描时机 | session/new 一次（frozen）+ 每 turn（非 frozen） | 不变 | 无 |
| 阻塞 | `spawn_blocking` | 不变 | 无 |
| 最坏情况 | 一层扫 N 个直接子目录 | 单 root 最多 1000 目录 × 6 层 | 略高，但有界 |
| canonicalize 开销 | 无 | 每个 dir 一次 syscall | 最多 1000 次/root，可接受 |

**结论**：1000 目录上限 + 6 层深度已足够保护，无需引入并发扫描或缓存。

---

## 7. 测试

### 7.1 新增测试（`loader_test.rs`）

| 测试名 | 验证点 |
|--------|--------|
| `test_scan_skill_roots_flat` | 平铺一层（与旧 list_skills 行为兼容） |
| `test_scan_skill_roots_nested` | 6 层嵌套内 SKILL.md 都能扫到（`a/b/c/d/e/f/SKILL.md`） |
| `test_scan_skill_roots_depth_limit` | 第 7 层的 SKILL.md 不被扫描（用 `scan_skill_roots_with_limits(roots, 6, 1000)`） |
| `test_scan_skill_roots_dir_count_limit` | 注入 max_dirs=4，构造 5 个子目录，扫描结果 ≤ 4 |
| `test_scan_skill_roots_symlink_loop` | A → B → A 不无限递归 |
| `test_scan_skill_roots_symlink_followed` | 正常 symlink 被跟随 |
| `test_scan_skill_roots_leaf_semantics` | `dir/SKILL.md` 存在时，`dir/sub/SKILL.md` 不被扫描 |
| `test_scan_skill_roots_dedup_across_roots` | 同名 skill 在 User 和 Project 都有 → User 胜出 |
| `test_scan_skill_roots_dedup_within_root` | 同一 root 内同名 skill → 按 sort 后物理顺序首个胜出 |
| `test_scan_skill_roots_source_tag` | 返回的 SkillMetadata.source/plugin_name 正确标记 |

### 7.2 现有测试影响

| 测试 | 影响 |
|------|------|
| `loader_test.rs::test_list_skills_dedup` | 通过 thin wrapper 跑通，断言不变 |
| `loader_test.rs::test_load_skill_metadata` | 不变（load_skill_metadata 未改） |
| `loader_test.rs::test_resolve_skill_dirs_*` | 改为 `test_resolve_skill_roots_*`，断言 PathBuf → SkillRoot.path |
| `mod_test.rs` | 字段改名（extra_dirs → plugin_roots） |
| `skill_preload_test.rs` | 同上 |
| `plugin/loader_test.rs` | `extract_skills_paths` 返回 SkillRoot，断言更新 |

### 7.3 跨平台兼容

- macOS/Linux 上 symlink 是一等公民，`std::os::unix::fs::symlink` 创建
- Windows 上 symlink 创建需要管理员权限或开发者模式，测试用 `cfg!(unix)` 跳过 symlink 测试
- canonicalize 在 Windows 上 `\\?\` 前缀正常，比较 visited 时一致

---

## 8. 影响面汇总

`plugin_skill_dirs: Vec<PathBuf>` 字段穿透在 peri-acp + peri-tui 的多个 struct/参数中。本期按"全链路改为 Vec<SkillRoot>"实施（保留 plugin_name 全程，便于日志诊断）。

**peri-middlewares（核心）**：

| 文件 | 改动类型 |
|------|----------|
| `peri-middlewares/src/skills/loader.rs` | 新增 `SkillSource`/`SkillRoot`/`scan_skill_roots`/`scan_dir_recursive`/`insert_skill`/常量；`resolve_skill_dirs` → `resolve_skill_roots`；`list_skills` 变 thin wrapper；`SkillMetadata` 加 source/plugin_name |
| `peri-middlewares/src/skills/loader_test.rs` | 新增 10 个测试；现有 list_skills 测试零改动；resolve_skill_dirs_* 改名 |
| `peri-middlewares/src/skills/mod.rs` | `SkillsMiddleware.extra_dirs` → `plugin_roots`；`resolve_dirs` → `resolve_roots`；`with_extra_dirs` → `with_plugin_roots`；override 分支构造 SkillRoot |
| `peri-middlewares/src/skills/mod_test.rs` | 字段改名跟随 |
| `peri-middlewares/src/subagent/skill_preload.rs` | `extra_dirs` → `plugin_roots`；调用点改名；测试文件跟随 |
| `peri-middlewares/src/plugin/loader.rs` | `extract_skills_paths` 返回 `Vec<SkillRoot>`；`LoadedPlugin.skills_dirs` → `skills_roots`；`PluginLoadResult.all_skill_dirs` → `all_skill_roots` |
| `peri-middlewares/src/plugin/loader_test.rs` | 断言更新跟随 |
| `peri-middlewares/src/plugin/middleware_test.rs` | 字段改名跟随 |

**peri-acp（传递链）**：

| 文件 | 改动类型 |
|------|----------|
| `peri-acp/src/agent/builder.rs` | `AgentBuildConfig.plugin_skill_dirs: Vec<PathBuf>` → `plugin_skill_roots: Vec<SkillRoot>`；调用点 `.with_extra_dirs` → `.with_plugin_roots` |
| `peri-acp/src/session/executor.rs` | `ExecutorConfig.plugin_skill_dirs` + `ExecutorState.plugin_skill_dirs` + 多个构造/透传点 |
| `peri-acp/src/session/frozen.rs` | 函数参数 `plugin_skill_dirs: &[PathBuf]` → `plugin_skill_roots: &[SkillRoot]` |
| `peri-acp/src/session/mod.rs` | 透传参数改名 |

**peri-tui（传递链 + UI 消费）**：

| 文件 | 改动类型 |
|------|----------|
| `peri-tui/src/acp_server/mod.rs` | `AcpServerConfig.plugin_skill_dirs` 字段改类型 |
| `peri-tui/src/acp_server/prompt.rs` | 函数参数改名 |
| `peri-tui/src/acp_server/commands.rs` | 函数参数改名 |
| `peri-tui/src/acp_server/requests.rs` (多处) | 透传参数改名 |
| `peri-tui/src/acp_server/requests_test.rs` | 测试构造改名 |
| `peri-tui/src/acp_stdio/context.rs` | 字段类型改 |
| `peri-tui/src/acp_stdio/session/create.rs` | 透传改名 |
| `peri-tui/src/acp_stdio/session/prompt_exec.rs` | 字段构造改名 |
| `peri-tui/src/acp_stdio/freeze.rs` | 透传改名 |
| `peri-tui/src/acp_stdio/init.rs` | 透传改名 |
| `peri-tui/src/main.rs` (多处) | 从 `pd.all_skill_roots` 读 + `scan_skill_roots` 调用 |
| `peri-tui/src/app/mod.rs` | `list_skills(&pd.all_skill_dirs)` → `scan_skill_roots(&pd.all_skill_roots)` |
| `peri-tui/src/cli_print.rs` | 同 main.rs |

**文档**：

| 文件 | 改动类型 |
|------|----------|
| `peri-tui/prompts/sections/13_skills.md` | 新增"嵌套子目录支持（深度上限 6）"说明 |
| `peri-middlewares/CLAUDE.md` | 更新 Skills 章节对 `with_plugin_roots` 的描述 |

---

## 9. 不在本次范围内

- ❌ 引入 System/Admin scope（保留现有 4 路径模型）
- ❌ `(scope_rank, name, path)` 排序键（保持先到先得）
- ❌ `MAX_SCAN_DEPTH` / `MAX_SKILLS_DIRS_PER_ROOT` 可配置（保持常量）
- ❌ Prompt section 大段说明改写（仅一行）
- ❌ 插件 manifest 新增字段（保持兼容现有 Claude Code 插件格式）
- ❌ 顶层 SKILL.md 文件支持（已确认放弃，与 Codex 行为一致）
- ❌ SubAgent 插件 roots 注入（独立 issue 处理）

---

## 10. 风险与缓解

| 风险 | 缓解 |
|------|------|
| 字段改名 `skills_dirs` → `skills_roots` 影响下游引用 | 全仓 grep `skills_dirs` / `all_skill_dirs` / `with_extra_dirs` 一次性替换 |
| 测试中 `scan_skill_roots_with_limits` 暴露 `pub(crate)` 可能被误用 | 仅在 `#[cfg(test)]` 下编译，不影响 prod API |
| `canonicalize` 在符号链接密集目录下的性能 | 1000/root 上限保证最坏情况有界 |
| 顶层 SKILL.md 文件被忽略破坏现有用户习惯 | 当前仓库与 ~/.claude/skills 均无此用法；如未来出现再补预检 |
