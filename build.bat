@echo off
echo ========================================
echo   Speed Limit - Build Release
echo ========================================
echo.

cargo build --release
if %ERRORLEVEL% NEQ 0 (
    echo.
    echo [ERROR] Build failed!
    pause
    exit /b 1
)

copy /Y "target\release\speed-limit.exe" "speed-limit.exe"

echo.
echo [OK] Build complete: speed-limit.exe
pause
