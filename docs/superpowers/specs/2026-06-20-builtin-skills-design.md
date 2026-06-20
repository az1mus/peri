# Builtin Skills 设计

**日期**: 2026-06-20
**作者**: KonghaYao + Claude
**状态**: Draft（待用户审阅）

## 背景与动机

Claude Code 提供了 **bundled skills** 特性——一批随二进制分发的 SKILL.md，所有用户开箱即用，无需从 marketplace 或远程仓库安装。官方 bundled skills 包括 `/code-review`、`/debug`、`/batch`、`/loop`、`/simplify` 等。bundled skill 与普通 skill 在加载/触发机制上完全一致，差别仅在存储位置（编译期嵌入 vs 磁盘文件）和优先级（最低，可被任意层级同名覆盖）。

Perihelion 目前的 skills 系统已具备完整的加载链（`SkillsMiddleware` 摘要注入 + `SkillPreloadMiddleware` 全文加载）、4 种来源（User/Global/Project/Plugin）、frozen summary 稳定性保证。同时 `peri-middlewares/src/subagent/built_in_agents.rs` 已经为"内置 agents"实现了成熟的编译期嵌入模式（`include_str!` + `&'static` 数组 + 最低优先级 + 可被项目级同名覆盖）。

本设计在现有 skills 系统上引入第 5 种来源 `SkillSource::Builtin`，复用 `built_in_agents.rs` 的成熟模式，让 perihelion 二进制自带一批开箱即用的 skill。

## 现状调研

### Perihelion Skills 系统

- `SkillsMiddleware`（`peri-middlewares/src/skills/mod.rs:52`）在 `before_agent` 钩子把 frozen summary 作为 system message prepend
- `SkillSource` 枚举（`loader.rs`）当前有 User / Global / Project / Plugin 四种
- `resolve_skill_roots` → `scan_skill_roots` → `build_summary` 是核心加载链
- `frozen_skill_summary` 在 session/new 时一次性扫描冻结，保证 prompt cache 稳定
- `SkillPreloadMiddleware` 在用户输入 `/skill-name` 时加载全文
- Skills 仅注入 name + description + path 到 system prompt（约 100 tokens/skill）
- 4 种来源优先级：User (`~/.claude/skills/`) > Global (`config.skillsDir`) > Project (`{cwd}/.claude/skills/`) > Plugin

### `built_in_agents.rs` 模板

```rust
// peri-middlewares/src/subagent/built_in_agents.rs:34-59
pub struct BuiltInAgent {
    pub agent_id: &'static str,
    pub content: &'static str,
}

pub static BUILT_IN_AGENTS: &[BuiltInAgent] = &[
    BuiltInAgent {
        agent_id: "coder",
        content: include_str!("agents/coder.md"),
    },
    // ...
];
```

最低优先级，项目级同名 agent 覆盖内置。本设计完全复用此模式。

### Claude Code bundled skills 关键特性

- SKILL.md 随二进制分发，不存于文件系统
- 加载方式与普通 skill 完全一致
- 同名覆盖（enterprise > personal > project），`disableBundledSkills: true` 全局禁用
- `skillOverrides` 可逐个开关（on/name-only/user-invocable-only/off）

## 目标与非目标

### 目标

1. 引入 `SkillSource::Builtin` 作为第 5 种 skill 来源，优先级最低
2. 用 `include_str!` 编译期嵌入 SKILL.md 到 `peri-middlewares` 二进制
3. 复用现有扫描/去重/优先级链路，改动面最小
4. 提供 `disableBundledSkills` 全局开关，session/new 时一次性冻结
5. 首批内置 1 个验证 skill（`use-artifacts`），证明整条链路可用
6. 保持系统提示词稳定性第一优先级

### 非目标

1. **不做逐个开关**：不实现 `skillOverrides`（on/name-only/user-invocable-only/off），用户只能通过全局开关或同名覆盖禁用 builtin
2. **不做 `/skills` 菜单集成**：不在 TUI 加内置 skill 的可视化标记或 Space 切换交互
3. **不支持多文件 skill**：每个 builtin skill 只能是单个 SKILL.md（不支持 assets/scripts/references 子目录）
4. **不实现 skill marketplace**：builtin 随二进制分发，不涉及远程仓库
5. **不对标 Claude Code 官方 bundled skill 内容**：首批仅内置 1 个 perihelion 专属 skill，后续 PR 增补

## 架构设计

设计原则：**最小侵入**。复用现有 `SkillRoot` struct（不引入新 trait，不新增 struct），通过给 `SkillSource` 加 `Builtin` 变体 + 在 `scan_skill_roots_impl` 主循环特判实现。

### SkillSource 枚举扩展

```rust
// peri-middlewares/src/skills/loader.rs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SkillSource {
    User,
    Global,
    Project,
    Plugin,
    Builtin,  // 【新增】
}
```

### BuiltinSkill 与 BUILTIN_SKILLS 常量

```rust
// peri-middlewares/src/skills/builtin/mod.rs
pub struct BuiltinSkill {
    pub name: &'static str,
    pub content: &'static str,  // include_str! 嵌入的 SKILL.md 全文
}

pub static BUILTIN_SKILLS: &[BuiltinSkill] = &[
    BuiltinSkill {
        name: "use-artifacts",
        content: include_str!("skills/use-artifacts/SKILL.md"),
    },
    // 后续 PR 在此追加
];

/// 从 SKILL.md 全文解析 frontmatter，返回 (name, description)。
/// 复用 loader::load_skill_metadata 的解析模式（gray_matter YAML engine）。
/// frontmatter 格式错误时返回 None（调用方决定是否跳过）。
pub fn parse_builtin_frontmatter(content: &str) -> Option<(String, String)> {
    let matter = gray_matter::Matter::<gray_matter::engine::YAML>::new();
    let parsed = matter.parse(content).ok()?;
    let data = parsed.data?;
    #[derive(serde::Deserialize)]
    struct Fm { name: String, description: String }
    let fm: Fm = data.deserialize().ok()?;
    Some((fm.name, fm.description))
}
```

复用 `built_in_agents.rs` 的 `&'static str` 编译期嵌入模式，零运行时 I/O。

**`include_str!` 路径解析**：SKILL.md 存放在 `peri-middlewares/src/skills/builtin/skills/<name>/SKILL.md`（与 crate 源码同管理，纳入版本控制）。从 `builtin/mod.rs` 出发用相对路径 `skills/<name>/SKILL.md` 解析。编译期解析，路径错误会编译失败。

**目录结构**：

```
peri-middlewares/src/skills/builtin/
├── mod.rs                       # BuiltinSkill + BUILTIN_SKILLS + parse_builtin_frontmatter
├── builtin_test.rs              # 单元测试
└── skills/                      # 内置 SKILL.md 存放处（随 crate 源码版本控制）
    └── use-artifacts/
        └── SKILL.md
```

参考 `peri-middlewares/src/subagent/agents/` 目录（`built_in_agents.rs` 用 `include_str!("agents/coder.md")` 嵌入内置 agent 定义），本设计采用对称结构。

### resolve_skill_roots 改动

`SkillRoot` struct 保持不变（path 字段对 Builtin 用 `PathBuf::new()` 占位，由 scan 阶段特判跳过 `is_dir()`）。`resolve_skill_roots` 增加 `disable_bundled` 参数：

```rust
// peri-middlewares/src/skills/loader.rs
pub fn resolve_skill_roots(
    cwd: &str,
    plugin_roots: Vec<SkillRoot>,
    disable_bundled: bool,  // 【新增参数】
) -> Vec<SkillRoot> {
    let mut roots = Vec::new();
    // ... 1. User / 2. Global / 3. Project / 4. Plugin 同现有逻辑 ...
    if !disable_bundled {
        roots.push(SkillRoot {
            path: PathBuf::new(),  // 占位，scan 阶段特判不读
            source: SkillSource::Builtin,
            plugin_name: None,
        });
    }
    roots
}
```

### scan_skill_roots_impl 改动（核心）

主循环对 `SkillSource::Builtin` 特判，绕过 `is_dir()` 检查，直接从 `BUILTIN_SKILLS` 常量构造 `SkillMetadata`：

```rust
// peri-middlewares/src/skills/loader.rs
fn scan_skill_roots_impl(
    roots: &[SkillRoot],
    max_depth: usize,
    max_dirs: usize,
) -> Vec<SkillMetadata> {
    let mut seen: HashMap<String, SkillMetadata> = HashMap::new();
    let mut ordered: Vec<String> = Vec::new();

    for root in roots {
        // 【新增】Builtin 特判：跳过磁盘扫描，直接从常量数组加载
        if matches!(root.source, SkillSource::Builtin) {
            for skill in builtin::BUILTIN_SKILLS {
                let Some((name, description)) =
                    builtin::parse_builtin_frontmatter(skill.content) else {
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

        // 原有磁盘扫描逻辑（is_dir 检查 + scan_dir_recursive）
        if !root.path.is_dir() { continue; }
        // ...
    }
    // ...
}
```

**虚拟路径语义**：`SkillMetadata.path = PathBuf::from("<builtin>/<name>")` 不对应真实文件系统路径。它在 system prompt 的 skill 列表中可见（`build_summary` 输出 `- **name**: {path.display()} {description}`），让 LLM 能识别"这是内置 skill"。`SkillPreloadMiddleware` 检测到 path 以 `<builtin>/` 开头（或更稳妥地，source == Builtin）时，直接从 `BUILTIN_SKILLS` 查找内容，不解析虚拟路径，无路径遍历风险。

### SkillsMiddleware::build_frozen_summary 改动

```rust
// peri-middlewares/src/skills/mod.rs
pub fn build_frozen_summary(
    cwd: &str,
    plugin_roots: Vec<SkillRoot>,
    disable_bundled: bool,  // 【新增参数】
) -> Option<String> {
    let roots = loader::resolve_skill_roots(cwd, plugin_roots, disable_bundled);
    let skills = scan_skill_roots(&roots);
    if skills.is_empty() { return None; }
    Some(Self::build_summary(&skills))
}
```

session/new 调用方在 `FrozenSessionData` 构造时一次性读取 `disableBundledSkills` 并传入，结果冻结到 `frozen_skill_summary`。

### SkillPreloadMiddleware 改动

加载 `/skill-name` 全文时，优先判断该 name 是否对应 builtin：

```rust
// 伪代码
fn load_full_content(name: &str, metadata: &SkillMetadata) -> Option<String> {
    if matches!(metadata.source, SkillSource::Builtin) {
        return builtin::BUILTIN_SKILLS.iter()
            .find(|s| s.name == name)
            .map(|s| s.content.to_string());
    }
    // 否则走磁盘读取
    std::fs::read_to_string(&metadata.path).ok()
}
```

由于 `scan_skill_roots` 已按优先级去重（同名只保留最高优先级的 summary），如果 builtin 被 user/project 同名 skill 覆盖，`SkillMetadata.source` 字段就不是 Builtin，preload 自然走磁盘分支——一致性天然成立。

### SkillsConfig 新增字段

```rust
// settings 反序列化结构
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkillsConfig {
    pub skills_dir: Option<PathBuf>,
    /// 【新增】禁用随二进制分发的内置 skill，默认 false
    #[serde(default)]
    pub disable_bundled_skills: bool,
}
```

读取时机：session/new 时读 settings，传给 `build_frozen_summary`。开关状态随 `frozen_skill_summary` 一并冻结。

## 数据流

### session/new

```
session/new
  → build_frozen_summary()
    → resolve_skill_roots(disable_bundled = frozen settings)
      → [User, Global, Project, Plugin, Builtin(if !disable_bundled)]
    → scan_skill_roots()
      → 主循环对 Builtin source 特判，直接遍历 BUILTIN_SKILLS 常量
      → 对其他 source 走磁盘递归扫描
      → 跨 root 按优先级去重（同名保留第一个遇到的）
    → frozen_skill_summary 存入 SessionState.frozen_*
```

### 每轮 agent

```
SkillsMiddleware::before_agent
  → 读取 frozen_skill_summary（含或不含 builtin summary）
  → prepend 为 system message
  → LLM 看到所有 skill 的 name + description
```

### /skill-name 触发

```
用户输入 /use-artifacts
  → SkillPreloadMiddleware
    → 在 BUILTIN_SKILLS 查找 name = "use-artifacts"
    → 命中 → 返回 include_str! 内容
    → 注入为 system message
```

## 配置

### disableBundledSkills

`~/.peri/settings.json`:

```json
{
  "config": {
    "disableBundledSkills": true
  }
}
```

- 默认 `false`（内置 skill 启用）
- **读取时机**：session/new 时一次性读取并冻结到 `SessionState.frozen_*`，会话进行中修改 settings 不影响当前会话
- **不每轮重新读取**：违反 CLAUDE.md 中"`DISABLE_COMPACT` 每轮读取"的常规模式，但符合"系统提示词稳定性第一优先级"——因为 disable_bundled_skills 直接影响 frozen_skill_summary，进而影响 frozen system prompt

### 同名覆盖规则

按 root 优先级，高优先级覆盖低优先级：

```
User       (~/.claude/skills/)            ← 最高
Global     (config.skillsDir)
Project    ({cwd}/.claude/skills/)
Plugin     (插件声明)
Builtin    (随二进制)                      ← 最低
```

`scan_skill_roots` 已有去重逻辑（按 root 顺序扫描，同名只保留第一个遇到的），builtin 永远不会覆盖用户级 skill。

**典型场景**：
- 用户在 `~/.claude/skills/use-artifacts/SKILL.md` 自定义了 → 用户版本生效，builtin 被忽略
- 项目 `.claude/skills/use-artifacts/` 有同名 → 项目版本生效，builtin 被忽略
- 都没有 → builtin 生效

## 系统提示词稳定性

本特性严格遵守"系统提示词稳定性第一优先级"原则。新增 Builtin root 必须满足以下不变量：

1. **冻结时机**：`disableBundledSkills` 在 session/new 时读取并冻结到 `frozen_skill_summary`，后续轮次不重新读取
2. **结构稳定**：`BUILTIN_SKILLS` 是编译期常量，跨会话完全相同（同一二进制版本）
3. **位置稳定**：builtin summary 始终在 skill 列表末尾（最低优先级），不会因 root 扫描顺序漂移
4. **路径稳定**：`<builtin>/<name>` 虚拟路径不依赖 cwd、不依赖用户 home 目录，跨环境完全一致
5. **SubAgent 一致性**：frozen_skill_summary 通过 `SubAgentMiddleware::with_frozen_data` 透传给 SubAgent（已有路径，无需新增）。SubAgent 看到的 builtin 列表与 main agent 完全一致
6. **PromptFeatures 漂移防护**：本特性不引入新的 `PromptFeatures` 字段，不触发 CLAUDE.md 中"`PromptFeatures::detect()` 与 SubAgent 漂移"的 [TRAP]

## 错误处理

Builtin 路径几乎没有运行时错误源（内容是编译期嵌入的）。仅有的潜在错误点：

1. **frontmatter 解析失败**：`include_str!` 嵌入的 SKILL.md frontmatter 格式错误
   - 处理：运行时 `parse_frontmatter_description()` 失败时该 builtin skill 被跳过（log warn），不阻断启动
   - 预防：集成测试 `test_builtin_skills_frontmatter_valid` 遍历 `BUILTIN_SKILLS` 验证每个都能正确解析 name + description

2. **name 冲突**：两个 builtin skill 同名
   - 处理：编译期通过常量数组手写，但加测试 `test_builtin_skills_unique_names` 防止后续 PR 误加

3. **虚拟路径解析**：`SkillPreloadMiddleware` 收到 `/name` 时按 name 查 builtin 常量数组，找不到则走磁盘
   - 无路径遍历风险（`<builtin>/<name>` 是虚拟标记，不参与文件系统读取）

## 测试矩阵

`peri-middlewares/src/skills/builtin_test.rs`:

| 测试 | 验证点 |
|------|--------|
| `test_builtin_skills_frontmatter_valid` | 遍历 `BUILTIN_SKILLS`，每个 `parse_builtin_frontmatter` 都返回 Some |
| `test_builtin_skills_unique_names` | 内置 skill name 不重复 |
| `test_builtin_skill_source_priority` | User/Project 同名 skill 时，scan 结果保留 User/Project 版本（source 不是 Builtin） |
| `test_scan_returns_builtin_summary` | roots 仅含 Builtin 时，scan 返回的 metadata 含 `source=Builtin` + `path=<builtin>/<name>` |
| `test_disable_bundled_skills_skips_root` | `disable_bundled=true` 时 `resolve_skill_roots` 返回值不含 Builtin source |
| `test_frozen_summary_includes_builtin` | `build_frozen_summary(cwd, plugin_roots, false)` 返回的 summary 字符串含 `use-artifacts` |
| `test_frozen_summary_excludes_builtin_when_disabled` | `build_frozen_summary(cwd, plugin_roots, true)` 不含 `use-artifacts` |
| `test_subagent_inherits_frozen_builtin` | SubAgent 通过 `with_frozen_summary` 接收到的 summary 与 main 一致（已有透传路径，验证不退化） |

## 集成验证场景

手动验证步骤：

1. `cargo run -p peri-tui` → 启动新 session → LLM 应在 skill summary 中看到 `use-artifacts`
2. 输入 `/use-artifacts` → `SkillPreloadMiddleware` 加载 builtin 全文 → 正常执行
3. 在 `~/.claude/skills/use-artifacts/SKILL.md` 写入自定义内容 → 重启 session → `/use-artifacts` 触发的是用户版本
4. `settings.json` 设 `disableBundledSkills: true` → 重启 session → skill summary 不含 builtin

## 首批验证 skill

**选择**：`use-artifacts`

**原因**：
- 单文件 SKILL.md（87 行），无附属文件，符合 `include_str!` 单文件嵌入约束
- frontmatter 完整（name/description/userInvocable/argumentHint）
- 与 perihelion 项目高度相关（artifact 工具是 perihelion 近期新增能力，见 commit `17495081: feat: add ArtifactTool`）
- 实际验证整条链路：编译嵌入 → SkillSource::Builtin 注册 → prompt summary 注入 → `/use-artifacts` 触发全文加载 → 项目级同名覆盖

**后续 PR 增补策略**：

新增 Builtin skill 时需要同步更新：

1. `BUILTIN_SKILLS` 常量数组（追加 entry）
2. 更新 `test_builtin_skills_frontmatter_valid` 断言
3. 若 skill 设计为可被 slash command 调用，确认 `/skill-name` 路径走通
4. 文档：在 `docs/blogs/` 或 spec 添加说明（非必须）

## 未来扩展

本 V1 设计预留的扩展点：

1. **多文件 skill 支持**：若需要 assets/scripts/references，升级 `BuiltinSkill` 内部实现为 `include_dir!`，外部接口（`scan_skill_roots_impl` 的 Builtin 特判分支 + `SkillPreloadMiddleware` 的 builtin 查找）不变
2. **逐个开关**：若需要 `skillOverrides`（on/name-only/user-invocable-only/off），在 `scan_skill_roots_impl` 的 Builtin 特判分支增加过滤层，读取 `config.skillOverrides`
3. **`/skills` 菜单集成**：在 TUI 加 builtin 标记和 Space 切换交互
4. **对标 Claude Code 官方 bundled**：增补 `/code-review`、`/debug`、`/batch`、`/loop`、`/simplify` 等通用工程 skill

## 实施清单

按依赖顺序：

1. **新增 `peri-middlewares/src/skills/builtin/mod.rs`**
   - `BuiltinSkill` struct（`name: &'static str` + `content: &'static str`）
   - `BUILTIN_SKILLS` 常量数组（含 use-artifacts entry）
   - `parse_builtin_frontmatter(content: &str) -> Option<(String, String)>` 辅助函数（复用 loader.rs 的 `gray_matter::Matter::<YAML>` 模式）

2. **扩展 `SkillSource` 枚举**（`loader.rs:11-20`）
   - 新增 `Builtin` 变体
   - 更新所有 match 分支（`Debug`/`Copy`/`PartialEq`/`Eq` 自动 derive）

3. **改造 `resolve_skill_roots`**（`loader.rs:241`）
   - 签名增加 `disable_bundled: bool` 参数
   - 末尾按条件追加 `SkillRoot { path: PathBuf::new(), source: SkillSource::Builtin, plugin_name: None }`

4. **改造 `scan_skill_roots_impl`**（`loader.rs:107`）
   - 主循环对 `SkillSource::Builtin` 特判：跳过 `is_dir()` 检查，直接遍历 `builtin::BUILTIN_SKILLS` 调用 `parse_builtin_frontmatter` 构造 `SkillMetadata`，复用 `insert_skill` 去重
   - frontmatter 解析失败时 `tracing::warn!` 跳过该 skill

5. **改造 `SkillsMiddleware::build_frozen_summary`**（`mod.rs:116`）
   - 签名增加 `disable_bundled: bool` 参数
   - 调用 `resolve_skill_roots(cwd, plugin_roots, disable_bundled)`

6. **改造 `SkillsMiddleware::resolve_roots`**（`mod.rs:131`）
   - 内部调用 `loader::resolve_skill_roots` 时传 `disable_bundled`（从字段或 settings 读）

7. **改造 `SkillPreloadMiddleware`**（`preload.rs`）
   - `/name` 加载全文时，先检查 metadata 的 `source` 字段；若是 `Builtin` 则从 `BUILTIN_SKILLS` 查内容，否则走磁盘

8. **新增 `SkillsConfig` 字段**（settings 反序列化路径）
   - `disable_bundled_skills: bool`，`#[serde(rename_all = "camelCase")]` + `#[serde(default)]`
   - 在 `build_frozen_summary` 调用前从 settings 读取并传入

9. **改造 session/new 调用链**
   - `FrozenSessionData` 构造时读 `disableBundledSkills` 传给 `build_frozen_summary`

10. **测试**：新增 `peri-middlewares/src/skills/builtin_test.rs`，覆盖测试矩阵的 8 项；更新 `loader_test.rs` 和 `mod_test.rs` 适配新签名（`resolve_skill_roots` / `build_frozen_summary` 多了参数）

11. **文档**：更新 CLAUDE.md "Tool Search 延迟加载" 章节附近的 skills 相关说明，提及 Builtin source；在 `peri-middlewares/CLAUDE.md` 的 Skills 段落补充 Builtin source 说明

## 参考

- `peri-middlewares/src/subagent/built_in_agents.rs` — 内置 agents 的成熟模式
- `peri-middlewares/src/skills/mod.rs:52` — SkillsMiddleware
- `peri-middlewares/src/skills/loader.rs` — SkillSource + resolve_skill_roots
- `peri-acp/src/session/executor.rs:75-91` — FrozenSessionData + frozen_skill_summary
- [Extend Claude with skills - Claude Code Docs](https://code.claude.com/docs/en/skills)
- [We've merged Slash Commands into Skills - bcherny announcement](https://x.com/bcherny/status/2014839121659986316)
