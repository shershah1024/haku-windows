@echo off
REM Haku uninstaller — removes the binary and (optionally) config.

setlocal
set INSTALL_DIR=%LOCALAPPDATA%\Haku

echo Removing %INSTALL_DIR%\haku.exe ...
if exist "%INSTALL_DIR%\haku.exe" del /F /Q "%INSTALL_DIR%\haku.exe"

set /p WIPE_CONFIG="Also remove ~/.haku config and models? [y/N] "
if /I "%WIPE_CONFIG%"=="y" (
    if exist "%USERPROFILE%\.haku" rmdir /S /Q "%USERPROFILE%\.haku"
    if exist "%LOCALAPPDATA%\Haku" rmdir /S /Q "%LOCALAPPDATA%\Haku"
    echo Removed ~/.haku and %LOCALAPPDATA%\Haku
)

echo Uninstalled. Note: PATH entry was not removed automatically.
endlocal
