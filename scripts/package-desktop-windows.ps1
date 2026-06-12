Set-StrictMode -Version Latest
$ErrorActionPreference = "Stop"

function Invoke-Native {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Command,

        [string[]] $Arguments = @()
    )

    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command exited with code $LASTEXITCODE"
    }
}

$repoRoot = Split-Path -Parent $PSScriptRoot
Set-Location $repoRoot

function Ensure-RustTarget {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Target
    )

    if (Get-Command rustup -ErrorAction SilentlyContinue) {
        $installedTargets = @(rustup target list --installed)
        if ($installedTargets -notcontains $Target) {
            Write-Host "installing Rust target: $Target"
            Invoke-Native -Command "rustup" -Arguments @("target", "add", $Target)
        }
    }
    else {
        Write-Warning "rustup not found; assuming Rust target $Target is already installed."
    }
}

function Get-DesktopVersion {
    $config = Get-Content -LiteralPath "apps/desktop/src-tauri/tauri.conf.json" -Raw | ConvertFrom-Json
    return [string] $config.version
}

function Get-PlatformName {
    param(
        [Parameter(Mandatory = $true)]
        [string] $Target
    )

    switch ($Target) {
        "x86_64-pc-windows-msvc" { return "windows-x64" }
        "aarch64-pc-windows-msvc" { return "windows-arm64" }
        "i686-pc-windows-msvc" { return "windows-x86" }
        default {
            return ($Target -replace "[^A-Za-z0-9]+", "-").Trim("-").ToLowerInvariant()
        }
    }
}

$target = $env:BANGDREAM_OPTIMIZE_DESKTOP_WINDOWS_TARGET
if ([string]::IsNullOrWhiteSpace($target)) {
    $target = "x86_64-pc-windows-msvc"
}

$binaryName = "bangdream-optimize-desktop-app"
$packageName = "bangdream-optimize-desktop"
$version = Get-DesktopVersion
$platformName = Get-PlatformName $target
$extension = ""
if ($target -like "*-pc-windows-*") {
    $extension = ".exe"
}

Ensure-RustTarget $target

$outputBinary = "apps/desktop/src-tauri/target/$target/release/$binaryName$extension"
$packageBinary = "apps/desktop/src-tauri/target/$target/release/$packageName-v$version-$platformName$extension"
Write-Host "packaging desktop binary for windows target: $target"
Write-Host "output binary: $outputBinary"
Write-Host "package binary: $packageBinary"

Invoke-Native -Command "cargo" -Arguments @(
    "build",
    "--manifest-path",
    "apps/desktop/src-tauri/Cargo.toml",
    "--release",
    "--target",
    $target
)

if (-not (Test-Path -LiteralPath $outputBinary -PathType Leaf)) {
    throw "packaging failed: expected binary not found at $outputBinary"
}

Copy-Item -LiteralPath $outputBinary -Destination $packageBinary -Force

Write-Host "desktop binary ready: $outputBinary"
Write-Host "desktop package ready: $packageBinary"
