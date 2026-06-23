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
用户计算或需要参考数据时，浏览器会读取配置的镜像。
计算时还会在 `api/cards/all.5.json` 中缺少选中卡片等级时同步
`api/cards/{cardId}.json`。
用户配置存储在 IndexedDB。
JSON 编辑器仅用于直接编辑当前 schema。

计算结果可含可选 `metrics`。
一次成功计算后，UI 可导出诊断 JSON，包括当前玩家配置、结果、metrics、运行时类型，
若为桌面模式还会附带桌面数据/缓存路径与引用数据计数。
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

## 运行服务

服务 `apps/web/` 静态文件。直接打开 `index.html` 不稳定，因浏览器模块、WASM 与 fetch 需要 HTTP 来源。

先启动后端（3100）：

```bash
./scripts/run-server.sh
```

再启动 Web（8080）：

```bash
./scripts/run-web.sh
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

本地开发时保持端口分离可用：

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
- 前端计算不需要 `/v1`；仅在需要对外暴露后端计算 API 时额外配置 `/v1` 反代。
