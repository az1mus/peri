# current_rss_mb() 使用 ru_maxrss（历史最大值）而非当前 RSS

**状态**：Open
**优先级**：高
**创建日期**：2026-06-19

## 问题描述

`peri-agent/src/metrics/mod.rs` 的 `current_rss_mb()` 通过 `libc::getrusage(RUSAGE_SELF)` 获取 RSS，但 macOS 上 `ru_maxrss` 是**进程历史上 RSS 的最大值**（单调递增，永不下降）。这导致 `sample.agent_turn_end` 和 `threshold.memory` 事件采集的 RSS 数据全部是历史最大值，而非当前实际 RSS。分析报告 `docs/analysis/memory-rss-专题.md` 的结论——包括 1430 MB 持续 29 分钟、瞬间跳变等——均基于此错误数据。

## 症状详情

### 核心问题：ru_maxrss 单调递增

`current_rss_mb()` 的实现（`peri-agent/src/metrics/mod.rs:56-74`）：

```rust
pub fn current_rss_mb() -> Option<u64> {
    #[cfg(unix)]
    {
        let mut usage: libc::rusage = unsafe { std::mem::zeroed() };
        let ret = unsafe { libc::getrusage(libc::RUSAGE_SELF, &mut usage) };
        if ret == 0 {
            #[cfg(target_os = "macos")]
            let rss_kb = (usage.ru_maxrss / 1024) as u64;  // ← 历史最大值
            #[cfg(not(target_os = "macos"))]
            let rss_kb = usage.ru_maxrss as u64;
            return Some(rss_kb / 1024);
        }
        None
    }
}
```

- **macOS**：`ru_maxrss` 单位为 bytes，除以 1024 得 KB，再除以 1024 得 MB。该值是进程生命周期中 RSS 的**峰值**（monotonic）
- **Linux**：`ru_maxrss` 单位为 KB。同样返回历史最大值
- **Windows**：返回 `None`

### 实证：2026-06-07 1430 MB 尖峰真相

通过分析 `~/.peri/metrics/2026-06-07.jsonl` 和 `~/.peri/threads/threads.db` 交叉验证：

| 时间 | RSS 报告值 | 迭代 | Input Tokens | 真实情况 |
|------|-----------|------|-------------|----------|
| 14:09:55 | 1430 MB | 50 | 6,559,308 | SubAgent fork 复制大上下文 → RSS 瞬时冲上 1430 → **ru_maxrss 永久锁定为 1430** |
| 14:25:24 | 1430 MB | 73 | 7,574,095 | 实际 RSS 可能已回落至 100-200 MB，但 ru_maxrss 不降 |
| 14:32:13 | 1430 MB | 35 | 2,358,045 | 同上 |
| 14:33:57 | 1430 MB | 34 | 3,751,753 | 同上 |
| 14:38:23 | 1430 MB | 1 | 251,169 | 进程最后一轮（仅 1 次迭代，实际 RSS 不可能 1430） |
| 06-09 01:01 | 74 MB | 31 | 1,839,368 | 进程已重启，ru_maxrss 重置 |

- RSS 序列在 14:09:55 后全部为 `[1430, 1430, 1430, 1430, 1430]`，完全单调非递减
- 进程 14:38 之后重启，06-09 首次正常读数 74 MB
- 同时活跃的所有会话消息总内容仅 3.2 MB（SQLite 存储），不可能产生 1.4 GB 的持续内存占用

### 已有正确实现作为对照

`peri-tui/src/alloc_config.rs:134` 的 `os_rss_mb()` 使用 `sysinfo::System::refresh_processes()` 获取**当前 RSS**，这是正确的实现：

```rust
pub fn os_rss_mb() -> Option<u64> {
    use sysinfo::{ProcessesToUpdate, System};
    let mut sys = System::new();
    let pid = sysinfo::get_current_pid().ok()?;
    sys.refresh_processes(ProcessesToUpdate::Some(&[pid]), true);
    sys.process(pid).map(|p| p.memory() / 1024) // KB → MB（当前实时值）
}
```

### 影响的数据

受影响的事件类型（均通过 `current_rss_mb()` 获取 RSS）：

| 事件 | 使用位置 |
|------|---------|
| `sample.agent_turn_end` → `rss_mb` | `final_answer.rs:174` |
| `threshold.memory` → `rss_mb` | `final_answer.rs:153, 162` |

## 影响

- **`docs/analysis/memory-rss-专题.md` 结论不可靠**："1430 MB 持续 29 分钟""瞬间跳变 1429→1430" 等结论均基于被误解的 ru_maxrss 数据
- **threshold.memory 阈值偏倚**：当前阈值 100/200 MB 是基于虚假的 ru_maxrss 数据设定的，修复后真实 RSS 会更低
- **无法监控真实内存压力**：`ru_maxrss` 永远反映历史峰值，无法感知当前内存使用是否回落

## 涉及文件

- `peri-agent/src/metrics/mod.rs:56-74` — `current_rss_mb()` 错误实现（使用 ru_maxrss）
- `peri-agent/src/agent/executor/final_answer.rs:141-167` — `sample.agent_turn_end` + `threshold.memory` 事件 emit 位置，调用 `current_rss_mb()`
- `peri-tui/src/alloc_config.rs:134` — 正确的 `os_rss_mb()` 实现（参考实现，使用 sysinfo）

## 修复范围

### P0：修复 current_rss_mb() 测量方法

**方案 A（推荐）**：改用 `sysinfo` 获取当前 RSS，与 `os_rss_mb()` 保持一致：
- 优点：跨平台（Unix + Windows），语义正确，项目已有使用先例
- 缺点：首次 `System::new()` 初始化有开销（约 1-2ms）

**方案 B**：macOS 上改用 `mach_task_basic_info.resident_size`：
- 优点：轻量，仅获取当前进程
- 缺点：仅限 macOS，Linux 需另实现

### P1：调整 threshold.memory 阈值

修复测量方法后，当前阈值 100/200 MB 基于错误数据。建议调整为：

| 当前 | 建议 | 理由 |
|------|------|------|
| Level 100: 100 MB | 150 MB | 修复后 P50≈100 MB（而非 ru_maxrss 的 102），100→150 避免每轮触发 |
| Level 200: 200 MB | 250 MB | P95≈160 MB（而非 ru_maxrss 的 198），200→250 仅真实异常触发 |

### P2：修正分析报告

`docs/analysis/memory-rss-专题.md` 需标注数据质量问题，并在修复后重新采集数据更新分析。

## 状态变更记录

| 日期 | 从 | 到 | 操作人 | 说明 |
|------|-----|-----|--------|------|
| 2026-06-19 | — | Open | agent | 创建：通过 metrics JSONL + threads.db 交叉验证确认 ru_maxrss 单调递增导致 RSS 数据失真 |

## 修复记录

（由 fix-issue 或 issue-verify skill 追加，创建时留空）
