# 击打优化

在活动页选择「最高得分」后可以开启「击打优化」。支持挑战 Live 和竞演 Live 的单曲，以及巡回演出的三曲。

结果页的「击打方案」显示技能区间和偏移按键。选中按键后可以编辑相对于参考帧的最早、最晚偏移。第 0 帧是最接近谱面时间的有效 Perfect 帧，每个限制范围都要包含它。应用限制后会重新搜索队伍、技能顺序和道具。巡回演出同时重算三首曲目。

## 计分模型

- 以 120 FPS、零判定偏移为基准，在谱面可见节点的判定窗口内搜索击打时机，并按技能触发顺序分别优化。原生位置距离、隐藏节点与实机核对仍待推进。
- 搜索使用浮点计分综合力，最终摆位确定后再生成显示综合力。
- 旧版击打结果需要重新计算，才能使用当前版本的击打方案和范围编辑。

## 代码入口

- [结果页击打方案](../../apps/web/src/pages/result/hit-timing-plan.js) · [结果版本与回放记录校验](../../apps/web/src/domain/hit-timing.js)
- [谱面输入](../../crates/data/src/timing_chart.rs) · [编队与道具搜索](../../crates/data/src/timing_medley/mod.rs) · [判定与计分回放](../../crates/core/src/hit_timing.rs)

早期研究和迁移记录见本地归档 `docs/archive/scoring/scoring-and-hit-timing-plan.md`。
