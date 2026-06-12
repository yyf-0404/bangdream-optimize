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
