# 统一计分基础：第一阶段交付

> 本文记录迁移前的独立模块阶段；正式入口迁移及当前验收见[原生算术迁移验收](scoring-migration.md)。

日期：2026-09-20。对应[推进计划](scoring-and-hit-timing-plan.md)第一阶段。

## 当前状态

`crates/core/src/scoring.rs` 与 `scoring/power.rs` 已提供可独立调用的正式 Rust 算术模块。第二阶段已补充[真实输入、谱面评分及压缩路径](scoring-integration.md)；原应用的 `Chart::get_score`、编队、缓存、剪枝和结果仍调用旧实现。**这是统一计分基础完成，不是线上评分修复已经生效。**

这样分开接入是为了避免最终重算使用新公式，而候选淘汰仍使用旧上界。后者可能让被淘汰的队伍无法在结果阶段恢复。

没有修改技能调度策略、数据存储、历史记录或前端，也没有提交、推送或部署。

## 已实现的边界

| 接口 | 输入与行为 |
| --- | --- |
| `AreaBonus::from_effects` | 接收按装备遍历顺序排列的原始固定加成及百分点，逐维以 f32 累加，百分点最后除以 100 |
| `MemberPower::from_bonuses` | 接收养成后的整数三维、道具固定值/倍率、活动百分点和已解析的额外三维加成，保留每步 f32 舍入 |
| `DeckPower::from_members` | 在给定成员顺序下，分别得到计分综合力和未取整的显示综合力；两者不强制共用一个数值 |
| `ScoringPower` | 有限、非负 f32；完整位模式参与相等、排序和缓存键；不再按整数综合力合并 |
| `NoteScorer` | 依次执行等级修正、按键数归一化、判定、Combo、第一次取整、Fever 与技能倍率合成、第二次取整 |
| `Crescendo` | 每次技能激活创建新状态；当前 Perfect 先累计百分点，Great 保留状态，按原生整数上限夹紧 |
| `DeckPower::medley_display` | 三队未取整的显示值先以 f64 相加，转回 f32，再取整一次 |

`ARITHMETIC_REVISION = cn-9.4.4-f32-v1` 仅标记算术规则，不代表 60/120 FPS 调度口径。它尚未写入应用结果；结果版本和历史兼容在第二阶段接入。

输入验证拒绝 NaN、无穷、负加成和越界分数，避免 Rust 的饱和转换把异常输入变成正常结果。计算出的单键分数使用原生无符号 32 位范围。

## 必须保留的运算顺序

1. 道具：`f32(逐项累加百分点 / 100)`，不能把各项提前除以 100 后合并为 f64。原生证据是 `DeckParamCalcUtility @0x2291704..0x2291734`。
2. 成员道具后参数：`f32(f32(原始值 + 固定值) × f32(1 + 道具比例))`。
3. 通用活动增量：先计算 `f32(f32(活动百分点 + 100) / 100)`，再执行 `f32(f32(原始值 × 倍率) - 原始值)`；不能代换成 `原始值 × 活动比例`。对应 `DeckParamDetail @0x21fb520..0x21fb58c`。
4. 成员最终参数依次加入通用和额外活动增量；显示用活动总量也依次加入，不能预先合并两者。
5. 计分综合力：按成员顺序分别累加 P/T/V，再计算 `T + (P + V)`。显示路径分别汇总原始、道具、活动的 `(P + T) + V`，最后按原始、道具、活动相加。两条路径的舍入不同。
6. 逐键公式见[原引擎审计](native-score-audit.md)。所有乘加分步执行，不使用 `mul_add` 或 fast-math 重新结合表达式。

这里接收的是已经解析好的养成和加成输入；尚未重新验证游戏主数据选取、养成整数生成、活动适用规则及所有模式的 PT 公式。

## 验证方法与结果

原生对照固定使用国服 9.4.4 的 ARM32 ELF，SHA-256：

`93538f6a8df021318a398871f3eab2431430b616ab7a548c157efcc45ba1141a`

`scripts/research/verify_native_scoring_core.py` 调用 Rust release 探针和原始 ARM32 指令片段，以完整浮点位模式/整数比较，而不是用容差近似比较。资源查找和简单属性存储由合成输入替代，算术执行原始指令；不包含 IFix、完整 Unity 客户端和设备实测。

- 6,853 组对照通过：Combo 1,001、普通技能倍率 11、逐键评分 5,228、成员参数汇总 203、道具累加 200、完整队伍参数 200、巡回显示 3、递增技能序列 7。
- 递增序列包含 1,380 个按键状态，覆盖 Perfect/Great/Good、封顶和基准等于上限的浮点边界。
- 5,228 次逐键比较中，2,000 次来自固定技能时间线：60/120 FPS 各 1,000 键，含小数综合力、Great、判定修正、递增、技能延后及 Fever 跨界。Rust、独立参考和原生结算逐键一致。
- 固定时间线使用研究参考产生的明确判定/技能状态，只验证相同状态的评分一致性，不据此宣称正式调度已迁移或两个帧率应得到相同总分。
- 四个已复现误差的新结果分别为 1,288,520、1,289,040、2,537,240、3,737,735，与原生一致。
- 核心全量测试：221 项通过、1 项原有跳过；随后补充道具累加边界用例的定向测试通过。研究参考 52 项测试通过。WebAssembly 编译检查通过；尚未做浏览器内逐值验证。

复现：

```powershell
cargo test --offline -p bangdream-optimize-core scoring:: --lib
cargo build --offline --release --manifest-path scripts/research/native-score-probe/Cargo.toml --target-dir target/native-score-audit --bin verify
python -B scripts/research/verify_native_scoring_core.py
python -B -m unittest discover -s scripts/research -p 'test_hit_timing_*.py'
cargo check --offline -p bangdream-optimize-core --target wasm32-unknown-unknown
```

对照工具需要相邻 `apk-reverse` 的固定 ELF、分析工具与 Unicorn。完整报告写入 `tmp/native-score-audit/core-verification.json`，目录中的报告是可再生输出。

## 第二阶段接入约束

现有数据已经丢弃了一部分新公式需要的中间值，不能只替换评分函数：

- `PreparedCard` 需保留逐维参数、通用活动百分点和额外活动增量；当前提前合并的 `event_add_stat` 不能直接当作精确输入。
- `AreaItemPercent` 需在原始道具层按顺序计算；将旧聚合 f64 强转 f32 无法恢复原来的舍入位置。
- 队伍成员顺序可能改变 f32 求和末位。推荐顺序、最高分命中概率和缓存必须采用同一个明确的队伍顺序口径，不能无条件认为所有排列共享同一综合力。
- `Chart::score_factors`、压缩评分、AVX2、队列状态评分需要保留原生各步舍入，不能把新综合力塞进旧的连续 f64 倍率。
- `floor_team_stat`、整数缓存键、控分整数区间及贡献/兼容队伍剪枝需一起迁移；要与小规模穷举核对保守上界。
- Auto 和巡回的特殊 Combo、判定倍率等模式输入继续单独核验；公共评分入口允许传入模式倍率，但不代替模式规则验证。
- 历史结果保留原计算值和口径；新计算才使用新标识。完成以上接入后，再复测两个真实档案的结果、候选数与分阶段用时。
