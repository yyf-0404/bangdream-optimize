# 服务器 API

后端服务器是与后端服务与 bot 集成的稳定 HTTP 边界。
它不再兼容旧版 Node `calcResult` 接口。

## 启动后端

最小化启动示例（MongoDB、本地游戏数据、静态 `/game-data` 与内部埋点）：

```bash
BANGDREAM_OPTIMIZE_HOST=127.0.0.1 \
BANGDREAM_OPTIMIZE_PORT=3100 \
BANGDREAM_OPTIMIZE_MONGODB_URI='mongodb://USER:PASSWORD@HOST:27017/' \
BANGDREAM_OPTIMIZE_MONGODB_DB=tsugu-bangdream-bot \
BANGDREAM_OPTIMIZE_GAME_DATA_ROOT=var/game-data \
BANGDREAM_OPTIMIZE_TELEMETRY_JSONL=var/telemetry/internal.jsonl \
cargo run -p bangdream-optimize-server --release
```

本地可直接使用：

```bash
./scripts/run-server.sh
```

该脚本默认会开启 `BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED=1`，且
`BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_INTERVAL_SECONDS=3600`，即启动一次并每小时更新一次；
如果你不希望启动时拉取，可通过 `BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED=0 ./scripts/run-server.sh` 关闭。

默认后端地址为 `http://127.0.0.1:3100`，后端静态 `/game-data` 地址为
`http://127.0.0.1:3100/game-data`。Web UI 应运行在独立端口。
在默认本地配置下，Web 也会从同一份本地镜像提供 `/game-data`，因此浏览器读取保持同源。

生产环境部署时，不要把前端配置成 `127.0.0.1`。
应将前后端放在同域，并在前端设置：

```js
globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  gameDataBaseUrl: '/game-data',
  apiBaseUrl: '',
};
```

前端计算不访问 `/v1/`，默认由浏览器 WASM 使用 `/game-data` 本地计算。
导入 Bestdori 玩家资料会访问同源 `/bestdori/player/*`，国服游戏账号导入会访问
同源 `/bangdream/user-data/import`。生产同域部署需要将这两个路径反代到后端。
只有需要对外暴露后端计算 API 时，才需要同时反代 `/v1`。
可直接参考 `docs/nginx-reverse-proxy.conf` 的可编辑示例。

前端配置固定为 `apiBaseUrl: ''`，通过反代即可避免浏览器跨域。
服务端默认带有宽松 CORS，用于外部系统直接跨域访问 `/v1/`。

可选环境变量：

- `RUST_LOG=info`
- `BANGDREAM_OPTIMIZE_GAME_DATA_BASE_URL`
- `BANGDREAM_OPTIMIZE_BESTDORI_SCORE_RANGE_CHART_META`：覆盖 score-range 模板文件路径
- `BANGDREAM_OPTIMIZE_GAME_DATA_CACHE_ROOT`
- `BANGDREAM_OPTIMIZE_BESTDORI_ROOT`
- `BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED`：启动时执行一次游戏资源同步，并可选按配置文件/参数执行定期更新。
- `BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_INTERVAL_SECONDS`：游戏资源同步周期（单位：秒），开启该变量会按周期执行更新。默认 `3600` 秒（1 小时）。
- `BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_CONFIG`：读取同步配置文件（JSON）。
- `BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_COMMAND`：指定用于执行同步的可执行文件，未配置时会尝试自动发现。
- `BANGDREAM_OPTIMIZE_ENABLE_CALC_ROUTES=false`：关闭
  `/v1/maximize`、`/v1/score-range`、`/v1/calc-result` 与候选计算路由，仅保留
  `/bestdori/player/...`、`/game-data` 与站点根目录；若同时开启
  国服游戏账号导入，也会保留
  `/bangdream/user-data/import`。该模式适用于仅作导入/静态代理且不对外暴露计算 API
  的部署。
- `BANGDREAM_OPTIMIZE_WEB_ROOT`：显式让后端进程同时提供静态 Web 根目录。
- `BANGDREAM_OPTIMIZE_ENABLE_BD_IMPORT=false`：关闭默认开启的国服游戏账号数据导入接口。
- `BANGDREAM_OPTIMIZE_BD_PERSIST`：覆盖固定 apk-reverse `persist.json` 路径；默认
  `var/bangdream-account/persist.json`。
- `BANGDREAM_OPTIMIZE_BD_PERSIST_DIR`：覆盖固定 persist 目录，后端读取其中的
  `persist.json`。

当同时设置 `BANGDREAM_OPTIMIZE_GAME_DATA_BASE_URL` 与
`BANGDREAM_OPTIMIZE_GAME_DATA_CACHE_ROOT` 时，计算数据会从缓存化的
`/game-data` 镜像客户端读取；否则后端会直接读取
`BANGDREAM_OPTIMIZE_GAME_DATA_ROOT` 指向的本地文件镜像。

## 启动时同步游戏资源

通过 `BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_*` 环境变量启动时拉取并可按周期更新游戏资源。例如：

```bash
BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED=1 \
BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_INTERVAL_SECONDS=3600 \
BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_CONFIG=configs/sync-game-data.json \
cargo run -p bangdream-optimize-server --release
```

`configs/sync-game-data.json` 中可包含要更新的范围：

```json
{
  "enabled": true,
  "intervalSeconds": 3600,
  "allEventDetails": true,
  "allCharts": true,
  "allCardDetails": true,
  "charts": ["232:expert", "86:expert"],
  "events": [287],
  "out": "var/game-data",
  "baseUrl": "https://bestdori.com",
  "repairDir": "tsugu-bangdream-bot/backend/config"
}
```

若未显式传入范围参数，则默认会拉取：

- `allEventDetails`
- `allCharts`
- `allCardDetails`

（即默认触发“尽量完整”同步并每小时更新）。若要回退到最小范围，请显式传入
`--game-data-sync-event`、`--game-data-sync-chart` 或对应配置文件字段。

也可以直接在启动参数中传入范围参数，例如：

```bash
BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED=1 \
cargo run -p bangdream-optimize-server --release -- \
  --game-data-sync-event 287 \
  --game-data-sync-chart 232:expert \
  --game-data-sync-chart 86:expert \
  --game-data-sync-all-card-details
```

## 健康检查

```http
GET /health
```

响应示例：

```json
{
  "status": "ok",
  "data": "healthy"
}
```

## 导入国服游戏账号数据

该接口默认开启。后端读取固定 `persist.json`，刷新 Unity 登录态，
拉取 `/suite/user/{userId}`，并返回 `PlayerConfig` 形状的数据补丁。
前端只传玩家 ID；不要把 persist、access key、token 或请求头暴露给浏览器。

默认真实 persist 路径为 `var/bangdream-account/persist.json`，该文件被
`.gitignore` 忽略。仓库只提交 `var/bangdream-account/persist.example.json`，
部署时复制 example 并填入真实值：

```bash
cp var/bangdream-account/persist.example.json var/bangdream-account/persist.json
```

```bash
cargo run -p bangdream-optimize-server --release
```

如需关闭该接口：

```bash
BANGDREAM_OPTIMIZE_ENABLE_BD_IMPORT=false \
cargo run -p bangdream-optimize-server --release
```

```http
POST /bangdream/user-data/import
Content-Type: application/json

{
  "userId": 1008159056
}
```

响应：

```json
{
  "status": "ok",
  "data": {
    "playerId": 1008159056,
    "server": "cn",
    "cardList": {},
    "areaItem": {},
    "characterBouns": {}
  }
}
```

该接口面向受控后端部署；错误响应不会返回 `access_key`、`X-Token`、persist
路径或原始响应头。

同域 Web/桌面部署时，Nginx 需要将
`/bangdream/user-data/import` 反代到后端进程。参考
`docs/nginx-reverse-proxy.conf` 中的 exact location 配置。

`characterBouns.potential` 来自 `/suite/user` field `401`。
`characterBouns.characterTask` 来自 field `456` 的完整角色任务加成倍率表；
后端不会用稀疏的 field `430` 记录冒充完整任务加成。

## 计算玩家

```http
POST /v1/maximize
```

兼容路由：`POST /v1/calc-result`。

请求体：

```json
{
  "playerId": 1008604961,
  "server": "jp",
  "eventId": 287,
  "options": {
    "solverPreference": "auto"
  }
}
```

字段说明：

- `playerId`：MongoDB 玩家 ID。
- `server`：`jp`、`en`、`tw`、`cn` 或 `kr`。
- `eventId`：可选。若省略则使用存储的 `currentEvent`。
- `options`：可选。

`options.solverPreference` 取值：

- `auto`
- `scalar`
- `randomBucket`
- `avx2`

响应示例：

```json
{
  "status": "ok",
  "data": {
    "eventId": 287,
    "eventType": "medley",
    "totalScore": 9110369,
    "totalStat": 1226892,
    "songs": [],
    "items": {
      "band": "2",
      "attribute": "happy",
      "magazine": "technique"
    },
    "solver": "avx2",
    "metrics": {
      "coreVersion": "0.2.1",
      "cardCount": 1950,
      "songCount": 3,
      "itemCombinationsBefore": 120,
      "itemCombinationsAfter": 1,
      "totalElapsedMs": 2585.0
    }
  }
}
```

`songs` 数组每项为一首歌对应的一支选定队伍：

```json
{
  "songId": 232,
  "difficulty": 4,
  "score": 2818843,
  "stat": 397885,
  "teamCardIds": [1920, 1713, 1807, 2096, 2190],
  "captainCardId": 2190
}
```

## 目标 PT 区间搜索

```http
POST /v1/score-range
```

请求体：

```json
{
  "playerId": 1008604961,
  "server": "jp",
  "eventId": 287,
  "currentPt": 100000,
  "targetTotalPt": 101000,
  "autoBaseMultiplier": 0.75,
  "maxResults": 20
}
```

`autoBaseMultiplier` 可选值为 `0.5` 或 `0.75`。省略时保持兼容行为：日服使用 `0.75`，
其他服务器使用 `0.5`。

`mission_live` 还必须显式传入 `missionSupportPtBonus`，其值是
`floor(支援队伍综合力 / 3000)`；明确传入 `0` 合法，省略会返回 `400`。其他活动忽略该字段。

接口只返回重复同一首 `(songId, difficulty)` 的方案。搜索在所有队伍、道具和歌曲之间先全局
最小化演奏次数，再在次数相同时最小化实际总火耗；`1/5/10/15` 倍分别消耗 `0/1/2/3`
火。响应只包含该最优层中的任意见证，达到
`maxResults` 后可以提前结束；最优层结果不足上限时不会用较差方案补足。

不依赖 MongoDB、直接提供完整配置的 API 客户端可以调用：

```http
POST /v1/score-range/from-config
```

字段与 `/v1/score-range` 相同，但用 `player` 替代 `playerId`：

```json
{
  "player": {
    "playerId": 1008604961,
    "currentEvent": 287,
    "cardList": {},
    "areaItem": {},
    "characterBouns": {}
  },
  "server": "jp",
  "eventId": 287,
  "currentPt": 100000,
  "targetTotalPt": 101000,
  "autoBaseMultiplier": 0.75,
  "maxResults": 20
}
```

该入口不读取 MongoDB 玩家配置。官方 Web 前端不调用该入口，而是读取静态 score-range
模板并在浏览器 WASM 中计算；服务端入口与浏览器使用相同模板文件和 `songs/all` 可用性判断。

响应中的每项包含五张卡、区域道具、综合力、活动 PT 加成和演奏动作：

```json
{
  "eventType": "versus",
  "targetDeltaPt": 1000,
  "teamCardIds": [1, 2, 3, 4, 5],
  "totalStat": 400000,
  "pointBonusBasisPoints": 0,
  "items": {
    "band": "1",
    "attribute": "cool",
    "magazine": "performance"
  },
  "playCount": 1,
  "distinctSongCount": 1,
  "totalFireCost": 1,
  "totalFireMultiplier": 5,
  "plays": [
    {
      "songId": 1,
      "difficulty": 3,
      "fireMultiplier": 5,
      "score": 1000000,
      "pt": 1000,
      "count": 1
    }
  ]
}
```

`totalFireCost` 是搜索使用的实际火耗。`totalFireMultiplier` 是各局火倍率乘以次数后的旧统计值，
仅为兼容保留，不参与最优解比较。

## 从候选队伍计算

```http
POST /v1/maximize/from-candidates
```

兼容路由：`POST /v1/calc-result/from-candidates`。

该路由接受已经构建好的候选队伍，仅执行最后的候选选择。
主要用于测试、诊断，或外部调用方自行构建候选队伍的场景。

请求结构：

```json
{
  "eventId": 287,
  "eventType": "medley",
  "songList": [
    { "songId": 232, "difficulty": 4 },
    { "songId": 86, "difficulty": 3 },
    { "songId": 669, "difficulty": 3 }
  ],
  "currentBest": 0,
  "solverPreference": "auto",
  "candidates": [
    {
      "mask": 1,
      "teamCardIds": [1, 2, 3, 4, 5],
      "captainCardIds": [1, 1, 1],
      "scores": [100, 90, 80],
      "stat": 1000
    }
  ]
}
```

当使用的紧凑卡片位数超过 64 时，可用 `maskWords` 代替 `mask`。

## 错误响应

错误使用同一顶层结构：

```json
{
  "status": "error",
  "message": "..."
}
```

状态码说明：

- `400`：请求无效或计算输入非法。
- `404`：找不到玩家、活动、歌曲、谱面或 game-data 实体。
- `503`：服务端存储/数据源未配置或不可用。

## Smoke 测试

服务启动后执行：

```bash
BANGDREAM_OPTIMIZE_BASE_URL=http://127.0.0.1:3100 \
BANGDREAM_OPTIMIZE_PLAYER_ID=1008604961 \
BANGDREAM_OPTIMIZE_SERVER=jp \
BANGDREAM_OPTIMIZE_EVENT_ID=287 \
bash scripts/server-smoke.sh
```
