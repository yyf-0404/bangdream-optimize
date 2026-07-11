# Captured medley solver fixtures

这些 `.bms` 文件是完整真实 fixture 在 solver 边界产生的输入，用于隔离候选生成成本后比较 exact 与 random-bucket。文件按输入内容哈希去重；当前包含 6 个 2,002–2,405 候选样本和 3 个 136,891–156,257 候选样本。

捕获新的样本：

```powershell
$env:BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_CAPTURE_DIR='D:\dev\bangdream-optimize\crates\medley-solver\tests\fixtures\captured'
$env:BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_CAPTURE_MIN_CANDIDATES='2000'
cargo test --release -p bangdream-optimize-data maximize_fixture_uses_complete_player_profile -- --ignored --nocapture
```

只有设置 `BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_CAPTURE_DIR` 时才会写文件；`CAPTURE_MIN_CANDIDATES` 缺省为 0。捕获逻辑不在 `wasm32` 构建中编译。

运行候选数基准：

```powershell
$env:BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_FIXTURE='crates\medley-solver\tests\fixtures\captured\narrow-156093-581f506543d22770.bms'
$env:BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_BENCH_SIZES='4096,8192,16384,32768,65536,131072,156093'
$env:BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_BENCH_REPEATS='1'
$env:BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_BENCH_KINDS='frontier,stratified,conflict'
$env:BANGDREAM_OPTIMIZE_MEDLEY_SOLVER_BENCH_ALGORITHMS='exact,random-bucket'
cargo test --release -p bangdream-optimize-medley-solver threshold_benchmark::benchmarks_candidate_count_threshold_from_captured_solver_input -- --ignored --nocapture
```

`FIXTURE` 也可指向目录，此时自动选择最大的 `.bms`。`BENCH_ALGORITHMS` 可单独设为 `exact` 或 `random-bucket`，便于隔离长时间运行。每个算法先预热一次，再执行 `BENCH_REPEATS` 次计时。

派生子集含义：

- `frontier`：轮流选择三首歌各自得分最高且尚未选择的候选。
- `stratified`：在每 256 个分数排名块内固定种子洗牌，再执行 frontier 选择。
- `conflict`：优先选择与三首最高分锚点卡位冲突较多的候选，再按歌曲得分排序。

二进制均为小端：4 字节 `BMS1`，1 字节掩码类型，3 字节保留位，`i32 current_best`，`u64 candidate_count`，`u64 word_count`，随后是所有候选掩码以及每个候选的 `[i32; 3]` 分数。类型 0 的窄掩码每个候选一个 `u64`；类型 1 的宽掩码每个候选包含 `word_count` 个 `u64`。
