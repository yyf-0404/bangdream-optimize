# 三曲位图扫描实验（2026-09-16）

结论：当前位图实现在新 1,456 卡档案的平均 PT 搜索上有明显收益，但没有在旧档案上表现出稳定优势，且慢于历史最高分样本的专用 AVX2 内核。保留为默认关闭的实验，不统一替换现有算法。

后续更新：平均 PT 的窄 / 宽 AVX2 路径已补上“整批冲突且四项均在分数带内时一次跳过”的快速路径，19 项测试通过，并已完成[同轮复测](avx2-band-retest.md)。下表的“原扫描”仍指补齐前的构建，不能把这里的位图加速比直接当成相对当前 AVX2 的收益。新基线下位图仅在新档案保持优势，旧档案与历史窄掩码大样本均由 AVX2 更快。

## 本轮完整档案对照

使用同一个原生 Release 程序，通过环境变量切换扫描后端；每份档案两种后端各跑一轮，先位图、后原扫描。所有重计算串行执行，计时包含排序和位图构建成本。设备为 Ryzen 5 5500U，Rust 1.93.1，构建使用 `-j 1`。

| 档案 / 阶段 | 原扫描 | 位图 | 本轮变化 |
|---|---:|---:|---:|
| 新 1,456 卡：完整计算 | 302.246 s | 164.355 s | 耗时减少 45.6% |
| 新 1,456 卡：三队扫描 | 236.461 s | 93.090 s | 耗时减少 60.6%，约 2.54 倍 |
| 旧 1,414 卡：完整计算 | 52.177 s | 55.844 s | 耗时增加 7.0% |
| 旧 1,414 卡：三队扫描 | 16.861 s | 18.191 s | 耗时增加 7.9% |

这是单轮原生核心计算对照，不是桌面 UI 或 WASM 实测，不代表跨设备的固定加速比。旧档案的差距较小，不能凭单轮确认稳定退化；但没有依据默认替换它的现有路径。新档案未改动的候选构造部分也有 60.620 / 66.719 秒的波动。

新档案固定国服活动 323，歌曲顺序为 186 SP / 395 SP / 772 EX，冻结输入 SHA256 为 `6f956ba69e10df7323e177378e59c43e753bcce6b8d637d2d04b8267beaa4313`。

旧档案使用仓库保存的 1,414 卡诊断 fixture、其自定义活动及 306 EX × 3。两种后端都使用当前加权概率模型，不能把本次计时与文档中旧等概率模型的计时拼成一次测试。

新档案最耗时的三套道具：

| 保留候选 | 合法三队 | 原扫描 | 位图 |
|---|---:|---:|---:|
| 157,897 | 46 | 89.691 s | 35.394 s |
| 129,159 | 0 | 78.113 s | 28.341 s |
| 138,495 | 0 | 68.405 s | 28.871 s |

后两套没有进入精确 PT 回调，收益来自扫描。三套原生原路径都是宽掩码 AVX2；旧档案三个主要扫描使用窄掩码 AVX2。这个差异值得进一步研究，但尚不足以确定通用自动分流规则。

## 历史最高分候选样本

读取已有 `.bms` 的同一组掩码和分数。近优带枚举器设置 `floor = current_best + 1`，分别模拟“首次命中即停”与“得分提高时收紧下界”。按实际掩码类型调用窄 / 宽入口，不把窄掩码套入宽入口计作正式对照。

下表近优带各后端为 4 次的中位数，最高分专用内核为 2 次的中位数；均含各自准备开销。这两份样本在初始界限以上均无合法方案，回调未执行，所以两种回调配置可以合并统计无命中扫描。

| 候选数 | 原近优带 AVX2 | 位图近优带 | 最高分原有 StrictExact AVX2 |
|---|---:|---:|---:|
| 2,005 | 22.474 ms | 14.559 ms | 8.233 ms |
| 156,257 | 6.237 s | 5.517 s | 1.526 s |

大样本原近优带为 4.763～8.919 秒，位图为 4.909～6.522 秒，存在明显波动，不能仅凭中位数宣布稳定改善。最高分原有内核明显更快，维持现状。

这也验证了不能把近优带的计时当成最高分内核的计时。两者在首次命中前都执行分数剪枝和互斥查询，但最高分已有整批冲突跳过等实现优势。本次未修改最高分内核，也没有降低搜索精确性。

初次将捕获样本传入宽掩码近优带入口的记录另存为 `captured-wide-adapter.json`，不用于上述正式比较。

## 实现和正确性

- 复用原歌曲遍历次序、候选分数排序和精确整数下界。
- 为第二、第三首歌分别建立每张卡的倒排位图，按 64 个候选位置排除冲突。
- 固定第一队后，第二队通过位图排除；第三队按需缓存与第一队不冲突的位置，再排除第二队的卡牌。
- 每次回调提高下界后，重新约束剩余位置；按原顺序访问合法候选，保留同分和提前停止语义。
- 不构造候选数平方的兼容矩阵。首套新样本两份位图数据的上界约 5.80 MiB，不包括成员列表等辅助结构，更不代表进程峰值内存。

两份完整档案的所有非计时字段逐项一致，包括平均 PT 的精确整数分子 / 分母、PT 范围、三队卡牌、队长、推荐顺序、候选数、逻辑检查数、合法方案数和分布缓存计数。

新档案平均 PT 为 `728394032679 / 1073741824`，46 个合法方案、70 个缓存分布，逻辑 pair / third 检查分别为 `1,711,694,751` / `22,820,971,532`。旧档案为 `789280615173 / 1073741824`，36 个合法方案、42 个缓存分布。

`pairCheckCount` / `thirdCheckCount` 仍按原逐位置扫描口径计数，批量排除的位置也包括在内，不代表优化后的实际指令次数。实验日志的 `implementation=Scalar` 指可移植的 u64 标量位运算。

测试覆盖 63/64/65、127/128/129 个候选的位图边界、跨 64 张卡的宽掩码、稀疏高位、空掩码、全冲突、同分、i64 饱和加法、动态下界和提前停止。回调的完整序列及逻辑计数与原标量实现对照；原有三重穷举测试也通过启用位图后的公共入口验证。

## 开关与复现

代码位于 `crates/medley-solver/src/bitmap_band.rs`。Cargo feature `experimental-bitmap-band` 默认关闭，正常网页 / 桌面构建不启用。启用后，原生设置 `BANGDREAM_OPTIMIZE_BAND_BACKEND=scan` 使用原扫描，其他值使用位图；WASM 编译时启用则使用位图。

```powershell
cargo test -j 1 --offline -p bangdream-optimize-medley-solver --features experimental-bitmap-band --lib
cargo build -j 1 --offline --release --manifest-path tmp/profile323-research/Cargo.toml --target-dir target --bin profile323-research --bin bitmap-fixture --bin bitmap-captured
python -X utf8 -u tmp/bitmap-band-study/run-native.py bitmap-1456 bitmap target/release/profile323-research.exe tmp/profile323-ab-benchmark/input.json
python -X utf8 -u tmp/bitmap-band-study/run-native.py scan-1456 scan target/release/profile323-research.exe tmp/profile323-ab-benchmark/input.json
python -X utf8 tmp/bitmap-band-study/compare.py tmp/bitmap-band-study/scan-1456.json tmp/bitmap-band-study/bitmap-1456.json
python -X utf8 -u tmp/bitmap-band-study/run-native.py bitmap-1414 bitmap target/release/bitmap-fixture.exe crates/data/tests/fixtures/bangdream-optimize-diagnostic-0-2026-06-13T13-41-23-273Z.json
python -X utf8 -u tmp/bitmap-band-study/run-native.py scan-1414 scan target/release/bitmap-fixture.exe crates/data/tests/fixtures/bangdream-optimize-diagnostic-0-2026-06-13T13-41-23-273Z.json
target/release/bitmap-captured.exe crates/medley-solver/tests/fixtures/captured/narrow-156257-93a98114e9e2c4bb.bms
target/release/bitmap-captured.exe --exact-only crates/medley-solver/tests/fixtures/captured/narrow-156257-93a98114e9e2c4bb.bms
cargo check -j 1 --offline -p bangdream-optimize-web-wasm -p bangdream-optimize-medley-solver --target wasm32-unknown-unknown --features bangdream-optimize-medley-solver/experimental-bitmap-band
```

完整输入、程序、结果、逐套日志、精确比较脚本及历史候选测量保存在 `tmp/bitmap-band-study/` 和 `tmp/profile323-research/`；这些本地实验材料未纳入 Git。

后续应在原扫描和位图之间研究自适应选择，并用不同掩码宽度、冲突分布和有效分数前缀验证；不能仅按档案卡牌数或总候选数切换。WASM 运行性能及进程峰值内存仍未实测。
