# Web 静态化方案

Web 端不需要运行时计算后端。
网页服务器、后端或 CDN 可提供静态 Bestdori 镜像，浏览器会将该镜像按需同步到 IndexedDB。

## 运行形态

- 浏览器加载 `bangdream-optimize-web-wasm`。
- 浏览器将用户数据存入 IndexedDB。
- 浏览器从配置的 `gameDataBaseUrl` 下载游戏数据。
- 浏览器在计算前刷新 `${gameDataBaseUrl}/manifest.json`，仅同步新增或变更的文件到 IndexedDB。
- Rust/WASM 只接收浏览器已加载的 JSON，返回 `BuildResult` 的 JSON 字符串。

## 生产部署

生产环境建议使用 Nginx 托管前端静态文件，并将 `/game-data/`、
`/bestdori/player/` 与 `/bangdream/user-data/import` 反向代理到后端。
后端负责挂载 `BANGDREAM_OPTIMIZE_GAME_DATA_ROOT` 并提供 `/game-data`。

部署前先准备 Rust/Cargo、WASM 工具链和基础系统工具，见
`docs/environment.md`。

部署机默认目录：

- 前端发布目录：`/var/www/bangdream-optimize/web`
- 后端项目目录：`/opt/bangdream-optimize`
- 后端运行数据：`/var/bangdream-optimize`
- game-data 镜像：`/var/bangdream-optimize/game-data`

1. 准备项目目录与运行目录

将仓库放在 `/opt/bangdream-optimize`。如果使用其他项目目录，需要同步修改
`docs/systemd/bangdream-optimize-backend.service` 中的 `WorkingDirectory` 与
`ExecStart`。

```bash
id -u bangdream >/dev/null 2>&1 || sudo useradd --system --home /opt/bangdream-optimize --shell /usr/sbin/nologin bangdream
sudo mkdir -p /etc/bangdream-optimize /var/bangdream-optimize/game-data /var/www/bangdream-optimize/web
sudo chown -R bangdream:bangdream /var/bangdream-optimize
sudo chown -R "$USER":"$USER" /var/www/bangdream-optimize/web
```

2. 在项目目录构建后端

```bash
cd /opt/bangdream-optimize
cargo build --release -p bangdream-optimize-server -p bangdream-optimize-sync-bestdori
chmod 0755 target/release/bangdream-optimize-server target/release/bangdream-optimize-sync-bestdori
```

后端启动时若开启游戏数据同步，会优先查找与
`bangdream-optimize-server` 同目录的 `bangdream-optimize-sync-bestdori`。
上述流程中二者都位于项目目录的 `target/release/`。

3. 安装并编辑后端配置

```bash
sudo cp docs/systemd/bangdream-optimize-backend.service /etc/systemd/system/bangdream-optimize-backend.service
sudo cp docs/systemd/bangdream-optimize-backend.env.example /etc/bangdream-optimize/backend.env
sudo nano /etc/bangdream-optimize/backend.env
```

`backend.env` 至少需要确认 MongoDB 连接信息与
`BANGDREAM_OPTIMIZE_GAME_DATA_ROOT=/var/bangdream-optimize/game-data`。
如果不希望后端启动时同步 game-data，可设置
`BANGDREAM_OPTIMIZE_GAME_DATA_SYNC_ENABLED=0`。
国服游戏账号导入默认开启，默认读取项目内
`var/bangdream-account/persist.json`。仓库只提交
`var/bangdream-account/persist.example.json`，部署时需要复制 example 并填入真实值。
如果要关闭该接口，设置 `BANGDREAM_OPTIMIZE_ENABLE_BD_IMPORT=false`。

4. 初始化 game-data 挂载目录

项目内 `var/game-data` 保存了本项目维护的修正文件和可随仓库带上的初始镜像文件。
部署时直接复制整个目录到后端挂载目录：

```bash
cd /opt/bangdream-optimize
sudo rsync -a var/game-data/ /var/bangdream-optimize/game-data/
sudo chown -R bangdream:bangdream /var/bangdream-optimize/game-data
```

后续 game-data 同步会在该目录内更新远端数据，并保留已有的 fix 文件，将它们写入
manifest。

5. 启动后端

```bash
sudo systemctl daemon-reload
sudo systemctl enable bangdream-optimize-backend
sudo systemctl start bangdream-optimize-backend
sudo systemctl status bangdream-optimize-backend
```

6. 配置前端同源路径

生产环境保持 `apps/web/config.js` 使用同源 `/game-data` 与空
`apiBaseUrl`：

```js
globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  gameDataBaseUrl: '/game-data',
  apiBaseUrl: '',
};
```

该部署下，前端计算在浏览器 WASM 内完成，不访问 `/v1/`；导入
Bestdori 玩家资料会访问同源 `/bestdori/player/`，国服游戏账号导入会访问同源
`/bangdream/user-data/import`，因此 Nginx 需要保留这两个反代。

7. 构建并发布前端

先让当前部署用户可写前端发布目录，然后直接运行发布脚本：

```bash
sudo mkdir -p /var/www/bangdream-optimize/web
sudo chown -R "$USER":"$USER" /var/www/bangdream-optimize/web
cargo install wasm-bindgen-cli
cd /opt/bangdream-optimize
./scripts/build-web-assets.sh
```

`build-web-assets.sh` 会构建 WASM，并将 `apps/web/` 全量部署到
`BANGDREAM_OPTIMIZE_WEB_ROOT`（默认 `/var/www/bangdream-optimize/web`）。
生产部署不需要 `--no-deploy`。

8. 安装 Nginx 配置

```bash
sudo cp docs/nginx-reverse-proxy.conf /etc/nginx/sites-available/bangdream-optimize.conf
sudo ln -sf /etc/nginx/sites-available/bangdream-optimize.conf /etc/nginx/sites-enabled/bangdream-optimize.conf
sudo nginx -t
sudo systemctl reload nginx
```

`docs/nginx-reverse-proxy.conf` 会：

- 从 `/var/www/bangdream-optimize/web` 托管前端；
- 将 `/game-data/` 反代到 `http://127.0.0.1:3100`；
- 将 `/bestdori/player/` 反代到 `http://127.0.0.1:3100`；
- 将 `/bangdream/user-data/import` 反代到 `http://127.0.0.1:3100`；
- 对其他路径做 SPA 回退。

9. 验证

```bash
curl --fail http://127.0.0.1:3100/health
curl --fail http://127.0.0.1:3100/game-data/manifest.json
curl --fail http://127.0.0.1/game-data/manifest.json
```

如果关闭了自动同步，请先生成或同步 game-data 到
`/var/bangdream-optimize/game-data`，否则 `manifest.json` 会不存在。
fix 文件也要保留在该目录；部署步骤中的 `install` 命令会完成复制。

## 静态镜像目录结构

推荐结构：

- `/game-data/api/cards/all.5.json`
- `/game-data/api/cards/{cardId}.json`：`api/cards/all.5.json` 中所有卡片对应文件
- `/game-data/api/characters/main.3.json`
- `/game-data/api/skills/all.10.json`
- `/game-data/api/areaItems/main.5.json`
- `/game-data/api/events/all.6.json`
- `/game-data/api/songs/all.7.json`
- `/game-data/api/scoreRangeChartMeta.1.json`：目标 PT 本地搜索模板
- `/game-data/api/events/{eventId}.json`
- `/game-data/api/charts/{songId}/{difficultyName}.json`
- 可选修正文件：
  - `/game-data/cardsCNfix.json`
  - `/game-data/skillsCNfix.json`
  - `/game-data/areaItemFix.json`
  - `/game-data/eventCharacterParameterBonusFix.json`

`difficultyName` 可选值：`easy`、`normal`、`hard`、`expert`、`special`。

## 清单文件（Manifest）

静态镜像应在 `gameDataBaseUrl` 目录下包含 `manifest.json`。

示例：

```json
{
  "version": "2026-06-02T00:00:00Z",
  "generatedAt": "2026-06-02T00:00:00Z",
  "files": {
    "api/cards/all.5.json": {
      "hash": "sha256:...",
      "size": 123
    },
    "api/charts/1/expert.json": {
      "hash": "sha256:...",
      "size": 456
    }
  }
}
```

浏览器会用 `hash`、`etag`、`version` 或 `updatedAt` 与 IndexedDB 记录比对并抓取变化文件。
若文件未在 manifest 列出，运行时仍可按需拉取，但有 manifest 能获得更稳定的变更检测。

## 浏览器同步

静态 UI 位于 `apps/web/index.html`，使用
`apps/web/config.js`、`apps/web/src/main.js`、`apps/web/src/user-storage.js` 与
`apps/web/src/game-data-sync.js`。
在计算或刷新选择器所需参考数据时按需同步核心游戏数据，并将新的
`PlayerConfig` 结构写入 IndexedDB。
JSON 编辑器仅用于对当前 Schema 做直接编辑。

若要在其他 UI 中直接集成，可复用 `apps/web/src/game-data-sync.js`：

```js
import { createGameDataClient } from './src/game-data-sync.js';
import init, { calculateFromStaticData } from './pkg/bangdream_optimize_web_wasm.js';

await init();

const gameData = createGameDataClient({
  baseUrl: '/game-data',
  onProgress: ({ type, path }) => console.log(type, path),
});

const core = await gameData.syncCore({ refreshManifest: true });

const payload = await gameData.buildCalculationPayload({
  player,
  server: 'cn',
  eventId,
  options: {},
  core,
});

const result = JSON.parse(calculateFromStaticData(JSON.stringify(payload)));
```

`syncCore` 会同步共享的选择器/计算数据：

- 核心文件：卡片、角色、技能、area item、活动、歌曲
- 若 manifest 中列出，则同步可选修正文件

`buildCalculationPayload` 继续同步：

- `api/events/{eventId}.json`（可选）
- `player.eventSongs[eventId]` 中出现的每张谱面

## WASM 入口

导出的计算函数为：

```ts
calculateFromStaticData(payloadJson: string): string
```

参数结构：

```ts
type WebCalculationPayload = {
  cards: unknown
  characters: unknown
  skills: unknown
  areaItems: unknown
  cardsFix?: unknown
  skillsFix?: unknown
  areaItemsFix?: unknown
  event: unknown
  songs: Record<string, unknown>
  charts: Array<{
    songId: number
    difficulty: number
    data: unknown
  }>
  player: PlayerConfig
  server: 'jp' | 'en' | 'tw' | 'cn' | 'kr'
  eventId?: number
  options?: ItemSearchOptions
}
```

浏览器可以通过只传入“当前玩家持有卡片 JSON”与“已选谱面 JSON”减少一次性传给 WASM 的数据量。
这并不影响镜像在 IndexedDB 中保持完整。

## 构建 WASM

生产环境按上方 Nginx 部署流程直接构建并发布前端资源：

```bash
cargo install wasm-bindgen-cli
./scripts/build-web-assets.sh
```

`apps/web/pkg` 目录未纳入 Git，需在部署时生成。

## 更新策略

可通过后端启动同步或定时同步更新 `BANGDREAM_OPTIMIZE_GAME_DATA_ROOT`，
或将 `apps/web/config.js` 指向后端/CDN 镜像。
运行时会从配置镜像同步到 IndexedDB。
计算不需要实时后端，浏览器不会直接请求 Bestdori。

更新镜像时请先上传变更 JSON 文件，再更新 `manifest.json`，避免出现 manifest 指向但文件尚未就绪的短暂不一致。

## 镜像生成

使用 Rust CLI：

```bash
cargo run -p bangdream-optimize-sync-bestdori -- \
  --out var/game-data \
  --repair-dir tsugu-bangdream-bot/backend/config \
  --event 100 \
  --chart 1:expert
```

CLI 总是同步以下核心文件：

- `api/cards/all.5.json`
- `api/characters/main.3.json`
- `api/skills/all.10.json`
- `api/areaItems/main.5.json`
- `api/events/all.6.json`
- `api/songs/all.7.json`

同时会为 `api/cards/all.5.json` 列出的每张卡同步
`api/cards/{cardId}.json`，因为卡片列表 API 不包含完整等级属性。
这些数据保留为独立文件，不会合并回 `api/cards/all.5.json`。

可选参数：

- `--event <id>` 或 `--events <id,id>`：抓取 `api/events/{eventId}.json`
- `--chart <songId:difficulty>` 或 `--charts <songId:difficulty,...>`：抓取谱面 JSON
- `--player <player.json>`：读取 `PlayerConfig`，并同步当前活动与引用谱面
- `--all-event-details`：为 `api/events/all.6.json` 中所有活动抓取完整活动详情
- `--all-charts`：抓取 `api/songs/all.7.json` 中所有谱面
- `--concurrency <n>`：并发下载数，默认 `8`
- `--retries <n>`：远端临时错误重试次数，默认 `2`
- `--repair-dir <dir>`：存在时复制 `cardsCNfix.json`、`skillsCNfix.json`、`areaItemFix.json`、`eventCharacterParameterBonusFix.json`

`difficulty` 可为 `0..4` 或 `easy`、`normal`、`hard`、`expert`、`special`。

输出目录会包含 `manifest.json`，其 hash 使用 `sha256:*`。
对外部 Bestdori 文件会同时记录 `etag` 与 `lastModified`。
下一次运行时会读取上次的 manifest，并发送 `If-None-Match`/`If-Modified-Since`，
对 `304 Not Modified` 结果重用本地元数据。
包括 `manifest.json` 在内的所有文件都先写入临时文件再原子重命名。

构建完整静态镜像可执行：

```bash
cargo run -p bangdream-optimize-sync-bestdori -- \
  --out var/game-data \
  --repair-dir tsugu-bangdream-bot/backend/config \
  --all-event-details \
  --all-charts \
  --concurrency 8 \
  --retries 2
```

`var/game-data` 不提交到 Git；生产环境建议由后端挂载该镜像目录并通过 Nginx 反代 `/game-data/`。

CLI 对 `version` 与 `generatedAt` 使用 Unix 秒数。
浏览器同步端只比较 `files` 下的元数据，因此无论是 Unix 秒还是 ISO 时间戳都可用于手工 manifest。
