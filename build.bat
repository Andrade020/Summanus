@echo off
cd /d "%~dp0"
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
if exist "C:\msys64\mingw64\bin\gcc.exe" (
    set "PATH=C:\msys64\mingw64\bin;%PATH%"
    cargo +stable-x86_64-pc-windows-gnu build --release --target x86_64-pc-windows-gnu --target-dir target\app
) else (
    cargo build --release --target-dir target\app
)
exit /b %errorlevel%
