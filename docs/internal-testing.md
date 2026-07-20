# 内部测试手册

该手册用于早期内部测试，不包含硬编码敏感信息。

## 1. 构建 WASM

```bash
./scripts/build-web-assets.sh --no-deploy
```

若缺少 `wasm-bindgen`：

```bash
cargo install wasm-bindgen-cli
```

### 浏览器 WASM 中的时间 API

`bangdream-optimize-web-wasm` 会把 `crates/core` 等共享 Rust crate 编译到
`wasm32-unknown-unknown`。这些共享代码不得直接调用
`std::time::Instant::now()` 或 `std::time::SystemTime::now()`：接口能够通过编译，
原生测试也能正常运行，但浏览器执行到该调用时可能以 `RuntimeError: unreachable`
终止。因此，这类问题无法只靠原生 `cargo check/test` 提前发现，并且容易在新增
profiling 后重复出现。

计时统一使用 `crates/core/src/timing.rs` 中的跨平台 `Timer`：原生目标使用
`std::time::Instant`，浏览器目标使用 `performance.now()`。仅供诊断的 profiling
还应显式控制是否启用；如果浏览器不需要该统计，应使用 `cfg(target_arch =
"wasm32")` 关闭，而不是先创建标准库计时器再忽略结果。

```rust
// 错误：共享求解路径在浏览器中可能运行到这里。
let started = std::time::Instant::now();

// 正确：需要跨平台计时时使用项目封装。
let started = Timer::start();
let elapsed_ms = started.elapsed_ms();

// 正确：可选 profiling 不启用时不要创建计时器。
let started = trace.then(Timer::start);
let elapsed_ms = optional_elapsed_ms(started);
```

WASM 静态资源的查询版本只负责部署缓存失效，不是时间 API 崩溃的修复手段；仅在
实际部署的缓存策略需要客户端换取新产物时更新。

## 2. 同步游戏数据

默认同步全量（含全部活动详情/全部谱面/全部卡片）：

```bash
./scripts/sync-game-data.sh
```

同步当前 smoke 用例数据：

```bash
./scripts/sync-game-data.sh \
  --event 287 \
  --chart 232:expert \
  --chart 86:expert \
  --chart 669:expert
```

显式使用全量参数（与默认无异）：

```bash
./scripts/sync-game-data.sh \
  --all-event-details \
  --all-charts \
  --all-card-details \
  --concurrency 8 \
  --retries 2
```

默认输出目录为 `var/game-data`，可通过以下方式覆盖：

```bash
BANGDREAM_OPTIMIZE_GAME_DATA_ROOT=/path/to/game-data ./scripts/sync-game-data.sh
```

## 3. 启动服务

先在外部设置 MongoDB：

```bash
export BANGDREAM_OPTIMIZE_MONGODB_URI='mongodb://USER:PASSWORD@HOST:27017/'
export BANGDREAM_OPTIMIZE_MONGODB_DB=tsugu-bangdream-bot
```

启动内部服务：

```bash
./scripts/run-server.sh
```

该脚本默认会按 1 小时（3600 秒）周期更新游戏资源，可通过环境变量关闭或修改：

```bash
BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED=0 ./scripts/run-server.sh
# 或
BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_INTERVAL_SECONDS=1800 ./scripts/run-server.sh
```

该脚本默认启动时触发一次游戏资源同步；如果你希望纯服务启动可先禁用：

```bash
BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED=0 ./scripts/run-server.sh
```

默认项：

- 后端：`http://127.0.0.1:3100`
- 参考数据端点：`http://127.0.0.1:3100/game-data`（来自 `var/game-data`）
- 埋点文件：`var/telemetry/internal.jsonl`
- cargo profile：`release`

再启动 Web UI：

```bash
./scripts/run-web.sh
```

默认项：

- Web：`http://127.0.0.1:8080`
- Web 游戏数据：`http://127.0.0.1:8080/game-data`（来自 `var/game-data`）

使用 dev profile 提升重建速度：

```bash
BANGDREAM_OPTIMIZE_CARGO_PROFILE=dev ./scripts/run-server.sh
```

## 4. Smoke

仅做健康检查：

```bash
./scripts/server-smoke.sh
```

完整 calc-result smoke：

```bash
BANGDREAM_OPTIMIZE_PLAYER_ID=1008604961 \
BANGDREAM_OPTIMIZE_SERVER=jp \
BANGDREAM_OPTIMIZE_EVENT_ID=287 \
./scripts/server-smoke.sh
```

## 5. 本地检查

发布前先执行：

```bash
./scripts/internal-check.sh
```

该脚本会运行 JS 语法检查、Rust 工作区检查、核心求解器测试、桌面 crate 测试，以及 Tauri 壳检查。

## 6. 收集内部数据

服务端埋点写入：

```text
var/telemetry/internal.jsonl
```

Web 与桌面 UI 可在成功计算后手动导出诊断 JSON。
导出内容包括玩家配置、计算结果、metrics、运行时类型及引用数据计数。
桌面模式还会附带本地用户数据与 game-data/cache 路径。

请勿要求测试者公开上传原始诊断导出文件。
仅在复现计算问题或性能瓶颈时内部使用。
