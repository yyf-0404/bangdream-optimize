# 计算用例结构

计算代码按“共享数据准备 + 独立搜索策略”组织：

```text
crates/core/src/
  maximize/       最大分数搜索
  score_range/    目标分数区间搜索
  model/          两种用例共享的领域模型

crates/data/src/
  snapshot.rs     游戏数据快照
  preparation.rs  活动与玩家档案的共享准备流程
  maximize.rs     最大化用例的数据适配器
  score_range.rs  区间搜索用例的数据适配器

crates/service/src/
  maximize.rs     最大化应用服务
  score_range.rs  区间搜索应用服务
```

`GameDataSnapshot` 保存卡片、区域道具、活动，以及按用例选择的原始谱面或 score-range
技能窗口模板。`prepare_event_context`
负责把活动参数和玩家档案转换为 `PreparedEventContext`，同时提供带活动综合力加成的卡片、
不带活动综合力加成的卡片以及单卡 PT 加成微单位。搜索策略不得重复实现档案、活动加成或
区域道具的准备逻辑。

最大化的主接口使用 `MaximizeInputBuilder`、`MaximizeService`、`MaximizeOptions` 和
`maximize_result_for_items`。旧的 `CalculationInputBuilder`、`OptimizerService`、
`ItemSearchOptions`、`calculate_best_result_for_items` 暂时作为兼容名称保留。

HTTP 主路由为 `/v1/maximize`、`/v1/maximize/from-candidates`、`/v1/score-range` 和
`/v1/score-range/from-config`。原有
`/v1/calc-result` 路由暂时保留并指向最大化处理器。

活动类型支持范围由各用例独立维护：

- `maximize`：`medley`、`versus`、`challenge`
- `score_range`：`medley`、`versus`、`challenge`、`live_try`、`festival`、`mission_live`

`festival` 仍被 `maximize` 拒绝，因为最大化所需的 fever 尚未实现；`score_range` 使用
5V5 单人公式，不依赖最大化 fever。

共享准备层只负责解析活动类型，不决定某个用例是否支持该类型。

score-range 的前后端统一读取 `api/scoreRangeChartMeta.1.json`。每张可搜索谱面按 17 个技能
时长保存总未激活节点数、6 个技能窗口各自的激活节点数和尾部风险；歌曲等级、服务器发布时间、
难度发布时间与 `closedAt` 继续来自 `api/songs/all.7.json`。浏览器将模板交给 Web Worker 中的
WASM，服务端与桌面端由 filesystem data adapter 读取同一文件。
