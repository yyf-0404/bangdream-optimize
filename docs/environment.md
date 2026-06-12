# 环境准备

本文说明从空机器开始构建、运行和打包本项目需要的基础工具。

## 基础要求

- Rust stable toolchain，包含 `cargo`。
- Git，用于拉取项目。
- Python 3，仅本地运行 `scripts/run-web.sh` 时需要。
- Linux 生产部署建议安装 `rsync`、`nginx`、`systemd`。

`cargo` 随 Rust 工具链一起安装，不需要单独安装。

## 安装 Rust 和 Cargo

### Linux / macOS

推荐使用 `rustup`：

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
source "$HOME/.cargo/env"
rustup default stable
rustc --version
cargo --version
```

Ubuntu / Debian 构建基础依赖：

```bash
sudo apt-get update
sudo apt-get install -y build-essential curl git python3 rsync pkg-config libssl-dev
```

### Windows

1. 安装 Visual Studio Build Tools，并勾选 `Desktop development with C++`。
2. 安装 WebView2 Runtime。Windows 11 通常已自带；较旧系统需要单独安装。
3. 从 Rust 官网安装 `rustup-init.exe`，选择默认 stable MSVC 工具链。
4. 打开新的 PowerShell 或 CMD，验证：

```powershell
rustc --version
cargo --version
rustup show
```

Windows 便携 EXE 默认使用：

```powershell
rustup target add x86_64-pc-windows-msvc
```

## Web 生产构建

Web 生产构建需要 WASM 目标和 `wasm-bindgen-cli`：

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
```

然后直接运行：

```bash
./scripts/build-web-assets.sh
```

## 后端构建

后端只需要基础 Rust/Cargo 环境：

```bash
cargo build --release -p bangdream-optimize-server -p bangdream-optimize-sync-bestdori
```

## 桌面端构建

桌面端便携 EXE 使用 Cargo 构建，不需要 Web WASM 包。

Linux 检查或运行 Tauri 桌面壳前，还需要系统 WebView 与 GTK 开发包：

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

Windows 便携 EXE：

```cmd
scripts\package-desktop-windows.bat
```

需要 Tauri 安装包 bundle 时，额外安装 Tauri CLI：

```bash
cargo install tauri-cli --version '^2'
```

## 快速验证

```bash
cargo check --workspace --all-targets
cargo check --manifest-path apps/desktop/src-tauri/Cargo.toml
```

如果提示找不到 `cargo`，通常是 Rust 安装后的 shell 环境还没刷新。关闭并重新打开终端，或执行：

```bash
source "$HOME/.cargo/env"
```
