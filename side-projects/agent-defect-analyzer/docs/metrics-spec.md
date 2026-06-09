# Agent 缺陷分析 — 标准指标

定义分析指标体系，按用户场景组织。标注含义：

- ✅ 可直接从 DB 计算
- ⚠️ 可计算但有限制或边界情况
- ❌ 不可行，已提供替代指标

## 场景一：工具可靠性

> 工具调用的正确性和稳定性。

1. ✅ **工具失败率** —— 按工具类型统计 is_error 占比（严重度 critical）
2. ✅ **错误类型分布** —— 参数错误（param_parse/timeout/out_of_range）/ 匹配错误（not_found/not_unique）/ 系统错误（interrupted/tool_not_found/subagent_error），三分类
3. ✅ **连续失败序列** —— 同一工具连续失败的最大长度
4. ✅ **Edit 执行成功率** —— Edit 调用中 is_error=false 的占比。注：只反映旧字符串是否精准匹配，不保证修改了正确位置
5. ✅ **Grep 重复搜索率** —— 同 session 内重复相同 pattern 的次数

## 场景二：会话效率

> 单次会话是否高效完成任务，减少无效轮次。

1. ✅ **人均消息数** —— `threads.message_count` 可直接计算分布
2. ✅ **工具调用/轮次** —— 每轮 assistant 消息的平均 tool_use block 数
3. ⚠️ **死循环检测** —— 同工具+同参数连续重复 N≥5 次。语义循环（同效果、不同参数）无法检测
4. ✅ **会话时长** —— `created_at` 到 `updated_at` 的时长分布
5. ✅ **冗余 Read** —— 同文件被 Read 多次且中间、后续均无编辑，排除 offset 递增的连续分页阅读
6. ✅ **搜索→Read 联动率** —— 搜索工具（Grep/Glob/WebSearch）调用后 N 步内对结果文件发起 Read 的占比。按工具和步数交叉细分：紧邻联动（1步）/ 延迟联动（N步）/ 零联动（搜索无效）

## 场景三：资源消耗

> 上下文窗口利用和 token 开销。

1. ✅ **编辑工具入参大小** —— LineEdit/Edit/Write 的 input JSON 字节分布（P50/P95/max）
2. ✅ **编辑工具出参大小** —— tool_result 字节分布
3. ✅ **超大入参检测** —— P95 超过阈值的具体消息
4. ✅ **超大出参检测** —— tool_result 超过阈值的具体消息
5. ✅ **手动 Compact 触发频率** —— 用户主动执行 `/compact` 命令的次数。自动 compact 需 tracing 埋点，当前不可统计

## 场景四：功能采纳

> 新功能的实际使用情况和效果。

1. ✅ **LineEdit 使用率** —— LineEdit / (LineEdit+Edit) 的占比
2. ✅ **LineEdit 成功率** —— is_error 占比
3. ⚠️ **Skill 调用频率** —— 两维度：System 消息中的 session 级加载标记；Agent 工具调用的 subagent_type 参数（通过子代理分发）。两者都不能完美映射 "LLM 每次使用 skill 知识" 的语义
5. ⚠️ **Skill 链深度** —— 通过 `parent_thread_id` 递归遍历子代理层级。一级嵌套直接可查，深层需递归
6. ✅ **工具使用多样性** —— 每 session 使用的不同 tool_use name 种数

## 场景五：编辑质量

> 文件编辑操作的正确性和效率。

1. ✅ **重读率（纯验证）** —— 编辑后读回同一文件且无后续编辑（严重度 high）
2. ✅ **重读率（编辑链）** —— 编辑后读回同一文件，含后续编辑（结构性重读，不可消除）
3. ✅ **连续编辑能力** —— 同文件连续 LineEdit 的链长分布
4. ✅ **Write 文件大小** —— Write 写入内容的字节分布

## 场景六：SubAgent 协作

> SubAgent 的调用效率和产出质量。按 SubAgent 类型分层分析。

1. ✅ **内置 Agent 分类分析** —— 按 `subagent_type` 分组，统计各类数量、均消息数、工具使用模式（搜索/编辑/执行占比）、Top 工具。编辑型中无 Write/LineEdit/Edit 产出的判定为空转。非编辑型（explore/web-researcher/verification/hello-agent/plan）排除在空转检测外。类型通过父线程 Agent 工具调用的 `subagent_type` 参数判定
2. ✅ **SubAgent 消息量** —— `threads.message_count` 分布（P50/P95/max）
3. ✅ **SubAgent 工具错误率** —— tool 消息 is_error 占比
4. ✅ **SubAgent 产出比** —— 编辑类工具调用 / 总 tool_use block 数。按类型分层：编辑型与探索型分别统计

## 使用方式

```bash
cd side-projects/agent-defect-analyzer

# 全量运行所有场景
bun run src/metrics/tool_reliability.ts --since 168
bun run src/metrics/session_efficiency.ts --since 168
bun run src/metrics/resource_consumption.ts --since 168
bun run src/metrics/feature_adoption.ts --since 168
bun run src/metrics/edit_quality.ts --since 168
bun run src/metrics/subagent_collab.ts --since 168

# 或通过 npm scripts
bun run tool-reliability -- --since 24
bun run session-efficiency
bun run resource-consumption
bun run feature-adoption
bun run edit-quality
bun run subagent-collab
```
