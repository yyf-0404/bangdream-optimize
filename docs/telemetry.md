# 内部埋点

计算结果可在 `metrics` 下携带可选结构化指标。
该数据仅供内部性能测试，不包含原始卡片数据、area-item 或请求体。

## 结果指标

`BuildResult.metrics` 当前会记录：

- `coreVersion`
- `cardCount`
- `songCount`
- `itemCombinationsBefore`
- `itemCombinationsAfter`
- `totalElapsedMs`
- `single`：单曲 DP 产生结果时出现
- `medley`：medley 候选求解器产生结果时出现

`PtMaximizeResult.metrics` 记录最大 PT 搜索的跨平台耗时和工作量：

- `single`：道具/模式数、累计保留卡片、计划与实际枚举队伍数、精确评价次数，以及候选构造、
  队伍搜索和精确计分耗时；
- `medley`：道具上界、候选构造、种子搜索和严格互斥求解耗时，以及累计候选数、组合检查数和
  精确分布缓存数。

这些计时统一使用 WASM 安全的单调时钟，浏览器计算路径不得直接调用 `std::time::Instant`。

单曲指标：

- `modeCount`
- `validModeCount`
- `solveMs`

Medley 指标：

- `candidateCount`
- `solverCandidateCount`
- `solverFilterMs`
- `solverMs`
- `candidateBuildMs`：当候选由 core 侧生成时出现
- `usedCardCount`：当从 compact solver input 中已知时出现

## 服务端 JSONL

当设置如下环境变量时，服务端会为每次成功计算追加一条 JSON 对象：

```bash
BANGDREAM_OPTIMIZE_TELEMETRY_JSONL=var/telemetry/internal.jsonl
```

每条记录包含：

- `schemaVersion`
- `timestampMs`
- `route`
- `server`：已知时
- `requestedEventId`：请求带入时
- `eventId`
- `eventType`
- `songCount`
- `totalScore`
- `totalStat`
- `solver`
- `metrics`

服务端会自动创建缺失的父目录；若未设置该变量则不写入埋点文件。

## 调试数据

完整卡牌明细、道具快照与 trace 日志应仅在显式调试导出时记录。
它们不属于默认埋点事件。
