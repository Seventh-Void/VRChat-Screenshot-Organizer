# VRChat Organizer Windows Build Script
$PSScriptRoot = Split-Path -Parent -Path $MyInvocation.MyCommand.Definition
$ProjectRoot = (Get-Item "$PSScriptRoot\..").FullName

Write-Host "Step 1: Installing dependencies..." -ForegroundColor Cyan
pip install pyinstaller pillow

Write-Host "Step 2: Building standalone executable..." -ForegroundColor Cyan
# --noconsole: Hides the terminal window when the GUI starts
# --onefile: Packs everything into a single .exe
# --clean: Clears PyInstaller cache before building
# --name: Sets the output filename
python -m PyInstaller --noconsole --onefile --name "VRChatOrganizer" --clean `
    --distpath "$ProjectRoot\dist" --workpath "$ProjectRoot\build" --specpath "$ProjectRoot" "$ProjectRoot\gui_vrchat_organizer.py"

Write-Host ""
Write-Host "Build Complete!" -ForegroundColor Green
Write-Host "The executable is located in: $ProjectRoot\dist\VRChatOrganizer.exe"
pause