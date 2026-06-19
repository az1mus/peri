# Cache Warning Toggle — 设计文档

**日期**: 2026-06-19  
**状态**: 已确认

## 背景

Status 面板 Context Tab 始终渲染缓存命中率图表，无关闭开关。
用户反馈：希望关闭"消息流中的黄色文本"——即缓存命中率 < 80% 时注入的警告 SystemNote。

## 需求

在 `/config` 面板的 General 分组加一个开关，控制缓存命中率警告消息（`"⚠ 提示缓存命中率 X% (req: ...)"`）在消息流中的显示/隐藏。

- **范围**: 仅控制这一种消息的显示，不扩大
- **默认**: 开（保持现状）
- **不关闭**: tracing::warn / metrics::emit 仍继续上报，仅消息流不显示

## 设计

### 改动范围（4 个文件）

| 文件 | 改动 |
|------|------|
| `peri-acp/src/provider/config.rs` | `AppConfig` 加字段 `show_cache_warning: bool`，默认 `true` |
| `peri-tui/src/app/config_panel.rs` | `ConfigPanel` 加 buf + 行常量 + cycle + apply_edit |
| `peri-tui/src/ui/main_ui/panels/config.rs` | 渲染 "缓存警告" 行（General 分组），on/off 切换 |
| `peri-tui/src/app/agent_ops/subagent.rs` | `handle_token_usage_update`，推送前检查 `peri_config` |

### 数据流

```
settings.json → AppConfig.show_cache_warning (bool, default true)
     ↓
ConfigPanel.buf_show_cache_warning  (编辑缓冲区)
     ↓ Space/←/→ cycle
     ↓ Esc/↑/↓ 保存
     ↓ apply_edit → AppConfig.show_cache_warning
     ↓
subagent.rs: handle_token_usage_update()
    读取 services.peri_config → show_cache_warning
    false → 跳过 push SystemNote（不创建、不入 pipeline）
    true  → 现有逻辑不变
```

### Config 面板布局

在 General 分组末（`ROW_PROACTIVENESS` 之后）插入新行：

```
 自动压缩          [on]  off
 缓存警告          [on]  off        ← 新增
 压缩阈值          85
 ...
```

### 行索引重排

`ROW_THRESHOLD` 2→3, `ROW_LANGUAGE` 3→4, `ROW_DIFF` 4→5, `ROW_STREAMING` 5→6,
`ROW_PROACTIVENESS` 6→7, `ROW_CACHE_WARNING` = 2（插入在 autocompact 和 threshold 之间），`ROW_COUNT` 11→12。

`SCREEN_LAYOUT`、`next_editable_row`、config 面板渲染 match 分支、`apply_edit` 同步更新。

### i18n

新增 key：
- `config-field-cache-warning` → "缓存警告"
- `config-desc-cache-warning` → "在消息流中显示缓存命中率过低警告"

### 不做的

- tracing::warn / metrics::emit 不受影响，监控告警保留
- Status 面板 Context Tab 缓存命中率图表不受影响
- `CacheWarning` VM 变体（已废弃但未删除）不动
