# 最大平均活动 PT 设计草案

状态：实现中。单曲与三曲组曲的严格计算主链、活动 PT 公式、数据/服务/HTTP/WASM/桌面入口和
基础前端表单已落地；单曲大卡池搜索已完成生产级严格剪枝，三曲组曲也已切换到共享的动态均分带
严格枚举器，当前继续以完整 fixture 收紧组曲候选生成和互斥查询。

## 0. 当前实现进度

已完成：

- `event_pt` 共享活动 PT 公式，以及逐样本取整后再求平均；
- 单曲完整队伍的 `5×6` 精确增量矩阵、32 状态计数 DP 和技能排队严格枚举；
- 协力的五名队长技能、可配置第六技能来源及 fever 精确计分；
- 单人、协力、竞演多人、5v5 多人和 Challenge CP 的场景输入与结果模型；
- 活动加成进入综合力/进入 PT 倍率两条共享数据准备路径；
- fixture/文件系统/静态镜像、service、HTTP、WASM、桌面命令和浏览器 Worker 入口；
- 三曲累计 Combo、三队卡片互斥、随机顺序均值 seed、动态严格均值下界和余数直方图卷积；
- 前端模式选择、统一/分别设置队友参数、按演出形态启用必填校验，以及单曲/三曲平均 PT 结果展示；
- 单曲全局 incumbent、整套 `道具 × mode` 严格 Meta 上界、递归后缀上界和叶子精确分布上界；
- 单曲 Mixed 大卡池快速路径：首套保留安全贡献支配以建立 incumbent，后续由严格分支上界直接搜索；
- 单曲候选支配按物理角色拆分为 `FullSkill`、`Captain`、`Filler`；协力独立构造队长池和四个填充位
  候选池，填充位完全不读取技能，只比较综合力和活动 PT 加成；
- `single` 共享层统一持有 mode 候选生成、卡片综合力/技能解析、技能 Meta 和单队贡献支配入口，
  `maximize` 与 `pt_maximize` 不再分别准备同一张卡；
- 协力“枚举整队五个队长”和“搜索阶段固定一个队长”共用同一个计分内核、fever 谱面缓存与分数
  直方图缓存。
- 组曲候选先按单曲可达均分带过滤，再由 `medley-solver` 共享的 i64 动态均分带枚举器自动选择歌曲
  查询排列，并使用窄/宽掩码的 AVX2 或标量内核枚举全部互斥三队。

已完成：

- 结果页顶部的“计算场景”只显示演出形态以及适用的排名/胜负；倍率选择紧随其后并使用分段选项，
  队长、队友、最低综合力和 fever 等输入细节不再混入场景摘要；
- 单曲和组曲的候选构造、求解耗时与候选计数继续保留在结构化结果 metrics 中，结果页只显示总耗时；
- 最优结果按“平均 PT、平均分数、canonical 卡片/队长 ID”依次比较。

待完成：

- 为演出形态切换、统一/分别必填状态、请求序列化和单曲/组曲结果渲染补充前端自动化测试；
- 完成单人、协力、竞演、5v5 和 Challenge CP 的完整真实 fixture 结果回归；
- 根据各演出形态的真实基准继续收紧缓存与候选生成，并评估浏览器标量 WASM 的组曲性能；

### 0.1 单曲完整 fixture 基线

当前单曲完整 fixture 枚举 108 套未被逐卡综合力支配的道具和 24 个 mode，共 2592 套
`道具 × mode`。生产级搜索实现后：

- 整套 Meta 上界在昂贵预剪枝前排除 2411/2592 套 mode；
- 递归实际访问约 109 万个节点和 39528 个五卡叶子；
- 39478 个叶子由 Meta 严格上界终止，约 50 个叶子继续进入严格精确分布计算；
- release 无追踪 fixture 从约 7.17 秒降至约 0.91 秒；
- 回归结果保持为平均 PT `550`、综合力 `379857`、卡片
  `[1525, 1983, 2040, 2280, 2284]`、道具 `Band 2 / Powerful / Performance`。

Meta 只用于严格上界，不决定最终分数或结果。技能贡献按谱面的六个物理技能位置预生成，并按
`(duration, score_up, rateup)` 技能签名缓存。综合力上界和分数上界均向外取整；综合力、PT 加成和
技能贡献可以分别来自不同的可行完成队伍，这只会放宽上界，不会漏解。最终候选仍使用 `5×6` 精确
整数增量矩阵、32 状态计数 DP 或技能排队严格时间线计算。

Mixed 快速路径只在已有全局 incumbent、完整队伍技能场景、无技能排队且非 Festival 时启用。
候选支配本身不再假设所有演出都由五张卡的技能计分，而是显式携带卡片承担的物理角色：

- `FullSkill`：单人、竞演、5v5、CP 挑战等完整队伍技能场景，沿用原贡献支配；
- `Captain`：协力中的本队队长，技能参与五名玩家的技能排列，并可能成为第六技能，允许沿用原贡献支配；
- `Filler`：协力中的其余四张卡，技能完全无效，只按同角色内的 `(综合力, PT 加成)` Pareto 支配，
  相同值保留卡片 ID 最小的规范解。

协力会分别生成 `Captain` 和 `Filler` 两个池，再按角色互异约束组合。昂贵的队长贡献图只在尚无
incumbent 时用于建立首个严格下界；已有 incumbent 后，队长使用不删卡的安全池，由相同的后缀综合力/
PT 上界和精确计分缓存完成剪枝。完整桌面缓存基准为 135 套道具、20 个 mode：结果保持平均 PT
`1596.4`、综合力 `400996`、卡片 `[1705, 1785, 2080, 2101, 2102]`、队长 `1785`；release 追踪运行
约 `2.48s`，其中准备约 `1.07s`、枚举约 `1.07s`、精确计分约 `0.12s`。若每套 mode 都重建队长贡献图，
相同输入约需 `13.6s`，其中预剪枝约 `12.1s`。共享层迁移后在不同系统负载下实测约
`2.48s～4.71s`，搜索计数与最终结果完全一致；迁移没有增加候选或精确计分次数。

### 0.2 三曲组曲完整 fixture 基线

当前组曲完整 fixture（1414 张卡、108 套未被逐卡综合力支配的道具）首套原始数据为 291961 个候选。
按每曲与另外两曲最大均分组成的安全上界过滤后保留 135063 个候选；Raw 候选构造约 2.9 秒，随机顺序
均值矩阵约 5～6 秒，512 轮 Random Bucket seed 约 0.7～1.2 秒。

近优带已复用 `medley-solver` 的共享严格枚举入口。它使用精确 i64 `mean_numerator`、从六种歌曲排列中
选择预计工作量最小者、随精确 PT incumbent 单调提高下界，并用窄/宽掩码 AVX2 批量检查第三队；与
最佳总分 solver 不同的是，近优带内不会在首个互斥第三队处停止。首套第三队位置检查由约 26.72 亿降至
18.27 亿，互斥三队 40 个，扫描约 11.0 秒，平均 PT 保持 `734.941326`。第二套保留 135484 个候选，
第三队位置约 20.92 亿，互斥三队 38 个，扫描约 23.7 秒（宽掩码）；改动前约 56.3 秒。

不同道具现在共享外层精确 `AveragePt` incumbent。转换为均分 numerator 下界时使用达到相同平均 PT
所需值的精确向上取整，而不是只保留严格超过 incumbent 的方案，因此平均 PT 相同但规范序更优的方案
仍会被枚举。首套道具照常构造三队 seed 以建立全局 incumbent；后续道具不再构造不会参与最终结果的
本地 seed，而是直接进入只返回全局改进的搜索。若一套道具不能追平全局下界，它可以安全地返回空结果，
由外层继续保留已有最优解。

共享前，完整 fixture 的 108 套道具中有 21 套进入完整搜索，总用时 `2056.65s`（34 分 16.65 秒）；
其中候选构造 `53.86s`、均值矩阵 `126.49s`、seed `36.27s`、近优带扫描 `1798.38s`。最终实现还把
全局下界推入 Raw 候选生成，并用每队“最佳技能顺序分数 × 120”作为随机顺序均分 numerator 的严格
上界，在构造昂贵的 `5×6` 精确增量矩阵前进行三曲安全带过滤。仍有 21 套通过外层粗上界，但其中只有
15 套需要进入严格扫描；总用时降至 `49.94s`，其中候选构造 `24.93s`、均值矩阵 `6.38s`、seed
`0.62s`、近优带扫描 `14.98s`、其他 `3.03s`。总用时约提升 `41.2×`，严格扫描约提升 `120×`，
最终平均 PT `734.941326`、PT 范围 `733～738` 和三支队伍均不变。

候选数仍高于最大分数搜索的原因不是 PT 多枚举了一类队伍，而是两者的严格阈值不同：最大分数搜索可用
高分精确 incumbent 持续过滤“最佳技能顺序分数”，PT 搜索必须围绕随机技能顺序的均值保留一个完整
PT 除数宽度，且不能使用最佳顺序分数排除均值仍可能获胜的队伍。候选末尾追加的 seed 回退项至多三个，
不是数量差异的主要来源。

已经试验但未保留为主路径的方案：

- 全量严格最大均值 seed：单套约 119 秒，远慢于近似 seed，且 seed 不承担正确性证明；
- 15000 轮 Random Bucket：约 44～48 秒，缩带收益不足，改为 512 轮并与廉价 seed 取均值较优者；
- 宽掩码 AVX2 连续扫描：没有改善 30 亿级检查的算法复杂度；
- 每卡倒排位图、单卡缺席表和两卡联合覆盖入口：在该候选分布上内存访问量更大，实测慢于直接扫描。

## 1. 目标

新增一个与 `maximize`、`score_range` 并列的计算用例：在用户指定歌曲、活动和演出形态后，
枚举队伍、队长和区域道具，寻找 **5 种技能随机排列时平均活动 PT 最高** 的方案。

三个用例的职责保持独立：

```text
maximize      最大化演奏分数
score_range   搜索指定活动 PT 对应的分数和演奏计划
pt_maximize   最大化随机技能顺序下的平均活动 PT
```

`pt_maximize` 必须复用共享卡片准备、综合力、谱面精确计分和活动 PT 公式，不复制活动加成逻辑。

## 2. 术语

“单人”和“多人”只描述演出参与方式，不能把所有多人玩法都称为“协力”。设计中使用以下演出形态：

```rust
enum LiveVariant {
    Solo,
    Cooperative,
    Versus,
    Festival,
    Medley,
    ChallengeCp,
}
```

- `Solo`：单人演出。
- `Cooperative`：任务 Live、Challenge 和 EX 的多人协力模式。
- `Versus`：竞演活动的多人模式，不称为协力。
- `Festival`：5v5 的多人模式，不称为协力。
- `Medley`：单人组曲，没有多人选项。
- `ChallengeCp`：Challenge 活动消耗 CP 的挑战模式。

活动类型和演出形态是两个维度。支持范围按 `(EventType, LiveVariant)` 维护，不能再只维护活动类型列表。

## 3. 初始支持矩阵

| 活动 | 单人 | 协力 | 竞演多人 | 5v5 多人 | 组曲 | CP 挑战 |
| --- | --- | --- | --- | --- | --- | --- |
| `mission_live` | 是 | 是 | 否 | 否 | 否 | 否 |
| `live_try` / EX | 是 | 是 | 否 | 否 | 否 | 否 |
| `challenge` | 是 | 是 | 否 | 否 | 否 | 是 |
| `versus` | 是 | 否 | 是 | 否 | 否 | 否 |
| `festival` | 是 | 否 | 否 | 是 | 否 | 否 |
| `medley` | 否 | 否 | 否 | 否 | 是 | 否 |

EX 对应当前代码中的 `EventType::LiveTry`。对外界面可以显示“EX”，内部 schema 继续使用
`live_try`，避免新增等价活动类型。

## 4. 优化目标和“平均”的定义

纳入平均的随机因素包括：**每首歌中前 5 次技能触发按 5 种技能的 `5! = 120` 个排列等概率随机**；
协力第六技能使用“最大综合力”时若多人并列，还要在并列玩家之间等概率随机；使用“随机”时则在
5 名玩家之间等概率随机。

谱面仍有 6 个技能键。前 5 个技能键让 5 种技能各触发一次，第 6 个技能键不参与排列：

- 使用自己完整队伍技能时，第 6 次固定重复队长技能；
- 任务 Live、Challenge、EX 协力时，第 6 次按用户选择的规则取队长技能：最大综合力、指定玩家，
  或在 5 名玩家中均匀随机。最大综合力规则存在并列时，在并列玩家之间等概率随机。

因此固定第六技能时每首歌的基础样本数是 120，而不是对 6 次触发做 `6!` 排列。最大综合力有 `k` 名
玩家并列时样本数为 `120 × k`；五人随机时样本数为 `120 × 5 = 600`。第六技能选择与前五次排列独立，
每个联合样本等概率。

队友预计分数、队友综合力、队友技能参数、队内排名和胜负均为用户给定的确定输入，不作为概率分布。

每一种技能顺序必须先执行完整的精确计分和活动 PT 向下/向上取整，再计算平均值：

```text
average_pt = Σ integer_pt(skill_order) / order_count
```

禁止先平均分数再代入 PT 公式，因为活动 PT 存在多层取整：

```text
E[PT(score)] != PT(E[score])
```

内部使用整数分子/分母表示平均 PT，方案比较时交叉相乘，不使用 `f64` 决定最优解：

```rust
struct AveragePt {
    pt_sum: u128,
    sample_count: u64,
}
```

结果对象保留最小 PT、最大 PT、样本数和精确分子/分母用于验证；前端摘要只显示取整后的平均 PT
和平均分数，不显示范围或样本数。

### 4.1 多曲组曲

组曲固定由三首用户指定歌曲组成，且没有多人模式。每首歌的技能顺序独立随机。队伍生成继续沿用
当前组曲的三队伍和卡片互斥规则，不改成整场固定同一支队伍。请求中的组曲歌曲数量必须恰好为三。

活动公式表中的“一曲、二曲、三曲”表示实际演奏的歌曲数量，不是歌曲序号。本功能只计算完整三曲，
因此固定使用三曲公式：

```text
D = 18500
PT(total_score) = 100 + floor(total_score / D)
```

取整作用于整场总分，因此不能分别计算每曲平均 PT 后相加，也不能先把各曲分数取平均再代入公式。

### 4.2 每支队伍的轻量候选

现有最大化组曲精确内核会构造无技能基础分和完整 `5 × 6` 技能整数增量矩阵。PT 最大化继续复用该
矩阵，但不执行“选择最高分技能顺序”的 32 状态 DP。

前五次技能的 120 个排列中，每张卡在每个位置恰好出现 24 次。第六次固定重复队长技能，而且改变
队长只会改变第六列的常数增量。因此每张谱面可以局部选择第六列增量最大的卡作为队长；它对所有
120 个排列逐点不差，不需要保留五个队长分支。

设 `base` 为无技能整数基础分，`delta[c][p]` 为卡 `c` 在技能位置 `p` 的精确整数增量，则 120 个
排列的分数和可以直接计算：

```text
mean_numerator = 120 × base
               + 24 × sum(delta[c][p], c=0..4, p=0..4)
               + 120 × max(delta[c][5], c=0..4)

expected_score = mean_numerator / 120
```

候选生成阶段只保存五张卡、卡片掩码、每曲队长和 `mean_numerator`，不为所有候选保存 120 项分布。
只有进入最终近优前沿的 `(team, chart)` 才按需用子集计数 DP 生成精确分数直方图并缓存。这样候选内存
仍与现有组曲处于同一数量级。

### 4.3 两阶段严格搜索

第一阶段把每曲的 `mean_numerator` 当成可加整数目标，复用现有组曲求解器的三队卡片互斥、按曲排序、
分支上界和窄/宽卡片掩码搜索，先找到较好的期望总分互斥方案作为 seed。实现时目标值使用 `i64`，
不把精确均值压回 `i32` 或提前取整；Random Bucket seed 只改善下界，不承担正确性证明。

第二阶段只搜索仍可能在 PT 取整后超过 seed 的近优前沿。因为：

```text
E[floor(S / D)] <= E[S] / D
E[floor(S / D)] >  E[S] / D - 1
```

所以期望总分比全局最大期望总分低至少 `D = 18500` 的方案，不可能得到更高的平均 PT。严格搜索仍按
每曲 `mean_numerator` 降序遍历；分支的最大可能期望分数代入上述上界后若不能超过 incumbent，即可
安全停止。共享 `medley-solver` 为此提供独立的动态均分带枚举入口：它复用最佳值求解器的排列选择、
分支截断和窄/宽掩码内核，但会继续检查阈值内全部互斥第三队；回调完成精确 PT 比较后可单调提高阈值。

Random Bucket 可以继续用于产生更好的 incumbent 或显式的近似模式，但严格模式不能把它作为最终结果。

### 4.4 近优方案的精确平均 PT

对进入近优前沿的每曲候选，按需用 32 状态子集计数 DP 生成精确技能顺序分数直方图，不逐条展开
120 个排列，也无需构造最多 `120^3` 项的完整总分列表。对单曲分数 `s` 分解：

```text
s = D × q + r
q = floor(s / D)
0 <= r < D
```

每曲保存 `Q_i = sum(q)` 和稀疏余数直方图 `H_i[r] = occurrence_count`。固定 `N = 120^3`，则候选
组合的精确平均 PT 分子为：

```text
pt_numerator = 100 × N
             + sum(Q_i) × (N / 120)
             + sum(floor(sum(r_i) / D) × product(H_i[r_i]))
```

最后一项只需要对三曲的稀疏余数直方图做卷积：先卷积较小的两个直方图，再与第三个直方图计算跨越
`D` 和 `2D` 的次数。方案比较继续使用整数分子/分母，不使用浮点数。

### 4.5 与现有最大化组曲核心的复用边界

可以直接复用或抽取为共享层的部分：

- 卡片准备、角色加成、区域道具和活动加成进入综合力的规则；
- mode/signature 枚举、五个不同角色约束和三队卡片互斥掩码；
- 能证明在所有谱面和所有物理技能位置上逐点不差的 hard/contribution 支配；
- 无技能基础分、`5 × 6` 精确增量矩阵和技能重叠处理；
- solver 的候选按曲排序、搜索曲序、窄/宽掩码以及 AVX2 互斥检测。

不能原样复用的部分：

- `RawTeamCandidate.scores: [i32; 3]` 和只保存每曲最高分技能顺序的叶子结果；
- 依赖“最高分技能顺序”或当前最高总分的 incumbent 候选过滤；
- exact solver 找到当前最高分的第一个互斥第三队后立即 `break` 的逻辑；
- 用单个可加分数直接决定最终方案的 `MedleySolverPlan`。

建议保留现有最大化入口不动，把枚举递归、逐点安全支配、精确矩阵构造和掩码扫描抽成共享组件；在
`pt_maximize/medley.rs` 中增加均值 seed、近优前沿和三曲余数卷积。组曲固定走互斥三队搜索，可以继续
使用长度为三的按曲数组。

### 4.6 单曲公共搜索骨架

现有单曲最大化会在搜索状态中同时选择五张卡和五个技能位置，因为技能顺序本身是可优化变量。
PT 最大化中的前五次技能顺序是等概率随机变量，不能沿用该状态空间。新搜索只枚举无序五卡集合，
技能位置分布在叶子上由精确子集计数 DP 或技能排队时间线计算。

每套区域道具、每个有效 mode 依次执行：

1. 使用所有允许卡片按角色分别取综合力、PT 加成、普通位置 Meta 和队长位置 Meta 的独立极值，
   在旧预剪枝前计算整套 `道具 × mode` 的严格 PT 上界；
2. 上界不可能超过全局 incumbent 时跳过整套 mode；否则准备未取整综合力、解析后的技能、原始活动
   PT 加成和角色 ID；
3. 首套 Mixed 和其他 mode 做包含 PT 加成维度的逐点安全支配；已有 incumbent 的 FullTeam Mixed
   快速路径改用规范化和同技能支配，避免重复执行昂贵的跨角色贡献支配；
4. 按角色分组，深度优先选择五个不同角色，不在递归状态中分配技能位置；
5. 为每个角色后缀按还需选择的卡数预计算综合力、技能 Meta、PT 加成和最小卡片 ID 上界；
6. 递归分支的严格 PT 上界不能超过当前最佳平均 PT 时剪枝；
7. 五卡叶子统一取整综合力和 PT 加成，检查最低综合力限制，再以更紧的技能 Meta 上界判断是否值得
   构造精确矩阵；
8. 生成精确技能顺序分布，对每个样本先算整数 PT，再比较平均 PT；
9. 平均 PT 相同时优先选择平均分数更高的方案；平均分数也相同时，再按 canonical 卡片 ID
   规则选择稳定结果。

活动加成进入综合力的模式不再保存独立 PT 加成维度。活动加成进入 PT 倍率的模式必须保存五张卡的
原始加成之和，并在完整队伍形成后统一取整，不能逐卡取整。

### 4.7 支配与分支上界

对于活动加成进入 PT 倍率的模式，只有替换卡在以下维度全部不差时才能建立逐点支配：

- 未取整综合力；
- 原始活动 PT 加成；
- 五个普通技能位置和第六个队长位置的精确/安全技能贡献；
- mode、角色唯一性和技能条件仍可满足。

活动加成已经进入综合力的模式不需要额外比较 PT 加成，可以复用现有单曲逐点支配。现有任何只证明
“最佳技能顺序分数不下降”的剪枝都不能复用；必须保证对随机顺序中的每个物理技能角色均不下降，才能
由活动 PT 公式的单调性推出安全。

递归分支先使用便宜的逐点上界：独立取剩余卡的最大综合力、最大技能贡献和最大 PT 加成，得到任意
完成队伍、任意技能顺序都不可能超过的 `score_upper` 和 `point_bonus_upper`，再调用公式层的
`pt_upper_bound`。这些极值不必来自同一组卡，放宽只影响剪枝率，不影响正确性。

叶子构造精确分布前再使用更紧的平均上界。每种活动 PT 公式提供忽略向下取整后向外取整的仿射包络：

```rust
trait EventPtBound {
    fn pointwise_upper(score_upper: i32, input: &FixedPtInput) -> u64;
    fn expected_upper(mean_score: Rational, input: &FixedPtInput) -> AveragePt;
}
```

固定队内排名、胜负、队友预计分数等场景输入包含在 `FixedPtInput`。任何不能证明仿射包络安全的公式
先退化为 `pointwise_upper`，不能使用经验近似剪枝。

### 4.8 使用自己完整队伍技能的单曲

单人、竞演多人、5v5 多人和 Challenge CP 挑战都使用自己五张卡的完整技能，区别只在 fever、活动
加成位置和最终 PT 公式。它们共用同一个五卡搜索器，场景对象负责选择计分规则和 PT 公式。

对没有技能排队的谱面，复用无技能基础分和 `5 × 6` 精确整数增量矩阵。前五列使用 32 状态子集计数
DP 直接生成 `分数增量 -> 出现次数` 直方图：

```text
dp[0] = {(0, 1)}
position = popcount(mask)
dp[mask | (1 << card)][sum + delta[card][position]] += count
```

最终 `dp[31]` 的出现次数总和为 120，但算法会立即合并相同的部分和与最终分数，不构造 120 条技能
顺序记录。第六列由队长决定；若某张卡的第六列增量最大，它对整个直方图逐点不差，可以只保留该队长。
这里必须比较当前谱面、最终综合力、fever 规则下的第六位置**精确整数增量**，不能比较连续 Meta 或只看
技能面板倍率。队长选择为：

```text
captain = argmax_c exact_delta[c][5]
```

前五次的子集计数 DP 与队长无关，只需运行一次；选出队长后把 `exact_delta[captain][5]` 加到最终直方图
的每个分数桶。活动 PT 对分数单调，因此更大的常数增量对全部随机顺序逐点占优，也必然不降低平均 PT。
精确增量相同时，两名队长产生相同分数分布，按最终统一的并列规则选择 canonical 队长。

对存在技能排队的谱面，第六技能的实际开始时间可能依赖前五次顺序，队长不能只按独立第六列局部选择。
此时保留所有未被逐点证明支配的队长，对每个 `(前五次排列, 队长)` 使用当前严格六技能时间线计分。
队长仍是方案的一部分，不能针对每个随机排列分别选择不同队长。对固定五卡队伍分别计算：

```text
average_pt(captain) = sum(PT(exact_score(order, captain))) / 120
captain = argmax_c average_pt(captain)
```

因此最坏需要计算五个队长分布；可以先用每个队长的安全平均 PT 上界排除不可能获胜的队长，再对剩余
队长运行严格时间线。若两个队长平均 PT 相同，仍交给统一并列规则处理。

每个固定队伍和队长产生：

```rust
struct SingleTeamDistribution {
    score_histogram: ScoreHistogram,
    score_sum: i64,
    min_score: i32,
    max_score: i32,
    sample_count: u32, // 通常为 120
}
```

5v5 在相同分数分布上启用谱面 fever 区间，并把四名队友预计分数、队内排名和胜负作为固定 PT 输入；
竞演多人不启用 fever，只加入固定队内排名。Challenge CP 搜索以 200 CP 单倍结果为基础；结果页可将
它换算为 400/800/1600 CP 对应的 2/4/8 倍 PT。

### 4.9 协力单曲专用路径

任务 Live、Challenge 和 EX 协力中，自己队伍只有队长技能参与演奏；其余四张卡只影响个人综合力、
活动 PT 加成和最低综合力约束。因此不应把协力强行套入完整五技能队伍搜索。

协力路径先枚举自己的队长卡，再从其他不同角色中选择四张卡。后四张卡的技能不参与计分。“最大综合力”
规则下不能跨全部综合力范围只按 `(综合力, PT 加成)` 做 Pareto：自己的综合力变化可能改变第六技能来源。
“指定”和“五人随机”规则的来源集合不随综合力变化，可以省略下面的综合力分桶。

设四名队友的最高预计综合力为 `T`。完整队伍综合力取整后，按以下第六技能分布分桶：

```text
self_stat < T  -> 只在综合力为 T 的队友队长技能间等概率随机
self_stat = T  -> 自己与所有综合力为 T 的队友队长技能间等概率随机
self_stat > T  -> 固定重复自己的队长技能
```

只有在第六技能分布相同的桶内，才允许对固定队长的后四张卡按 `(未取整综合力, 原始 PT 加成)` 保留
Pareto 前沿和 canonical solution。跨桶支配必须额外证明第六技能分布逐点不差；初版不做这种剪枝。
可以先完成四卡 DP/枚举，再按最终取整综合力与 `T` 的比较结果分桶。

完整队伍形成后：

1. 前五次技能由自己的队长技能和四名队友的固定队长技能组成；
2. 无排队时用子集计数 DP 统计五个技能的 120 个等概率排列；有排队时走严格时间线；
3. 根据用户选择的队长规则确定第六技能来源集合；
4. 最大综合力并列或五人随机时，对候选队长技能做独立的等概率分支；
5. 使用 fever 区间精确计分，并对每个样本应用协力 PT 公式；
6. Challenge 结果再对每个整数 PT 样本计算 `ceil(PT / 20)` 的 CP 获取。

协力分数分布缓存键只需要包含最终个人综合力、五个玩家队长技能、第六技能候选集合及其概率和 fever
规则，不包含自己队伍中四张非队长卡的技能。

### 4.10 缓存与延迟精确化

单曲搜索使用两层缓存：

```text
(chart, final_stat, fever_rule, ordered six skills)
    -> exact score / compiled timeline

(chart, final_stat, fever_rule, five-skill multiset, captain or sixth-skill distribution)
    -> score histogram
```

候选先经过分支上界和叶子平均上界，只有仍可能超过 incumbent 时才构造完整直方图。无排队谱面一次构造
增量矩阵后使用 32 状态子集计数 DP；有排队谱面复用当前编译时间线和分数缓存。相同技能与相同部分和
在 DP 中立即合并；有排队时使用多重集合排列和出现次数权重压缩。`sample_count` 必须保持与完整 120 个
等概率排列一致。

### 4.11 单曲与现有核心的复用边界

可以直接复用的领域规则和数据结构：

- `PreparedCard`、`SongMode`、`TeamCardSkill`、`AreaItemPercent`、`SelectedAreaItems`；
- 共享综合力入口、五张卡未取整求和后统一 `floor_team_stat` 的规则；
- `SongMode::allows` 和统一 band/attribute 技能的 `resolve_skill`；
- 区域道具组合、零级整组停用和按卡片实际收益支配道具组合的规则；
- 谱面节点、理想 60 FPS/相位 0 时间、fever 区间和 Rate-up 的精确按键计分；
- `EventType`、歌曲/难度选择、服务层、WASM 和桌面端已有的输入准备模式。

已经从现有最大化实现中抽成共享组件并复用：

- `single::mode_candidates` 中的 mode 生成和无效 mode 删除；`maximize::mode_candidates` 仅保留兼容
  re-export；
- Chart 内部的无技能基础分与 `5 × 6` 精确整数增量矩阵构造；
- `single::exact` 的技能 ID 压缩、编译六技能时间线缓存和 `(stat, skill_ids) -> score` 缓存；
- `single::candidate::resolve_card` 中的卡片综合力与统一技能解析；
- `single::dominance` 中的单队角色化支配入口，以及顶层 `team_prune` 中由最大化组曲和单曲共同使用的
  复杂贡献/跨角色占用证明引擎；
- 共享 `event_pt` 中的正向活动 PT/CP 公式，由 score-range 与 PT 最大化共同调用。

按角色分组、剩余角色可完成性、综合力/PT/Meta suffix bound 和搜索 metrics 仍属于目标函数相关搜索
状态：最高分与平均 PT 所需状态不同，因此只共享其输入原语和严格计分内核，不强行合并递归状态。

支配框架只能复用结构，具体边必须按活动加成位置区分：活动加成进入综合力时，现有综合力/技能逐点支配
可以继续使用；活动加成进入 PT 倍率时，必须把原始 PT 加成加入支配维度。协力还必须按第六技能分布分桶，
不能跨桶套用综合力单调支配。

不能复用的目标相关实现：

- `single::exact::SearchState` 中把卡片放入五个技能位置的递归状态；
- 最大化单个技能顺序的 `normal_meta`、`position_order` 和 incumbent 分数剪枝；
- 叶子只计算一个最高分顺序和按最高分选择队长的逻辑；
- 只保存 `score/stat/team/captain` 的 `SingleSongResult`；
- score-range 的 MITM 查询、目标分数区间反解和演奏计划 DP；
- Medley 三队互斥 solver；单曲没有跨队卡片冲突，不应为复用而套用该 solver。

推荐把共享边界落在“卡片准备 + mode + 逐点支配 + 精确计分原语”，而不是让 `pt_maximize` 调用现有
`calculate_single_song`。后者已经把随机技能顺序压缩成唯一最高分顺序，进入该入口后信息无法恢复。

## 5. 不同演出形态的计分规则

### 5.1 单人

- 使用玩家自己队伍的全部技能。
- 使用当前理想 60 FPS、相位 0 的最大化精确计分规则。
- 前 5 次按 5 张卡的技能做等概率 `5!` 排列，第 6 次固定使用队长技能。
- 任务 Live 单人模式由用户输入支援乐队 PT 加成，并在单人 PT 公式中应用。
- Challenge 活动除普通单人演出外，还要单独计算 `ChallengeCp`。

### 5.2 任务 Live、Challenge、EX 协力

- 必须计算 fever。
- 一局使用 5 名玩家各自的队长技能，而不是自己队伍的全部技能。
- 前 5 次按 5 名玩家的队长技能做等概率 `5!` 排列。
- 第六次技能支持三种规则：使用预计综合力最高的玩家（并列时等概率随机）、固定指定玩家，或在
  5 名玩家中等概率随机；该选择与前五次技能排列共同参与平均 PT 计算。
- 自己的队长技能来自正在优化的队伍。
- 其余 4 名队友由用户提供预计综合力、预计队长技能加成和预计持续时间。
- 初版队友技能只支持普通 Score Up，不支持 Rate-up、统一技能或其他条件技能。
- 用户可以统一填写一份队友参数，也可以分别填写 4 份。
- 用户可以设置自己的最低综合力限制，不满足限制的队伍不能进入结果。
- 任务 Live 协力不计算支援乐队 PT 加成，也不显示或提交该输入。

### 5.3 5v5 多人

- 必须计算 fever。
- 自己的演奏分数使用自己队伍的全部技能。
- 用户填写 4 名队友的预计分数；可以统一填写，也可以分别填写。
- 活动 PT 使用自己的分数和队友预计分数形成的总分。
- 用户明确选择队内排名。
- 用户明确选择是否胜利。
- 队内排名和胜负是固定场景输入，不根据预计分数自动推导。

### 5.4 竞演多人

- 不称为协力。
- 用户明确选择队内排名。
- 当前需求没有要求 fever，暂按无 fever 记录。
- 使用竞演多人活动 PT 公式，不复用竞演单人公式。

### 5.5 Challenge CP 挑战

- 与 Challenge 的普通单人/协力演出分开建模和搜索。
- 使用 CP 挑战专用活动 PT 公式。
- 队伍搜索只计算固定消耗 200 CP 的单倍情况，不为其他 CP 档位重复搜索队伍。
- CP 档位不改变演奏分数；200/400/800/1600 CP 分别显示为单倍结果的 1/2/4/8 倍活动 PT。
- 活动加成加入综合力，按普通单人精确计分且不使用 fever；最终活动 PT 使用 CP 挑战专用公式换算。
- 结果页提供 200/400/800/1600 CP 选择并联动平均活动 PT。

### 5.6 Challenge 的 CP 获取

Challenge 活动的普通单人和协力结果除最大平均活动 PT 外，还要显示该方案对应的平均 CP 获取。

CP 必须对每一种技能顺序产生的整数活动 PT 分别计算，然后再求平均；不能使用已平均的 PT 再取整。
单个排列的 CP 获取按 `ceil(final_event_pt / 20)` 计算。

## 6. 活动加成的应用位置

活动加成只允许按以下两种方式之一应用，不能同时加进综合力又乘入活动 PT：

| 活动与演出形态 | 活动加成应用位置 |
| --- | --- |
| 组曲 | 加进综合力 |
| Challenge CP 挑战 | 加进综合力 |
| 竞演多人 | 加进综合力 |
| 5v5 多人 | 加进综合力 |
| 其他全部受支持形态 | 直接加入活动 PT 加成倍率 |

任务 Live、Challenge、EX 的协力模式需要最低个人综合力限制，但这些模式的活动加成属于 PT 倍率，
因此最低综合力按角色加成和区域道具等通常综合力规则计算，不包含活动加成。最低限制使用 `>=` 比较。

## 7. 输入模型

建议请求模型如下。字段名称是草案，最终以 Rust/JSON schema 为准。

```rust
struct PtMaximizeRequest {
    event_type: EventType,
    live_variant: LiveVariant,
    songs: Vec<SongSelection>,
    minimum_personal_stat: Option<i32>,
    mission_support_pt_bonus: Option<u64>,
    multiplayer: Option<MultiplayerInput>,
}

enum MultiplayerInput {
    Cooperative(CooperativeInput),
    Versus(VersusInput),
    Festival(FestivalInput),
}

struct CooperativeTeammate {
    expected_stat: i32,
    leader_score_up: FixedRate,
    leader_skill_duration_millis: u32,
}

enum TeammateInput<T> {
    Uniform(T),
    Individual([T; 4]),
}

struct CooperativeInput {
    teammates: TeammateInput<CooperativeTeammate>,
    leader_selection: CooperativeLeaderSelection,
}

enum CooperativeLeaderSelection {
    MaxStat,
    Specified { player_index: u8 },
    Random,
}

struct FestivalTeammate {
    expected_score: i32,
}

struct FestivalInput {
    teammates: TeammateInput<FestivalTeammate>,
    team_rank: u8,
    won: bool,
}

struct VersusInput {
    team_rank: u8,
}

```

`minimum_personal_stat` 只对任务 Live、Challenge、EX 的协力模式生效。服务端和 WASM 核心都必须
校验“演出形态与输入 variant 匹配”，不能依赖前端隐藏字段保证正确性。

`mission_support_pt_bonus` 只对任务 Live 单人模式生效且必须提供；任务 Live 协力和其他活动忽略该
字段并按 `0` 处理。

核心搜索仍统一计算单倍 PT，倍率不进入请求，也不改变最优队伍。结果页对 Challenge CP 提供
200/400/800/1600 CP，对应 1/2/4/8 倍 PT；其他模式提供 0/1/2/3 火，对应 1/5/10/15 倍
PT 和 CP 获取。组曲三曲作为一场整体应用同一火倍率。

## 8. 输出模型

```rust
struct PtMaximizeResult {
    event_type: EventType,
    live_variant: LiveVariant,
    team_card_ids: Vec<u32>,
    captain_card_id: u32,
    total_stat: i32,
    point_bonus_basis_points: u32,
    items: SelectedAreaItems,
    songs: Vec<PtMaximizeSongResult>,
    average_pt: AveragePt,
    min_pt: u64,
    max_pt: u64,
    average_cp_gain: Option<AverageValue>,
    challenge_cp_cost: Option<u32>,
}
```

每首歌结果至少保存歌曲、难度、技能顺序分数分布、是否使用 fever，以及用于多人计算的固定输入摘要。
前端默认显示整数平均 PT、整数平均分数、固定场景摘要、队伍、队长、道具和歌曲；诊断信息再显示
分布明细。

## 9. 活动 PT 公式层

当前 `score_range/pt.rs` 中的正向活动 PT 公式不应继续属于 score-range。建议提取为共享模块：

```text
crates/core/src/
  event_pt/
    mod.rs
    model.rs
    solo.rs
    cooperative.rs
    versus.rs
    festival.rs
    medley.rs
    challenge_cp.rs
    cp.rs
  score_range/
    interval.rs
```

公式入口使用带类型的枚举，避免通过“把其他人得分设为 0”模拟单人：

```rust
enum EventPtInput {
    Solo(SoloPtInput),
    Cooperative(CooperativePtInput),
    Versus(VersusPtInput),
    Festival(FestivalPtInput),
    Medley(MedleyPtInput),
    ChallengeCp(ChallengeCpPtInput),
}
```

- `event_pt` 只负责一局确定结果的整数 PT/CP 计算。
- `score_range` 负责从 PT 反解分数区间。
- `pt_maximize` 负责枚举队伍、精确计分、技能顺序分布和平均值。
- 所有公式的取整层级必须由单元测试固定。

## 10. 核心项目结构

```text
crates/core/src/
  maximize/                    现有最大分数用例
  score_range/                 现有目标 PT 用例
  event_pt/                    共享活动 PT/CP 正向公式
  pt_maximize/
    mod.rs                     公开入口、支持矩阵、错误类型
    model.rs                   Request/Result/LiveVariant
    scenario.rs                统一/四人队友输入展开
    distribution.rs            技能排列分数分布与组曲卷积
    candidate.rs               队伍候选和 Pareto 前沿
    single.rs                  使用自己完整队伍技能的单曲搜索
    cooperative.rs             协力队长技能专用搜索
    medley.rs                  固定三曲互斥队伍搜索
    multiplayer.rs             竞演、5v5 固定场景与 PT 编排
    challenge_cp.rs            CP 挑战搜索
    bound.rs                   平均 PT 安全上界

crates/data/src/
  preparation.rs               共享活动、卡片、综合力、道具准备
  pt_maximize.rs               SnapshotPtMaximizeInputBuilder
  traits.rs                    PtMaximizeInputBuilder

crates/service/src/
  pt_maximize.rs               PtMaximizeService

apps/server/
  POST /v1/pt-maximize

crates/web-wasm/
  pt_maximize_from_static_data
```

## 11. 活动加成与候选队伍

最大分数队伍不一定是最大平均 PT 队伍。在活动加成直接乘入 PT 的演出形态中，高活动 PT 加成卡
可能降低个人分数但提高最终 PT 倍率。因此不能先调用现有 `maximize` 取唯一最高分队伍，再换算活动 PT。

准备层应把当前按用例硬编码的 `maximize_cards()` / `score_range_cards()` 改为规则驱动：

```rust
enum EventBonusApplication {
    TeamStat,
    PointMultiplier,
}
```

候选层至少按以下键保留 canonical solution/Pareto 前沿：

```text
(area_items, team_mode, point_bonus, score_distribution_bound)
```

对于没有活动 PT 倍率取舍的活动，可以把现有最大分数结果作为快速路径。对于有 PT 倍率的活动，
必须联合枚举综合力、技能、队长和 PT 加成。

## 12. 技能顺序分布与缓存

无技能排队时以 32 状态子集计数 DP 精确统计 5 种技能的等概率排列；存在技能排队时以严格时间线的
穷举结果作为正确性基准。缓存键必须包含会影响精确分数的全部信息：

```text
(chart, final_stat, fever_rule, five_skills, repeated_skill)
    -> score histogram
```

分布使用直方图而不是保存所有排列：

```rust
struct ScoreHistogram {
    entries: Vec<(i32, u32)>, // score, occurrence count
}
```

相同分数的排列立即合并。组曲通过直方图卷积得到总分分布。活动 PT 公式只需遍历压缩后的分数分布。

## 13. Fever

以下多人模式必须走含 fever 的精确分数计算：

- 5v5；
- 任务 Live 协力；
- Challenge 协力；
- EX 协力。

竞演多人、所有单人模式、Challenge CP 挑战和组曲不使用 fever。多人 fever 固定成功，不把成功率、
Fever Chance 充能情况或队友表现作为输入和随机因素。

联网资料和已有实现均指向固定 `2.0` 倍：fever 是按键分数的独立乘数，与当时生效的技能倍率乘算。

```text
existing_note_score_term × skill_multiplier × fever_multiplier

fever_multiplier = 2.0  // 按键位于 fever 区间
fever_multiplier = 1.0  // 其他按键
```

这里的 `existing_note_score_term` 表示当前精确计分内核已经算出的按键基础项；原有的中间取整和最终取整
位置保持不变，fever 只在按键最终取整前加入一个乘数，不能为了套用上式合并或移动既有取整。

因此技能生效且处于 fever 时使用 `skill_multiplier × 2.0`，不能把 fever 写成技能倍率的加法修正。
Rate-up 仍先按该按键位置求出实际技能倍率，再与 `2.0` 相乘。

### 13.1 谱面区间

Bestdori 原始谱面使用 `System` 节点标记 fever 区间：

```json
{"type":"System","data":"cmd_fever_start.wav","beat":256}
{"type":"System","data":"cmd_fever_end.wav","beat":348.25}
```

解析器使用同一组 BPM 变化点，把起止 beat 转换为绝对时间。计分区间暂按闭区间处理：

```text
fever_start_time <= note_time <= fever_end_time
```

`cmd_fever_ready.wav` 和按键上的 `charge: true` 用于 Fever Chance/充能，不定义最终计分区间。由于本用例
固定 fever 成功，初版只读取 start/end，忽略充能模拟。

当前 `crates/data/src/chart.rs` 会忽略全部 `System` 节点。实现时应把区间解析为核心谱面模型的一部分：

```rust
const FEVER_SCORE_MULTIPLIER: f64 = 2.0;

struct FeverWindow {
    start_time: f64,
    end_time: f64,
}

enum FeverRule {
    Disabled,
    ChartWindow,
}
```

`Chart` 保存可选 `FeverWindow`；是否启用由 `FeverRule` 决定，不能只根据活动类型推断。这样同一张谱面
可以同时用于无 fever 的单人计算和有 fever 的多人计算。标记点缺失、顺序错误或只有单侧标记时应返回
数据错误，不允许静默估算区间。

### 13.2 资料依据

- BanG Dream! Girls Band Party! 官方 FAQ 确认 Fever Time 内每个按键都会获得分数加成：
  <https://bang-dream-gbp-en.bushiroad.com/faq/?id=2164>
- 社区记录的按键公式把 `Skill Multiply` 和 `Fever Time Bonus` 列为两个相乘因子：
  <https://bandori-en.tumblr.com/post/173619402961/score-system>
- 社区玩法说明记录 Fever 区间的分数为 `2x`：
  <https://bangdreaming.tumblr.com/post/162046874039/multi-live-scores>
- 原始谱面来自 Bestdori chart API：
  `https://bestdori.com/api/charts/{songId}/{difficulty}.json`。

## 14. 前端交互

1. 选择活动后，只显示支持的演出形态；界面名称依次使用“自由演出、协力演出、竞演演出、巡回演出、
   团队演出、挑战演出”。
2. 歌曲和难度由用户指定；组曲必须选择恰好三首。
3. 结果页顶部先显示单行“计算场景”（演出形态以及适用的排名/胜负），随后以与“计算目标”相同的
   分段选项提供倍率选择：挑战演出使用 200/400/800/1600 CP 档位，其他演出使用 0/1/2/3 火
   档位；搜索仍只执行一次单倍计算。
4. 协力显示最低个人综合力和队友参数输入；任务 Live 的支援乐队 PT 加成只在单人模式显示。
5. “队友参数”与“队长选择”使用同级小标题，并排在队长选择之后；队友参数支持“统一设置”和
   “四人分别设置”切换，统一设置仍显示一行“队友”。
6. 协力队长选择支持“最大综合力”“指定”“随机”；指定玩家选择直接显示在队长选择栏下方。
7. 竞演显示“排名”。
8. 5v5 显示队友预计分数、队内排名和是否胜利，默认选择队伍获胜。
9. Challenge 显示普通单人、协力和 CP 挑战三个入口；普通单人/协力结果额外显示随火倍率联动的
   CP 获取，CP 挑战结果显示所选 CP 消耗。
10. 结果主体复用最高分数的歌曲、难度、队伍、队长和道具展示，道具区显示“道具选择”小标题；
    摘要显示整数平均 PT、整数平均分数和综合力，不显示 PT 范围或排列样本数。
11. 每种活动类型分别持久化最后选择的演出形态；切换活动类型不会覆盖其他类型的选择。
12. 分段选项按用途使用固定单项宽度并整体左对齐，不随容器宽度动态均分；计算目标、演出模式、
    排名/胜负、队长和倍率选择分别使用适合其文案长度的宽度。

## 15. 实现阶段

### 阶段一：公式和领域模型

- 提取共享 `event_pt`；
- 建立 `LiveVariant` 和支持矩阵；
- 固定所有正向 PT、CP 获取和 CP 挑战公式测试；
- 保证现有 score-range 结果不变。

### 阶段二：单人和组曲

- 实现单曲 5 技能排列分布；
- 实现固定三曲组曲直方图卷积；
- 实现 PT 加成与分数联合优化；
- 输出 Challenge 单人 CP 获取。

### 阶段三：多人

- 实现统一/四人队友输入；
- 实现协力 5 名队长技能和第六技能规则；
- 实现 fever；
- 实现竞演和 5v5 固定排名/胜负输入；
- 输出 Challenge 协力 CP 获取。

### 阶段四：性能与诊断

- 已完成单曲技能分布缓存、整套 mode/递归分支/叶子三级严格 PT 上界；
- 已完成单曲完整 fixture 性能基线和细分 metrics；
- 已完成多曲分数直方图去重、商/余数分解和安全余数卷积；
- 已完成组曲候选生成、种子搜索、严格互斥查询和精确分布缓存的结构化 metrics；后续按性能诊断
  需要继续细分精确计分、分布卷积和 PT 计算；
- 完成所有演出形态的真实完整 fixture 回归基线。

## 16. 仍待确认

最优结果并列规则已确定：单曲和组曲都先比较精确平均 PT；相同时比较精确平均分数；仍相同时按
卡片/队长 ID 选择规范结果。最低 PT 和综合力不参与并列排序。
