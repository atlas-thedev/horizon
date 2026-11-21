@echo off
setlocal
title Native Horizon - Pull
cd /d "%~dp0."
if errorlevel 1 exit /b 1

git switch main
if errorlevel 1 goto :failed
git pull --tags --ff-only origin main
if errorlevel 1 goto :failed
git log -1 --oneline
pause
exit /b 0

:failed
echo Pull stopped. Review Git's message above. Your local changes are preserved.
pause
exit /b 1
