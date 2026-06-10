# VRChat Organizer Windows Build Script
Write-Host "Checking for build dependencies..." -ForegroundColor Cyan

# Ensure PyInstaller and Pillow are installed
pip install pyinstaller Pillow

Write-Host "Building VRChatOrganizer.exe..." -ForegroundColor Cyan

# --onefile: Bundle into a single executable
# --noconsole: Hide the command prompt window when running the GUI
# --name: Set the output filename
# --clean: Clean PyInstaller cache and remove temporary files before building
pyinstaller --onefile --noconsole --clean --name "VRChatOrganizer" "gui_vrchat_organizer.py"

# Cleanup build artifacts (keeping only the .exe in dist)
Write-Host "Cleaning up build artifacts..." -ForegroundColor Gray
Remove-Item -Path "build" -Recurse -ErrorAction SilentlyContinue
Remove-Item -Path "VRChatOrganizer.spec" -ErrorAction SilentlyContinue

Write-Host "------------------------------------------------" -ForegroundColor Green
Write-Host "Build Complete! Check the 'dist' folder for VRChatOrganizer.exe." -ForegroundColor Green
Write-Host "You can now distribute this .exe without needing Python." -ForegroundColor Green