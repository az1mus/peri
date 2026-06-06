# LineEdit V3 Diff-Apply 设计文档

**日期**：2026-06-06
**状态**：Approved
**前序**：V2（action 枚举 + expected_lines + 原子性）→ 完全替换为 V3

---

## 1. 目标

完全替换 LineEdit V2 的行号定位模式，改用标准 unified diff 作为输入。通过 5 级匹配回退 + 3 层验证，将 LLM 编辑错误率从 V2 的 ~14% 降至 <2%。

## 2. 设计决策

| 决策点 | 选择 | 理由 |
|--------|------|------|
| 输入格式 | 标准 unified diff | LLM 训练数据中大量存在，最自然的编辑格式 |
| 匹配策略 | 5 级回退 | 覆盖率 ~99%，平衡精度与复杂度 |
| 多位置冲突 | 拒绝 + 列出候选 | 防止错误替换 |
| 原子性 | 全有或全无 | 与 V2 一致 |
| 验证层 | 3 层串联 | Diff Sanity → 括号平衡 → Tree-sitter AST |
| 验证策略 | 硬拒绝 | 括号不平衡/AST 新错误/上下文破坏直接拒绝 |

## 3. 工具接口

### 3.1 参数 Schema

```json
{
  "type": "object",
  "properties": {
    "patches": {
      "type": "array",
      "items": {
        "type": "object",
        "properties": {
          "file_path": {
            "type": "string",
            "description": "Absolute path to the file to modify"
          },
          "diff": {
            "type": "string",
            "description": "Standard unified diff string. Use '--- a/file', '+++ b/file', '@@ -L,N +L,N @@' headers. Context lines (space prefix) locate the edit. Lines with '-' are removed, '+' are added."
          }
        },
        "required": ["file_path", "diff"]
      }
    }
  },
  "required": ["patches"]
}
```

### 3.2 Diff 格式规范

标准 unified diff，每个 patch 可包含多个 hunk：

```
--- a/path/to/file.rs
+++ b/path/to/file.rs
@@ -10,3 +10,3 @@
 fn main() {
-    let x = 1;
+    let x = 2;
 }
```

- `---` / `+++` 行：文件标识（路径仅作参考，实际路径由 `file_path` 字段决定）
- `@@ -L,N +L,N @@`：hunk header，L 为起始行号，N 为行数
- ` `（空格前缀）：context 行，用于匹配定位
- `-` 前缀：删除行
- `+` 前缀：新增行

### 3.3 工具描述

```rust
const LINE_EDIT_DESCRIPTION: &str = r#"Applies unified diff patches to files.

Use after reading a file with Read. Copy the lines you want to change and construct a standard unified diff.

Input format — standard unified diff per patch:
- Each patch has a file_path and a diff string
- Diffs use "--- a/file", "+++ b/file", "@@ -L,N +L,N @@" headers
- Lines prefixed with " " (space) are context for matching
- Lines prefixed with "-" are removed
- Lines prefixed with "+" are added
- Context lines locate the edit position — even if line numbers are stale, context matching finds the right place

Matching (5-level fallback):
1. Exact match → 2. Whitespace-normalized → 3. Line similarity (similar crate) → 4. First/last context line anchoring → 5. Line number fallback
If a hunk matches multiple locations, the edit is rejected — add more context lines to disambiguate.

Atomicity: all patches in one call succeed or none are written.

Verification (after apply, before write):
- Bracket balance: { } ( ) [ ] must match
- Diff sanity: no unexpected context corruption, no duplicate lines
- AST check (Rust/TS/JS/Python/Go only): rejects edits that introduce new syntax errors

Rules:
- Always include 2-3 context lines around changes for reliable matching
- For large changes, break into multiple hunks with overlapping context
- All patches to the same file are applied bottom-to-top
- new_string replaces the ENTIRE target range — do not duplicate adjacent lines

Example:
{
  "patches": [{
    "file_path": "/path/to/file.rs",
    "diff": "--- a/file.rs\n+++ b/file.rs\n@@ -10,3 +10,3 @@\n fn main() {\n-    let x = 1;\n+    let x = 2;\n }"
  }]
}"#;
```

## 4. 匹配引擎（5 级回退）

### 4.1 匹配流程

```
对每个 hunk:
  1. 提取 context 行（无 +/- 前缀的行）和变更行
  2. 从 hunk header 解析目标起始行号（作为搜索起点）
  3. 在文件内容中全文搜索匹配（hunk header 行号仅作 L5 兜底参考）
  4. 首个匹配成功的级别即使用
  5. 所有级别失败 → 报错，返回失败 hunk 详情
```

### 4.2 各级别策略

| 级别 | 策略 | 说明 |
|------|------|------|
| L1 | 精确匹配 | context 行 + 变更行完全一致（含空白） |
| L2 | 空白归一化 | tab↔4spaces 归一化 + trim 尾部空白后匹配 |
| L3 | 行级相似度 | `similar` crate 计算 context 行与候选位置的 ratio，>0.8 即匹配 |
| L4 | 关键行锚定 | 只取 hunk 的首行和末行 context 做锚定，中间忽略 |
| L5 | 行号兜底 | 放弃内容匹配，直接用 hunk header 中的行号定位 |

### 4.3 多位置冲突

L1-L4 如果匹配到多个位置，返回错误列出所有候选位置（行号），要求 LLM 扩展 context 行消除歧义。L5（行号兜底）不存在多位置问题。

### 4.4 性能

- L1-L2：O(n) 线性扫描
- L3：O(n×m)，m 为 hunk 行数（通常 <20）
- L4：O(n)
- L5：O(1)
- 典型文件 <1000 行，总延迟 <5ms

## 5. 验证层（3 层串联）

编辑应用到内存后、`atomic_write` 之前，依次通过 3 层验证。任一层 ERROR → 拒绝整个批次。

### 5.1 层 A：Diff Sanity Guard

**适用**：所有文件类型
**延迟**：<5ms（`similar::TextDiff` 计算）

检查项：
| 检查 | 级别 | 说明 |
|------|------|------|
| 上下文破坏 | ERROR | 编辑区域外 ±5 行被意外修改 |
| 改动幅度异常 | ERROR | 实际删除行数 > 预期 × 2 |
| 空行吞没 | WARN | 编辑区域紧邻的空行被意外删除 |
| 重复行 | WARN | 编辑结果出现与相邻行完全相同的行 |

实现：用 `similar::TextDiff::from_lines()` 计算编辑前后 diff，提取统计指标。

### 5.2 层 B：括号平衡 + 缩进一致性

**适用**：所有文件类型
**延迟**：<0.5ms

**括号平衡**：遍历全文，维护 `{}` `()` `[]` 计数器。忽略字符串/注释中的括号（简单有限状态机：`'` `"` `` ` `` 进入字面量模式，`//` `/*` 进入注释模式）。要求三个计数器最终归零。

**缩进一致性**：取编辑区域 ±2 行作为参考基线。检查新内容行缩进风格（tab/space）是否与基线一致。

| 检查 | 级别 |
|------|------|
| 括号不平衡 | ERROR |
| 缩进风格不一致（tab/space 混用） | WARN |

### 5.3 层 C：Tree-sitter AST Guard

**适用**：`.rs` `.ts` `.tsx` `.js` `.jsx` `.py` `.go`
**延迟**：10-20ms（两次 AST 解析）

流程：
1. 根据文件扩展名选择 grammar
2. 解析编辑前文件 → 统计 ERROR 节点数 `errors_before`
3. 解析编辑后文件 → 统计 ERROR 节点数 `errors_after`
4. 判断：`errors_after > errors_before` → 拒绝

| 结果 | 级别 |
|------|------|
| 新增语法错误 | ERROR |
| 原有错误未增/减少 | WARN |
| 非支持文件类型 | SKIP |

### 5.4 短路逻辑

```
层 A ERROR → 直接拒绝（不进入 B/C）
层 B ERROR → 直接拒绝（不进入 C）
层 C ERROR → 拒绝
全部通过 → atomic_write
```

## 6. 反馈格式

### 6.1 成功

```
✓ src/main.rs (sanity:ok brackets:ok ast:ok)
  3 hunks applied (5 additions, 3 deletions)
   41 | impl Handler for MyService {
   42 |-    let config = self.config.lock().unwrap();
   43 |-    process(req, config)
   42 |+    let opts = self.opts.lock().unwrap();
   43 |+    process(input, opts)
   44 | }
```

### 6.2 警告（匹配降级 + 验证警告）

```
⚠ src/main.rs:1 (matched:L3-similarity) (sanity:warn:空行吞没 brackets:ok ast:ok)
  L1-L2 精确匹配失败，使用行级相似度匹配（ratio=0.85）
  建议扩展 context 行以提高匹配精度
  1 | -use crate::old_module;
  1 | +use crate::new_module;
```

### 6.3 失败（匹配失败）

```
✗ src/main.rs hunk 1: context 行未匹配（尝试 L1→L5 全部失败）
  搜索内容:
    fn handle(&self, req: Request) -> Result {
  文件中未找到匹配内容。建议 Re-read 文件获取当前内容后重试。

未执行任何编辑。
```

### 6.4 失败（多位置冲突）

```
✗ src/main.rs hunk 1: 匹配到 3 个位置
  位置 1: 第 42 行
  位置 2: 第 128 行
  位置 3: 第 256 行
  请扩展 diff 中的 context 行以消除歧义。

未执行任何编辑。
```

### 6.5 失败（验证拒绝）

```
✗ src/main.rs 验证失败: 括号不平衡
  '{' 多出 1 个，缺少 '}'
  请检查 diff 中是否遗漏了闭合括号。

未执行任何编辑。
```

### 6.6 汇总行

多文件/多 hunk 时末尾追加：
```
3 patches: 2✓ 1⚠ (12 additions, 8 deletions across 3 files)
```

## 7. 执行流程

```
invoke(input):
  1. 解析 patches: Vec<PatchEntry>
  2. patches 为空 → 报错
  3. 按文件分组
  4. 读取所有文件内容
  5. 阶段 1：解析 + 匹配
     对每个 patch 的每个 hunk:
       a. 解析 hunk header（起始行号、行数）
       b. 提取 context 行和变更行
       c. 5 级匹配回退
       d. 匹配失败 → 收集错误
       e. 多位置冲突 → 收集错误
  6. 如有匹配错误 → 返回全部错误
  7. 阶段 2：应用编辑到内存
     对每个文件，按 hunk 从后往前应用变更
  8. 阶段 3：验证
     层 A → 层 B → 层 C（短路）
     任一层 ERROR → 返回错误，不写入
  9. 阶段 4：写入
     逐文件 atomic_write
  10. 构建反馈（含验证标签 + diff + 匹配级别）
```

## 8. 依赖

无新依赖。`similar = "3"` 和 tree-sitter 系列已在 `peri-middlewares/Cargo.toml` 中。

## 9. 关键文件

| 文件 | 改动 |
|------|------|
| `peri-middlewares/src/tools/filesystem/line_edit.rs` | 完全重写：Diff-Apply 引擎、5 级匹配、3 层验证、反馈格式 |
| `peri-middlewares/src/tools/filesystem/line_edit_test.rs` | 完全重写：覆盖匹配/验证/反馈/原子性 |
| `peri-middlewares/src/middleware/filesystem.rs` | 注册逻辑不变（工具名不变） |
| `peri-middlewares/src/tool_search/core_tools.rs` | TOOL_LINE_EDIT 常量不变 |
| `CLAUDE.md` | 更新 lineEdit beta 描述 |
| `prompts/lineedit_stress_test.txt` | 更新为 V3 说明 |
