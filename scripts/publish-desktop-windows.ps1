<#
.SYNOPSIS
Build a Windows portable EXE and publish it to a Linux server over SSH.
.EXAMPLE
scripts\publish-desktop-windows.bat -InitConfig
.EXAMPLE
scripts\publish-desktop-windows.bat -DryRun
.EXAMPLE
scripts\publish-desktop-windows.bat
#>
[CmdletBinding()]
param(
    [string] $ConfigPath = '',
    [switch] $InitConfig,
    [switch] $DryRun,
    [switch] $SkipBuild
)

Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
# Windows PowerShell's batch entry may not auto-discover this module's functions.
Import-Module Microsoft.PowerShell.Utility -ErrorAction Stop
$repoRoot = Split-Path -Parent $PSScriptRoot
if ([string]::IsNullOrWhiteSpace($ConfigPath)) {
    $ConfigPath = Join-Path $PSScriptRoot 'publish-desktop-windows.local.psd1'
}

function Invoke-Checked {
    param([string] $Command, [string[]] $Arguments)
    & $Command @Arguments
    if ($LASTEXITCODE -ne 0) {
        throw "$Command failed (exit $LASTEXITCODE)."
    }
}

if ($InitConfig) {
    if (Test-Path -LiteralPath $ConfigPath) {
        throw "Config already exists; edit it directly: $ConfigPath"
    }
    Copy-Item -LiteralPath (Join-Path $PSScriptRoot 'publish-desktop-windows.example.psd1') -Destination $ConfigPath
    Write-Host "Created $ConfigPath"
    Write-Host 'Fill in SshHost and RemoteDirectory, then run this script again.'
    return
}
if (-not (Test-Path -LiteralPath $ConfigPath -PathType Leaf)) {
    throw 'Config not found. Run scripts\publish-desktop-windows.bat -InitConfig first.'
}

$config = Import-PowerShellDataFile -LiteralPath $ConfigPath
$expectedKeys = @('SshHost', 'SshUser', 'SshPort', 'IdentityFile', 'RemoteDirectory', 'Target', 'BuildJobs')
foreach ($key in $config.Keys) {
    if ($key -notin $expectedKeys) { throw "Unknown config setting: $key" }
}
foreach ($key in $expectedKeys) {
    if (-not $config.ContainsKey($key)) { throw "Missing config setting: $key" }
}
$sshHostName = [string] $config.SshHost
$sshUserName = [string] $config.SshUser
if ($sshHostName -notmatch '^[A-Za-z0-9][A-Za-z0-9_.-]*$') {
    throw 'Set SshHost to an SSH alias, IPv4 address, or hostname (without user@ or a port).'
}
if ($sshUserName -and $sshUserName -notmatch '^[A-Za-z0-9_][A-Za-z0-9_.-]*$') {
    throw 'SshUser contains unsupported characters.'
}
if ($config.SshPort -isnot [int] -or $config.SshPort -lt 0 -or $config.SshPort -gt 65535) {
    throw 'SshPort must be an integer from 1 to 65535, or 0 to inherit SSH configuration.'
}
if ($config.BuildJobs -isnot [int] -or $config.BuildJobs -lt 1 -or $config.BuildJobs -gt 64) {
    throw 'BuildJobs must be an integer from 1 to 64.'
}
$remoteDirectory = ([string] $config.RemoteDirectory).TrimEnd('/')
# Keep scp's destination unambiguous for both legacy SCP and SFTP transports.
if ($remoteDirectory -notmatch '^/[A-Za-z0-9_./-]+$' -or
    @($remoteDirectory.Split('/') | Where-Object { $_ -eq '.' -or $_ -eq '..' }).Count -gt 0) {
    throw 'RemoteDirectory must be an absolute Linux path (not /), without spaces, dot segments or shell characters.'
}
$target = [string] $config.Target
$platform = switch ($target) {
    'x86_64-pc-windows-msvc' { 'windows-x64' }
    'aarch64-pc-windows-msvc' { 'windows-arm64' }
    'i686-pc-windows-msvc' { 'windows-x86' }
    default { throw "Unsupported Windows target: $target" }
}
$version = [string] (Get-Content -LiteralPath "$repoRoot/apps/web/package.json" -Raw | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+(?:-[A-Za-z0-9.-]+)?(?:\+[A-Za-z0-9.-]+)?$') {
    throw 'Invalid desktop version in apps/web/package.json.'
}
$fileName = "bangdream-optimize-desktop-v$version-$platform.exe"
$artifact = Join-Path $repoRoot "apps/desktop/src-tauri/target/$target/release/$fileName"
$remoteFile = "$remoteDirectory/$fileName"
$destination = $sshHostName
if ($sshUserName) { $destination = "$sshUserName@$sshHostName" }
$sshArguments = @('-o', 'ConnectTimeout=15', '-o', 'ServerAliveInterval=15', '-o', 'ServerAliveCountMax=3')
$scpArguments = @($sshArguments)
if ($config.SshPort -gt 0) {
    $sshArguments += @('-p', [string] $config.SshPort)
    $scpArguments += @('-P', [string] $config.SshPort)
}
if ($config.IdentityFile) {
    $identityPath = [string] $config.IdentityFile
    if ($identityPath.StartsWith('~/') -or $identityPath.StartsWith('~\')) {
        $identityPath = Join-Path ([Environment]::GetFolderPath('UserProfile')) $identityPath.Substring(2)
    }
    if (-not [IO.Path]::IsPathRooted($identityPath)) {
        $identityPath = Join-Path (Split-Path -Parent (Resolve-Path -LiteralPath $ConfigPath).Path) $identityPath
    }
    $identityPath = (Resolve-Path -LiteralPath $identityPath).Path
    if (-not (Test-Path -LiteralPath $identityPath -PathType Leaf)) { throw 'IdentityFile must be a file.' }
    $sshArguments += @('-i', $identityPath, '-o', 'IdentitiesOnly=yes')
    $scpArguments += @('-i', $identityPath, '-o', 'IdentitiesOnly=yes')
}

Write-Host "Desktop version: $version ($platform)"
Write-Host "Local EXE: $artifact"
Write-Host "Upload to: ${destination}:$remoteFile"
if ($DryRun) {
    Write-Host "Dry run: build=$(-not $SkipBuild), jobs=$($config.BuildJobs). No build, SSH connection or upload performed."
    return
}
foreach ($command in @('ssh', 'scp')) {
    if (-not (Get-Command $command -ErrorAction SilentlyContinue)) {
        throw "$command is missing. Install the Windows OpenSSH Client optional feature."
    }
}
if ($SkipBuild -and -not (Test-Path -LiteralPath $artifact -PathType Leaf)) {
    throw "No package for the current version/target: $artifact. Run without -SkipBuild."
}

Write-Host 'Checking SSH connection and download directory...'
# No sudo or service reload: this account must already have directory access.
$preflight = "set -eu; command -v sha256sum >/dev/null; mkdir -p -- '$remoteDirectory'; test -d '$remoteDirectory'; test -w '$remoteDirectory'"
Invoke-Checked 'ssh' ($sshArguments + @($destination, $preflight))

if (-not $SkipBuild) {
    & (Join-Path $PSScriptRoot 'package-desktop-windows.ps1') -Target $target -Jobs $config.BuildJobs
}
if (-not (Test-Path -LiteralPath $artifact -PathType Leaf) -or (Get-Item -LiteralPath $artifact).Length -eq 0) {
    throw "Build did not produce a non-empty package: $artifact"
}
$checksum = (Get-FileHash -LiteralPath $artifact -Algorithm SHA256).Hash.ToLowerInvariant()
$temporaryFile = "$remoteDirectory/.$fileName.upload-$([guid]::NewGuid().ToString('N'))"
Write-Host "SHA-256: $checksum"
Write-Host 'Uploading portable EXE...'
try {
    Invoke-Checked 'scp' ($scpArguments + @($artifact, "${destination}:$temporaryFile"))
    # Publish only after verification; a failed transfer leaves the old EXE intact.
    $publish = @'
set -eu
printf '%s  %s\n' '__HASH__' '__TEMP__' | sha256sum --check --status
chmod 0644 -- '__TEMP__'
mv -fT -- '__TEMP__' '__FINAL__'
'@
    $publish = $publish.Replace('__HASH__', $checksum).Replace('__TEMP__', $temporaryFile).Replace('__FINAL__', $remoteFile)
    Invoke-Checked 'ssh' ($sshArguments + @($destination, ($publish -replace "`r", '')))
}
catch {
    $failure = $_
    try {
        Invoke-Checked 'ssh' ($sshArguments + @($destination, "rm -f -- '$temporaryFile'"))
    }
    catch {
        Write-Warning "Could not remove partial upload: $temporaryFile"
    }
    throw $failure
}
Write-Host "Published: ${destination}:$remoteFile"
Write-Host 'Refresh the website download list. No Nginx restart is needed for an existing /downloads/ location.'
