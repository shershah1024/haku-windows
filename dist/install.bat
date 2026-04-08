@echo off
REM Haku Windows installer
REM Copies haku.exe to %LOCALAPPDATA%\Haku and adds it to the user PATH.

setlocal
set INSTALL_DIR=%LOCALAPPDATA%\Haku

echo Installing Haku to %INSTALL_DIR%...

if not exist "%INSTALL_DIR%" mkdir "%INSTALL_DIR%"
copy /Y "%~dp0haku.exe" "%INSTALL_DIR%\haku.exe" >nul
if errorlevel 1 (
    echo Failed to copy haku.exe
    exit /b 1
)

REM Add to user PATH if not already there
for /f "tokens=2*" %%A in ('reg query "HKCU\Environment" /v Path 2^>nul') do set "USER_PATH=%%B"
echo %USER_PATH% | findstr /C:"%INSTALL_DIR%" >nul
if errorlevel 1 (
    setx PATH "%USER_PATH%;%INSTALL_DIR%" >nul
    echo Added %INSTALL_DIR% to PATH (open a new terminal to use it)
) else (
    echo PATH already contains %INSTALL_DIR%
)

echo.
echo Installed. Next steps:
echo   1. Open a new PowerShell window
echo   2. Run: haku --setup
echo      (This will optionally download the embedding model, ~313MB)
echo   3. Run: haku
echo      (Starts the MCP server on 127.0.0.1:19820)
echo.
endlocal
