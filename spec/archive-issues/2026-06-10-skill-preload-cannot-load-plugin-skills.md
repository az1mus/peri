> 归档于 2026-06-11，原路径 spec/issues/2026-06-10-skill-preload-cannot-load-plugin-skills.md

# SkillPreloadMiddleware 无法加载插件提供的 Skill 全文

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-10

## 问题描述

用户在主 Agent 中通过 `/skill-name` 引用插件提供的 skill（如 `/supergoal`）时，skill 全文不会被注入到 agent state。`SkillsMiddleware` 能在系统提示中展示插件 skill 的摘要（name + description），但 `SkillPreloadMiddleware` 的 `resolve_dirs()` 只搜索三个固定路径，不包含插件 skill 目录，导致全文加载静默失败——LLM 看到 skill 摘要，引用后却拿不到全文。

## 症状详情

| 维度 | 详情 |
|------|------|
| 期望行为 | 用户输入 `/supergoal` 后，`SkillPreloadMiddleware` 找到插件 `~/.claude/plugins/cache/.../skills/supergal/SKILL.md`，以 fake Read 工具调用注入全文 |
| 实际行为 | `resolve_dirs()` 返回的三个目录中均无 `supergoal/SKILL.md`，`before_agent` 跳过注入，LLM 仅看到摘要 |
| LLM 行为 | LLM 从摘要中得知 skill 存在，主动引用 `/supergoal`，但未收到全文内容，无法按 skill 指令行动 |
| 错误信息 | 无报错——找不到的 skill 静默跳过（`skill_preload.rs:150` `if skill_contents.is_empty() { return Ok(()) }`） |

### 验证 #1（2026-06-10）—— Verified

用户确认修复完成。全量编译通过，850 测试通过，4 个文件改动符合设计 spec。SubAgent 路径不改的决策经过 adviser 分析确认合理。

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 安装含 skill 的插件（如 supergoal v0.6.1，其 plugin.json 中 `skills: ["./skills/"]`）
  2. 启动 TUI，输入 `/supergoal` 或任何包含 `/supergoal` 的消息
  3. 观察：agent 上下文中没有 skill 全文，LLM 不知道如何执行 skill 指令
- **环境**：所有环境，与 provider/模型无关

## 涉及文件

- `peri-middlewares/src/subagent/skill_preload.rs:75-90` —— `resolve_dirs()` 硬编码三个搜索路径，不包含插件 skill 目录
- `peri-acp/src/agent/builder.rs:410-417` —— `SkillsMiddleware` 通过 `with_extra_dirs(plugin_skill_dirs)` 包含插件目录，但紧接其后的 `SkillPreloadMiddleware::new(preload_skills, &cwd)` 未传入插件目录
- `peri-middlewares/src/plugin/loader.rs:267-308` —— `extract_skills_paths()` 正确提取了插件 skill 目录，`PluginLoadResult.all_skill_dirs` 已可用

## 根因

`SkillPreloadMiddleware` 构造函数 `new(skill_names, cwd)` 只接收 skill 名称列表和 cwd，没有接收额外的搜索目录。`resolve_dirs()` 硬编码了三个路径：

```rust
fn resolve_dirs(&self) -> Vec<PathBuf> {
    let user_dir = /* ~/.claude/skills/ */;
    let global_dir = /* settings.json skillsDir */;
    let project_dir = /* {cwd}/.claude/skills/ */;
    // 缺少: plugin_skill_dirs
}
```

而 `SkillsMiddleware` 通过 `with_extra_dirs()` 方法支持插件目录：

```rust
// builder.rs:411 — SkillsMiddleware 有插件目录
SkillsMiddleware::new().with_extra_dirs(plugin_skill_dirs)
// builder.rs:417 — SkillPreloadMiddleware 没有插件目录
SkillPreloadMiddleware::new(preload_skills, &cwd)
```

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-10 | — | Open | agent | 创建 |
| 2026-06-10 | Open | Fixed | agent | 修复完成 |
| 2026-06-10 | Fixed | Verified | user | 用户确认完成 |

## 修复记录

| 日期 | 提交 | 说明 |
|------|------|------|
| 2026-06-10 | — | 修复方案：抽取 `resolve_skill_dirs()` 公共函数（`skills/loader.rs`），`SkillsMiddleware` 和 `SkillPreloadMiddleware` 均委托此函数。`SkillPreloadMiddleware` 新增 `extra_dirs` + `with_extra_dirs()`，`builder.rs` 传入 `plugin_skill_dirs`。SubAgent 路径暂不改（其 `SkillsMiddleware` 也无插件目录，整体支持作为独立 issue）。涉及 4 个文件，全量 850 测试通过。 |
