# Medley mode 与支配剪枝现状（临时）

> 状态：临时分析文档，记录 2026-07-15 的当前实现。本文描述现状，不代表后续联合 DP 的最终设计。
>
> 本文的 `mode` 指 Medley 队伍的 band/attribute 统一形态，即代码中的 `MedleyPruneSignature`；不是区域道具的 mode。

## 1. 当前如何枚举 mode

### 1.1 mode 定义

`crates/core/src/medley/prune/signature.rs` 中定义了四类签名：

```rust
enum MedleyPruneSignature {
    Mixed,
    UnifiedBand(u32),
    UnifiedAttribute(Attribute),
    UnifiedBandAttribute(u32, Attribute),
}
```

含义如下：

| mode | 最终五卡条件 | 预选卡池条件 |
| --- | --- | --- |
| `Mixed` | band 不全相同，并且 attribute 不全相同 | 接受所有卡 |
| `UnifiedBand(b)` | band 全为 `b`，attribute 不全相同 | 只接受 band=`b` |
| `UnifiedAttribute(a)` | attribute 全为 `a`，band 不全相同 | 只接受 attribute=`a` |
| `UnifiedBandAttribute(b,a)` | band 全为 `b` 且 attribute 全为 `a` | 只接受 band=`b`、attribute=`a` |

这里要区分“预选允许条件”和“最终精确条件”。例如 `Mixed::allows` 无条件接受所有卡，但五卡最终还必须同时打破 band 统一和 attribute 统一。

### 1.2 seed mode 的生成

`seed_signatures(cards)` 先加入一个 `Mixed`，然后扫描当前卡池，对每张卡去重加入：

- `UnifiedBand(card.band_id)`；
- `UnifiedAttribute(card.attribute)`；
- `UnifiedBandAttribute(card.band_id, card.attribute)`。

因此双统一 mode 不是对 band 和 attribute 做理论笛卡尔积，而是只生成卡池中实际出现过的 `(band, attribute)` 组合。

当前完整 fixture 中观察到 8 个 band、4 个普通 attribute，且 32 个组合均出现，因此每套道具配置生成：

```text
1 Mixed + 8 UnifiedBand + 4 UnifiedAttribute + 32 UnifiedBandAttribute = 45 modes
```

这个 45 是当前数据的结果，不是写死常量；如果输入卡池中实际出现的 band/attribute 组合变化，mode 数也会变化。

### 1.3 每个 mode 的候选池生成流程

入口为 `crates/core/src/medley/prune/pool.rs::signature_candidate_pools`。每个 mode 当前依次执行：

1. 用 `signature.allows(card)` 建立允许卡池。
2. 做同形状贡献预剪枝。同形状键为 `(character_id, duration, rateup)`，在桶内比较贡献支配。
3. 用 `signature_can_complete_with_card` 检查：强制包含当前卡后，在 `allows` 卡池里是否还能凑出五个不同角色。
4. 若已有 incumbent，使用“强制包含该卡”的上界判断此 mode 是否仍可能超过当前最优值。
5. 在当前 active 卡池上构造 hard/contribution 支配图，先做全局同角色支配剪枝。
6. 用同角色剪枝后的卡池重新构图，再做全局跨角色支配剪枝。
7. 如果卡池缩小，则重新计算综合力/Meta 上下界和支配关系，循环到固定点。
8. 按角色分组，计算五个不同角色的候选容量；各 mode 按估计候选数升序进入精确枚举。

`signature_can_complete_with_card` 只检查 `allows` 范围内能否找到五个不同角色。它不检查 `Mixed` 是否真的同时包含 band breaker 和 attribute breaker，也不检查单统一 mode 是否真的包含另一个维度的 breaker。因此这个完成性检查是必要条件，不是 mode 的精确可行性判断。

候选容量当前是：从不同角色组中选五组，再乘所选组的卡数并求和。它也没有计入最终精确 mode 条件：

```text
capacity = sum(product(size(character_group)))
           over all choices of 5 character groups
```

### 1.4 mode 在精确枚举中的确认

精确五卡队伍形成后，`selected_resolved_team_signature` 按 `(same_band, same_attribute)` 唯一分类：

| `same_band` | `same_attribute` | 最终 mode |
| --- | --- | --- |
| false | false | `Mixed` |
| true | false | `UnifiedBand` |
| false | true | `UnifiedAttribute` |
| true | true | `UnifiedBandAttribute` |

递归枚举过程中，`PrefixSignatureState` 维护第一张卡的 band/attribute、当前是否仍全同 band/attribute 以及已选卡数。最后一层的标量或 AVX2 路径用它验证精确 mode。

各 mode 还决定技能的 ScoreUp 解析参数：

| mode | `team_band_id` | `team_attribute` |
| --- | --- | --- |
| `Mixed` | `None` | `None` |
| `UnifiedBand(b)` | `Some(b)` | `None` |
| `UnifiedAttribute(a)` | `None` | `Some(a)` |
| `UnifiedBandAttribute(b,a)` | `Some(b)` | `Some(a)` |

因此 mode 不只是枚举分区，也会改变统一条件技能的精确 ScoreUp。

## 2. 当前如何做跨角色支配剪枝

### 2.1 支配图的含义

每个 mode、每轮 active 卡池会构造两张有向图：

- hard dominance graph：只接受逐项不差的静态支配；
- contribution dominance graph：以 hard graph 为基础，再加入“综合力与技能贡献可互相补偿”的安全边。

边 `A -> B` 表示：在该 mode 和当前安全上下文内，A 可以替换 B。图构造后会做传递闭包，所以覆盖计数也包含通过支配链推导出的替换卡。

hard 支配要求：

- `stat(A) >= stat(B)`；
- 三张谱面、六个技能位置的签名解析后 Meta 均满足 `meta(A) >= meta(B)`；
- 至少一项严格更好；完全相等时用更小 `card_id` 作为规范解。

### 2.2 同角色阶段

如果支配 B 的卡与 B 属于同一角色，那么替换不会改变队伍的角色集合。

- 单曲只有一支队伍，因此存在 1 张同角色支配卡即可删除 B。
- 组曲有三支队伍，同一角色可以在三支队伍中各出现一次。最坏情况下另外两张支配卡已被其他两队占用，因此需要至少 3 张同角色支配卡才能删除 B。

实现中门槛直接使用 `team_count`：单曲为 1，组曲为 3。

### 2.3 跨角色阶段的最坏情况覆盖

跨角色替换可能和 B 所在队伍的四名队友发生角色冲突，也可能被其他两支队伍占用，所以不能因为“存在一个更强的不同角色卡”就删除 B。

设：

- `n_g`：角色组 `g` 中能够支配 B 的卡数；
- `D = Σ_g n_g`：所有支配卡数；
- `T`：B 所在队伍的四个队友角色集合，`|T| <= 4`；
- `R = team_count - 1`：其他队伍数，Medley 中 `R=2`；
- 其他队伍总卡位为 `5R=10`。

当前队友角色会直接挡住：

```text
B(T) = Σ(g in T) n_g
```

排除这些角色后，其他两队对每个角色至多各使用一张，因此最多吸收：

```text
C(T) = min(10, Σ(g not in T) min(n_g, 2))
```

必然还空闲、可用于替换 B 的支配卡为：

```text
Free(T) = D - B(T) - C(T)
```

剪枝必须对最坏队友选择仍成立：

```text
cross_cover = min Free(T), for all T with |T| <= 4
prune B iff cross_cover > 0
```

实际实现没有暴力枚举所有四角色集合，而是在 `crates/team-prune/src/lib.rs::dominator_cover_summary_after_worst_teammate_groups` 中做一个小 DP。DP 状态跟踪：

- 已选择多少个队友角色；
- 选择这些角色会从其他队伍移除多少角色容量；
- 在同一状态下最多能直接阻塞多少支配卡。

然后从每个状态计算 `free_replacements`，取其中最坏值。这样同时处理了当前队伍的角色冲突与另外两队的占用约束。

### 2.4 2026-07-15：精细跨角色覆盖 DP

当前生产路径保留上述数量公式作为快速第一层。若粗略 `cross_cover > 0`，直接剪枝；只有粗略结果为 0 的 near-miss 才进入精细 DP。

精细 DP 不再分别估计 `B(T)` 和 `C(T)`，而是按角色逐行联合分配：

```text
dp[target_teammates][other_song_j_slots][other_song_k_slots][break_mask]
    = 最多能变为 unavailable 的支配卡数量
```

每个角色行可以执行以下转移：

1. 选择该角色的一张真实卡作为目标队友。目标队友数加一，该角色的所有支配卡均被角色冲突阻塞，同时更新 band/attribute breaker。
2. 不作为目标队友，将该角色的一张支配卡放入另一曲 `j`。
3. 不作为目标队友，将一张支配卡放入另一曲 `k`。
4. 使用两张不同实体卡分别放入 `j、k`。
5. 不使用该角色。

每支其他队伍仍最多五个槽位、每个角色最多一张。支配卡只有在“强制包含该卡的谱面安全上界 + 另外两曲上界”仍可能严格超过 incumbent 时，才允许占用对应谱面的槽位。

目标队伍最终必须恰好选择四个队友，并满足精确 mode：

- `Mixed`：band breaker 和 attribute breaker 均出现；
- `UnifiedBand`：attribute breaker 出现；
- `UnifiedAttribute`：band breaker 出现；
- `UnifiedBandAttribute`：允许池本身已保证双统一。

最终计算：

```text
FreeMin = D - max_unavailable
```

Medley 中目标卡可能位于任意一曲，因此分别以三首歌作为目标曲运行，取最小 `FreeMin`。只有三种目标曲的最坏结果仍大于 0 才剪枝。

当关闭 mode、谱面 eligibility 等新增信息时，精细 DP 退化为原来的 `D-B(T)-C(T)` 松弛模型；新增限制只会降低“可能被占用”的安全上界。

### 2.5 固定点顺序

每轮严格按照以下顺序执行：

```text
active cards
  -> 构造 hard/contribution graph
  -> 同角色 cover
  -> same survivors
  -> 在 survivors 上重新构造 graph
  -> 跨角色 cover
  -> next active cards
```

只要本轮删掉了卡，就以更小的 active 卡池重新计算全部上下文和支配关系，直到人数不再变化。这样后续轮次能利用更紧的综合力/Meta 范围。

### 2.6 技能重叠谱面的退化路径

如果任一谱面存在 overlap warning，技能排队会让静态“每个激活位置的 Meta 可加”假设失效。当前实现会停用基于该假设的：

- hard dominance；
- contribution dominance；
- incumbent 上界剪枝。

这时只保留 `allows + 五个不同角色可完成性`，避免不安全剪枝。

## 3. 当前如何估计贡献支配的综合力和 Meta 上下界

### 3.1 单卡基础 profile

每张卡先生成 `MedleyCardPruneProfile`：

- `stat`：应用当前区域道具/角色加成等规则后的单卡未取整综合力；
- ScoreUp 变体：默认值，以及统一条件触发后确实不同的变体；
- 对每个 ScoreUp 变体、每张谱面、六个技能位置，计算精确 `skill_meta_value`。

六个位置为五个普通激活位置加一个队长追加激活位置。进入具体 mode 后，再用该 mode 的 band/attribute 参数解析唯一 ScoreUp 变体。

### 3.2 上下文只使用当前 active 卡池

固定点的每一轮都会把当前 active indices 复制成局部 cards/profiles，再为该局部卡池构造贡献上下文。因此已经删除的卡不会继续把下一轮的区间撑宽。

但 mode 约束仍只使用 `signature.allows`：

- `UnifiedBand/Attribute/BandAttribute` 的允许卡池较窄；
- `Mixed` 的允许卡池仍是全部 active 卡；
- 没有在 bounds 构造阶段加入 breaker 的精确可行性。

### 3.3 队友综合力范围

先按角色聚合单卡 stat。对每个角色 `g`：

```text
stat_range(g) = [该角色允许卡的最小 stat, 该角色允许卡的最大 stat]
```

被比较卡之外还有四名不同角色队友，故初始范围为：

```text
stat_low  = 角色 stat 下界中最小的 4 个之和
stat_high = 角色 stat 上界中最大的 4 个之和
max_card_stat = 所有角色 stat 上界的最大值
```

这里 `y` 表示四名队友的综合力之和，而被比较卡自身的 stat 记为 `s`。

### 3.4 incumbent 反推的综合力下界

若组曲当前最优总分为 `current_best`，对第 `i` 张谱面先用另两张谱面的全局安全上界反推该谱面至少需要的分数：

```text
S_i = max(0, current_best - U_j - U_k)
```

再计算该 mode 在该谱面上五张卡的 Meta 安全上界：

```text
M_i_upper = no_skill_meta + normal_plus_captain_meta_upper(5 cards)
```

由于精确逐键计分有取整，代码使用安全反推：

```text
team_stat_floor_i = max(S_i - 1, 0) / M_i_upper
```

最终队友综合力下界收紧为：

```text
y_low  = max(stat_low, team_stat_floor_i - max_card_stat)
y_high = stat_high
```

减去 `max_card_stat` 的原因是：这里只需要得到“另外四名队友”的必要下界，而被替换位置上的卡最多可以贡献 `max_card_stat`。这是安全但偏宽的处理。

### 3.5 队友 Meta 范围

对每张卡、每张谱面，先取其五个普通技能位置 Meta 的最小/最大值；再按角色聚合为该角色的 Meta 范围。

普通四队友 Meta：

```text
normal_low  = 角色普通 Meta 下界中最小的 4 个之和
normal_high = 角色普通 Meta 上界中最大的 4 个之和
```

“普通 + 队长追加”范围不是把两个独立极值直接相加，而是枚举队长候选：

1. 固定某张卡为队长；
2. 加上该卡的普通 Meta 范围和第六次队长 Meta；
3. 从其他角色中取三张普通 Meta 的最低/最高和；
4. 对所有队长候选再取总最小/最大。

这样至少保持了“队长普通激活与第六次激活必须来自同一张卡”的约束。

### 3.6 10 个物理技能角色

贡献支配不会只比较一个技能位置，而是每张谱面比较 10 个实际可达角色：

- 5 个普通角色：卡位于普通位置 `p=0..4`，队长是另外四张卡之一；
- 5 个队长耦合角色：同一张卡同时承担普通位置 `p` 和队长追加位置 `5`，其中 `p=0..4`。

对于普通角色，被比较卡只占一个普通技能位置，因此队友 `x` 使用：

```text
x = no_skill_meta + teammates(normal + captain)
```

对于耦合位置，被比较卡自身已经承担队长追加，队友 `x` 只使用：

```text
x = no_skill_meta + teammates(normal)
```

旧实现还单独检查了一个 `captain-only` 场景。该角色不可达：队长必然也是五张普通激活卡之一，不可能只承担第六次激活而没有普通位置。这个额外场景只会造成支配假阴性，现已删除。

### 3.7 仿射贡献比较

设被比较卡自身综合力为 `s`、自身技能 Meta 为 `m`、队友综合力为 `y`、其余基础/队友技能 Meta 为 `x`，连续模型的队伍贡献为：

```text
(s + y)(x + m)
= sx + my + sm + xy
```

比较两张卡 A、B 时，`xy` 完全相同，可以消掉，只比较：

```text
F(s,m;x,y) = sx + my + sm
```

两张卡的差为：

```text
Delta(x,y)
  = (s_A - s_B)x
  + (m_A - m_B)y
  + (s_A m_A - s_B m_B)
```

这是关于 `x,y` 的仿射函数，因此矩形区间上的最小值可直接在端点取得：

```text
x = x_low  if s_A - s_B >= 0 else x_high
y = y_low  if m_A - m_B >= 0 else y_high
```

只有 A 在三张谱面、每张谱面的 10 个物理角色中最小 margin 都不小于 0，才建立 `A -> B`。至少一个场景严格更好；完全相等时仍用更小 `card_id` 规范化。

## 4. 当前区间为什么仍然偏宽

当前贡献支配安全但保守，主要损失来自：

1. `x` 与 `y` 被放进独立矩形。实际高综合力队友和高 Meta 队友通常来自具体卡片选择，两者有相关性。
2. `stat_low/high`、`normal_low/high` 可能分别由不同的四个角色、不同的卡取得，并不保证能组成同一支队伍。
3. `Mixed` bounds 只知道“所有卡都允许”，不知道最终必须同时存在 band breaker 与 attribute breaker。
4. 单个角色的 stat 极值和 Meta 极值也可能来自该角色的不同卡；全局上下界进一步丢失了卡片级对应关系。
5. 贡献关系使用连续仿射模型，不直接表达逐键 `floor`。最终候选计分仍走精确公式，但预剪枝能证明的支配关系受连续区间限制。

因此，最大 mode 的共享卡池仍可能保留很多“单看全局矩形端点不能证明支配、实际可行队伍中却不会更优”的卡。后续不必构造 stat/Meta 联合前沿；优先把队友 stat 下界和 Meta 上界分别改为目标卡、谱面、普通/队长角色相关，并在各自的独立 DP 中加入精确 mode 可行性。

## 5. 当前热点观测

最近一次完整 fixture 诊断中，最大 mode 候选池为 36 张卡，角色组大小为：

```text
[5, 5, 4, 4, 4] + 14 个单卡角色组 = 19 个角色组
```

按当前容量公式估计为 198,440 支五卡队伍。精确 mode 校验后，真正属于该 `Mixed` mode 的队伍为 116,823，占 58.87%；另有 81,617 支队伍实际属于三类统一 mode。

这说明 `Mixed::allows = all` 与最终 `Mixed` 条件之间的差距确实是热点的一部分。后续若继续收紧，应分别模拟：

- 只加入精确 mode 可完成性后的卡池/候选数；
- 分别加入目标卡/角色相关 stat 下界与 Meta 上界后的支配边和卡池变化。

### 5.1 精细 DP 后的真实 fixture 结果

2026-07-15 使用完整 1,414 卡 fixture、Release、单线程、AVX2 对照。两组均已使用 10 个物理技能角色，唯一区别是是否启用精细跨角色覆盖：

| 指标 | 粗略 `D-B-C` | 精细 DP | 变化 |
| --- | ---: | ---: | ---: |
| 内部总耗时 | `120.573s` | `90.401s` | `-25.0%` |
| 全部原始候选 | 4,374,209 | 3,818,796 | `-12.7%` |
| solver 候选 | 177,366 | 156,093 | `-12.0%` |
| candidate-build | `30.812s` | `26.461s` | `-14.1%` |
| solver | `73.787s` | `50.411s` | `-31.7%` |

总分 `11,880,244`、总综合力 `1,487,827`、获胜道具、三支队伍、技能顺序和队长均保持一致。

详细 trace 的 945 个 `(道具, mode)` 观测中，最大 active 卡数从旧记录的 36 降到 34；对应最大 `Mixed` 估计候选为 155,254。精细 trace 中全部 21 次候选构造的 contribution cover 合计约 `1.305s`，明显小于后续候选和 solver 节省的时间。

### 5.2 贡献下界共享包络（2026-07-15）

贡献支配继续让同一 `(signature, chart, physical_role)` 的所有卡使用同一个 `y_low`，因此支配边仍定义在同一矩形域上，普通传递闭包和 hard base graph 无需改变。新的 Mixed 下界按以下方式计算：

1. 固定候选卡 `c`，排除它的角色；四名队友必须角色互异，并满足 Mixed 的 band breaker 与 attribute breaker。
2. 分别 DP 得到队友综合力最小值 `L_struct(c)`、普通技能 Meta 最大值 `M_normal(c,q)`，以及“普通技能 + 四名队友中一名队长”的 Meta 最大值 `M_normal_captain(c,q)`。
3. 普通位置 `p` 使用 `no_skill + meta_c[p] + M_normal_captain`；卡片兼任队长的位置使用 `no_skill + meta_c[p] + meta_c[5] + M_normal`。
4. 对谱面分数种子下界 `S_i`，使用严格取整安全式：

```text
L_score(c,q) = (S_i - 1) / M_upper(c,q) - stat(c)
L(c,q) = max(L_struct(c), L_score(c,q))
y_low(q) = min over feasible c of L(c,q)
```

最后一步对所有可行 `c` 取最小值正是共享包络；没有使用单卡域或成对域，所以不会破坏传递性。`x` 与 `y_high` 仍保持原共享安全界，stat/Meta 也仍未联合成前沿。

实现上先按目标 `(band, attribute)` 扫描一次卡池，再把每个角色压缩为 4 种 breaker mask。每种 mask 分别保存最小 stat、每谱面最大 normal Meta、最大 `normal + captain` Meta。角色序列建立前缀/后缀 5 选 DP；排除任一角色时只合并其左、右两段，不再为 40 个角色分别重跑整段 DP。结果再按 `(character, band, attribute)` 复用，避免每张固定卡重复工作。

零 score-floor 的同形状初筛没有分数下界收益，继续使用原公共四队友结构下界。完整 fixture 中精细包络只对最大的 `Mixed` 池启用；统一 mode 使用原粗界，因为全 mode 启用时增加了构造成本，却没有减少最终 solver 候选。

完整 1,414 卡 fixture 的最终定向版本保持总分、综合力、道具、三支队伍、技能顺序和队长完全一致。原始候选从精细跨角色覆盖基线的 `3,818,796` 降到 `3,794,455`（减少 `24,341`，约 `0.64%`），solver 候选仍为 `156,093`。前后缀版本最后一轮 candidate-build 为 `34.323s`；此前未做前后缀复用的同公式版本为 `41.304s`。多次运行的 solver 墙钟波动很大，且历史旧基线 `26.461s` 并非同轮 A/B，因此目前只能确认包络更精确和局部构造明显加速，尚不能声称端到端净加速。后续应增加同进程开关式 A/B，或先用便宜粗界筛出真正需要精细包络的热点 mode。

## 6. 关键源码位置

- mode 定义和 seed：`crates/core/src/medley/prune/signature.rs`
- mode 候选池与固定点：`crates/core/src/medley/prune/pool.rs`
- hard 支配与单卡 Meta profile：`crates/core/src/medley/prune/hard.rs`
- 粗略跨角色最坏占用 DP：`crates/team-prune/src/lib.rs`
- 精细 mode/谱面占用 DP：`crates/core/src/medley/prune/hard.rs`
- 贡献上下文与仿射支配：`crates/core/src/medley/prune/contribution.rs`
- 最终 mode 分类与 ScoreUp 解析：`crates/core/src/medley/scoring.rs`
- 精确枚举中的前缀 mode 状态：`crates/core/src/medley/enumeration.rs`
