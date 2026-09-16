@echo off
setlocal
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0publish-desktop-windows.ps1" %*
exit /b %ERRORLEVEL%
