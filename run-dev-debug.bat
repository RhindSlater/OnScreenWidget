@echo off
setlocal enableextensions

echo ============================================
echo   OnScreen Widget - DIAGNOSTIC launcher
echo ============================================
echo.

cd /d "%~dp0"
echo [step] Working directory: %CD%
echo.

set "VSWHERE=%ProgramFiles(x86)%\Microsoft Visual Studio\Installer\vswhere.exe"
echo [step] Looking for vswhere at:
echo        %VSWHERE%
if exist "%VSWHERE%" (
  echo        FOUND
) else (
  echo        NOT FOUND
)
echo.

set "VSPATH="
for /f "usebackq delims=" %%i in (`
  "%VSWHERE%" -latest -products * ^
    -requires Microsoft.VisualStudio.Component.VC.Tools.x86.x64 ^
    -property installationPath
`) do set "VSPATH=%%i"
echo [step] VS install path with C++ toolset:
echo        "%VSPATH%"
echo.

set "VCVARS=%VSPATH%\VC\Auxiliary\Build\vcvarsall.bat"
echo [step] Expecting vcvarsall.bat at:
echo        %VCVARS%
if exist "%VCVARS%" (
  echo        FOUND
) else (
  echo        NOT FOUND
)
echo.

echo [step] Loading C++ environment (vcvarsall x64)...
call "%VCVARS%" x64
echo [step] vcvarsall exit code: %errorlevel%
echo.

echo [step] Checking tools on PATH:
where node
where npm
where cargo
echo.

echo [step] LIB variable (should include a ...\VC\Tools\MSVC\...\lib\x64 entry):
echo %LIB%
echo.

echo [step] About to run: npm run tauri:dev
echo        (Leave this window open; the app window should appear.)
echo ============================================
echo.
call npm run tauri:dev

echo.
echo ============================================
echo [step] npm exited with code: %errorlevel%
echo ============================================
echo.
echo Diagnostics complete. This window will stay open.
pause
endlocal
