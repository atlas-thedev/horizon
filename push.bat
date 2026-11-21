@echo off
setlocal EnableExtensions EnableDelayedExpansion
title Native Horizon - Push
cd /d "%~dp0."
if errorlevel 1 exit /b 1

for /f "delims=" %%B in ('git branch --show-current') do set "BRANCH=%%B"
if /i not "!BRANCH!"=="main" (
    echo Switch to main before pushing.
    goto :failed
)
git fetch origin main
if errorlevel 1 goto :failed
git merge-base --is-ancestor origin/main HEAD
if errorlevel 1 (
    echo Pull and merge the latest main before pushing.
    goto :failed
)

set "HAS_CHANGES="
for /f "delims=" %%I in ('git status --porcelain') do set "HAS_CHANGES=1"
if defined HAS_CHANGES (
    git status --short
    set "MESSAGE="
    set /p "MESSAGE=Commit message ^(empty cancels^): "
    if not defined MESSAGE exit /b 0
    git add -A
    if errorlevel 1 goto :failed
    git commit -m "!MESSAGE!"
    if errorlevel 1 goto :failed
)

git push --no-follow-tags origin HEAD:main
if errorlevel 1 goto :failed
pause
exit /b 0

:failed
echo Push stopped. Review Git's message above.
pause
exit /b 1
