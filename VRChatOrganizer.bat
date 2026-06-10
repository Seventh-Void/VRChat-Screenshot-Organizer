@echo off
echo Starting VRChat Organizer...

:: Check for Python
python --version >nul 2>&1
if %errorlevel% neq 0 (
    echo Error: Python is not installed or not in PATH.
    pause
    exit /b
)

:: Check for Pillow
python -c "import PIL" >nul 2>&1
if %errorlevel% neq 0 (
    echo Installing missing dependencies (Pillow)...
    python -m pip install Pillow
)

start "" pythonw gui_vrchat_organizer.py