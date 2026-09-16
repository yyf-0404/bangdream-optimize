# 压缩评分的原生 AVX2 编译路径（2026-09-16）

本轮保留一项候选评分优化：让压缩评分的整个小型内核在支持 AVX2 的 CPU 上使用单独编译的入口。两轮完整档案开关对照中，候选评分时间平均减少 23.4% / 27.7%，完整计算减少 7.6% / 9.3%。没有减少候选、改变剪枝、调整概率或替换精确计算。

同轮还尝试了第三队前缀的单卡 / 双卡冲突证明，因收益不足未采用，见 [冲突证明实验](prefix-blocker-experiment.md)。此前的 AVX2 前缀复用继续保留。

## 完整档案对照

Windows 原生 Rust Release，Ryzen 5 5500U，Rust 1.93.1。沿用此前冻结输入：

- 1456 卡：国服活动 323，活动顺序为 186 SP / 395 SP / 772 EX；SHA-256 为 `6f956ba69e10df7323e177378e59c43e753bcce6b8d637d2d04b8267beaa4313`。
- 1414 卡：历史 fixture，自定义活动 0、306 EX 三曲；SHA-256 为 `fe90d65d670ec2aacc94814b6c11c1ff58c926f9fabf9c238ea18c3b3708aabc`。

使用当前 96 种排列 / 1024 条路径的加权概率与完整精确搜索。倍率压缩、覆盖 DP、兼容性上界剪枝、技能初始顺序缓存、五卡分配 DP 展开和 AVX2 前缀复用均开启。唯一开关是 `BANGDREAM_OPTIMIZE_COMPRESSED_NATIVE=off/on`；关闭仍使用此前的压缩评分和已有 AVX2 渐增技能辅助函数。

每份档案两轮开关对照，使用同一二进制，轮间未改代码或重新编译。第一轮为 1456 开 → 关、1414 关 → 开，第二轮反转顺序。没有额外预热，计时不与编译或测试并行。

两轮算术平均：

| 档案 / 阶段 | 关闭 | 开启 | 时间减少 |
|---|---:|---:|---:|
| 1456 / 候选评分 | 9.919 s | 7.599 s | 23.4% |
| 1456 / 五卡枚举 | 12.074 s | 9.747 s | 19.3% |
| 1456 / 完整计算 | 31.574 s | 29.161 s | 7.6% |
| 1414 / 候选评分 | 6.219 s | 4.497 s | 27.7% |
| 1414 / 五卡枚举 | 7.496 s | 5.742 s | 23.4% |
| 1414 / 完整计算 | 19.479 s | 17.673 s | 9.3% |

候选评分是 `build_candidate_ms`，包含在五卡枚举阶段中；这些行不能相加。完整候选生成（含各项准备和剪枝）从 19.324 → 16.964 s、12.837 → 10.952 s。三队阶段为 9.272 → 9.245 s、4.295 → 4.340 s，基本未变，不将其波动解释为本轮优化贡献。

每轮完整计算时间：

| 档案 | 第一轮关闭 → 开启 | 第二轮关闭 → 开启 |
|---|---:|---:|
| 1456 | 31.880 → 28.870 s | 31.268 → 29.451 s |
| 1414 | 19.448 → 17.671 s | 19.510 → 17.675 s |

两轮样本说明本机两份输入的收益，不代表所有 CPU、档案或浏览器环境的固定加速比。

完整阶段、枚举内部、剪枝和三种杂志的搜索耗时见 [详细耗时分解](compressed-native-timings.md)。

## 改动及汇编证据

代码在 `crates/core/src/model/chart/compressed.rs`：

1. 原评分函数体提取为内联的共用实现，普通入口与 AVX2 入口调用同一份代码。
2. x86 / x86_64 平台先检测 CPU 的 AVX2 支持，再进入 `#[target_feature(enable = "avx2")]` 的函数；其他平台或不支持的 CPU 继续普通路径。
3. 保留所有原有适用条件：未开启压缩、启用 fever、模式不匹配或无倍率段时继续原回退。

此前仅渐增技能的辅助求和函数使用 AVX2。压缩评分本体仍按通用 CPU 目标编译，每段基础分、普通技能加成和部分尾部处理会调用通用 `floor`。新入口允许编译器直接使用相应指令并进行合法的向量化。

用 MSVC `dumpbin` 检查 Release 库：旧内核有五处 `call floor` 指令位置，新 AVX2 内核中为零，出现 `vroundsd` / `vroundpd`；未发现融合乘加指令。这是静态汇编位置数，不是每个候选的实际调用次数。新同二进制中的普通路径仍保留通用取整调用。

没有开启 fast-math，没有手写近似取整或把多次累加合并成乘法。原来的浮点乘法、两层 floor、渐增倍率递推、技能重复行复用、五卡分配与同分选择保持不变。没有新增缓存、堆分配或候选索引；代价是额外的原生机器码及入口检测。

WASM 与非 x86 平台不使用此入口，本轮不声称网页端或桌面 UI 端到端性能已获得同样改善。

## 一致性验证

- 核心库 Debug 测试 192 项通过、1 项原有压力测试跳过。
- Release 优化构建的四项压缩评分测试全部通过，包含新增的 6,000 组逐项对照。
- WASM core 编译通过，保留原有 solver 四条和 core 三条未使用代码警告；没有新增此类警告。
- 新增 6,000 组 AVX2 / 普通路径矩阵对照，覆盖三种 combo、技能窗口交叠与不交叠、所有支持的时长、普通 / 渐增技能、倍率上限附近、重复技能、零与随机综合力，以及复用 scratch 后的状态变化。
- 逐项比较基础分、五卡六次技能增量、基础分数组和渐增倍率序列；已有压缩评分与逐音符评分对照继续通过。
- 两轮共八次完整计算，所有非耗时结果与上一轮冻结结果一致，包括队伍、道具、队长、推荐顺序、分数分布和精确平均 PT。
- 原始候选 38 / 21 批指纹相同，累计覆盖 5,860,064 / 3,693,336 条候选；最终候选仍为 78,079 / 117,831。搜索计数、合法方案数、精确分布数、技能初始顺序缓存命中数均保持不变。
- 精确平均 PT 保持 `728394032679 / 1073741824` 和 `789280615173 / 1073741824`。
- 两轮程序与输入哈希一致，计时后源码哈希未变化；`git diff --check` 与修改文件的 `rustfmt --check` 通过。

## 复现和记录

正常原生构建在支持 AVX2 时默认启用。实验 feature `experimental-compressed-native` 提供同二进制开关，不改变生产配置界面。

```powershell
cargo test -j 1 --offline --config profile.test.debug=0 --config profile.test.incremental=false -p bangdream-optimize-core --lib
cargo test -j 1 --offline --release -p bangdream-optimize-core --lib model::chart::compressed::tests
cargo check -j 1 --offline -p bangdream-optimize-core --target wasm32-unknown-unknown
cargo build -j 1 --offline --release --manifest-path tmp/profile323-research/Cargo.toml --target-dir target --bin profile323-research --bin bitmap-fixture
python -X utf8 tmp/compressed-native-study/suite.py
python -X utf8 tmp/compressed-native-study/analyze.py
python -X utf8 tmp/compressed-native-retest/suite.py
python -X utf8 tmp/compressed-native-retest/analyze.py
python -X utf8 tmp/compressed-native-study/report-data.py
```

原始结果、日志和程序 / 输入哈希在 `tmp/compressed-native-study/` 与 `tmp/compressed-native-retest/`。前者还保存两轮汇总 `summary.json`、源码哈希、实现差异和前后汇编。脚本拒绝覆盖已有结果，再次计时应复制至新目录。

没有提交、推送、修改版本号或部署。
