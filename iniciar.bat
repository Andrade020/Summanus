@echo off
cd /d "%~dp0"
if exist "target\app\release\summanus.exe" (
    "target\app\release\summanus.exe" %*
    exit /b %errorlevel%
)
if exist "target\app\x86_64-pc-windows-gnu\release\summanus.exe" (
    "target\app\x86_64-pc-windows-gnu\release\summanus.exe" %*
    exit /b %errorlevel%
)
if exist "target\release\summanus.exe" (
    "target\release\summanus.exe" %*
    exit /b %errorlevel%
)
if exist "target\x86_64-pc-windows-gnu\release\summanus.exe" (
    "target\x86_64-pc-windows-gnu\release\summanus.exe" %*
    exit /b %errorlevel%
)
call build.bat
if errorlevel 1 pause & exit /b 1
if exist "target\app\release\summanus.exe" (
    "target\app\release\summanus.exe" %*
    exit /b %errorlevel%
)
if exist "target\app\x86_64-pc-windows-gnu\release\summanus.exe" (
    "target\app\x86_64-pc-windows-gnu\release\summanus.exe" %*
    exit /b %errorlevel%
)
echo Erro: build.bat nao gerou o executavel esperado.
exit /b 1
