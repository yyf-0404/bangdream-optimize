# 新计分的缓存、控分边界与剪枝上界

> 本文记录迁移前的独立模块阶段；正式入口迁移及当前验收见[原生算术迁移验收](scoring-migration.md)。

日期：2026-09-20。接续[真实输入与谱面接入](scoring-integration.md)。

## 完成范围

新计分模块现在有完整浮点缓存、目标分数到浮点综合力区间的反查，以及适用于已确定技能时间线的保守分数上下界。已在真实档案回放中调用并验证这些接口。

**旧应用的搜索尚未改用这些接口。** 这一步完成的是搜索所需的基础边界，不是整个贡献剪枝、MITM 控分和自动编队已经迁移。旧贡献剪枝未删除，线上行为未变，未改版本或历史结果。

| 接口 | 作用 |
| --- | --- |
| `scoring::CachedScore` | 缓存完整 `ScoringPower` 位模式，绑定不可变的 `CompiledScore` 和综合力范围 |
| `CachedScore::lower_bound` | 返回第一个达到目标分数的可表示 f32 综合力，不存在时返回 `None` |
| `CachedScore::powers_for_scores` | 返回能落入指定整数分数区间的浮点综合力闭区间 |
| `PowerRange::from_member_choices` | 按物理队伍位置的三维候选范围，执行原生顺序的浮点累加，覆盖所有合法补全 |
| `CompiledScore::envelope` | 为一个确定时间线预计算随综合力变化的线性上下界及舍入误差 |
| `ScoreEnvelope::bounds` | 常数时间返回保守整数分数范围；超出已验证综合力范围时报错 |

源码为 `crates/core/src/scoring/search.rs`、`bounds.rs`，状态压缩模块新增只读分组访问接口。

## 浮点边界查找

有限非负 f32 的位模式与数值顺序一致。新接口在这个离散有序集合上二分，不按 `±1` 修改综合力，最多进行 31 次中点评分。

固定判定、Combo、Fever 和技能时间线后，评分是综合力的单调非降函数：每个非负乘除、舍入和取整步骤都保持单调性。因此分数可能有平台，也可能跳过某个整数，但目标分数的逆像仍是一个闭区间或空集。

若第一个使分数大于目标上限的浮点值为 `p`，有效上限是 `p` 的前一个可表示浮点值，不能减去 `1.0`。缓存同时保存评分和目标分数的下边界；改变谱面、模式或技能时间线需创建新的缓存，不能复用旧边界。

构造时先验证范围最大综合力的全部算术和 uint32 转换。溢出作为错误返回，不当作“很高的分数”或静默饱和值参与二分。

## 上下界推导

对一个已确定状态的按键，保留原生 f32 等级倍率 `L`、判定倍率 `J`、Combo 倍率 `C`，以及已经按 f32 合成的 `F = Fever × Skill`。令按键数转换到 f32 后为 `N`，不舍入的参考系数为：

`a = ((L / N) × 3) × J × C × F`

它与旧连续系数不同：使用原生已经舍入的输入值，不重新用 f64 计算等级倍率或技能合成。系数本身的 f64 运算也逐步向外扩展上下界，避免把用于证明的浮点数误差忽略掉。

在已验证最大综合力下，计算各步最大的 f32 运算结果。每步误差用该最大结果处的一个完整 ULP 覆盖，包括次正规数和跨数量级边界，并沿后续乘除传播。使用完整 ULP 是保守取值，不是假设每一步都产生该误差。

第一段取整损失小于 `1`，传至末尾小于 `F`；第二段取整损失小于 `1`。`floor(f32)` 在有效 uint32 范围内仍可由 f32 精确表示，因此整数转回 f32 不再额外损失精度。

按状态重复次数汇总后，对真实浮点综合力 `P`：

`P × a_lower − E_round − E_truncate ≤ score(P) ≤ P × a_upper + E_round`

下界向上取整、上界向下取整，得到整数分数范围。这里 `E_truncate = Σ(F + 1)`，`E_round` 由上述逐步误差传播得到，不采用经验常数。零倍率按键的贡献直接为零。

队伍综合力上界单独处理：每个物理位置分别取所有候选卡最终 P/T/V 的逐维上限，再按正式计分路径累加。它放宽了卡牌关联性和互斥约束，因此只能高估；下界同理。**不能用五张卡的标量总和代替逐维、按位置的浮点累加。**

这份证明只覆盖同一已确定时间线；不能把某个顺序的界直接用于所有技能排列、不同队长或可能改变的排队状态。

## 验证结果

- 新增 7 项测试：完整浮点缓存、边界二分穷举、次正规数与跨数量级边界、溢出拒绝、随机数量级上下界、三维候选范围和分支剪枝。
- 核心全量测试 238 项通过、1 项原有跳过；WebAssembly 前端依赖编译通过。浏览器实际执行仍未纳入本轮验收。
- 小规模验证为每个位置 4 张候选、5 个位置，共 1,024 个组合，固定技能时间线；开启分支上界后保留相同最高分。检查每个叶子位于全部祖先的综合力区间内。
- 原 1,456 卡档案中的六队、手动／Auto 50%／Auto 75%、每组 120 种技能顺序，共 2,160 条时间线；各自检查当前综合力周围连续 129 个可表示浮点值，共 **278,640 次邻近输入检查**。上下界均包含实际评分，控分逆像与逐点检查一致。
- 抽取 18 条时间线，用原生 ARM32 指令检查每个目标分数区间的首尾点，以及相邻的外侧两个浮点点，共 72 个边界输入、51,840 次按键结算；加上原综合力回放的 12,960 键，共 **64,800 次原生结算对照通过**。
- 固定队伍内搜索技能顺序时，上界共排除 1,993／2,160 个顺序，每组排除 99～118／120 个，保留与全枚举相同的最高分。只在上界严格小于当前最佳时排除，保留最高分并列的顺序。
- 测试范围上限设为综合力 1,000,000；样本中额外单精度舍入预算为约 **1.273～4.634 分**。它不包含取整导致的连续估算与实际分数差；样本总上界余量为 86～1,216 分，二者不能混称。

顺序排除数是边界有效性验证，不是完整搜索计时。生成时间线、压缩和构造上下界本身有成本；需要在搜索中复用后才能判断端到端收益。

复现：

```powershell
cargo test --offline -p bangdream-optimize-core --lib
cargo check --offline -p bangdream-optimize-web-wasm --target wasm32-unknown-unknown
cargo build --offline --release --manifest-path scripts/research/native-score-probe/Cargo.toml --target-dir target/native-score-audit --bin integration
target/native-score-audit/release/integration.exe tmp/profile323-ab-benchmark/input.json tmp/profile323-ab-benchmark/results.json tmp/native-score-audit/profile323-integration.json
target/native-score-audit/release/integration.exe tmp/queue-three-song-benchmark/input.json tmp/queue-three-song-benchmark/optimized-max-1.json tmp/native-score-audit/queue-integration.json
python -B scripts/research/verify_native_scoring_integration.py tmp/native-score-audit/profile323-integration.json tmp/native-score-audit/queue-integration.json
```

## 应用迁移仍需处理的具体位置

| 原位置 | 依赖与后续改动 |
| --- | --- |
| `model/chart/queue/ordered.rs` | `HashMap<i32, StatScores>` 依赖旧整数综合力及旧单键值；需与窗口增量评分同时迁移 |
| `pt_maximize/distribution.rs` | 协力分数缓存的 `stat: i32`，分布与加权结果必须使用同一新口径 |
| `score_range/mitm.rs` | 下边界缓存、`±1` 区间、整数候选键需替换；二卡／三卡标量相加并不保留新三维累加顺序，不能只修改类型 |
| `team_prune/hard.rs`、`medley/enumeration.rs` | 旧 `stat × meta` 上界需替换；未知成员／技能不能套用单个已确定时间线的系数 |
| `team_prune/contribution.rs` 及 `queued/` | 要把连续贡献差、两侧舍入损失、替换后的三维综合力以及不同排队状态一起覆盖，不能直接沿用旧预算 |
| `medley/scoring/cached.rs` | 只缓存候选技能顺序的种子，不缓存最终分数；保留这类复用思路，但最终评分与剪枝必须统一 |

几何窗口、卡牌冲突位图等与评分算术无关的缓存可以保留。下一步重点是把新边界接入完整候选贡献模型，并验证替换卡牌后仍安全；之后再迁移实际搜索入口、结果规则标识和全规模性能基准。
