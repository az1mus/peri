# PluginManifest.skills String-or-Array 兼容 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 修复 `PluginManifest.skills` 字段反序列化，使之同时接受字符串 `"./skills/"` 和数组 `["./skills/"]` 两种格式，兼容 Claude Code 插件生态。

**Architecture:** 在 `types.rs` 中添加 `deserialize_string_or_vec` 辅助函数，通过 `#[serde(deserialize_with)]` 注解到 `skills` 字段。类型签名不变（`Option<Vec<String>>`），零下游影响。

**Tech Stack:** Rust, serde, serde_json

---

### Task 1: 添加反序列化辅助函数 + 修改 skills 字段注解

**Files:**
- Modify: `peri-middlewares/src/plugin/types.rs`

- [ ] **Step 1: 在 PluginManifest 定义之前添加 `deserialize_string_or_vec` 函数**

在 `types.rs` 中，`pub struct PluginManifest {` 之前（约第 120 行），插入：

```rust
/// 反序列化 string | string[] 为 Vec<String>
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

- [ ] **Step 2: 修改 `skills` 字段添加 serde 注解**

在 `PluginManifest` 结构体中，将第 133 行：

```rust
    pub skills: Option<Vec<String>>,
```

改为：

```rust
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub skills: Option<Vec<String>>,
```

- [ ] **Step 3: 构建验证编译**

Run: `cargo build -p peri-middlewares`
Expected: 编译成功

- [ ] **Step 4: 确认现有测试仍然通过**

Run: `cargo test -p peri-middlewares --lib -- plugin::types::test`
Expected: 所有现有测试 PASS

- [ ] **Step 5: Commit**

```bash
git add peri-middlewares/src/plugin/types.rs
git commit -m "fix: skills field accepts string or array in plugin.json

Parse both \"skills\": \"./skills/\" and \"skills\": [\"./skills/\"] formats
to match Claude Code plugin.json compatibility.

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 2: 添加测试

**Files:**
- Modify: `peri-middlewares/src/plugin/types_test.rs`

在文件末尾添加以下测试（`test_plugin_manifest_mcp_servers_rename` 测试之后）：

- [ ] **Step 1: 添加 `test_skills_field_string` — 单字符串解析**

```rust
#[test]
fn test_skills_field_string() {
    let json = r#"{"name":"p","skills":"./skills/"}"#;
    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    assert_eq!(manifest.skills.as_ref().unwrap(), &vec!["./skills/".to_string()]);
}
```

- [ ] **Step 2: 添加 `test_skills_field_array` — 数组解析**

```rust
#[test]
fn test_skills_field_array() {
    let json = r#"{"name":"p","skills":["./a/","./b/"]}"#;
    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    assert_eq!(
        manifest.skills.as_ref().unwrap(),
        &vec!["./a/".to_string(), "./b/".to_string()]
    );
}
```

- [ ] **Step 3: 添加 `test_skills_field_null` — null 解析为 None**

```rust
#[test]
fn test_skills_field_null() {
    let json = r#"{"name":"p","skills":null}"#;
    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    assert!(manifest.skills.is_none());
}
```

- [ ] **Step 4: 添加 `test_skills_field_absent` — 字段缺失解析为 None**

```rust
#[test]
fn test_skills_field_absent() {
    let json = r#"{"name":"p"}"#;
    let manifest: PluginManifest = serde_json::from_str(json).unwrap();
    assert!(manifest.skills.is_none());
}
```

- [ ] **Step 5: 运行测试验证**

Run: `cargo test -p peri-middlewares --lib -- plugin::types::test`
Expected: 所有测试 PASS（包括新增的 4 个）

- [ ] **Step 6: Commit**

```bash
git add peri-middlewares/src/plugin/types_test.rs
git commit -m "test: add skills field string-or-array deserialization tests

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### Task 3: 集成验证（可选）

- [ ] **Step 1: 运行完整的 plugin 测试套件**

Run: `cargo test -p peri-middlewares --lib -- plugin::`
Expected: 所有测试 PASS

- [ ] **Step 2: 运行 clippy 检查**

Run: `cargo clippy -p peri-middlewares`
Expected: 无新增 warning
