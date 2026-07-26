@echo off
echo ═══ TailSync v2 Dev Launcher (Windows) ═══
echo.
echo [1/2] Building Rust daemon...
cd /d "%~dp0src-tauri"
cargo build 2>&1 | findstr /C:"Finished" /C:"error"
cd /d "%~dp0"

echo [2/2] Starting...
echo.
start /B cargo run --manifest-path src-tauri\Cargo.toml
echo.
echo ✅ TailSync is running!
echo Look for the TailSync icon in your system tray.
echo.
pause
