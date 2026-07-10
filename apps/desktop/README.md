# bangdream-optimize Desktop

桌面应用是围绕共享 `apps/web` UI 的 Tauri 壳。

运行模型：

- `apps/desktop/src-tauri` 通过 Tauri 的 `frontendDist` 加载 `apps/web/`。
- `apps/web/src/runtime/index.js` 检测到 Tauri 时会选择桌面运行时适配器。
- 浏览器模式仍使用 IndexedDB 与 WASM。
- 桌面模式调用 Tauri 命令，底层由 `crates/desktop` 提供。
- 共享 UI 在计算后可导出诊断 JSON；桌面模式下会附带桌面运行时类型、本地用户数据根目录及 game-data/cache 路径。

游戏数据来源：

- 设置 `BANGDREAM_OPTIMIZE_DESKTOP_GAME_DATA_ROOT` 或 `BANGDREAM_OPTIMIZE_GAME_DATA_ROOT` 使用本地静态镜像目录。
- 或设置 `BANGDREAM_OPTIMIZE_DESKTOP_GAME_DATA_BASE_URL` 或 `BANGDREAM_OPTIMIZE_GAME_DATA_BASE_URL` 将静态镜像同步到桌面应用数据缓存。
- 若均未设置，本地开发默认回退到 `var/game-data`。
- 目标 PT 搜索不从 Bestdori 请求 `api/scoreRangeChartMeta.1.json`。桌面端首次搜索时同步
  当前服务器已发布的谱面并在本地缓存生成该文件；“同步全部游戏数据”会生成完整模板。

桌面端游戏账号导入地址保存在 Git 忽略的 `../web/config.desktop.js`。首次配置时复制
`../web/config.desktop.example.js` 并填写 `bangDreamImportApiBaseUrl`；桌面运行时会按需加载，
浏览器端不会读取该文件。

Tauri crate 有意不加入根 Cargo 工作区，以避免常规工作区测试下载或编译 Tauri。

基础 Rust/Cargo、Windows 工具链和发布工具安装见 `../../docs/environment.md`。

## Ubuntu 22.04 依赖

在检查或运行 Tauri 应用前先安装 Linux WebView 与 GTK 开发包：

```bash
sudo apt-get install -y \
  build-essential \
  curl \
  wget \
  file \
  libssl-dev \
  libglib2.0-dev \
  libgtk-3-dev \
  libwebkit2gtk-4.1-dev \
  libayatana-appindicator3-dev \
  librsvg2-dev \
  libxdo-dev
```

然后验证桌面壳：

```bash
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
```

开发时运行桌面壳：

```bash
BANGDREAM_OPTIMIZE_DESKTOP_GAME_DATA_ROOT=var/game-data \
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml
```

使用静态镜像替代本地文件镜像：

```bash
BANGDREAM_OPTIMIZE_DESKTOP_GAME_DATA_BASE_URL=http://127.0.0.1:3100/game-data \
BANGDREAM_OPTIMIZE_DESKTOP_GAME_DATA_CACHE_ROOT=var/desktop-game-data-cache \
cargo run --manifest-path apps/desktop/src-tauri/Cargo.toml
```
