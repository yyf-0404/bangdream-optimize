# 当前进度

最后更新：2026-07-18。

本文只描述当前生产路径和仍需处理的问题。已经撤回的算法只在“历史实验结论”中保留简短结论，不再保存旧流程、旧配置、旧计时表或已删除代码的实现细节。

## 支持范围

| 计算模式 | 当前支持活动 | 明确不支持 |
| --- | --- | --- |
| `maximize` | `medley`、`versus`、`challenge` | `festival`、`live_try`、`mission_live` |
| `score_range` | `medley`、`versus`、`challenge`、`live_try`、`festival`、`mission_live` | 无 |

`festival` 最大化仍因 fever 尚未实现而拒绝。控分使用完整单人 PT 公式，因此支持六类活动。

## 项目结构

计算代码按“共享数据准备 + 独立搜索策略”组织：

- `crates/core`：领域模型、共享综合力计算、最大化与控分编排、单曲精确搜索和 Medley 候选生成。
- `crates/team-prune`：单曲与 Medley 共用的支配图和分组覆盖剪枝。
- `crates/medley-solver`：候选队伍上的严格标量/AVX2 搜索和 random-bucket 回退。
- `crates/data`：Bestdori/game-data 镜像与计算快照构建。
- `crates/service`：最大化和控分应用服务。
- `crates/web-wasm`：浏览器 WASM 计算边界。
- `apps/server`：HTTP 服务。
- `apps/web`：Web 与桌面端共用界面。
- `apps/desktop`：Tauri 桌面端。
- `tools/sync-bestdori`：静态 Bestdori 镜像生成工具。

旧 Node/CPP 运行时不再是本项目的执行依赖。

## 共享准备与综合力

`prepare_event_context` 是最大化和控分共用的准备入口，统一处理：

- 玩家档案、卡片等级、突破和技能等级；
- 角色、属性和指定卡面的活动加成；
- 区域道具和 magazine；
- 活动综合力加成与活动 PT 加成的分流；
- 单卡未取整综合力和五卡统一向下取整；
- 卡片 PT 加成先求和再统一取整。

最大化和控分共用 135 套原始区域道具组合。同类型、同目标键的组件构成一组；任一组内组件在玩家档案中显式为 0 级时，该组整体不生效。59/72 互斥替代品仍选择已持有加成较高者。

## 最大化

### 单曲

生产入口位于 `crates/core/src/single.rs`，使用共享安全预剪枝后进入 `single/exact.rs` 的严格五卡搜索：

1. 按角色分组，只枚举五个不同角色。
2. 使用同形状、同角色和跨角色安全支配缩小卡池。
3. 使用综合力和连续技能 Meta 的安全上下界做分支限界。
4. 在五卡叶子枚举队长位置并执行精确整数计分。
5. 最终分数、综合力、技能顺序和队长都来自精确时间线。

`DpChartModel` 仍用于构造连续 Meta 安全界，但不决定最终分数或最终技能顺序。

### Medley 候选生成

每套道具按以下签名枚举队伍：

- `Mixed`
- `UnifiedBand(band)`
- `UnifiedAttribute(attribute)`
- `UnifiedBandAttribute(band, attribute)`

每个签名池依次执行同形状初筛、全局同角色支配、全局跨角色支配和固定点收缩。贡献剪枝使用综合力区间与十个物理角色场景的连续 Meta 安全界；若无法证明严格支配则保留卡片。

形成五卡队伍后，为三首谱面分别计算精确综合力和技能顺序，再交给 `crates/medley-solver` 选择三支互斥队伍。

### 精确技能排序

最大化统一假设理想 60 FPS、无掉帧且歌曲零点与帧边界对齐。普通键与技能键同时出现时，允许普通键延后一帧点击并获得技能加成。

Medley 的生产路径对所有谱面使用相同算法：

1. 生成无技能基础分数组。
2. 为五张卡和六个技能位置构造完整 `5 × 6` 精确整数增量矩阵。
3. 前五个位置使用 32 状态子集 DP，队长位置单独取最大值。
4. 技能窗口重叠时仍独立计算每个窗口，重叠增量直接相加。

`skillQueueRisk` 只提示该谱面使用独立窗口重叠相加模型，不切换算法。严格游戏内排队时间线仅保留在公共严格计分入口和差分测试中，不属于生产 Medley 路径。

### Solver 选择

- `StrictExact`：严格全局搜索，优先 AVX2，不可用时使用标量。
- `FastApproximate`：直接使用 random-bucket，并把结果标记为 `approximate`。
- `Auto`：候选数不超过 196,608 时使用 exact，否则使用 random-bucket。

候选数只是保守分流条件；exact 的实际成本主要由冲突结构和动态剪枝决定。

## Score range

控分流程位于 `crates/core/src/score_range`，与最大化并列维护活动支持范围和计分规则。

当前约束：

- 整个方案使用同一支队伍；
- 只重复一种 `(song_id, difficulty)`，不同难度视为不同歌曲；
- 每局可独立选择 `1/5/10/15` 倍火；
- 全局先最小化演奏次数，再最小化实际火耗；
- 单曲无解直接返回无解。

队伍与道具搜索使用：

```text
mode → 技能桶 → 道具上下文 → PT/得分反解区间 → 2+3 MITM 行列批查询
```

MITM 索引使用 `floor(Σ 单卡未取整综合力)` 的真实综合力公式，并保存 canonical 见证。

### Auto 计分与 PT

- 日服 Auto 基础倍率为 `0.75`，其他服务器默认为 `0.5`，请求可显式选择。
- 无 Combo 加成。
- Rate-up 仍按经过键数逐键增长。
- 同时出现的普通键在技能键之前结算，因此不获得刚触发技能的加成。
- `mission_live` 的支援队伍 PT 加成由用户输入。
- `challenge`、`live_try` 和 `mission_live` 的活动加成进入 PT 倍率，不直接进入综合力。

### 谱面模板与过滤

前后端统一使用 `api/scoreRangeChartMeta.2.json`。歌曲与难度分别作为候选，并应用以下过滤：

- 目标服务器已发布歌曲；
- 难度及 Special 发布时间已到；
- 任一 `closedAt` 存在则进入黑名单；
- 必须恰好有六个技能键；
- 技能结束和排队风险按 60/120 FPS 包络筛除。

本地完整镜像由 4,167 份谱面生成 3,364 个安全歌曲/难度候选。实际 CDN/静态服务器发布仍属于仓库外部署步骤。

## 最近性能快照

完整 1,414 卡诊断 fixture 的当前单线程 Release 结果：

| 指标 | 当前值 |
| --- | ---: |
| 总分 | `11,815,764` |
| 总综合力 | `1,507,664` |
| 内部总耗时 | `32.371s` |
| candidate-build | `25.391s` |
| solver | `4.709s` |
| seed | `1.292s` |
| 道具上界 | `0.891s` |
| 原始队伍候选 | `3,556,703` |
| solver 候选 | `134,938` |
| exact 工作量 | `584,693,727` |

主要瓶颈仍是 Medley 候选构造。单曲技能覆盖风险 fixture 的热运行约为 `2.436s`，包含约 817 万搜索节点、20 万次精确计分和 25 万条编译时间线。

## 历史实验结论

以下方案曾实现或测量，但收益不足、复杂度不合适或已被更严格的算法替代：

- 单曲 `Sa × (D + Sb) + C` 近似 DP：无法满足严格全局精确要求，相关独立 crate 已删除。
- score-range 两曲规划、完全背包和三阶段综合力 DP：与当前“只重复一种歌曲”的产品约束不一致，已删除。
- score-range NTT/快速卷积和延迟恢复：在改用目标因数区间与 MITM 后不再需要，已删除。
- Medley Meta 最终计分、延迟精确化和按需技能格：完整 `5 × 6` 精确矩阵更简单稳定，旧路径已删除。
- Top-2 分配证书和有界 exact 探测：附加成本超过剪枝收益，未进入生产。
- 按谱面拆分共享卡池：模拟收益不足，已撤回。

## 验证状态

最近检查点已通过：

```bash
cargo test --workspace
cargo check --workspace --all-targets
cargo fmt --all -- --check
git diff --check
```

其中 core 为 116 个通过、1 个 ignored；另有完整 Medley fixture、单曲性能 fixture、六个随机剪枝 seed、data/WASM 和桌面端检查。v2 谱面模板已离线生成并完成数据测试。

## 已知风险与下一步

1. Medley 独立窗口重叠相加是显式产品模型，不模拟游戏内技能排队状态；前端必须继续展示 `skillQueueRisk`。
2. 单曲严格搜索仍有较高峰值内存和大量编译时间线，需要继续观察不同设备上的延迟。
3. random-bucket 仍是近似回退，结果必须保留 `quality`/solver 类型可见性。
4. 贡献支配已覆盖固定反例和随机穷举，但仍应扩大 unified、rate-up 和不同冲突图样本。
5. 发布 `scoreRangeChartMeta.2.json` 到实际 CDN/静态服务器并执行线上资源 smoke。
6. 所有真实性能与回归统一使用完整诊断 fixture；Mongo 导出工具必须显式指定输出路径，不再默认写入测试 fixture。
