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

同时生成带版本和架构的便携版文件，例如
`bangdream-optimize-desktop-v0.4.3-windows-x64.exe`。PowerShell 脚本固定使用仓库内的
`apps/desktop/src-tauri/target` 输出目录，锁定 Cargo 依赖，默认单任务构建；可用 `-Jobs 2` 调整并发。

### Windows 一键打包并上传

在本地 Windows 仓库根目录运行。脚本调用上述便携版打包流程，上传 EXE 到 Linux 服务器，
不构建网页或后端。需要本机可用的 `cargo`、Windows MSVC 工具链、`ssh` 和 `scp`。

1. 生成本地配置：

   ```powershell
   .\scripts\publish-desktop-windows.bat -InitConfig
   ```

2. 编辑 `scripts/publish-desktop-windows.local.psd1`。该文件已被 Git 忽略。至少填写
   `SshHost` 和 `RemoteDirectory`，其余可以保留默认值：

   ```powershell
   @{
       SshHost = 'my-server'
       SshUser = ''
       SshPort = 0
       IdentityFile = ''
       RemoteDirectory = '/var/www/bangdream-optimize/downloads'
       Target = 'x86_64-pc-windows-msvc'
       BuildJobs = 1
   }
   ```

   `SshHost` 可以直接使用 `%USERPROFILE%\.ssh\config` 中的 Host 别名，例如：

   ```sshconfig
   Host my-server
       HostName your-server-ip
       User root
       Port 22
       IdentityFile ~/.ssh/id_ed25519
   ```

   也可以在 `.psd1` 中分别填写 IP、用户名、端口、密钥文件；空用户名、端口 `0`、空密钥路径
   表示沿用 OpenSSH 配置。相对密钥路径按 `.psd1` 所在目录解析，支持 `~/.ssh/...`。
   使用 SSH 密钥或 ssh-agent 可避免重复输入密码；脚本不保存密码，也不跳过服务器指纹检查。

   `RemoteDirectory` 是 Nginx `location /downloads/` 中 `alias` 对应的**服务器绝对目录**，
   不是 `https://...` 或网页 URL。示例路径需要核对实际 Nginx 配置；路径不能含空格或 `..`。
   SSH 用户需有目录写入权限，脚本不自动提权。目标服务器需有 `sha256sum` 和 GNU `mv`（Ubuntu 默认提供）。

3. 检查路径并执行：

   ```powershell
   .\scripts\publish-desktop-windows.bat -DryRun
   .\scripts\publish-desktop-windows.bat
   ```

   `-DryRun` 只检查配置并显示计划，不构建、不连接服务器。正式运行先检查 SSH 与目录权限，
   再构建当前版本，上传到同目录临时文件，核对 SHA-256 后以 `0644` 权限原子替换同名 EXE。
   其他版本文件保留。失败时不发布未校验的文件，并尝试清理本次临时上传。

可选参数：

- `-ConfigPath C:\path\publish.psd1`：使用另一个配置文件。
- `-SkipBuild`：重试上传已有的当前版本、当前架构 EXE。仅上传失败且源代码未改动时使用；
  它不检查产物是否包含最新代码。正常发布直接运行、不加此参数。
- 配置中的 `Target` 支持 `x86_64-pc-windows-msvc`、`aarch64-pc-windows-msvc`、
  `i686-pc-windows-msvc`，对应架构需要本机安装相应工具链。

若桌面端需要反馈功能，打包前按 `apps/desktop/README.md` 配好 `apps/web/config.desktop.js`，
该配置会随前端资源打入 EXE。此发布脚本不会修改它。

脚本回归检查可运行 `powershell -NoProfile -ExecutionPolicy Bypass -File scripts/test-publish-desktop-windows.ps1`。
检查使用本地模拟 SSH/SCP 和 Bash 执行发布命令，覆盖失败保留旧文件、校验、清理及入口兼容；
需要 Git Bash 或 MSYS2 的 `bash`（也可传 `-Bash` 指定路径），不会连接服务器或执行真实构建。

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

Windows 可用上面的一键上传脚本，或把不同版本的安装包、便携版手动上传到该目录。
已有 `/downloads/` 配置时，上传文件后刷新下载列表即可，不必重启 Nginx。前端会按文件
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
