@echo off
setlocal EnableExtensions EnableDelayedExpansion
title Native Horizon - Release
cd /d "%~dp0."
if errorlevel 1 exit /b 1

rem release.bat                           Build the latest upstream Nightly.
rem release.bat native-YYYY-MM-DD-SHA     Build a particular upstream Nightly.
rem release.bat native-YYYY-MM-DD-SHA rpc2  Publish a separate build of that Nightly.
set "REPO=atlas-thedev/horizon"
set "WORKFLOW=native-build.yml"
set "NIGHTLY=%~1"
set "SUFFIX=%~2"
if not "%~3"=="" goto :usage
if defined SUFFIX if not defined NIGHTLY goto :usage

where gh >nul 2>&1
if errorlevel 1 (
    echo Install GitHub CLI and run gh auth login first.
    goto :failed
)
set "BRANCH="
for /f "delims=" %%B in ('git branch --show-current') do set "BRANCH=%%B"
if not "!BRANCH!"=="main" (
    echo Switch to main before releasing.
    goto :failed
)
git fetch origin main
if errorlevel 1 goto :failed
set "LOCAL="
set "REMOTE="
for /f %%C in ('git rev-parse HEAD') do set "LOCAL=%%C"
for /f %%C in ('git rev-parse refs/remotes/origin/main') do set "REMOTE=%%C"
if not defined LOCAL goto :failed
if not "!LOCAL!"=="!REMOTE!" (
    echo Sync with origin/main using pull.bat or push.bat before releasing.
    goto :failed
)
for /f "delims=" %%I in ('git status --porcelain') do (
    echo Local changes found. Use push.bat to include them in this release.
    goto :failed
)

echo Build Native Horizon for Windows on GitHub Actions from !LOCAL!.
if defined NIGHTLY (
    echo Upstream Nightly: !NIGHTLY!
) else (
    echo Upstream Nightly: newest
)
if defined SUFFIX echo Release suffix: !SUFFIX!
set "GO="
set /p "GO=Start the release? (y/N): "
if /i not "!GO!"=="y" exit /b 0

if defined NIGHTLY (
    gh workflow run "%WORKFLOW%" --repo "%REPO%" --ref main -f "nightly=!NIGHTLY!" -f "suffix=!SUFFIX!"
    if errorlevel 1 goto :failed
) else (
    gh workflow run "%WORKFLOW%" --repo "%REPO%" --ref main
    if errorlevel 1 goto :failed
)
echo Workflow submitted. Existing release tags are skipped.
echo Progress: https://github.com/%REPO%/actions/workflows/%WORKFLOW%
pause
exit /b 0

:usage
echo Usage: release.bat [native-YYYY-MM-DD-SHA [suffix]]
:failed
echo Release stopped.
pause
exit /b 1
