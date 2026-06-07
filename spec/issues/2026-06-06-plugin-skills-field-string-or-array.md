# PluginManifest.skills 字段不支持字符串格式，导致 supergoal 插件解析失败

**状态**：Open
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

加载 supergoal 插件（v0.6.1）时，`load_plugin_manifest()` 解析 `.claude-plugin/plugin.json` 报错：`invalid type: string "./skills/", expected a sequence at line 8 column 23`。Claude Code 的 `plugin.json` 允许 `skills` 字段是单个字符串路径，当前 `PluginManifest` 只接受 `Vec<String>`（数组），导致插件无法加载。

## 症状详情

| 维度 | 详情 |
|------|------|
| 出错文件 | `~/.claude/plugins/marketplaces/supergoal/.claude-plugin/plugin.json` |
| 出错字段 | `skills`，值为 `"./skills/"`（字符串），非 `["./skills/"]`（数组） |
| 出错位置 | `peri-middlewares/src/plugin/config.rs:487`，`serde_json::from_str` 反序列化 `PluginManifest` |
| 错误类型 | `PluginConfigError::ParseError`——硬错误，插件整体加载失败 |
| 影响插件 | supergoal v0.6.1 |
| 临时绕过 | 手动编辑 plugin.json 把 `"skills": "./skills/"` 改为 `"skills": ["./skills/"]` |

## 复现条件

- **复现频率**：必现（安装 Claude Code 生态中 `skills` 字段使用字符串格式的插件时）
- **触发步骤**：
  1. 安装 supergoal 插件（其 plugin.json 中 `skills` 字段值为字符串 `"./skills/"`）
  2. peri 启动时加载插件清单
  3. 报错退出
- **环境**：所有环境

## 涉及文件

- `peri-middlewares/src/plugin/types.rs:133` —— `PluginManifest.skills` 类型为 `Option<Vec<String>>`，不支持单个字符串
- `peri-middlewares/src/plugin/types.rs:141` —— `PluginManifest.output_styles` 同样为 `Option<Vec<String>>`，潜在相同问题
- `peri-middlewares/src/plugin/config.rs:479-494` —— `load_plugin_manifest()` 直接反序列化，无兼容处理

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |

## 修复记录

（待修复后追加）
