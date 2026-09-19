# bangdream-optimize

`bangdream-optimize` 提供 BanG Dream 团队分数计算能力，包含以下 3 个产物：

- 桌面端：本地运行的 Tauri 桌面应用（`apps/desktop`）
- 后端：HTTP API 服务（`apps/server`）
- 网页端：静态前端（`apps/web`）

说明：网页端不依赖 Node 打包工具，克隆仓库后通过脚本构建和启动；生产发布会自动生成带资源版本的静态目录。

默认本地启动方式：

```bash
./scripts/run-server.sh
./scripts/run-web.sh
```

应用版本统一配置在 `apps/web/package.json` 的 `version` 字段；Web 标题、桌面应用、打包文件名
和核心诊断版本均读取该值。

后端默认监听 `http://127.0.0.1:3100`，网页端默认监听 `http://127.0.0.1:8080`。

前端已接入统一的活动、卡牌、道具与角色、结果、档案五页布局。实施及验收记录见
[前端接入记录](docs/frontend-redesign-progress.md)。开发预览也可使用
`python scripts/serve-web.py --port 8093`；仍需已有的 `apps/web/pkg` 和 `var/game-data`。

首次运行前请先准备 Rust/Cargo 等基础环境：

- `docs/environment.md`

Windows 桌面端一键打包并通过 SSH 上传下载目录：先运行
`scripts\publish-desktop-windows.bat -InitConfig` 并填写配置，再运行
`scripts\publish-desktop-windows.bat`。详细参数见 [桌面端发布](docs/release.md#windows-一键打包并上传)。

生产部署建议将前端与后端同域部署，并将 `apiBaseUrl` 置空（如
`globalThis.BANGDREAM_OPTIMIZE_CONFIG = { apiBaseUrl: '' }`），
使前端请求走同源：`/game-data/...`。
所有服务器（含国服）统一通过 Bestdori 导入主乐队公开资料；该流程不读取完整持有卡牌列表。

生产环境静态部署（Nginx）可直接参考：

- `docs/web-static.md`（包含构建、文件部署和同源示例）
- `docs/nginx-reverse-proxy.conf`（可直接安装到 `sites-available`）
- `docs/systemd/bangdream-optimize-backend.service`（后端 systemd 示例服务文件）
- `docs/systemd/bangdream-optimize-backend.env.example`（后端 systemd 环境变量示例）

`docs/nginx-reverse-proxy.conf` 示例已包含：
- `apps/web` 生成的 `target/web-dist` 静态产物托管
- `/game-data/` 的后端反向代理
- `/bestdori/player/` 的同源 API 反向代理
- `/bestdori/header/` 的活动头图 PNG 通道（轻量纹理补全）
- `/api/feedback` 的同源反馈 API 反向代理
- 反馈附件上传所需的 12 MiB 请求体上限
- SPA 回退配置

服务端内部遥测与 API 说明见：
- `docs/telemetry.md`
- `docs/server-api.md`
- `docs/internal-testing.md`
- `docs/web-static.md`
- `docs/persistence.md`（网页/桌面用户配置、结果缓存和游戏数据缓存边界）
