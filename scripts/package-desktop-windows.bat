@echo off
setlocal

set "SCRIPT_DIR=%~dp0"
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%SCRIPT_DIR%release\package-desktop-windows.ps1" %*
exit /b %ERRORLEVEL%
