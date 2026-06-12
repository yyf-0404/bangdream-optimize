# bangdream-optimize

`bangdream-optimize` 提供 BanG Dream 团队分数计算能力，包含以下 3 个产物：

- 桌面端：本地运行的 Tauri 桌面应用（`apps/desktop`）
- 后端：HTTP API 服务（`apps/server`）
- 网页端：静态前端（`apps/web`）

说明：网页端与后端不走额外打包流程，仅需克隆仓库后运行脚本即可启动。

默认本地启动方式：

```bash
./scripts/run-server.sh
./scripts/run-web.sh
```

后端默认监听 `http://127.0.0.1:3100`，网页端默认监听 `http://127.0.0.1:8080`。

首次运行前请先准备 Rust/Cargo 等基础环境：

- `docs/environment.md`

生产部署建议将前端与后端同域部署，并将 `apiBaseUrl` 置空（如
`globalThis.BANGDREAM_OPTIMIZE_CONFIG = { apiBaseUrl: '' }`），
使前端请求走同源：`/game-data/...`。

生产环境静态部署（Nginx）可直接参考：

- `docs/web-static.md`（包含构建、文件部署和同源示例）
- `docs/nginx-reverse-proxy.conf`（可直接安装到 `sites-available`）
- `docs/systemd/bangdream-optimize-backend.service`（后端 systemd 示例服务文件）
- `docs/systemd/bangdream-optimize-backend.env.example`（后端 systemd 环境变量示例）

`docs/nginx-reverse-proxy.conf` 示例已包含：
- `apps/web` 的静态托管
- `/game-data/` 的后端反向代理
- `/bestdori/player/` 的同源 API 反向代理
- SPA 回退配置

服务端内部遥测与 API 说明见：
- `docs/telemetry.md`
- `docs/server-api.md`
- `docs/internal-testing.md`
- `docs/web-static.md`
