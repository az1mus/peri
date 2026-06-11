> 归档于 2026-06-11，原路径 spec/issues/2026-06-06-plugin-marketplace-delete-not-persisted.md

# Plugin 面板 marketplace 删除后重新打开面板仍在

**状态**：Verified
**优先级**：中
**创建日期**：2026-06-06

## 问题描述

在 Plugin 面板的 Marketplaces 视图下，通过 Backspace → Enter 删除一个自定义 marketplace 后，UI 中该条目消失（显示已删除），但关闭面板后重新打开 `/plugin`，该 marketplace 重新出现在列表中。

## 症状详情

| 步骤 | 操作 | 观察结果 |
|------|------|----------|
| 1 | `/plugin` → Marketplaces 标签，选中一个自定义 marketplace | 正常显示 |
| 2 | 按 Backspace → 提示确认删除 → 按 Enter | marketplace 条目从 UI 列表中消失 |
| 3 | Esc 关闭面板 | — |
| 4 | `/plugin` 重新打开 → Marketplaces 标签 | 刚才删除的 marketplace 仍在列表中 |

## 复现条件

- **复现频率**：必现
- **触发步骤**：
  1. 通过 Plugin 面板的 Add 输入框添加一个自定义 marketplace（URL/Git/本地路径）
  2. 切换到 Marketplaces 视图
  3. 选中该 marketplace，按 Backspace 触发删除确认
  4. 按 Enter 确认删除（UI 中条目消失）
  5. Esc 关闭面板，再次 `/plugin` 打开 → Marketplaces
- **环境**：macOS，面板 Add 输入框添加

## 涉及文件

- `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/delete.rs` —— 删除确认处理，负责从内存中移除条目并调用持久化
- `peri-tui/src/app/plugin_panel/handlers/plugin_handlers/persistence.rs` —— `persist_marketplace_delete` 持久化逻辑，通过名称匹配过滤 `known_marketplaces.json` 中的条目
- `peri-tui/src/app/panel_plugin.rs` —— `open_plugin_panel` 每次打开面板时重新从 `known_marketplaces.json` 加载 marketplace 列表

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-06 | — | Open | agent | 创建 |
| 2026-06-06 | Open | Fixed | agent | 修复：统一使用 `MarketplaceManager::extract_name()` 做名称匹配 |
| 2026-06-06 | Fixed | Pending | agent | 修复完成，等待用户验证 |
| 2026-06-06 | Pending | Verified | agent | 用户验证通过 |

## 修复记录

### 修复 #1（2026-06-06）

- **操作人**：agent
- **用户原意**：通过 Backspace 删除 marketplace 后，应持久化删除，重新打开面板不再出现
- **根因**：`persist_marketplace_delete` 自行实现了名称提取逻辑并与 `MarketplaceManager::extract_name()` 不一致。具体表现：
  - **File 类型**：`extract_name` 用 `file_stem()`（无扩展名），`persist_marketplace_delete` 用 `file_name()`（有扩展名，如 `manifest.json` ≠ `manifest`）
  - **Npm 类型**：`extract_name` 返回完整包名（如 `@scope/name`），`persist_marketplace_delete` 用 `split('@').next()` 返回空串
  - **Git 类型（无 .git 后缀）**：`strip_suffix` 失败后 fallback 为 `"marketplace"`，而非实际路径末段
  - **Url 类型（有查询参数）**：字符串分割含查询参数，无法正确匹配
- **修复内容**：将 `persist_marketplace_delete` 中 40 行的自定义名称匹配替换为 `MarketplaceManager::extract_name(&km.source)` 单一调用，与面板构建时使用的名称提取逻辑完全一致
- **涉及 commit**：待提交
- **验证状态**：待验证

## 验证详情

### 验证 #1（2026-06-06）—— 通过

用户反馈：修复后删除自定义 marketplace 可正常持久化，重新打开面板不再出现。
