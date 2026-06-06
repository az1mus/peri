# PluginManifest.skills 字段 String-or-Array 兼容反序列化 设计文档

**日期**：2026-06-06
**状态**：Approved
**关联 Issue**：`spec/issues/2026-06-06-plugin-skills-field-string-or-array.md`

---

## 1. 目标

修复 `PluginManifest` 解析 supergoal 插件时 `skills` 字段的兼容性问题。Claude Code 的 `plugin.json` 允许 `skills` 为单个字符串路径（`"./skills/"`），当前实现只接受 `Vec<String>` 数组格式（`["./skills/"]`），导致反序列化失败。

## 2. 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 修复范围 | 仅 `skills` 字段 | 目前只有 supergoal 插件确认受影响，最小化改动 |
| 实现方式 | 自定义 `#[serde(deserialize_with)]` 辅助函数 | 类型签名不变（仍为 `Option<Vec<String>>`），零下游影响 |
| 函数位置 | `types.rs` 内 `PluginManifest` 定义旁 | 局部工具函数，不跨文件共享 |
| 序列化行为 | 不自定义 | 默认 serde 对 `Option<Vec<String>>` 的序列化始终输出数组，与 Claude Code 兼容 |

## 3. 实现设计

### 3.1 反序列化辅助函数

```rust
fn deserialize_string_or_vec<'de, D>(deserializer: D) -> Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    match value {
        serde_json::Value::Null => Ok(None),
        serde_json::Value::String(s) => Ok(Some(vec![s])),
        serde_json::Value::Array(arr) => {
            let strings: Result<Vec<String>, _> = arr
                .into_iter()
                .map(|v| match v {
                    serde_json::Value::String(s) => Ok(s),
                    _ => Err(serde::de::Error::custom("skills 数组元素必须是字符串")),
                })
                .collect();
            Ok(Some(strings?))
        }
        _ => Err(serde::de::Error::custom(
            "skills 字段应为字符串或字符串数组",
        )),
    }
}
```

### 3.2 字段注解变更

`PluginManifest.skills` 只需加一个注解：

```rust
// Before:
pub skills: Option<Vec<String>>,

// After:
#[serde(default, deserialize_with = "deserialize_string_or_vec")]
pub skills: Option<Vec<String>>,
```

### 3.3 边界情况

| 输入 | 解析结果 |
|------|---------|
| 字段不存在 | `None`（`#[serde(default)]`）|
| `"skills": null` | `None` |
| `"skills": "./skills/"` | `Some(vec!["./skills/"])` |
| `"skills": ["./skills/"]` | `Some(vec!["./skills/"])` |
| `"skills": ["./a/", "./b/"]` | `Some(vec!["./a/", "./b/"])` |
| `"skills": 42` | 反序列化错误 |
| `"skills": ["./a/", 42]` | 反序列化错误 |

## 4. 影响范围

- **修改文件**：仅 `peri-middlewares/src/plugin/types.rs`
- **下游影响**：无——`skills` 字段类型不变（`Option<Vec<String>>`），所有消费者（`loader.rs`、`hooks/loader.rs` 等）零改动

## 5. 测试

需在 `types_test.rs` 中新增：

- `test_skills_field_string` — 单字符串 `"./skills/"` 解析为 `vec!["./skills/"]`
- `test_skills_field_array` — 数组 `["./a/", "./b/"]` 照常解析
- `test_skills_field_null` — `null` 解析为 `None`
- `test_skills_field_absent` — 字段缺失时解析为 `None`
