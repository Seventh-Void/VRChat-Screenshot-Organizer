# Installation Guide

## ⚠️ Critical Prerequisite: VRCX

**VRCX must be installed and running for this tool to work!**

1. Download [VRCX](https://github.com/vrcx-team/VRCX/releases) from GitHub
2. Install and launch VRCX
3. Go to VRCX Settings
4. Enable "Screenshot Metadata" checkbox
5. Keep VRCX running in the background when taking screenshots

Without VRCX, your screenshots won't have world metadata embedded, and this organizer won't be able to identify which world each screenshot is from.

## System Requirements

- **VRCX** installed and "Screenshot Metadata" enabled (see above!)
- Python 3.7 or higher
- 50 MB disk space for the application and dependencies
- Unix-like system (Linux, macOS) or Windows with Python 3.7+

## Installation Methods

### Method 1: Direct Download (Recommended for Linux)

1. Download the latest `VRChatOrganizer.AppImage` from the releases page
2. Make it executable:
   ```bash
   chmod +x VRChatOrganizer.AppImage
   ```
3. Run it:
   ```bash
   ./VRChatOrganizer.AppImage
   ```

### Method 2: Standalone Executable (Windows)

1. Download the latest `VRChatOrganizer.exe`.
2. Double-click the executable to run the GUI.
   *Note: This method does not require Python to be installed on your system.*

   **To build the .exe yourself:**
   1. Open PowerShell in the project folder.
   2. Run the build script:
   ```powershell
   .\docs\build_windows.ps1
   ```

### Method 2: Clone Repository

```bash
git clone https://github.com/yourusername/VRChat-Organizer.git
cd VRChat-Organizer
pip install -r requirements.txt
```

### Method 3: Manual Installation (All Platforms)

1. Ensure Python 3.7+ is installed
2. Download or clone this repository
3. Install dependencies:
   ```bash
   pip install -r requirements.txt
   ```
4. Run the GUI:
   ```bash
   python3 gui_vrchat_organizer.py
   ```
   Or CLI:
   ```bash
   python3 organize_vrchat.py ~/Pictures/VRChat/VRChat
   ```

## Verifying Installation

Test your installation:

```bash
python3 -c "from PIL import Image; print('Pillow installed successfully')"
```

If you get an error, reinstall dependencies:

```bash
pip install --upgrade -r requirements.txt
```

## ⚠️ IMPORTANT: Before You Run

**This tool modifies your file system!** Before running for the first time:

1. **Create a backup** of your VRChat pictures folder
2. **Always run `--dry-run` first:**
   ```bash
   python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --dry-run
   ```
3. **Review the output carefully** - make sure you agree with what it will do
4. **Only run without `--dry-run`** if you're comfortable with the proposed changes

If you don't wish to proceed after seeing the dry-run results, simply don't run the command without the `--dry-run` flag. Your files will remain unchanged.

**Use at your own risk!** The author assumes no responsibility for data loss. See the main README.md for full disclaimer.

## Troubleshooting Installation

### Issue: ModuleNotFoundError: No module named 'PIL'

**Solution**: Install Pillow manually
```bash
pip install Pillow
```

### Issue: Python 3 not found

**Solution**: Install Python 3.7 or higher
- **Ubuntu/Debian**: `sudo apt install python3 python3-tk`
- **Fedora**: `sudo dnf install python3 python3-tkinter`
- **macOS**: `brew install python3`
- **Windows**: Download from [python.org](https://www.python.org/downloads/)

### Issue: tkinter not available (GUI won't start)

**Solution**: Install tkinter
- **Ubuntu/Debian**: `sudo apt install python3-tk`
- **Fedora**: `sudo dnf install python3-tkinter`
- **macOS**: `brew install python-tk`

### Issue: Cannot build AppImage

**Solution**: Ensure you have appimagetool installed
```bash
# Ubuntu/Debian
sudo apt install appimage-builder

# Or download pre-built tool
wget https://github.com/AppImage/AppImageKit/releases/download/13/appimagetool-x86_64.AppImage
chmod +x appimagetool-x86_64.AppImage
sudo mv appimagetool-x86_64.AppImage /usr/local/bin/appimagetool
```

## Getting Help

If you encounter issues:
1. Check this file for common problems
2. Review the [README.md](../README.md)
3. Open an issue on GitHub with error messages and your setup
