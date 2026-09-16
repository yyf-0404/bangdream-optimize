# Integration checks with local ssh/scp doubles. No network or real build.
# Requires PowerShell 5.1+ and Git Bash / MSYS2 Bash for the remote shell commands.
[CmdletBinding()]
param([string] $Bash = 'bash')
Set-StrictMode -Version Latest
$ErrorActionPreference = 'Stop'
$bashCommand = (Get-Command $Bash -ErrorAction Stop).Source
$repoRoot = Split-Path -Parent $PSScriptRoot
$testRoot = Join-Path $repoRoot "tmp/publish-test-$([guid]::NewGuid().ToString('N'))"
$fixtureScripts = Join-Path $testRoot 'scripts'
$remoteDirectory = Join-Path $testRoot 'downloads'
$target = 'x86_64-pc-windows-msvc'
$fileName = 'bangdream-optimize-desktop-v1.2.3-windows-x64.exe'
$artifact = Join-Path $testRoot "apps/desktop/src-tauri/target/$target/release/$fileName"
$finalFile = Join-Path $remoteDirectory $fileName
$remoteUnix = $remoteDirectory.Replace('\', '/') -replace '^([A-Za-z]):', '/$1'
[void] (New-Item -ItemType Directory -Force -Path $fixtureScripts, "$testRoot/apps/web", (Split-Path $artifact), $remoteDirectory)
Copy-Item -LiteralPath "$PSScriptRoot/publish-desktop-windows.ps1", "$PSScriptRoot/publish-desktop-windows.example.psd1", "$PSScriptRoot/publish-desktop-windows.bat" -Destination $fixtureScripts
Set-Content -LiteralPath "$testRoot/apps/web/package.json" -Value '{"version":"1.2.3"}'
$entry = Join-Path $fixtureScripts 'publish-desktop-windows.ps1'
$configFile = Join-Path $fixtureScripts 'publish-desktop-windows.local.psd1'
$script:testState = @{
    Calls = [Collections.Generic.List[object]]::new()
    Failure = ''
    Artifact = $artifact
    RemoteDirectory = $remoteDirectory
    BuildCount = 0
}

# These functions shadow the real clients only inside this test script.
function ssh {
    $nativeArgs = @($args)
    $testState.Calls.Add(@{ Command = 'ssh'; Arguments = $nativeArgs })
    if ($testState.Failure -eq 'connection') { $global:LASTEXITCODE = 255; return }
    & $bashCommand --noprofile --norc -c $nativeArgs[-1]
    $global:LASTEXITCODE = $LASTEXITCODE
}
function scp {
    $nativeArgs = @($args)
    $testState.Calls.Add(@{ Command = 'scp'; Arguments = $nativeArgs })
    $temporaryName = ($nativeArgs[-1] -split '/')[-1]
    $temporaryLocal = Join-Path $testState.RemoteDirectory $temporaryName
    if ($testState.Failure -in @('transfer', 'checksum')) {
        Set-Content -LiteralPath $temporaryLocal -Value 'broken upload'
    }
    else { Copy-Item -LiteralPath $nativeArgs[-2] -Destination $temporaryLocal }
    $global:LASTEXITCODE = 0
    if ($testState.Failure -eq 'transfer') { $global:LASTEXITCODE = 1 }
}
@'
param($Target, $Jobs)
if ($Target -ne 'x86_64-pc-windows-msvc' -or $Jobs -ne 1) { throw 'Wrong build settings' }
$testState.BuildCount++
if ($testState.Failure -eq 'build') { throw 'Simulated build failure' }
Set-Content -LiteralPath $testState.Artifact -Value 'fresh build'
'@ | Set-Content -LiteralPath "$fixtureScripts/package-desktop-windows.ps1"

function Assert-True($Condition, [string] $Message) {
    if (-not $Condition) { throw "Assertion failed: $Message" }
}
function Assert-Fails([scriptblock] $Action, [string] $MessagePart) {
    $caught = $null
    try { & $Action | Out-Null } catch { $caught = $_ }
    Assert-True ($null -ne $caught) 'Expected failure'
    Assert-True ($caught.ToString().Contains($MessagePart)) "Unexpected error: $caught"
}
function Write-TestConfig([string] $Directory = $remoteUnix) {
    @"
@{
    SshHost = 'test-server'
    SshUser = 'uploader'
    SshPort = 2222
    IdentityFile = ''
    RemoteDirectory = '$Directory'
    Target = '$target'
    BuildJobs = 1
}
"@ | Set-Content -LiteralPath $configFile
}
function Reset-Case([string] $Failure = '') {
    $testState.Calls.Clear()
    $testState.Failure = $Failure
    $testState.BuildCount = 0
    Write-TestConfig
    Set-Content -LiteralPath $artifact -Value 'new package'
    Set-Content -LiteralPath $finalFile -Value 'previous public package'
}
function Assert-NoPartialFiles {
    Assert-True (@(Get-ChildItem -LiteralPath $remoteDirectory -Force -Filter '*.upload-*').Count -eq 0) 'Partial upload was not cleaned up'
}

# Exercise the actual -File entry too: PSScriptRoot is unavailable in parameter
# defaults in Windows PowerShell 5.1, although invocation with & may work.
& powershell.exe -NoProfile -ExecutionPolicy Bypass -File $entry -InitConfig
Assert-True ($LASTEXITCODE -eq 0) 'CLI initialization failed'
$initialConfig = Get-Content -LiteralPath $configFile -Raw
Assert-Fails { & $entry -InitConfig } 'already exists'
Assert-True ((Get-Content -LiteralPath $configFile -Raw) -eq $initialConfig) 'Init overwrote config'
Write-Host 'PASS: initialize config without overwriting'

Reset-Case
Push-Location $testRoot
try {
    & (Join-Path $fixtureScripts 'publish-desktop-windows.bat') -DryRun
    Assert-True ($LASTEXITCODE -eq 0) 'Batch entry failed from another working directory'
}
finally { Pop-Location }
& $entry -DryRun
Assert-True ($testState.Calls.Count -eq 0 -and $testState.BuildCount -eq 0) 'Dry run performed work'
foreach ($invalidPath in @('/', '/tmp/../etc', '/tmp/a b', '/tmp/x;touch-bad', 'https://example/downloads')) {
    Write-TestConfig $invalidPath
    Assert-Fails { & $entry -DryRun } 'RemoteDirectory must'
}
Assert-True ($testState.Calls.Count -eq 0) 'Invalid path connected to server'
Write-Host 'PASS: dry run and invalid destination guards'

Reset-Case
# Missing current-version package must not upload another old EXE.
Move-Item -LiteralPath $artifact -Destination "$artifact.old"
Assert-Fails { & $entry -SkipBuild } 'No package for the current version'
Assert-True ($testState.Calls.Count -eq 0) 'Missing artifact connected to server'
Write-Host 'PASS: exact version/architecture selection'

Reset-Case 'connection'
Assert-Fails { & $entry } 'ssh failed'
Assert-True ($testState.BuildCount -eq 0) 'Connection failure still built the app'
Write-Host 'PASS: connection failure stops before build'

Reset-Case 'build'
Assert-Fails { & $entry } 'Simulated build failure'
Assert-True (@($testState.Calls | Where-Object Command -eq 'scp').Count -eq 0) 'Failed build uploaded a stale EXE'
Write-Host 'PASS: failed build never uploads stale output'

foreach ($failure in @('transfer', 'checksum')) {
    Reset-Case $failure
    Assert-Fails { & $entry -SkipBuild } 'failed'
    Assert-True ((Get-Content -LiteralPath $finalFile -Raw).Trim() -eq 'previous public package') 'Failure replaced the live EXE'
    Assert-NoPartialFiles
    Write-Host "PASS: $failure failure preserves old file and cleans partial upload"
}

Reset-Case
Set-Content -LiteralPath "$remoteDirectory/another-version.exe" -Value 'keep this version'
& $entry
Assert-True ($testState.BuildCount -eq 1) 'Build was not invoked'
Assert-True ((Get-Content -LiteralPath $finalFile -Raw).Trim() -eq 'fresh build') 'Published wrong package'
Assert-True ((Get-FileHash $artifact).Hash -eq (Get-FileHash $finalFile).Hash) 'Published bytes differ'
Assert-True ((Get-Content -LiteralPath "$remoteDirectory/another-version.exe" -Raw).Trim() -eq 'keep this version') 'Deleted older version'
Assert-NoPartialFiles
$upload = @($testState.Calls | Where-Object Command -eq 'scp')[0]
Assert-True ($upload.Arguments -ccontains '-P') 'scp did not receive uppercase port flag'
Assert-True ($testState.Calls[0].Arguments -ccontains '-p') 'ssh did not receive lowercase port flag'
Assert-True ($upload.Arguments[-1].StartsWith('uploader@test-server:')) 'Wrong SSH destination'
Write-Host 'PASS: build, transfer, checksum and atomic replacement'

Reset-Case
& $entry -SkipBuild
Assert-True ($testState.BuildCount -eq 0) 'SkipBuild invoked build'
Assert-True ((Get-Content -LiteralPath $finalFile -Raw).Trim() -eq 'new package') 'Retry uploaded wrong artifact'
Assert-NoPartialFiles
Write-Host 'PASS: upload retry without rebuilding'
Write-Host "All publish checks passed. Local fixtures retained at $testRoot"
