@echo off
rem Pro Internet Manager build (Windows, MSVC toolchain).
rem Output: "Pro Internet Manager_restyle.exe" next to this file, log in build.log.
rem Optional environment variables:
rem   VCVARS        path to vcvars64.bat (default: Visual Studio 2022 Build Tools)
rem   PIM_SDKROOT   standalone Windows SDK root (Include\ and bin\), only if the SDK is not installed normally
rem   PIM_SDKLIB    standalone Windows SDK lib root (ucrt\x64, um\x64)
rem   PIM_SDKVER    SDK version folder (default 10.0.28000.0)
rem   CARGO         cargo executable (default: cargo from PATH)
rem   PIM_VERSION_URL, PIM_FALLBACK_VERSION_URL, PIM_DOWNLOAD_URL  update sources (see README)
setlocal
if not defined VCVARS set "VCVARS=C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Auxiliary\Build\vcvars64.bat"
if exist "%VCVARS%" call "%VCVARS%" >nul 2>nul
if not defined PIM_SDKVER set "PIM_SDKVER=10.0.28000.0"
if not defined PIM_SDKROOT goto nosdk
set "INCLUDE=%INCLUDE%;%PIM_SDKROOT%\Include\%PIM_SDKVER%\ucrt;%PIM_SDKROOT%\Include\%PIM_SDKVER%\shared;%PIM_SDKROOT%\Include\%PIM_SDKVER%\um;%PIM_SDKROOT%\Include\%PIM_SDKVER%\winrt"
set "PATH=%PATH%;%PIM_SDKROOT%\bin\%PIM_SDKVER%\x64"
set "RC_EXE=%PIM_SDKROOT%\bin\%PIM_SDKVER%\x64\rc.exe"
:nosdk
if defined PIM_SDKLIB set "LIB=%LIB%;%PIM_SDKLIB%\ucrt\x64;%PIM_SDKLIB%\um\x64"
if not defined CARGO set "CARGO=cargo"
set "CARGO_TARGET_DIR=%~dp0target"
set "TMP=%~dp0build-tmp"
set "TEMP=%~dp0build-tmp"
if not exist "%TMP%" mkdir "%TMP%"
"%CARGO%" build --release --manifest-path "%~dp0Cargo.toml" > "%~dp0build.log" 2>&1
set "RESULT=%ERRORLEVEL%"
echo EXIT %RESULT% >> "%~dp0build.log"
if not "%RESULT%"=="0" exit /b %RESULT%
copy /Y "%~dp0target\release\rust-download-manager.exe" "%~dp0Pro Internet Manager_restyle.exe" >nul
if errorlevel 1 (
    echo Copy failed: close Pro Internet Manager_restyle.exe and run again. >> "%~dp0build.log"
    exit /b 1
)
echo Built: %~dp0Pro Internet Manager_restyle.exe >> "%~dp0build.log"
