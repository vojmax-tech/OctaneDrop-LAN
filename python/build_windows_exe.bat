@echo off
setlocal
cd /d %~dp0

set "PY_CMD="
py -3.12 -c "import sys" >nul 2>nul
if not errorlevel 1 goto have_py312

python --version >nul 2>nul
if errorlevel 1 goto no_python

for /f "tokens=2 delims= " %%v in ('python --version 2^>^&1') do set "PY_VER=%%v"
if "%PY_VER:~0,5%"=="3.12." goto have_python312

echo ERROR: Desktop EXE build currently requires Python 3.12. Found %PY_VER%.
exit /b 1

:no_python
echo ERROR: Python not found. Install Python 3.12 (x64) and retry.
exit /b 1

:have_py312
set "PY_CMD=py -3.12"
goto build

:have_python312
set "PY_CMD=python"

:build
echo Using Python runtime: %PY_CMD%
%PY_CMD% -m pip install --upgrade pip
if errorlevel 1 exit /b %errorlevel%
%PY_CMD% -m pip install -r requirements-build.txt
if errorlevel 1 exit /b %errorlevel%
%PY_CMD% -m PyInstaller --noconfirm --clean voltdrop_onedir.spec
if errorlevel 1 exit /b %errorlevel%

echo Built dist\VoltDrop\VoltDrop.exe