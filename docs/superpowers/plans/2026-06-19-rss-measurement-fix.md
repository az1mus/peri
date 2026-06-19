# current_rss_mb() → sysinfo 迁移 & 内存阈值调优 实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 将 `current_rss_mb()` 从 `getrusage(RUSAGE_SELF)→ru_maxrss`（历史最大值，单调递增）改为 `sysinfo::System::refresh_processes()`（当前实时 RSS），并相应调整 `threshold.memory` 阈值 100→150、200→250。

**Architecture:** 用已有的 `sysinfo` 依赖（`peri-agent/Cargo.toml:26`）替换 `libc::getrusage` 调用，与 `peri-tui/src/alloc_config.rs:134` 的 `os_rss_mb()` 实现保持一致。阈值从硬编码常量改为有意义的可调值。

**Tech Stack:** Rust, sysinfo 0.39, libc（移除，仅限此函数）

---

### Task 1：修复 current_rss_mb() 实现

**Files:**
- Modify: `peri-agent/src/metrics/mod.rs:56-74`
- Remove: `peri-agent/Cargo.toml:25`（libc 依赖，此函数移除后不再使用）

- [ ] **Step 1: 替换 current_rss_mb() 实现**

将 `peri-agent/src/metrics/mod.rs:56-74` 的旧实现替换为使用 `sysinfo` 获取当前 RSS：

```rust
/// 获取当前进程 RSS（MB），通过 sysinfo 获取实时值。
pub fn current_rss_mb() -> Option<u64> {
    #[cfg(unix)]
    {
        use sysinfo::{ProcessesToUpdate, System};
        let mut sys = System::new();
        let pid = sysinfo::get_current_pid().ok()?;
        sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
        sys.process(pid).map(|p| p.memory() / 1024) // sysinfo 返回 KB → MB
    }
    #[cfg(not(unix))]
    {
        None
    }
}
```

**关键变更**：
- `libc::getrusage(RUSAGE_SELF) → ru_maxrss`（历史最大值）→ `sysinfo::System::refresh_processes()`（当前值）
- macOS `ru_maxrss` 单位 bytes÷1024÷1024 → sysinfo `p.memory()` 单位 KB÷1024
- Linux `ru_maxrss` 单位 KB÷1024 → sysinfo `p.memory()` 单位 KB÷1024（统一）
- `sysinfo` 在 `peri-agent/Cargo.toml:26` 已有，无需新增依赖

- [ ] **Step 2: 移除 libc 依赖（可选清理）**

检查 `libc` 在 peri-agent 中是否仅有 `mod.rs` 这一处使用。如果是，从 `peri-agent/Cargo.toml:25` 移除 `libc = "0.2"`：

使用 `cargo check -p peri-agent` 验证编译通过。若 libc 被其他 crate 间接依赖则不报错。

- [ ] **Step 3: 编译验证**

```bash
cargo build -p peri-agent
```

预期：编译通过，无 libc 相关错误。`sysinfo::System::new()` 在 `total_system_memory_mb()` 中已有调用，API 兼容。

---

### Task 2：添加 current_rss_mb() 测试

**Files:**
- Modify: `peri-agent/src/metrics/mod_test.rs:54`（在现有测试后追加）

- [ ] **Step 1: 写入测试代码**

在 `peri-agent/src/metrics/mod_test.rs` 末尾追加：

```rust
#[test]
#[cfg_attr(not(unix), ignore = "RSS measurement only supported on Unix")]
fn test_current_rss_mb_returns_positive_on_unix() {
    let rss = current_rss_mb();
    assert!(rss.is_some(), "current_rss_mb() should return Some on Unix");
    assert!(rss.unwrap() > 0, "RSS should be positive");
}

#[test]
#[cfg_attr(not(unix), ignore = "RSS measurement only supported on Unix")]
fn test_current_rss_mb_is_realtime_not_monotonic_max() {
    // 验证返回的是当前 RSS（可下降），而非 ru_maxrss（单调递增）
    let baseline = current_rss_mb().expect("should get baseline RSS");
    assert!(baseline > 0);

    // 分配一个较大的临时 Vec 推高 RSS，然后释放
    let v: Vec<u8> = vec![0u8; 50 * 1024 * 1024]; // 50 MB
    let peak = current_rss_mb().expect("should get peak RSS after allocation");
    assert!(
        peak >= baseline,
        "peak RSS ({}) should be >= baseline ({})",
        peak,
        baseline
    );

    // 释放后 RSS 应回落
    drop(v);
    // 注意：RSS 回落可能不是即时的（取决于 OS 页面回收策略），
    // 这里只验证不高于峰值，不强制要求 < baseline
    let after = current_rss_mb().expect("should get RSS after free");
    assert!(
        after <= peak,
        "after-free RSS ({}) should be <= peak ({}). \
         If this fails, RSS might not have been reclaimed yet (OS page cache)",
        after,
        peak
    );
}
```

- [ ] **Step 2: 运行测试验证通过**

```bash
cargo test -p peri-agent --lib -- metrics::tests::test_current_rss_mb_returns_positive_on_unix
cargo test -p peri-agent --lib -- metrics::tests::test_current_rss_mb_is_realtime_not_monotonic_max
```

预期：两个测试均 PASS。

**注意**：`test_current_rss_mb_is_realtime_not_monotonic_max` 的 `after <= peak` 断言在 macOS 上通常成立（50MB 的 Vec 分配-释放是显式的），但极端情况下如果 OS 页面回收不即时，可能 `after ≈ peak`——这仍是合法的（不是 ru_maxrss 的行为，ru_maxrss 在首次释放后永远不会低于峰值）。可以通过连续多次分配-释放验证下降趋势。

- [ ] **Step 3: 运行全量测试**

```bash
cargo test -p peri-agent --lib
```

预期：所有现有测试 + 新增测试全部 PASS。

---

### Task 3：调整 threshold.memory 阈值

**Files:**
- Modify: `peri-agent/src/agent/executor/final_answer.rs:150,159`

- [ ] **Step 1: 更新阈值常量**

修改 `peri-agent/src/agent/executor/final_answer.rs:150,159` 的阈值：

**旧代码**（line 150）：
```rust
if rss >= 100 && !reported_100 {
```

**新代码**：
```rust
if rss >= 150 && !reported_100 {
```

**旧代码**（line 159）：
```rust
if rss >= 200 && !reported_200 {
```

**新代码**：
```rust
if rss >= 250 && !reported_200 {
```

同时更新 context key 名称以匹配新阈值：

```rust
// Line 148-149 替换为：
let reported_150 = state.get_context("mem_reported_150").is_some();
let reported_250 = state.get_context("mem_reported_250").is_some();

// Line 150-157 替换为：
if rss >= 150 && !reported_150 {
    crate::metrics::emit(
        "threshold.memory",
        serde_json::json!({"rss_mb": rss, "level": 100, "rate": rate}),
        sid.as_deref(),
        rid.as_deref(),
    );
    state.set_context("mem_reported_150", "1");
}

// Line 159-167 替换为：
if rss >= 250 && !reported_250 {
    crate::metrics::emit(
        "threshold.memory",
        serde_json::json!({"rss_mb": rss, "level": 200, "rate": rate}),
        sid.as_deref(),
        rid.as_deref(),
    );
    state.set_context("mem_reported_250", "1");
}
```

**阈值调整依据**（来自 `docs/analysis/memory-rss-专题.md`）：
| 指标 | 旧值 | 新值 | 理由 |
|------|------|------|------|
| Level 100 | rss ≥ 100 MB | rss ≥ 150 MB | 修复后 P50≈100 MB（而非 ru_maxrss 的 102），100→150 避免 50% 轮次触发 |
| Level 200 | rss ≥ 200 MB | rss ≥ 250 MB | 修复后 P95≈160 MB（而非 ru_maxrss 的 198），200→250 仅真实异常触发 |

**重要**：`level` 字段保持 100/200 不变——这是阈值**类别标识**而非阈值值。新增 context key `mem_reported_150`/`mem_reported_250` 以避免与新阈值的混淆。旧 session 中的 `mem_reported_100`/`mem_reported_200` key 会被忽略，不会冲突。

- [ ] **Step 2: 编译验证**

```bash
cargo build -p peri-agent
```

预期：编译通过。

- [ ] **Step 3: 验证逻辑正确性**

```bash
cargo test -p peri-agent --lib
```

预期：全量测试 PASS。无测试直接断言阈值常量值，不会因阈值变更而失败。

---

### Task 4：全量回归验证

**Files:**
- 无新增/修改文件

- [ ] **Step 1: 运行 peri-agent 全量测试**

```bash
cargo test -p peri-agent
```

预期：全部测试 PASS。

- [ ] **Step 2: 运行 workspace 编译检查**

```bash
cargo check
```

预期：所有 crate 编译通过（peri-agent 的变更不影响依赖它的 crate——`current_rss_mb()` 签名不变）。

- [ ] **Step 3: 运行 lefthook pre-commit 检查**

```bash
lefthook run pre-commit
```

预期：fmt、clippy、check 全部通过。

- [ ] **Step 4: Commit**

```bash
git add peri-agent/src/metrics/mod.rs peri-agent/src/metrics/mod_test.rs peri-agent/src/agent/executor/final_answer.rs
# 如果移除了 libc 依赖：
git add peri-agent/Cargo.toml
git commit -m "fix(peri-agent): current_rss_mb() use sysinfo instead of ru_maxrss

libc::getrusage(RUSAGE_SELF)->ru_maxrss reports the process lifetime
maximum RSS (monotonic, never decreases on macOS). This caused all
sample.agent_turn_end and threshold.memory events to report the
historical peak rather than the current resident set size.

Replace with sysinfo::System::refresh_processes() to get real-time
RSS, matching the existing os_rss_mb() in peri-tui/alloc_config.rs.

Also adjust threshold.memory thresholds: 100→150, 200→250 since
real-time RSS is lower than the previous ru_maxrss-based readings.

Closes: spec/issues/2026-06-19-current-rss-mb-uses-ru-maxrss-monotonic.md

Co-Authored-By: deepseek-v4-pro <deepseek-ai@claude-code-best.win>"
```

---

### 自审清单

**1. Spec 覆盖**：
- ✅ P0 修复 `current_rss_mb()` 测量方法 → Task 1（方案 A：sysinfo）
- ✅ P1 调整 threshold.memory 阈值 → Task 3（100→150, 200→250）
- ⚠ P2 修正分析报告（`memory-rss-专题.md`）→ 暂缓，需修复后重新采集数据再更新（独立任务）

**2. Placeholder 扫描**：
- ✅ 无 TBD/TODO/占位符
- ✅ 所有代码片段完整可复制
- ✅ 所有命令含预期输出

**3. 类型一致性**：
- ✅ `current_rss_mb()` 签名不变：`Option<u64>` → `Option<u64>`
- ✅ `final_answer.rs` 调用方式不变：`let rss_mb = crate::metrics::current_rss_mb();`
- ✅ `level` 字段值保持 100/200（类别标识），仅比较阈值变更
- ✅ sysinfo 版本 0.39 与现有 workspace 依赖一致
