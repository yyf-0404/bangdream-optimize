# 发布（桌面端）

`bangdream-optimize` 目前保留桌面端的发布流程。

桌面端发布说明：

环境准备先参考：

- `docs/environment.md`

- 构建桌面运行时产物：

```bash
./scripts/package-desktop.sh
```

- 需要 Tauri bundle 时（安装 `cargo-tauri`）：

```bash
BANGDREAM_OPTIMIZE_DESKTOP_BUNDLE=1 ./scripts/package-desktop.sh
```

缺少 Tauri CLI 时：

```bash
cargo install tauri-cli --version '^2'
```

桌面端复用 `apps/web` 作为 `frontendDist`，但运行时通过 Tauri adapter 调用
原生 Rust 命令计算，不依赖浏览器 WASM 包。

发布前校验：

```bash
./scripts/internal-check.sh
```

该命令会校验 JS 语法、Rust 工作区检查、核心求解测试、桌面 crate 测试与 Tauri shell 检查。

### Windows 便携版

Windows 便携版 EXE 打包请使用独立脚本（在 Windows 目标构建环境下执行）。
桌面端使用原生 Rust 命令计算，不需要先构建 Web WASM 包。
Windows 原生命令行推荐使用 `.bat` 入口：

```cmd
scripts\package-desktop-windows.bat
```

可选变量：

- `BANGDREAM_OPTIMIZE_DESKTOP_WINDOWS_TARGET`
  - 默认：`x86_64-pc-windows-msvc`

示例：

```cmd
cd C:\path\to\bangdream-optimize
set BANGDREAM_OPTIMIZE_DESKTOP_WINDOWS_TARGET=x86_64-pc-windows-msvc
scripts\package-desktop-windows.bat
```

PowerShell 也可以直接运行：

```powershell
cd C:\path\to\bangdream-optimize
$env:BANGDREAM_OPTIMIZE_DESKTOP_WINDOWS_TARGET = 'x86_64-pc-windows-msvc'
.\scripts\package-desktop-windows.ps1
```

产物路径示例：

```
apps/desktop/src-tauri/target/<target>/release/bangdream-optimize-desktop-app.exe
```

### 自托管桌面端下载

网站前端会在浏览器运行时显示“下载桌面端”入口。点击后会读取：

```
/downloads/
```

该路径需要由 nginx 暴露为 JSON 目录列表。服务器文件目录不要求固定路径，
只要 nginx 的 `alias` 指向你实际上传 exe 的目录即可。示例：

```nginx
location /downloads/ {
  alias /var/www/bangdream-optimize/downloads/;
  autoindex on;
  autoindex_format json;
  add_header Content-Disposition "attachment";
}
```

Windows 打包后，把不同版本的安装包或便携版手动上传到该目录。前端会按文件
更新时间倒序显示，每个文件一个下载按钮。支持的后缀：

- `.exe`
- `.msi`
- `.zip`
- `.7z`

如果下载目录 URL 不是 `/downloads/`，可以在 `apps/web/config.js` 或部署时注入：

```js
globalThis.BANGDREAM_OPTIMIZE_CONFIG = {
  desktopDownloadsUrl: '/your-download-path/',
};
```
