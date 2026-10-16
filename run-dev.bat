@echo off
setlocal enableextensions
title OnScreen Widget (dev)

REM ============================================================
REM  OnScreen Widget launcher (dev mode)
REM  Loads the Visual Studio C++ build environment (so the Rust
REM  linker can find msvcrt.lib) and then starts the app.
REM
REM  Double-click to run. The window always stays open at the end
REM  so you can read any error. Use "run-dev.bat --check" to verify
REM  the environment without starting the app.
REM ============================================================

cd /d "%~dp0"

REM CPU / motherboard / RAM temperatures need administrator rights (the sensor
REM driver). Relaunch elevated unless told not to (--no-admin) or just checking.
net session >nul 2>&1
if errorlevel 1 (
  if /i not "%~1"=="--check" if /i not "%~1"=="--no-admin" if /i not "%~1"=="--elevated" (
    echo Requesting administrator rights for CPU temperature sensors...
    powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -ArgumentList '--elevated' -Verb RunAs"
    exit /b
  )
)

echo ============================================
echo   OnScreen Widget - starting dev build
echo ============================================
echo.

REM --- 1. Find the MSVC environment script (skip if already loaded) ---
where link.exe >nul 2>nul
if not errorlevel 1 if defined LIB goto :env_ready

set "VCVARS="
set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if exist "%VSWHERE%" (
  for /f "usebackq delims=" %%i in (`"%VSWHERE%" -latest -products * -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 -property installationPath`) do set "VCVARS=%%i\VC\Auxiliary\Build\vcvarsall.bat"
)

REM Fallback: scan the standard install folders (covers newer VS layouts).
if not defined VCVARS call :scan_vs

if not defined VCVARS (
  echo [ERROR] Could not find Visual Studio with the C++ toolset.
  echo         Install the "Desktop development with C++" workload.
  goto :fail
)
if not exist "%VCVARS%" (
  echo [ERROR] vcvarsall.bat not found at:
  echo         %VCVARS%
  goto :fail
)

REM --- 2. Load the x64 MSVC environment (sets LIB/INCLUDE/PATH) ---
echo Loading C++ build environment...
call "%VCVARS%" x64 >nul
if errorlevel 1 (
  echo [ERROR] Failed to initialize the MSVC environment.
  goto :fail
)

:env_ready
where cargo >nul 2>nul
if errorlevel 1 (
  echo [ERROR] cargo not found on PATH. Install Rust from https://rustup.rs
  goto :fail
)
where npm >nul 2>nul
if errorlevel 1 (
  echo [ERROR] npm not found on PATH. Install Node.js.
  goto :fail
)

if /i "%~1"=="--check" (
  echo Environment OK.
  goto :done
)

REM --- 3. Install JS deps on first run ---
if not exist "node_modules" (
  echo Installing npm dependencies...
  call npm install
  if errorlevel 1 goto :fail
)

REM --- 4. Run the app in dev mode ---
REM Close a leftover widget / sensor helper first (they lock the build output).
taskkill /f /im onscreen-widget.exe >nul 2>&1
taskkill /f /im sensor-helper.exe >nul 2>&1
timeout /t 2 /nobreak >nul
netstat -ano | findstr /R /C:":1420 .*LISTENING" >nul
if not errorlevel 1 (
  echo [ERROR] Port 1420 is already in use - a previous dev session is still
  echo         running. Close its widget / terminal window, then try again.
  goto :fail
)
echo Starting OnScreen Widget (npm run tauri:dev)...
echo.
call npm run tauri:dev
if errorlevel 1 (
  echo.
  echo [ERROR] The app exited with an error. See the output above.
  echo         If you see "cannot open file ...exe", the widget is still
  echo         running - quit it from the tray icon first.
  goto :fail
)
goto :done

:scan_vs
for %%Y in (18 2022 2019 17) do (
  for %%E in (Community Professional Enterprise BuildTools) do (
    for %%P in ("%ProgramFiles%" "%ProgramFiles(x86)%") do (
      if not defined VCVARS if exist "%%~P\Microsoft Visual Studio\%%Y\%%E\VC\Auxiliary\Build\vcvarsall.bat" set "VCVARS=%%~P\Microsoft Visual Studio\%%Y\%%E\VC\Auxiliary\Build\vcvarsall.bat"
    )
  )
)
exit /b 0

:fail
echo.
echo Launch failed. Press any key to close.
pause >nul
endlocal
exit /b 1

:done
echo.
echo Finished. Press any key to close.
pause >nul
endlocal
