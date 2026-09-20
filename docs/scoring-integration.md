# 计分修正：真实输入与谱面评分接入

> 本文记录迁移前的独立模块阶段；正式入口迁移及当前验收见[原生算术迁移验收](scoring-migration.md)。

日期：2026-09-20。对应[推进计划](scoring-and-hit-timing-plan.md)第二阶段的首个里程碑。

后续进展：[浮点缓存、控分边界和已确定时间线的保守上下界](scoring-search-bounds.md)已实现并通过对照；完整贡献模型与应用搜索仍待迁移。

## 当前状态

已打通“真实档案／主数据 → 成员逐维加成 → 浮点计分综合力 → 现有技能时间线 → 新逐键算术 → 状态压缩和 AVX2”的独立调用路径。固定队伍可以用它回放整曲。

**应用的指定队伍、自动编队、控分及结果页仍使用旧评分入口，尚未切换。** 新路径通过独立入口调用，避免先更换最终评分、却继续用旧上界淘汰候选。此次没有更改历史结果，也没有提交、推送或部署。

界面显示综合力仍然是整数。内部区分计分浮点值、显示路径取整前的浮点值、最终显示整数；前两者不能强行共用。巡回总显示值要先合计取整前的显示值，再取整一次。

## 接入内容

| 位置 | 变化 |
| --- | --- |
| `crates/data/src/bestdori/area_item.rs` | 保留主数据原始百分点，旧倍率字段保持原行为；服务器回退不变 |
| `crates/core/src/scoring/preparation.rs` | 从养成后的整数三维重新解析活动加成，按明确装备顺序汇总适用道具；不使用已经合并的旧 `event_add_stat` |
| `crates/core/src/model/chart/native.rs` | 新增 `native_score_notes` 和 `compile_native_score`，保留现有技能调度、同刻节点顺序和模式输入，只更换计分算术 |
| `crates/core/src/scoring/compiled.rs` | 按判定、Combo、技能和 Fever 的完整 f32 位模式压缩相同状态，提供可移植评分与 AVX2 评分 |

压缩发生在技能状态确定之后；递增技能仍逐键推进状态。不能因为数学上乘积相同，就合并跨越取整位置或单精度舍入的表达式。AVX2 保留两段取整，并检查原生 uint32 范围，不能误用有符号转换。

旧的已解析道具快照可以继续反序列化。缺少原始百分点时，新入口明确要求重新读取主数据，不从旧聚合倍率猜回原值；旧评分入口不受影响。

## 补充源码证据

原生版本和 ELF 校验值与[第一阶段](scoring-core.md)一致。国服 9.4.4 的方法指针表基址为 `0x5900118`，不能用反汇编工具默认的其他版本地址解释方法名。

- `EventBuffData.getCharacterAndAttributeBuffPercent @0x24d3924..0x24d3954`：先角色、属性，再按两者均大于零判断叠加匹配加成。
- `EventBuffData.GetTotalParameterBuffPercent @0x24d381c..0x24d382c`：上述结果再依次加卡牌加成和突破加成。
- `DeckParamCalcUtility.CalculateEventParameterBuff @0x2290f94..0x2291014`：额外三维逐项乘百分比并除以 100，然后按 `(P+T)+V` 合计。原生随后按活动指定参数类型加到相应轴；本次真实主数据仅一个非零轴，与新适配器一致。
- `AreaItemUtility.GetPutAreaItemModels @0x225a554`：源于放置道具映射的遍历，本段没有给出统一的数值 ID 排序。档案不含原设备放置顺序，因此新适配器要求调用者明确传入顺序；研究回放明确使用数值 ID 顺序。
- 本次六队所选道具百分点均为小范围的 0.5 倍数，逐项和能被 f32 精确表示，已检查它们的加法顺序不影响结果；不能推广到任意未来主数据。
- `MedleyInGameController.GetComboCorrectionRate @0x1c1ec60` 按主数据区间读取 `MedleyComboRateModel.ScoreRate`，不能仅从这段代码证明现有巡回 Combo 硬编码表完整正确。新入口本次沿用旧表，尚未把完整巡回主数据对照列为通过项。

## 真实档案验证

使用用户此前提供的 1,456 卡档案，保留已保存结果中的六支队伍、队长、道具和队伍摆放顺序。

1. 活动 323 的原对照配置：186 SP、395 SP、772 EX。
2. 三曲技能延后配置：8 SP、66 SP、59 SP。

每队分别验证手动、Auto 50%、Auto 75%，各枚举全部 120 种前五次技能触发顺序，第六次保持队长技能：共 **2,160 次完整谱面回放**。新压缩评分、AVX2 和不压缩逐键评分逐值一致。此处枚举的是技能触发顺序，不是重新摆放队伍；也没有执行选队搜索。

从每曲每模式抽取一个顺序，共 **12,960 个按键**，用原始 ARM32 指令片段再次验证逐键结算；同时核对六队的适用道具加成、活动加成、计分综合力和显示综合力位模式。全部通过。原生指令验证算术，不包含完整客户端、IFix 或设备时间线执行。

下表为手动模式、同一固定队伍下 120 个顺序的“新分数减旧分数”。算术列仍给新公式传旧整数综合力，用于分开观察综合力输入与逐键算术的影响；这些分差不是击打优化收益。

| 曲目 | 旧计分整数 | 新计分浮点值 | 完整分差范围 | 仅替换逐键算术的分差 |
| --- | ---: | ---: | ---: | ---: |
| 186 SP | 458534 | 458534.875 | 0 ～ +1 | 0 ～ +1 |
| 395 SP | 566577 | 566577.5 | −56 ～ +105 | −56 ～ +105 |
| 772 EX | 487786 | 487786.90625 | +130 ～ +180 | 0 |
| 8 SP | 466061 | 466061.40625 | 0 ～ +1 | 0 ～ +1 |
| 66 SP | 480260 | 480260.375 | −109 ～ +23 | −109 ～ +23 |
| 59 SP | 566577 | 566577.5 | 0 | 0 |

8 SP 这支队伍的计分浮点值是 `466061.40625`，显示路径的内部值是 `466061.4375`，最终界面显示 **466061**。这说明内部需要分开保存，并非建议显示小数。

样本首个顺序压缩后为 4～70 种状态，原谱为 541～805 键。这只说明压缩有效，不能代表完整搜索提速比例；暂未完成同条件搜索性能复测。

## 测试与复现

- 核心全量测试：230 项通过、1 项原有跳过；之后补充无符号分数边界测试并进行定向复测。
- 数据模块：28 项通过、8 项原有跳过。
- 第一阶段原生对照再次通过：6,853 组，含 1,380 个递增状态和 2,000 个固定时间线按键。
- WebAssembly 前端依赖编译通过；这是编译验证，尚非浏览器内逐值验证。
- 本次真实回放不包含贡献剪枝、概率加权、PT 换算和重新选队。

```powershell
cargo test --offline -p bangdream-optimize-core --lib
cargo test --offline -p bangdream-optimize-data --lib
cargo check --offline -p bangdream-optimize-web-wasm --target wasm32-unknown-unknown
cargo build --offline --release --manifest-path scripts/research/native-score-probe/Cargo.toml --target-dir target/native-score-audit --bin integration
target/native-score-audit/release/integration.exe tmp/profile323-ab-benchmark/input.json tmp/profile323-ab-benchmark/results.json tmp/native-score-audit/profile323-integration.json
target/native-score-audit/release/integration.exe tmp/queue-three-song-benchmark/input.json tmp/queue-three-song-benchmark/optimized-max-1.json tmp/native-score-audit/queue-integration.json
python -B scripts/research/verify_native_scoring_integration.py tmp/native-score-audit/profile323-integration.json tmp/native-score-audit/queue-integration.json
```

真实档案和谱面输入沿用本地实验文件，不纳入版本库。生成报告位于 `tmp/native-score-audit/`；原生验证仍需要相邻 `apk-reverse` 中固定版本的 ELF 和工具。

## 尚未完成的迁移

1. **整数缓存与控分区间**：迁移为完整计分综合力位模式；检查边界查找，不能沿用每次只增减整数综合力的假设。
2. **贡献剪枝与队伍上界**：原来的连续 f64 评分界限需要保守覆盖新运算的逐步舍入，再与小规模完整枚举比较。不可直接删掉贡献剪枝，也不能只替换最终重算。
3. **队伍摆放顺序**：成员累加顺序可能影响浮点综合力末位。推荐摆放、最高分命中概率、平均 PT 和缓存应共同使用明确的顺序口径。
4. **模式及数据边界**：养成整数生成继续使用旧模型；完整巡回 Combo 主数据、Auto 模式取值和多轴自定义额外加成尚未全部原生核验。真实样本通过不等于这些边界已消失。
5. **应用统一切换**：指定队伍、单曲／巡回最高分、最大平均 PT、Auto、控分共同接入；保留旧历史计算值及口径，新计算记录规则标识。
6. **性能验收**：完成上面迁移后，再用原来的两份档案做完整搜索复测，记录候选数和各阶段耗时。

第 1、2 项的基础接口进展见[缓存与上界记录](scoring-search-bounds.md)。接下来处理完整贡献模型、MITM 三维范围和实际搜索调用；击打时机优化仍按既定计划排在旧误差修复之后。
