# bangdream-optimize

BanG Dream 编队与分数计算工具，包含静态网页端 `apps/web`、Tauri 桌面端 `apps/desktop` 和 HTTP API 服务 `apps/server`。

## 本地运行

首次运行需要准备工具链、构建 WASM 和同步游戏数据。见[环境准备](docs/guides/environment.md)和[本地测试手册](docs/guides/internal-testing.md)。

在仓库根目录用两个终端启动后端和网页。默认端口分别为 `3100` 和 `8080`。

```bash
./scripts/run-server.sh
./scripts/run-web.sh
```

已有 `apps/web/pkg` 和 `var/game-data` 时，也可以用 `python scripts/serve-web.py --port 8093` 单独启动网页预览。

## 开发与发布

- [前端开发说明](apps/web/README.md)
- [Web 构建与部署](docs/deployment/web-static.md)
- [桌面打包与发布](docs/deployment/release.md)
- [文档索引](docs/README.md)
- [脚本索引](scripts/README.md)

网页、桌面应用、打包文件名和核心诊断共用一个版本号，即 `apps/web/package.json` 中的 `version`。 [版本配置](apps/web/package.json)
