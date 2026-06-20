# Builtin Skills Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 `SkillSource::Builtin` 作为第 5 种 skill 来源，用 `include_str!` 编译期嵌入 SKILL.md，首批内置 `use-artifacts`。

**Architecture:** 复用现有 `SkillRoot` struct（不引入新 trait），通过给 `SkillSource` 加 `Builtin` 变体 + 在 `scan_skill_roots_impl` 主循环特判实现。`resolve_skill_roots` 末尾按条件追加 Builtin root，开关 `disableBundledSkills` 在 session/new 时一次性冻结。

**Tech Stack:** Rust 2021 edition, `gray_matter` (YAML frontmatter), `include_str!` 编译期嵌入, `tracing` 日志, `tokio::task::spawn_blocking` 异步扫描。

**Spec:** `docs/superpowers/specs/2026-06-20-builtin-skills-design.md`

---

## File Structure

| 文件 | 操作 | 职责 |
|------|------|------|
| `peri-middlewares/src/skills/builtin/mod.rs` | Create | `BuiltinSkill` struct + `BUILTIN_SKILLS` 常量 + `parse_builtin_frontmatter` |
| `peri-middlewares/src/skills/builtin_test.rs` | Create | Builtin 模块测试 |
| `peri-middlewares/src/skills/builtin/skills/use-artifacts/SKILL.md` | Move (from `.claude/skills/`) | 内置 SKILL.md，随 crate 源码版本控制，`include_str!` 编译期嵌入 |
| `peri-middlewares/src/skills/loader.rs` | Modify | `SkillSource::Builtin` 变体 + `resolve_skill_roots` 加参数 + `scan_skill_roots_impl` Builtin 特判 |
| `peri-middlewares/src/skills/loader_test.rs` | Modify | 适配 `resolve_skill_roots` 新签名 + 新增 Builtin 测试 |
| `peri-middlewares/src/skills/mod.rs` | Modify | `pub mod builtin` 声明 + `build_frozen_summary` 加参数 + `load_disable_bundled_skills` 函数 + `resolve_roots` 加字段 |
| `peri-middlewares/src/skills/mod_test.rs` | Modify | 现有测试不受影响（用 builder），无需大改 |
| `peri-middlewares/src/subagent/skill_preload.rs` | Modify | Builtin source 走 `BUILTIN_SKILLS` 查找全文 |
| `peri-acp/src/session/executor.rs` | Modify | `FrozenSessionData::build` 读取 settings + 传 `disable_bundled` 给 `build_frozen_summary` |

---

## Task 1: 新增 `builtin/mod.rs` 基础设施

**Files:**
- Create: `peri-middlewares/src/skills/builtin/mod.rs`
- Create: `peri-middlewares/src/skills/builtin_test.rs`
- 已迁移（controller 完成）: `peri-middlewares/src/skills/builtin/skills/use-artifacts/SKILL.md`（从 `.claude/skills/` 移入，frontmatter 已用 `description: >` 块样式修复）
- Modify: `peri-middlewares/src/skills/mod.rs:1`（加 `pub mod builtin;`）

- [ ] **Step 1: 在 `skills/mod.rs` 顶部声明 `builtin` 子模块**

修改 `peri-middlewares/src/skills/mod.rs:1`：

```rust
pub mod builtin;
pub mod loader;
```

- [ ] **Step 2: 写失败测试 `builtin_test.rs`**

Create `peri-middlewares/src/skills/builtin_test.rs`：

```rust
use super::builtin::{parse_builtin_frontmatter, BuiltinSkill, BUILTIN_SKILLS};

#[test]
fn test_builtin_skills_non_empty() {
    // 至少含 use-artifacts 验证用例
    assert!(BUILTIN_SKILLS.iter().any(|s| s.name == "use-artifacts"),
        "BUILTIN_SKILLS 应含 use-artifacts");
}

#[test]
fn test_builtin_skills_unique_names() {
    let mut names: Vec<&str> = BUILTIN_SKILLS.iter().map(|s| s.name).collect();
    names.sort();
    let original_len = names.len();
    names.dedup();
    assert_eq!(names.len(), original_len, "BUILTIN_SKILLS 名称不应重复");
}

#[test]
fn test_builtin_skills_frontmatter_valid() {
    // 每个 BUILTIN_SKILLS 的 frontmatter 都应能解析出 name + description
    for skill in BUILTIN_SKILLS {
        let parsed = parse_builtin_frontmatter(skill.content);
        assert!(parsed.is_some(),
            "builtin skill {} frontmatter 解析失败", skill.name);
        let (name, desc) = parsed.unwrap();
        assert_eq!(name, skill.name,
            "builtin skill {} frontmatter name 字段不匹配", skill.name);
        assert!(!desc.is_empty(),
            "builtin skill {} description 为空", skill.name);
    }
}

#[test]
fn test_parse_builtin_frontmatter_invalid_returns_none() {
    // 格式错误的 frontmatter 应返回 None
    let bad = "no frontmatter here";
    assert!(parse_builtin_frontmatter(bad).is_none());

    let bad2 = "---\nname: only_name\n---\nbody";
    assert!(parse_builtin_frontmatter(bad2).is_none(),
        "缺 description 字段应返回 None");
}

#[test]
fn test_parse_builtin_frontmatter_valid() {
    let content = "---\nname: test-skill\ndescription: 测试 skill\n---\n\n# Body\n";
    let parsed = parse_builtin_frontmatter(content).unwrap();
    assert_eq!(parsed.0, "test-skill");
    assert_eq!(parsed.1, "测试 skill");
}
```

- [ ] **Step 3: 运行测试验证失败**

Run: `cargo test -p peri-middlewares --lib skills::builtin::tests 2>&1 | head -30`
Expected: 编译失败，错误为 `unresolved module builtin` 或 `cannot find value BUILTIN_SKILLS`

- [ ] **Step 4: 实现 `builtin/mod.rs`**

Create `peri-middlewares/src/skills/builtin/mod.rs`：

```rust
//! Builtin skills —— 随二进制分发的 SKILL.md，编译期嵌入。
//!
//! 复用 `built_in_agents.rs` 的 `include_str!` + `&'static str` 模式：
//! 零运行时 I/O，最低优先级（被 User/Global/Project/Plugin 同名覆盖）。
//!
//! 新增 Builtin skill 步骤：
//! 1. 把 SKILL.md 放到 `.claude/skills/<name>/SKILL.md`
//! 2. 在 `BUILTIN_SKILLS` 数组追加 entry
//! 3. 更新 `builtin_test.rs::test_builtin_skills_frontmatter_valid`（已自动遍历，无需手改）

use gray_matter::{engine::YAML, Matter};

/// 单个 builtin skill 的编译期嵌入数据
pub struct BuiltinSkill {
    pub name: &'static str,
    /// SKILL.md 全文（含 frontmatter），通过 `include_str!` 编译期嵌入
    pub content: &'static str,
}

/// 所有 builtin skills 的注册表（编译期常量数组）
///
/// 顺序不影响功能（scan_skill_roots_impl 按 name 去重），但建议按字母排序便于维护。
pub static BUILTIN_SKILLS: &[BuiltinSkill] = &[
    BuiltinSkill {
        name: "use-artifacts",
        content: include_str!("skills/use-artifacts/SKILL.md"),
    },
    // 后续 PR 在此追加 entry
];

/// 从 SKILL.md 全文解析 frontmatter，返回 `(name, description)`。
///
/// 复用 `loader::load_skill_metadata` 的 `gray_matter::Matter::<YAML>` 解析模式。
/// frontmatter 格式错误或缺字段时返回 `None`，由调用方决定是否跳过。
pub fn parse_builtin_frontmatter(content: &str) -> Option<(String, String)> {
    let matter = Matter::<YAML>::new();
    let parsed = matter.parse(content).ok()?;
    let data = parsed.data?;

    #[derive(serde::Deserialize)]
    struct Fm {
        name: String,
        description: String,
    }
    let fm: Fm = data.deserialize().ok()?;
    Some((fm.name, fm.description))
}

#[cfg(test)]
mod tests {
    include!("builtin_test.rs");
}
```

- [ ] **Step 5: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib skills::builtin::tests -- --nocapture`
Expected: 5 个测试全部 PASS

- [ ] **Step 6: Commit**

```bash
git add peri-middlewares/src/skills/builtin/mod.rs \
        peri-middlewares/src/skills/builtin_test.rs \
        peri-middlewares/src/skills/mod.rs
git commit -m "feat(skills): 新增 builtin 模块基础设施——BuiltinSkill + BUILTIN_SKILLS 常量"
```

---

## Task 2: 扩展 `SkillSource` 枚举 + 改造 `resolve_skill_roots` 加 `disable_bundled` 参数

**Files:**
- Modify: `peri-middlewares/src/skills/loader.rs:11-20`（`SkillSource` 枚举）
- Modify: `peri-middlewares/src/skills/loader.rs:241-277`（`resolve_skill_roots` 签名 + 末尾追加 Builtin root）
- Modify: `peri-middlewares/src/skills/loader_test.rs:268, 292, 308`（适配新签名）
- Modify: `peri-middlewares/src/skills/mod.rs:127, 175`（内部调用适配）
- Modify: `peri-middlewares/src/subagent/skill_preload.rs:109`（调用适配）
- Modify: `peri-tui/src/app/mod.rs:321`（调用适配）
- Modify: `peri-acp/src/session/executor.rs:110`（`build_frozen_summary` 调用方暂传 `false`，Task 5 改为读 settings）

- [ ] **Step 1: 写失败测试 `test_resolve_skill_roots_includes_builtin_when_enabled`**

在 `peri-middlewares/src/skills/loader_test.rs` 末尾追加（注意新签名 `resolve_skill_roots(cwd, plugin_roots, disable_bundled)`）：

```rust
#[test]
fn test_resolve_skill_roots_includes_builtin_when_enabled() {
    let roots = resolve_skill_roots("/tmp", vec![], false);
    assert!(
        roots.iter().any(|r| r.source == SkillSource::Builtin),
        "disable_bundled=false 时应含 Builtin root"
    );
}

#[test]
fn test_resolve_skill_roots_excludes_builtin_when_disabled() {
    let roots = resolve_skill_roots("/tmp", vec![], true);
    assert!(
        !roots.iter().any(|r| r.source == SkillSource::Builtin),
        "disable_bundled=true 时不应含 Builtin root"
    );
}

#[test]
fn test_resolve_skill_roots_builtin_is_lowest_priority() {
    // Builtin root 应在末尾（最后被扫描，最低优先级）
    let roots = resolve_skill_roots("/tmp", vec![], false);
    let builtin_idx = roots
        .iter()
        .position(|r| r.source == SkillSource::Builtin)
        .expect("应含 Builtin root");
    assert_eq!(
        builtin_idx,
        roots.len() - 1,
        "Builtin root 应在列表末尾（最低优先级）"
    );
}
```

- [ ] **Step 2: 同时更新现有 3 处旧签名调用**

修改 `peri-middlewares/src/skills/loader_test.rs:268`：

```rust
let roots = resolve_skill_roots(cwd, vec![], false);
```

修改 `peri-middlewares/src/skills/loader_test.rs:292`：

```rust
let roots = resolve_skill_roots("/tmp", vec![plugin_root], false);
```

修改 `peri-middlewares/src/skills/loader_test.rs:308`：

```rust
let roots = resolve_skill_roots("/tmp", vec![nonexistent], false);
```

- [ ] **Step 3: 运行测试验证失败**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests 2>&1 | head -30`
Expected: 编译失败，错误为 `wrong number of arguments` 或 `no variant Builtin in SkillSource`

- [ ] **Step 4: 扩展 `SkillSource` 枚举**

修改 `peri-middlewares/src/skills/loader.rs:11-20`：

```rust
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
    /// 随二进制分发的内置 skill（include_str! 编译期嵌入）
    Builtin,
}
```

- [ ] **Step 5: 改造 `resolve_skill_roots`**

修改 `peri-middlewares/src/skills/loader.rs:241-277`，整体替换为：

```rust
/// 统一解析 skill 根列表，按优先级返回 `SkillRoot`。
///
/// 顺序即去重优先级：User → Global → Project → Plugin → Builtin（先到先得）。
/// 这是 skill 目录解析的 single source of truth，`SkillsMiddleware` 和
/// `SkillPreloadMiddleware` 都应委托此函数。
///
/// `disable_bundled=true` 时跳过 Builtin root（用户通过 settings.json
/// `config.disableBundledSkills: true` 关闭内置 skill）。
pub fn resolve_skill_roots(
    cwd: &str,
    plugin_roots: Vec<SkillRoot>,
    disable_bundled: bool,
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

    // 5. Builtin（最低优先级，path 字段占位，scan 阶段特判跳过 is_dir()）
    if !disable_bundled {
        roots.push(SkillRoot {
            path: PathBuf::new(),
            source: SkillSource::Builtin,
            plugin_name: None,
        });
    }

    roots
}
```

- [ ] **Step 6: 更新 `SkillsMiddleware::resolve_roots_static` 和 `resolve_roots`**

修改 `peri-middlewares/src/skills/mod.rs:126-128`：

```rust
pub fn resolve_roots_static(
    cwd: &str,
    plugin_roots: Vec<SkillRoot>,
    disable_bundled: bool,
) -> Vec<SkillRoot> {
    loader::resolve_skill_roots(cwd, plugin_roots, disable_bundled)
}
```

修改 `peri-middlewares/src/skills/mod.rs:174-176`（`resolve_roots` 内部 `else` 分支）：

```rust
} else {
    loader::resolve_skill_roots(cwd, self.plugin_roots.clone(), self.disable_bundled)
}
```

同时在 `SkillsMiddleware` struct 加字段（`mod.rs:52-59` 附近）：

```rust
pub struct SkillsMiddleware {
    project_skills_dir: Option<PathBuf>,
    global_skills_dir: Option<PathBuf>,
    user_skills_dir: Option<PathBuf>,
    plugin_roots: Vec<SkillRoot>,
    /// Frozen skills summary (None = scan each turn from disk).
    frozen_summary: Option<String>,
    /// 是否禁用 builtin skill（session/new 时一次性读取冻结）
    disable_bundled: bool,
}
```

更新 `SkillsMiddleware::new()`（`mod.rs:62-70`）：

```rust
pub fn new() -> Self {
    Self {
        project_skills_dir: None,
        global_skills_dir: None,
        user_skills_dir: None,
        plugin_roots: vec![],
        frozen_summary: None,
        disable_bundled: false,
    }
}
```

在 `impl SkillsMiddleware` 中（如 `with_frozen_summary` 之后，约 `mod.rs:110`）追加 builder：

```rust
/// 设置是否禁用 builtin skill（默认 false）
pub fn with_disable_bundled(mut self, disable: bool) -> Self {
    self.disable_bundled = disable;
    self
}
```

- [ ] **Step 7: 更新 `SkillPreloadMiddleware` 调用点**

修改 `peri-middlewares/src/subagent/skill_preload.rs:59-79`，struct 加 `disable_bundled` 字段：

```rust
pub struct SkillPreloadMiddleware {
    skill_names: Vec<String>,
    cwd: String,
    plugin_roots: Vec<SkillRoot>,
    disable_bundled: bool,
}

impl SkillPreloadMiddleware {
    pub fn new(skill_names: Vec<String>, cwd: &str) -> Self {
        Self {
            skill_names,
            cwd: cwd.to_string(),
            plugin_roots: Vec::new(),
            disable_bundled: false,
        }
    }

    /// 追加插件 skills 搜索根
    pub fn with_plugin_roots(mut self, roots: Vec<SkillRoot>) -> Self {
        self.plugin_roots = roots;
        self
    }

    /// 设置是否禁用 builtin skill
    pub fn with_disable_bundled(mut self, disable: bool) -> Self {
        self.disable_bundled = disable;
        self
    }
}
```

修改 `peri-middlewares/src/subagent/skill_preload.rs:109`：

```rust
let roots = resolve_skill_roots(&self.cwd, self.plugin_roots.clone(), self.disable_bundled);
```

- [ ] **Step 8: 更新 `peri-tui/src/app/mod.rs:321`**

```rust
let skill_roots = peri_middlewares::skills::resolve_skill_roots(
    &self.services.cwd,
    plugin_skill_roots,
    false,  // TUI 侧仅用于显示，Builtin 总是参与
);
```

- [ ] **Step 9: 更新 `peri-acp/src/session/executor.rs:110`（暂时硬编码 false）**

```rust
// 暂时硬编码 false，Task 5 改为从 settings 读取
let skill_summary = peri_middlewares::SkillsMiddleware::build_frozen_summary(
    cwd,
    plugin_skill_roots.to_vec(),
    false,
);
```

- [ ] **Step 10: 更新 `SkillsMiddleware::build_frozen_summary` 签名**

修改 `peri-middlewares/src/skills/mod.rs:116-123`：

```rust
pub fn build_frozen_summary(
    cwd: &str,
    plugin_roots: Vec<SkillRoot>,
    disable_bundled: bool,
) -> Option<String> {
    let roots = Self::resolve_roots_static(cwd, plugin_roots, disable_bundled);
    let skills = scan_skill_roots(&roots);
    if skills.is_empty() {
        return None;
    }
    Some(Self::build_summary(&skills))
}
```

- [ ] **Step 11: 全量编译验证**

Run: `cargo build -p peri-middlewares 2>&1 | tail -20`
Expected: 编译通过（所有调用点已更新）

Run: `cargo build -p peri-acp -p peri-tui 2>&1 | tail -20`
Expected: 编译通过

- [ ] **Step 12: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib skills:: 2>&1 | tail -30`
Expected: 所有测试 PASS（含 3 个新增 Builtin root 测试）

- [ ] **Step 13: Commit**

```bash
git add peri-middlewares/src/skills/loader.rs \
        peri-middlewares/src/skills/loader_test.rs \
        peri-middlewares/src/skills/mod.rs \
        peri-middlewares/src/subagent/skill_preload.rs \
        peri-tui/src/app/mod.rs \
        peri-acp/src/session/executor.rs
git commit -m "feat(skills): SkillSource::Builtin + resolve_skill_roots 加 disable_bundled 参数"
```

---

## Task 3: 改造 `scan_skill_roots_impl` 加 Builtin 特判分支

**Files:**
- Modify: `peri-middlewares/src/skills/loader.rs:107-139`（`scan_skill_roots_impl` 主循环）
- Modify: `peri-middlewares/src/skills/loader_test.rs`（追加测试）

- [ ] **Step 1: 写失败测试**

在 `peri-middlewares/src/skills/loader_test.rs` 末尾追加：

```rust
#[test]
fn test_scan_skill_roots_builtin_returns_metadata() {
    // 仅含 Builtin root 时，应返回 BUILTIN_SKILLS 的 metadata
    let roots = vec![SkillRoot {
        path: PathBuf::new(),
        source: SkillSource::Builtin,
        plugin_name: None,
    }];
    let skills = scan_skill_roots(&roots);
    assert!(
        skills.iter().any(|s| s.name == "use-artifacts"
            && s.source == SkillSource::Builtin
            && s.path == PathBuf::from("<builtin>/use-artifacts")),
        "应含 use-artifacts 的 Builtin metadata，path=<builtin>/use-artifacts，实际: {:?}",
        skills
    );
}

#[test]
fn test_scan_skill_roots_builtin_skipped_when_path_invalid() {
    // 即使 path 字段是空（占位），Builtin source 也应正常扫描
    // （特判分支跳过 is_dir() 检查）
    let roots = vec![SkillRoot {
        path: PathBuf::new(),  // 空路径占位
        source: SkillSource::Builtin,
        plugin_name: None,
    }];
    let skills = scan_skill_roots(&roots);
    assert!(!skills.is_empty(), "Builtin source 不应因 path 为空被跳过");
}

#[test]
fn test_scan_skill_roots_user_overrides_builtin() {
    // User root 有同名 use-artifacts，应胜出（User 描述 + source=User）
    let user_dir = tempdir().unwrap();
    write_skill_file(
        &user_dir.path().join("use-artifacts").join("SKILL.md"),
        "use-artifacts",
        "from user override",
    );

    let roots = vec![
        SkillRoot {
            path: user_dir.path().to_path_buf(),
            source: SkillSource::User,
            plugin_name: None,
        },
        SkillRoot {
            path: PathBuf::new(),
            source: SkillSource::Builtin,
            plugin_name: None,
        },
    ];
    let skills = scan_skill_roots(&roots);
    let ua = skills.iter().find(|s| s.name == "use-artifacts").unwrap();
    assert_eq!(ua.source, SkillSource::User, "User 应覆盖 Builtin");
    assert_eq!(ua.description, "from user override");
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests::test_scan_skill_roots_builtin 2>&1 | tail -20`
Expected: FAIL（当前 `is_dir()` 检查会跳过空 path，builtin root 被忽略）

- [ ] **Step 3: 改造 `scan_skill_roots_impl` 加 Builtin 特判**

修改 `peri-middlewares/src/skills/loader.rs:107-139`，整体替换为：

```rust
fn scan_skill_roots_impl(
    roots: &[SkillRoot],
    max_depth: usize,
    max_dirs: usize,
) -> Vec<SkillMetadata> {
    let mut seen: HashMap<String, SkillMetadata> = HashMap::new();
    let mut ordered: Vec<String> = Vec::new();

    for root in roots {
        // Builtin 特判：跳过磁盘扫描，直接从编译期常量数组加载。
        // path 字段对 Builtin 是占位（PathBuf::new()），不走 is_dir() 检查。
        if matches!(root.source, SkillSource::Builtin) {
            for skill in crate::skills::builtin::BUILTIN_SKILLS {
                let parsed = crate::skills::builtin::parse_builtin_frontmatter(skill.content);
                let Some((name, description)) = parsed else {
                    tracing::warn!(
                        "builtin skill {} frontmatter 解析失败，跳过",
                        skill.name
                    );
                    continue;
                };
                let meta = SkillMetadata {
                    name,
                    description,
                    path: PathBuf::from(format!("<builtin>/{}", skill.name)),
                    source: SkillSource::Builtin,
                    plugin_name: None,
                };
                insert_skill(meta, root, &mut seen, &mut ordered);
            }
            continue;
        }

        if !root.path.is_dir() {
            continue;
        }
        // 每 root 独立 visited/dir_count，避免跨 root 配额污染与误判环
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

    ordered
        .into_iter()
        .filter_map(|n| seen.remove(&n))
        .collect()
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib skills::loader::tests 2>&1 | tail -20`
Expected: 所有测试 PASS（含 3 个新增 Builtin 扫描测试）

- [ ] **Step 5: 全量回归**

Run: `cargo test -p peri-middlewares --lib 2>&1 | tail -10`
Expected: 全部 PASS

- [ ] **Step 6: Commit**

```bash
git add peri-middlewares/src/skills/loader.rs \
        peri-middlewares/src/skills/loader_test.rs
git commit -m "feat(skills): scan_skill_roots_impl 加 Builtin 特判分支，从常量数组加载"
```

---

## Task 4: 新增 `load_disable_bundled_skills` 函数

**Files:**
- Modify: `peri-middlewares/src/skills/mod.rs`（追加 `load_disable_bundled_skills` 函数）

- [ ] **Step 1: 写失败测试**

在 `peri-middlewares/src/skills/mod_test.rs` 末尾追加：

```rust
#[test]
fn test_load_disable_bundled_skills_defaults_false_when_missing() {
    // settings.json 无 disableBundledSkills 字段时返回 false
    let tmp = tempdir().unwrap();
    let settings_path = tmp.path().join("settings.json");
    std::fs::write(
        &settings_path,
        r#"{"config": {}}"#,
    )
    .unwrap();

    let value = super::load_disable_bundled_skills_from_path(&settings_path);
    assert!(!value, "缺字段时应默认 false");
}

#[test]
fn test_load_disable_bundled_skills_reads_true() {
    let tmp = tempdir().unwrap();
    let settings_path = tmp.path().join("settings.json");
    std::fs::write(
        &settings_path,
        r#"{"config": {"disableBundledSkills": true}}"#,
    )
    .unwrap();

    let value = super::load_disable_bundled_skills_from_path(&settings_path);
    assert!(value, "disableBundledSkills=true 时应返回 true");
}

#[test]
fn test_load_disable_bundled_skills_reads_false_explicit() {
    let tmp = tempdir().unwrap();
    let settings_path = tmp.path().join("settings.json");
    std::fs::write(
        &settings_path,
        r#"{"config": {"disableBundledSkills": false}}"#,
    )
    .unwrap();

    let value = super::load_disable_bundled_skills_from_path(&settings_path);
    assert!(!value);
}

#[test]
fn test_load_disable_bundled_skills_handles_missing_file() {
    // 文件不存在时返回 false
    let value =
        super::load_disable_bundled_skills_from_path(std::path::Path::new("/nonexistent.json"));
    assert!(!value);
}
```

- [ ] **Step 2: 运行测试验证失败**

Run: `cargo test -p peri-middlewares --lib skills::tests::test_load_disable_bundled_skills 2>&1 | tail -20`
Expected: 编译失败，错误 `cannot find function load_disable_bundled_skills_from_path`

- [ ] **Step 3: 实现 `load_disable_bundled_skills` + `_from_path`**

在 `peri-middlewares/src/skills/mod.rs` 中 `load_global_skills_dir` 函数（行 23-41）之后追加：

```rust
/// 从 `~/.peri/settings.json` 读取 `disableBundledSkills` 配置（默认 false）
///
/// session/new 时一次性读取并冻结，会话内不再重新读取（保持系统提示词稳定性）。
pub fn load_disable_bundled_skills() -> bool {
    load_disable_bundled_skills_from_path(&global_config_path())
}

/// 测试注入入口：从指定 settings 文件读取 disableBundledSkills
pub fn load_disable_bundled_skills_from_path(path: &std::path::Path) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(json): Result<serde_json::Value, _> = serde_json::from_str(&content) else {
        return false;
    };
    // 支持嵌套 { "config": { "disableBundledSkills": ... } } 或扁平
    json.get("config")
        .and_then(|c| c.get("disableBundledSkills"))
        .or_else(|| json.get("disableBundledSkills"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
}
```

- [ ] **Step 4: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib skills::tests::test_load_disable_bundled_skills 2>&1 | tail -10`
Expected: 4 个测试全部 PASS

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/skills/mod.rs \
        peri-middlewares/src/skills/mod_test.rs
git commit -m "feat(skills): 新增 load_disable_bundled_skills 从 settings 读取配置"
```

---

## Task 5: `FrozenSessionData::build` 读 settings + 传 `disable_bundled`

**Files:**
- Modify: `peri-acp/src/session/executor.rs:100-113`（`FrozenSessionData::build`）
- Modify: `peri-tui/src/acp_server/requests.rs:100, 239`（Task 2 发现的额外调用点——`session/new` + `session/load` 的 AvailableCommands 扫描）
- Modify: `peri-tui/src/acp_stdio/commands.rs:16`（Task 2 发现的额外调用点——Stdio `send_available_commands`）

> **注**：Task 2 实现期间发现这 3 处 `resolve_roots_static` 调用点也需要从 settings 读 `disable_bundled`。但这些是 TUI/Stdio 显示路径（用于响应 AvailableCommands），与 main agent 的 frozen_skill_summary 路径独立。
>
> - `executor.rs::FrozenSessionData::build` 走 main agent frozen 路径，必须在 session/new 时一次性读 settings 冻结
> - `requests.rs` + `commands.rs` 是每次 session/new、session/load 时同步调用，会读取最新 settings——直接调 `load_disable_bundled_skills()` 即可（无需 frozen）

- [ ] **Step 1: 修改 `FrozenSessionData::build`**

修改 `peri-acp/src/session/executor.rs:100-113`，整体替换 `build` 函数开头部分（含 `let skill_summary = ...`）：

```rust
pub fn build(
    cwd: &str,
    language: Option<&str>,
    plugin_skill_roots: &[peri_middlewares::skills::SkillRoot],
    plugin_agent_dirs: &[std::path::PathBuf],
    frozen_date: &str,
) -> Self {
    let (claude_md, claude_local_md) =
        peri_middlewares::AgentsMdMiddleware::read_frozen_content(cwd);

    // 一次性读取 disableBundledSkills 并冻结到 frozen_skill_summary
    // （保持系统提示词稳定性：会话内不重读）
    let disable_bundled = peri_middlewares::skills::load_disable_bundled_skills();
    let skill_summary = peri_middlewares::SkillsMiddleware::build_frozen_summary(
        cwd,
        plugin_skill_roots.to_vec(),
        disable_bundled,
    );

    let features = crate::prompt::PromptFeatures::detect();
    // ... 后续代码不变 ...
```

- [ ] **Step 2: 修改 `peri-tui/src/acp_server/requests.rs` 两处**

找到 Task 2 中加的 `false,  // TUI 侧仅用于显示...` 注释（约行 100 和 239），替换为：

```rust
let disable_bundled = peri_middlewares::skills::load_disable_bundled_skills();
let roots = peri_middlewares::SkillsMiddleware::resolve_roots_static(
    cwd,
    plugin_skill_roots,
    disable_bundled,
);
```

如果原代码直接调用 `resolve_skill_roots`（非 `resolve_roots_static`），同样加 `load_disable_bundled_skills()` 读取。

- [ ] **Step 3: 修改 `peri-tui/src/acp_stdio/commands.rs:16`**

同 Step 2，把 `false` 替换为 `peri_middlewares::skills::load_disable_bundled_skills()`。

- [ ] **Step 4: 编译验证**

Run: `cargo build -p peri-acp -p peri-tui 2>&1 | tail -10`
Expected: 编译通过

- [ ] **Step 5: 验证 `load_disable_bundled_skills` 是 pub**

如果上一步报 `cannot find function`，在 `peri-middlewares/src/lib.rs` 的 `pub mod skills;` 之外确认 `skills::load_disable_bundled_skills` 已通过 `pub fn` 暴露（`mod.rs` 中是 `pub fn`，自动 re-export）。

Run: `cargo doc -p peri-middlewares --no-deps 2>&1 | tail -5`
Expected: 无错误

- [ ] **Step 6: 运行现有 executor 测试确保无回归**

Run: `cargo test -p peri-acp --lib session::executor 2>&1 | tail -10`
Expected: 全部 PASS（如果存在的话）

- [ ] **Step 7: Commit**

```bash
git add peri-acp/src/session/executor.rs \
        peri-tui/src/acp_server/requests.rs \
        peri-tui/src/acp_stdio/commands.rs
git commit -m "feat(acp/tui): 读取 disableBundledSkills 并传入 resolve_roots/build_frozen_summary"
```

---

## Task 6: 改造 `SkillPreloadMiddleware` 支持 Builtin source 全文加载

**Files:**
- Modify: `peri-middlewares/src/subagent/skill_preload.rs:113-136`（`spawn_blocking` 内的 `filter_map`）
- Modify: `peri-middlewares/src/subagent/skill_preload.rs`（构造处传入 disable_bundled——可选）
- Modify: `peri-middlewares/src/subagent/tool/mod.rs:92-93`（如需传入 frozen disable_bundled 给 SkillPreloadMiddleware）

- [ ] **Step 1: 先了解 SkillPreloadMiddleware 的构造路径**

Run: `grep -rn "SkillPreloadMiddleware::new\|SkillPreloadMiddleware::" peri-middlewares/src peri-acp/src 2>&1 | head -20`
记录所有构造点。典型构造点在 `subagent/tool/mod.rs`（SubAgent 路径）和 `peri-acp/src/agent/builder.rs`（Main agent 路径）。

- [ ] **Step 2: 写失败测试**

在 `peri-middlewares/src/subagent/skill_preload_test.rs` 末尾追加（参考文件已有测试风格）：

```rust
#[tokio::test]
async fn test_preload_loads_builtin_skill_content() {
    // SubAgent 路径：显式 skill_names 含 use-artifacts，应能从 BUILTIN_SKILLS 加载全文
    let mut state = peri_agent::agent::state::AgentState::new("/tmp");
    state.add_message(peri_agent::messages::BaseMessage::human("hi"));

    let mw = super::SkillPreloadMiddleware::new(
        vec!["use-artifacts".to_string()],
        "/tmp",
    );

    mw.before_agent(&mut state).await.unwrap();

    // 应注入 Ai + Tool 消息（共 2 条），且 ToolResult 含 BUILTIN_SKILLS 的 SKILL.md 内容
    let msgs = state.messages();
    let tool_result_content = msgs
        .iter()
        .filter_map(|m| match m {
            peri_agent::messages::BaseMessage::Tool { content, .. } => Some(content.clone()),
            _ => None,
        })
        .next()
        .expect("应有 ToolResult 消息");
    assert!(
        tool_result_content.contains("Artifact"),
        "ToolResult 应含 BUILTIN_SKILLS 的 SKILL.md 全文，实际: {}",
        tool_result_content
    );
}
```

- [ ] **Step 3: 运行测试验证失败**

Run: `cargo test -p peri-middlewares --lib subagent::skill_preload::tests::test_preload_loads_builtin_skill_content 2>&1 | tail -20`
Expected: FAIL（当前 `std::fs::read_to_string(&s.path)` 对 `<builtin>/use-artifacts` 路径返回错误，filter_map 跳过）

- [ ] **Step 4: 改造 `skill_preload.rs` 的 `filter_map` 分支**

修改 `peri-middlewares/src/subagent/skill_preload.rs:113-136`，整体替换 `spawn_blocking` 闭包：

```rust
let skill_contents = tokio::task::spawn_blocking(move || {
    let all_skills = scan_skill_roots(&roots);
    all_skills
        .into_iter()
        .filter(|s| {
            let skill_name_lower = s.name.to_lowercase();
            names_lower.iter().any(|name| {
                // 精确匹配（/plan）
                skill_name_lower == *name
                // 或去掉命名空间前缀后匹配（/ecc:plan → plan）
                || name.rsplit_once(':').map(|(_, n)| n.to_lowercase()).as_deref() == Some(&skill_name_lower)
            })
        })
        .filter_map(|s| {
            // Builtin source 走常量数组查找，其他走磁盘读取
            // 注意：本文件在 peri-middlewares crate 内部，必须用 crate:: 路径
            // （不能用 peri_middlewares::，否则编译失败）
            let content = if matches!(s.source, crate::skills::SkillSource::Builtin) {
                crate::skills::builtin::BUILTIN_SKILLS
                    .iter()
                    .find(|bs| bs.name == s.name)
                    .map(|bs| bs.content.to_string())
            } else {
                std::fs::read_to_string(&s.path).ok()
            };
            content.map(|c| (s.path.to_string_lossy().to_string(), c))
        })
        .collect::<Vec<_>>()
})
.await
.map_err(|e| peri_agent::error::AgentError::MiddlewareError {
    middleware: "SkillPreloadMiddleware".to_string(),
    reason: format!("spawn_blocking 失败: {e}"),
})?;
```

注意：`SkillSource` 需要在 `peri-middlewares/src/skills/mod.rs` 的 `pub use loader::{...}` 中导出（已含 `SkillSource`，见 `mod.rs:7`）。

- [ ] **Step 5: 确保 `SkillPreloadMiddleware` 默认 disable_bundled=false（构造时含 Builtin）**

Main agent 构造路径在 `peri-acp/src/agent/builder.rs`。Run：

```bash
grep -n "SkillPreloadMiddleware" peri-acp/src/agent/builder.rs
```

在构造处添加 `.with_disable_bundled(...)`（如果需要从 frozen 配置传）。对于 V1，preload 始终允许 Builtin（默认 false 即可），所以无需改动 Main agent 构造。

SubAgent 构造路径在 `peri-middlewares/src/subagent/tool/mod.rs`。Run：

```bash
grep -n "SkillPreloadMiddleware" peri-middlewares/src/subagent/tool/mod.rs
```

同样保持默认（false）即可——SubAgent 也需要能 `/use-artifacts`。

- [ ] **Step 6: 运行测试验证通过**

Run: `cargo test -p peri-middlewares --lib subagent::skill_preload::tests 2>&1 | tail -15`
Expected: 所有测试 PASS（含新增的 `test_preload_loads_builtin_skill_content`）

- [ ] **Step 7: 全量回归**

Run: `cargo test -p peri-middlewares --lib 2>&1 | tail -10`
Expected: 全部 PASS

Run: `cargo build -p peri-acp -p peri-tui 2>&1 | tail -10`
Expected: 编译通过

- [ ] **Step 8: Commit**

```bash
git add peri-middlewares/src/subagent/skill_preload.rs \
        peri-middlewares/src/subagent/skill_preload_test.rs
git commit -m "feat(skills): SkillPreloadMiddleware 支持 Builtin source 全文加载"
```

---

## Task 7: 端到端集成测试 + 手动验证

**Files:**
- 无代码改动，仅验证

- [ ] **Step 1: 验证 build_frozen_summary 含 builtin**

写一个一次性测试验证整条链路。Create `peri-middlewares/src/skills/e2e_test.rs`（临时，验证后删除）：

```rust
#[test]
fn test_e2e_frozen_summary_contains_builtin_use_artifacts() {
    let summary = super::SkillsMiddleware::build_frozen_summary(
        "/tmp",
        vec![],
        false,  // disable_bundled=false
    );
    let summary = summary.expect("非空时应返回 Some");
    assert!(
        summary.contains("use-artifacts"),
        "frozen summary 应含 builtin use-artifacts，实际: {}",
        summary
    );
    assert!(
        summary.contains("<builtin>/use-artifacts"),
        "frozen summary 应含虚拟路径 <builtin>/use-artifacts，实际: {}",
        summary
    );
}

#[test]
fn test_e2e_frozen_summary_excludes_builtin_when_disabled() {
    let summary = super::SkillsMiddleware::build_frozen_summary(
        "/tmp",
        vec![],
        true,  // disable_bundled=true
    );
    // 可能返回 None（无任何 skill）或 Some（仅含磁盘 skill）
    if let Some(s) = summary {
        assert!(
            !s.contains("use-artifacts") || !s.contains("<builtin>/use-artifacts"),
            "disable_bundled=true 时不应含 Builtin use-artifacts，实际: {}",
            s
        );
    }
}
```

- [ ] **Step 2: 在 `skills/mod.rs` 的 `#[cfg(test)] mod tests` 中临时 include**

修改 `peri-middlewares/src/skills/mod.rs:242-249` 的 tests mod：

```rust
#[cfg(test)]
mod tests {
    use peri_agent::agent::state::AgentState;
    use tempfile::tempdir;

    use super::*;
    include!("mod_test.rs");
    include!("e2e_test.rs");
}
```

- [ ] **Step 3: 运行测试验证**

Run: `cargo test -p peri-middlewares --lib skills::tests::test_e2e 2>&1 | tail -10`
Expected: 2 个 E2E 测试 PASS

- [ ] **Step 4: 删除临时 E2E 测试文件，把核心断言搬到 `mod_test.rs`**

Delete `peri-middlewares/src/skills/e2e_test.rs`。

把 `mod.rs` 的 tests mod 恢复：

```rust
#[cfg(test)]
mod tests {
    use peri_agent::agent::state::AgentState;
    use tempfile::tempdir;

    use super::*;
    include!("mod_test.rs");
}
```

把两个 E2E 测试函数追加到 `mod_test.rs` 末尾（保持长期覆盖）。

- [ ] **Step 5: 手动验证**

Run: `cargo run -p peri-tui -- -a`
在 TUI 中：
1. 启动新 session
2. 让 LLM 看到的 system prompt 含 `use-artifacts`（可以通过追问"列出所有可用 skills"验证）
3. 输入 `/use-artifacts`，验证 SkillPreloadMiddleware 能加载 builtin 全文（agent 能描述 artifact 工具用法）

- [ ] **Step 6: 手动验证 disableBundledSkills 开关**

Run: 在 `~/.peri/settings.json` 写入：
```json
{ "config": { "disableBundledSkills": true } }
```

重启 TUI，验证 system prompt 不再含 `<builtin>/use-artifacts`。

验证后恢复（删除 disableBundledSkills 字段或设为 false）。

- [ ] **Step 7: 全量回归**

Run: `cargo test --workspace 2>&1 | tail -20`
Expected: 全部 PASS（无回归）

Run: `lefthook run pre-commit 2>&1 | tail -10`
Expected: 全部 PASS

- [ ] **Step 8: Commit**

```bash
git add peri-middlewares/src/skills/mod_test.rs \
        peri-middlewares/src/skills/mod.rs
git commit -m "test(skills): 端到端验证 build_frozen_summary 含/不含 Builtin"
```

---

## Task 8: 文档更新

**Files:**
- Modify: `peri-middlewares/CLAUDE.md`（Skills 段落补充 Builtin source）
- Modify: `CLAUDE.md`（项目根，提及 Builtin source）

- [ ] **Step 1: 更新 `peri-middlewares/CLAUDE.md` 的 Skills 段落**

找到 `**Skills**：搜索顺序 ...` 段落（约 `peri-middlewares/CLAUDE.md` 中部），改为：

```markdown
**Skills**：搜索顺序 `~/.claude/skills/` → `skillsDir` → `./.claude/skills/` → 插件 skills → **Builtin（随二进制分发）**。统一收口到 `scan_skill_roots(roots: &[SkillRoot])`：递归扫描（深度上限 6，目录数上限 1000/root），symlink 跟随 + canonicalize 防环，叶子语义（含 SKILL.md 则停止下钻），跨根去重按 roots 顺序先到先得。`SkillsMiddleware.with_plugin_roots()` 是插件扩展点。

**Builtin skills**：`include_str!` 编译期嵌入 SKILL.md 到二进制（注册表 `skills::builtin::BUILTIN_SKILLS`），最低优先级，被任意层级同名覆盖。`scan_skill_roots_impl` 主循环对 `SkillSource::Builtin` 特判（跳过 `is_dir()` 检查，直接从常量数组加载）。虚拟路径 `<builtin>/<name>` 不对应磁盘文件，`SkillPreloadMiddleware` 通过 `source == Builtin` 判断走常量查找。settings.json `config.disableBundledSkills: true` 全局禁用，session/new 时一次性冻结。新增 builtin skill：把 SKILL.md 放到 `.claude/skills/<name>/`，在 `BUILTIN_SKILLS` 数组追加 entry（`test_builtin_skills_frontmatter_valid` 自动覆盖）。
```

- [ ] **Step 2: 更新项目根 `CLAUDE.md` 的 Tool Search 章节**

找到 "Tool Search 延迟加载" 章节，在末尾追加段落：

```markdown
**Builtin Skills（随二进制分发的 SKILL.md）**：参考 Claude Code bundled skills 特性，`SkillSource::Builtin` 是第 5 种 skill 来源，最低优先级。`include_str!` 编译期嵌入，`scan_skill_roots_impl` 特判分支加载，`settings.json::config.disableBundledSkills: true` 全局禁用。详见 `docs/superpowers/specs/2026-06-20-builtin-skills-design.md`。
```

- [ ] **Step 3: Commit**

```bash
git add peri-middlewares/CLAUDE.md CLAUDE.md
git commit -m "docs: 更新 CLAUDE.md 记录 Builtin skills 特性"
```

---

## Self-Review

**Spec coverage 检查**：

| Spec 章节 | 实现 Task |
|----------|----------|
| SkillSource 枚举扩展 | Task 2 Step 4 |
| BuiltinSkill + BUILTIN_SKILLS 常量 | Task 1 Step 4 |
| parse_builtin_frontmatter | Task 1 Step 4 |
| resolve_skill_roots 加 disable_bundled 参数 | Task 2 Step 5 |
| scan_skill_roots_impl Builtin 特判 | Task 3 Step 3 |
| SkillsMiddleware::build_frozen_summary 加参数 | Task 2 Step 10 |
| SkillsMiddleware struct 加 disable_bundled 字段 | Task 2 Step 6 |
| SkillPreloadMiddleware Builtin 全文加载 | Task 6 Step 4 |
| SkillsConfig.disable_bundled_skills 字段 | Task 4（用函数 `load_disable_bundled_skills` 实现，而非 struct 字段——更轻量） |
| session/new 读 settings 传参 | Task 5 Step 1 |
| 测试矩阵 8 项 | Task 1（2 项）+ Task 2（3 项）+ Task 3（3 项）+ Task 4（4 项）+ Task 6（1 项）+ Task 7（2 项 E2E） |
| 文档更新 | Task 8 |

**Placeholder 扫描**：✅ 无 TBD/TODO/等占位符

**Type consistency**：
- `SkillSource::Builtin` 全程一致
- `BUILTIN_SKILLS`、`parse_builtin_frontmatter`、`BuiltinSkill` 命名一致
- `disable_bundled: bool` 参数名一致
- `<builtin>/<name>` 虚拟路径格式一致
- `load_disable_bundled_skills` 函数名一致

**潜在问题**：
- Task 2 Step 6 同时改了 `SkillsMiddleware` struct 和 `resolve_roots_static`，调用方（`build_frozen_summary` Task 2 Step 10）依赖 `resolve_roots_static` 新签名——顺序正确
- Task 6 Step 1 的 grep 命令需要工程师执行后确认实际构造点——已说明
