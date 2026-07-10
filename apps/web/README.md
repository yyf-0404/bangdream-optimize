# bangdream-optimize Web

该目录为静态 web 目标。

运行模型：

- web 服务器、后端或 CDN 可托管静态 game-data 镜像；
- 浏览器会按需刷新 `${gameDataBaseUrl}/manifest.json` 并同步变更文件到 IndexedDB；
- 用户数据保存在 IndexedDB；
- `bangdream-optimize-web-wasm` 在浏览器本地执行计算；
- 桌面壳复用同一套 UI，并切换到 Tauri runtime adapter，调用原生 Rust 命令，而不是浏览器的 IndexedDB/WASM。

Web 启动时读取 `apps/web/config.js`。
默认指向同源 `/game-data`。
本地运行时 `./scripts/run-web.sh` 会服务 `apps/web`，并将 `/game-data` 映射到 `var/game-data`。
后端在 3100 端口同样提供其 `/game-data`，来源于同一镜像。
最高得分计算或需要参考数据时，浏览器会读取配置的镜像。
目标 PT 搜索按需同步 `api/scoreRangeChartMeta.1.json`，并使用界面选择的 `0.5/0.75` Auto
倍率在 Web Worker 中由 WASM 本地计算；
该文件只保存各技能时长下的激活/未激活节点数和尾部风险，不包含服务器可用性。
活动选择器同时列出受支持的单曲与组曲活动，并根据所选活动自动确定歌曲数量。
目标 PT 搜索固定只请求首个全局最优方案。
服务器与浏览器都从 `api/songs/all.7.json` 判断歌曲和难度是否已在目标服务器发布。
桌面端通过 Tauri 原生命令读取同一模板文件。
计算时还会在 `api/cards/all.5.json` 中缺少选中卡片等级时同步
`api/cards/{cardId}.json`。
用户配置存储在 IndexedDB。
JSON 编辑器仅用于直接编辑当前 schema。

计算结果可含可选 `metrics`。
计算成功后，UI 可导出包含当前玩家配置、结果、metrics 和运行时类型的诊断 JSON。
计算执行失败时会进入结果页，并生成包含错误信息与调用栈的失败诊断；若为桌面模式，
诊断还会附带桌面数据/缓存路径与引用数据计数。
该导出为手动操作，仅用于内部测试。

## 构建 WASM

安装 `wasm-bindgen-cli`：

```bash
cargo install wasm-bindgen-cli
```

构建浏览器包：

```bash
./scripts/build-web-assets.sh --no-deploy
```

脚本输出到 `apps/web/pkg`。

## 生成游戏数据

生成静态镜像：

```bash
cargo run -p bangdream-optimize-sync-bestdori -- \
  --out var/game-data \
  --repair-dir tsugu-bangdream-bot/backend/config \
  --event 100 \
  --chart 1:expert
```

生成用于定时部署的完整镜像：

```bash
cargo run -p bangdream-optimize-sync-bestdori -- \
  --out var/game-data \
  --repair-dir tsugu-bangdream-bot/backend/config \
  --all-event-details \
  --all-charts \
  --concurrency 8 \
  --retries 2
```

已有完整谱面目录时，可不联网重建目标 PT 模板及 manifest：

```bash
cargo run -p bangdream-optimize-sync-bestdori -- \
  --out var/game-data \
  --generate-score-range-meta-only
```

## 运行服务

服务 `apps/web/` 静态文件。直接打开 `index.html` 不稳定，因浏览器模块、WASM 与 fetch 需要 HTTP 来源。

仅使用本地计算时直接启动 Web：

```bash
./scripts/run-web.sh
```

需要账号导入 API 或由后端托管 `/game-data` 时，再启动后端：

```bash
./scripts/run-server.sh
```

然后打开 `http://127.0.0.1:8080`。

此时：

- Web UI：`http://127.0.0.1:8080`
- Web 镜像：`http://127.0.0.1:8080/game-data`
- 后端镜像：`http://127.0.0.1:3100/game-data`

生产环境建议同源配置（推荐通过反向代理）：

```js
globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  gameDataBaseUrl: '/game-data',
  apiBaseUrl: '',
};
```

仓库默认在 `127.0.0.1:8080` 自动连接 `127.0.0.1:3100`。其他跨端口开发环境可显式配置：

```js
globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  gameDataBaseUrl: '/game-data',
  apiBaseUrl: 'http://127.0.0.1:3100',
};
```

生产反向代理示例（Nginx）：

```nginx
server {
  server_name example.com;
  root /path/to/apps/web;
  index index.html;

  location /game-data {
    proxy_pass http://127.0.0.1:3100;
  }

  location /bestdori/player {
    proxy_pass http://127.0.0.1:3100;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
  }

  location = /bangdream/user-data/import {
    proxy_pass http://127.0.0.1:3100;
    proxy_set_header Host $host;
    proxy_set_header X-Real-IP $remote_addr;
  }

  location / {
    try_files $uri $uri/ /index.html;
  }
}
```

也可直接使用 `docs/nginx-reverse-proxy.conf` 中的可编辑反向代理示例。

上线注意：
- 前端配置为 `gameDataBaseUrl: '/game-data'`，`apiBaseUrl: ''`，确保请求为同源，不写死 `127.0.0.1`。
- 国服游戏账号导入启用时，保留 `/bangdream/user-data/import` 到后端的反代。
- 最高得分与目标 PT 搜索均在本地 WASM 中执行，不需要 `/v1` 计算路由。
