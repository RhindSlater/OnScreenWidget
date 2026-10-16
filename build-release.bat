@echo off
setlocal enableextensions

REM ============================================================
REM  OnScreen Widget - build a standalone release
REM  Produces a native .exe + installer under
REM  src-tauri\target\release\ (and the bundle\ subfolder).
REM
REM  After this succeeds you can run the app directly from the
REM  built .exe WITHOUT this script or the dev environment.
REM ============================================================

cd /d "%~dp0"

echo ============================================
echo   OnScreen Widget - release build
echo ============================================
echo.

REM --- Close any running widget / sensor helper (they lock the build output) ---
set "TRIES=0"
:kill_loop
taskkill /f /im onscreen-widget.exe >nul 2>&1
taskkill /f /im sensor-helper.exe >nul 2>&1
timeout /t 1 /nobreak >nul
tasklist /fi "imagename eq onscreen-widget.exe" /nh 2>nul | find /i "onscreen-widget.exe" >nul
if errorlevel 1 goto :killed
set /a TRIES+=1
if %TRIES% lss 8 goto :kill_loop

REM Still alive after ~8s: it is probably elevated. Retry from an elevated prompt.
net session >nul 2>&1
if errorlevel 1 if /i not "%~1"=="--elevated" (
  echo Widget is running as administrator. Requesting admin rights to close it...
  powershell -NoProfile -Command "Start-Process -FilePath '%~f0' -ArgumentList '--elevated' -Verb RunAs"
  exit /b
)
echo [WARNING] onscreen-widget.exe may still be running; continuing anyway.
:killed

REM --- Build the sensor helper that gets bundled with the app ---
where dotnet >nul 2>nul
if errorlevel 1 (
  echo [ERROR] dotnet SDK not found; it is needed to build the sensor helper.
  goto :fail
)
echo Building sensor helper...
dotnet publish sensor-helper -c Release -r win-x64 --self-contained false -o src-tauri\sensor-helper-bin -v q -nologo
if errorlevel 1 (
  echo [ERROR] Sensor helper build failed.
  goto :fail
)
echo.

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
if not exist "%VSWHERE%" (
  echo [ERROR] vswhere.exe not found. Is Visual Studio / Build Tools installed?
  goto :fail
)

set "VSPATH="
for /f "usebackq delims=" %%i in (`
  "%VSWHERE%" -latest -products * ^
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 ^
    -property installationPath
`) do set "VSPATH=%%i"

if not defined VSPATH (
  echo [ERROR] No Visual Studio install with the C++ toolset was found.
  echo         Install the "Desktop development with C++" workload.
  goto :fail
)

set "VCVARS=%VSPATH%\VC\Auxiliary\Build\vcvarsall.bat"
if not exist "%VCVARS%" (
  echo [ERROR] vcvarsall.bat not found at %VCVARS%
  goto :fail
)

echo Loading C++ build environment...
call "%VCVARS%" x64 >nul
if errorlevel 1 (
  echo [ERROR] Failed to initialize the MSVC environment.
  goto :fail
)

echo Building release (this can take several minutes)...
echo.
call npm run tauri:build
if errorlevel 1 (
  echo.
  echo [ERROR] Build failed. See the output above.
  goto :fail
)

echo.
echo ============================================
echo   Build complete.
echo   Find the .exe and installer under:
echo     src-tauri\target\release\
echo     src-tauri\target\release\bundle\
echo ============================================
goto :end

:fail
echo.
echo Press any key to close.
pause >nul
exit /b 1

:kill_running
taskkill /f /im onscreen-widget.exe >nul 2>&1
taskkill /f /im sensor-helper.exe >nul 2>&1
REM give Windows a moment to release the file locks
timeout /t 2 /nobreak >nul
exit /b 0

:end
echo.
echo Press any key to close.
pause >nul
endlocal
