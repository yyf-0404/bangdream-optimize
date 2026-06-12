# 当前进度

最后更新：2026-06-07。

本文记录 Rust 重构后 `bangdream-optimize` 的当前状态。它是状态快照，不是最终设计规范。

## 项目结构

项目现为 Rust 工作区，并按独立运行时边界划分：

- `crates/core`：领域模型、得分计算、道具搜索、队伍生成、单曲 DP 主入口与 medley 候选生成。
- `crates/single-dp`：用于 `Sa * (D + Sb) + C` 模型的单曲按位 DP 引擎。
- `crates/team-prune`：共享的支配图与分组覆盖剪枝组件，供单曲 DP 与 medley 候选剪枝复用。
- `crates/medley-solver`：在已构建候选队伍上进行 medley 候选选择，拆分为精确标量/AVX2 与 random-bucket 回退路径。
- `crates/data`：Bestdori/game-data 映射与计算快照构建。
- `crates/service`：共享优化服务编排。
- `crates/storage-local`：桌面端本地 JSON 玩家存储。
- `crates/storage-mongodb`：服务端 MongoDB 玩家存储。
- `crates/web-wasm`：浏览器 WASM 计算边界。
- `apps/server`：独立 HTTP 后端。
- `apps/web`：Web 与桌面共用的前端 UI，含 `game-data` 同步与用户配置。
- `apps/desktop`：复用 `apps/web` 的 Tauri 壳。
- `tools/sync-bestdori`：静态 Bestdori 镜像生成工具。

`tsugu-bangdream-bot` 的旧 Node/CPP 运行时不再是本项目的必经执行链。迁移后预期
`tsugu-bangdream-bot` 适配该独立服务，而非继续保留旧接口。

## 运行目标

### Server

后端可从 MongoDB 与本地静态 Bestdori 镜像读取玩家数据和游戏数据，再通过 Rust 进行计算。
对外稳定路由为新的 `/v1/calc-result`，不再兼容旧的 `calcResult` 契约。

### Web

Web 端设计为无需运行时计算服务。网站服务器或 CDN 提供静态 `/game-data` 镜像。
浏览器将该镜像同步到 IndexedDB，并向 WASM 边界传入本地 JSON 完成计算。

### Desktop

桌面端通过 Tauri 重用 Web UI。
用户数据使用本地 JSON 与缓存存储；计算由原生 Rust 完成，不依赖浏览器 WASM。

## 当前核心计算流程

### 单曲

单曲计算使用 `crates/single-dp`。
DP 扩展实现位于 `src/lib.rs`，输入预剪枝在 `src/prune.rs`。

模型定义：

```text
score = floor(Sa * (D + Sb) + C)
```

当前 DP 状态：

- `Sa`：已选卡牌属性值加和。
- `Sb`：已选技能/加成项加和。
- `C`：按卡片舍入得到的修正项。
- bitmask：已占用的技能位。
- frontier 索引中的队长使用标记。
- 用于结果重建的已选卡牌 ID 与队长 ID。

DP 按角色分组，防止同角色重复出场通过分组扩展实现。
一个状态可以保留“未选队长”或“当前卡牌选为队长”。
Pareto 剪枝会剔除在 `Sa`、`Sb`、`C` 三维上被支配的状态。
后缀界与 seed 上界用于进一步剪枝。

在 DP 扩展前，单曲卡片会先构建共享的 `team-prune` 支配图。
当存在同角色更优卡支配其属性、所有普通位贡献和队长位贡献时，该卡可被覆盖淘汰。
当在限制最多屏蔽 4 个队友角色桶后，每个目标卡仍存在跨角色替代，则也会被淘汰。
此外，当强制包含该卡的所有队伍上界都严格低于当前 seed 值时，该卡也可被移除。

设置 `BANGDREAM_OPTIMIZE_DP_TRACE=1` 后，单曲计算会输出每组剪枝与 DP 扩展统计：旧/新 frontier 大小、插入尝试、可行性剪枝、上界剪枝、已完成状态、incumbent 更新、Pareto 拒绝/插入，以及被移除的支配状态。

### Medley

Medley 当前没有完整的 medley DP。
当前流程为：

1. 构建经过修正的卡牌属性与剪枝配置。
2. 构建签名（signature）对应的可用卡池。
3. 从卡池枚举合法 5 卡候选队伍。
4. 使用精确谱面 Meta、技能顺序与每首歌队长对候选队伍计分。
5. 压缩已使用卡位掩码。
6. 将候选队伍交给 `crates/medley-solver`。
7. 用标量或 AVX2 求解器选择 3 支互斥队伍。

Medley 实现按职责进一步拆分：

- `team.rs`：公共候选构建编排、trace 汇总与候选生成测试。
- `enumeration.rs`：签名池枚举、seed 候选过滤、原始候选追踪、掩码压缩。
- `scoring.rs`：5 卡候选精确计分、技能 Meta 缓存、技能排序、队长选择、签名分类。
- `seed.rs`：精确/局部搜索构建 seed incumbent。
- `prune/`：签名池构建、卡牌预剪枝与 trace 诊断。

当有 AVX2 可用时会自动走精确 AVX2 求解。
窄掩码使用 `u64`，宽掩码通过 `Vec<u64>` 表示。
当求解输入超过精确阈值时，切换 random-bucket 回退；该路径也包含 AVX2 双指针扫描实现。

### Medley 候选剪枝

候选剪枝现在以签名池为粒度。
每个签名池独立生成，最终枚举前会再次校验候选队伍的精确签名。

medley 专用剪枝实现位于 `crates/core/src/medley/prune`：

- `signature.rs`：签名定义与完成检查。
- `hard.rs`：剪枝配置、硬支配、incumbent 上界、与剪枝约束。
- `contribution.rs`：仿射贡献支配、贡献覆盖剪枝与相关诊断。
- `pool.rs`：签名池活动卡构建与候选数量估计。
- `global.rs`：跨约束上下文的 trace 全局剪枝汇总（仅 trace）。
- `stats.rs`：统计计数与诊断输出格式化。

签名类型：

- `Mixed`
- `UnifiedBand(band_id)`
- `UnifiedAttribute(attribute)`
- `UnifiedBandAttribute(band_id, attribute)`

重要说明：`Mixed` 在池层允许全部卡牌。
完整的 mixed/unified 分类仅在选满一支队伍后再做精确检查。

### 严格安全剪枝

以下剪枝规则目标是对当前精确得分模型保持安全。

#### 签名可成队性

若某张卡在该签名下无法与不同角色的卡组合成合法 5 卡队伍，则从签名池剔除。

#### Incumbent 上界

当已有 incumbent 时，如果某卡/签名对强制参赛时的上界 + 其他歌曲上界之和无法超过 incumbent，则可移除。
该上界会有意高估队友属性与技能 Meta。

该规则对“严格更优解”安全；可能会移除与 incumbent 等分的备选。

#### 同角色硬支配

在固定签名下，卡 A 同角色硬支配卡 B，当：

- `stat(A) >= stat(B)`
- 在 3 首歌 x 6 个技能位的所有解析后 meta 点均 `>=`
- 至少一个比较项更大，或卡 id 作为 tie-break 时 A 更优

若一张卡至少有 3 个同角色支配者，则可剔除，因为任意 medley 最多只会选 3 支队伍。

#### 跨角色硬支配

在固定签名下，硬支配边通过共享 `team-prune` 的支配图辅助构建并作传递闭包。
同角色与跨角色覆盖计算同样复用 `crates/team-prune`。

跨角色删除采用覆盖计算：

- 目标队伍最多屏蔽 4 个队友角色桶；
- 其余 2 支队伍最多可占用 10 张卡；
- 每个角色在其他两队最多占用 2 张；
- 目标角色的支配卡不受目标队友卡位屏蔽。

若最坏情况下可替换数仍为正值，则目标卡可移除。

该规则的安全性依赖其底层支配边安全。

#### 候选 incumbent 过滤

当某候选队伍已精确计分后，如果它与其他歌曲上界之和无法超过 incumbent，则可跳过该候选。
该规则对“严格提升 incumbent”是安全的。

### 模型化剪枝

当前贡献剪枝基于模型近似，在其仿射近似内安全，但不足以证明对完整谱面计分模型是严格正确的。

#### 贡献函数

对每张卡、签名与谱面，当前模型为：

```text
f(x, y) = stat * x + meta * y + round(stat * meta)
```

其中：

```text
meta = normal_est + max(0, captain_meta - captain_est)
```

- `normal_est`：卡牌 5 个普通技能位 meta 的平均值。
- `captain_meta`：卡牌第 6 位（队长位）meta。
- `captain_est`：该签名/谱面下已有队长的签名级种子估计。
- `x`：谱面 `D` + 同伴普通位 meta 范围 + `captain_est`。
- `y`：同伴属性范围。

比较会检查签名级 `x/y` 矩形四个顶点。
若 `left.f(x, y) >= right.f(x, y)` 在所有顶点和所有谱面上成立，则在该模型下 left 可替代 right。

生成的贡献支配图还会做传递闭包后用于覆盖剪枝。

#### 安全性说明

当前不属于完全精确安全，原因：

- `normal_est` 是平均值，不是谱面 DP 实际选择的技能顺序。
- `captain_est` 是估计值，不是每支队伍的真实队长贡献。
- 精确 medley 计分在枚举后仍会重排技能并为每谱面单独选队长。

该剪枝在真实数据下能显著降低候选量，但在严格验证时应视为启发式，除非将其替换为严格区间/分段证明方案。

## 内部埋点

`BuildResult` 已携带可选 `metrics` 用于内部测试。
服务端还可在每次成功计算后追加一行 JSONL：

```bash
BANGDREAM_OPTIMIZE_TELEMETRY_JSONL=var/telemetry/internal.jsonl
```

默认埋点仅记录规模与性能计数，不包含卡牌明细、area-item 快照或完整请求体。
参见 `docs/telemetry.md` 与 `docs/server-api.md`。

## 验证状态

当前改动后的验证命令：

```bash
cargo test -p bangdream-optimize-core
cargo test -p bangdream-optimize-single-dp -p bangdream-optimize-core -p bangdream-optimize-team-prune
cargo check --workspace --all-targets
```

以上命令均通过。

Mongo smoke 也通过当前代码运行。该命令仅使用本地环境变量，不在此记录私密连接信息。

## 已知风险

1. 基于贡献支配的剪枝尚非精确安全。
2. `Mixed` 签名剪枝范围较大，因为 mixed 池允许全部卡牌，精确签名在形成完整队伍后才校验。
3. 实际性能受签名池后续保留卡牌数量影响较大。

## 建议下一步

1. 增加 feature/开关以启用或关闭模型化贡献剪枝。
2. 若需严格安全的贡献剪枝，考虑用分段的队长函数替代 `captain_est`。
3. 若需要完全精确安全，进一步用严格的普通技能区间或位点感知证明替代 `normal_est`。
4. 增加更细的 contribution 边计数、闭包新增边与覆盖失败诊断计数。
5. 对代表性测试集新增 Rust 与旧实现的黄金结果对比。
