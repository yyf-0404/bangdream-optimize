# 脚本索引

下列命令从仓库根目录运行。Windows 可以用 Git Bash 运行 `.sh`，用 PowerShell 或 CMD 运行 `.bat`。

## 本地运行

| 命令 | 用途 |
| --- | --- |
| `./scripts/run-web.sh` | 启动网页预览，默认端口 `8080` |
| `python scripts/serve-web.py --port 8093` | 用已有的 WASM 和游戏数据启动网页预览 |
| `./scripts/run-server.sh` | 启动后端，默认端口 `3100` |
| `./scripts/sync-game-data.sh` | 生成或更新本地游戏数据镜像 |
| `./scripts/internal-check.sh` | 运行前端、打包脚本和 Rust 检查 |
| `./scripts/server-smoke.sh` | 检查已运行的后端 |

环境变量、数据同步和 smoke 参数见[内部测试手册](../docs/guides/internal-testing.md)。

## 构建与发布

| 命令 | 用途 |
| --- | --- |
| `./scripts/build-web-assets.sh --no-deploy` | 构建生产静态产物到 `target/web-dist` |
| `./scripts/update-production.sh --help` | 查看生产更新脚本的参数 |
| `./scripts/package-desktop.sh` | 构建桌面运行时产物 |
| `./scripts/package-desktop-windows.sh` 或 `.\scripts\package-desktop-windows.bat` | 打包 Windows 便携版 |
| `.\scripts\publish-desktop-windows.bat` | 打包 Windows 便携版并上传 |

Windows 上传配置在 `scripts/publish-desktop-windows.local.psd1`。初始化、预演和上传选项见[桌面发布手册](../docs/deployment/release.md#windows-一键打包并上传)。线上更新见[Web 部署手册](../docs/deployment/web-static.md#更新策略)。

## 专项工具

- [checks/](checks/) 存放档案格式、发布产物、Windows 上传和头图代理的专项检查。
- [design/](design/) 存放设计稿导入、图标生成和页面截图对照工具。截图命令见[界面开发规范](../apps/web/README.md#界面开发规范)。
- [release/](release/) 存放 Web 发布产物生成器、Windows 打包与上传的 PowerShell 实现和配置示例。
