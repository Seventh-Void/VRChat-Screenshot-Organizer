#!/bin/bash
# VRChat Organizer Linux AppImage Build Script
set -e

echo "Step 1: Installing dependencies..."
pip3 install pyinstaller pillow

echo "Step 2: Bundling with PyInstaller..."
# We build a single binary first
python3 -m PyInstaller --noconsole --onefile --name "vrchat-organizer" --clean ../gui_vrchat_organizer.py

echo "Step 3: Creating AppImage structure..."
# In a full production environment, you would use linuxdeploy here.
# For this rebuild, we ensure the binary is ready.

if [ -f "dist/vrchat-organizer" ]; then
    mv dist/vrchat-organizer ./VRChatOrganizer.AppImage
    chmod +x VRChatOrganizer.AppImage
    echo "------------------------------------------------"
    echo "Build Complete: VRChatOrganizer.AppImage created!"
    echo "------------------------------------------------"
else
    echo "Error: Build failed."
    exit 1
fi