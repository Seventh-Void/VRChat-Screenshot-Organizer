# VRChat Screenshot Organizer

> Automatically organize your VRChat screenshots by world while maintaining your year/month folder structure

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)
[![Python 3.7+](https://img.shields.io/badge/Python-3.7+-blue.svg)](https://www.python.org/downloads/)

## 🎯 Features

- ✨ **Automatic World Detection** - Reads world metadata from VRChat screenshot EXIF/metadata
- 📁 **Smart Organization** - Creates world-named subfolders within each month folder
- 👁️ **Preview Mode** - See what will be organized before making any changes
- 📊 **Detailed Logging** - Track exactly what's happening with comprehensive logs and real-time GUI updates
- 🛡️ **Safe Operations** - Automatically handles duplicate filenames and edge cases
- 🎨 **Print Handling** - Automatically separates 2048x1440 prints into a dedicated "Prints" folder
- 📅 **Bulk Scanning** - Option to scan all historical month folders at once
- 🌙 **Dark Mode** - Native dark theme support in the GUI for night owls
- ⚙️ **Autostart Setup** - Easily install startup entries for Windows and Linux (systemd)
- 🖥️ **GUI & CLI** - Use either a graphical interface or command-line tools
- 📦 **Standalone AppImage** - Deploy as a single executable file on Linux
- 👀 **Watch Mode** - Automatically organize new screenshots as they're created

## 📋 Requirements

- **Python 3.7 or higher**
- **Pillow library** (automatically installed on first run)
- **[VRCX](https://github.com/vrcx-team/VRCX)** installed and running
  - Required to embed world metadata in screenshots
  - Must enable **"Screenshot Metadata"** in VRCX settings
  - Must be running in background for watch mode to work

### ⚠️ Important: VRCX Setup Required

This tool reads world metadata that **VRCX embeds into your screenshots**. Without VRCX:

1. **No metadata embedded** → screenshots won't be organized
2. **VRCX must be running** → especially for watch mode
3. **Enable in settings** → Go to VRCX Settings → Enable "Screenshot Metadata"

If you see "No world metadata found" messages, verify VRCX is running and Screenshot Metadata is enabled.

## 🤖 AI NOTICE & DISCLAIMER

This project has been developed with assistance from an AI coding tool. While the AI helped generate code and documentation, it may still have bugs, edge cases, or inaccuracies.

**RUN AT YOUR OWN RISK!** This tool modifies your file system by moving files.

### Always Do This Before Running:

1. **Always run with `--dry-run` first:**
   ```bash
   python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --dry-run
   ```
   Review the output carefully. If you don't like what you see, **do not proceed**.

2. **Create a backup:**
   - Backup your entire VRChat pictures folder before running
   - Or backup at least the month folders you're organizing
   - Better safe than sorry!

3. **Understand the risk:**
   - This tool moves files on your hard drive
   - While it handles edge cases safely, **no guarantees**
   - The author assumes **no responsibility for data loss**
   - Use at your own risk

**Recommendation:** Always do a `--dry-run` first and keep backups. If you don't wish to proceed after seeing the dry-run results, simply don't run the command without `--dry-run`.

## 🚀 Quick Start

### Before You Start

1. **Install VRCX**: Download from [github.com/vrcx-team/VRCX](https://github.com/vrcx-team/VRCX)
2. **Enable Screenshot Metadata**: In VRCX Settings → Media → Enable "Screenshot Metadata"
3. **Keep VRCX Running**: Especially if using watch mode

### Option 1: Command Line

#### Windows
1. Install [Python 3](https://www.python.org/downloads/) (Check "Add Python to PATH")
2. Open Command Prompt and run:
   ```cmd
   pip install Pillow
   python organize_vrchat.py "C:\Users\YourName\Pictures\VRChat\VRChat"
   ```

#### Linux
```bash
# Preview what will be organized (no changes made)
python3 preview_vrchat.py ~/Pictures/VRChat/VRChat

# Run the actual organization
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat

# Watch mode: automatically organize new screenshots (VRCX must be running!)
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --watch --interval 30
```

### Option 2: Graphical Interface

#### Windows
1. Download the `VRChatOrganizer.exe` from the releases page.
2. Double-click the executable.
3. *Alternatively, if you have Python installed, you can run `python3 gui_vrchat_organizer.py`.*


#### Linux
```bash
python3 gui_vrchat_organizer.py
```

### Option 3: Standalone AppImage (Linux)

```bash
chmod +x build_appimage.sh
./build_appimage.sh

# Run the AppImage
./VRChatOrganizer.AppImage
```

## 📖 Usage Guide

### Command Line Options

```
usage: organize_vrchat.py [-h] [--dry-run] [--watch] [--interval INTERVAL] 
                          [--single-folder] [--software-filter SOFTWARE_FILTER] 
                          [path]

Organize VRChat screenshots by world

positional arguments:
  path                  Path to VRChat pictures directory or specific folder

optional arguments:
  -h, --help           Show this help message and exit
  --dry-run            Show what would be done without making changes
  --watch              Keep monitoring the folder and organize automatically
  --interval INTERVAL  Watch interval in seconds (default: 5)
  --single-folder      Treat path as a single folder to organize (not as a root with YYYY-MM folders)
  --scan-all-months    Scan all month folders instead of just the latest one
  --template TEMPLATE  Custom subfolder naming template (e.g., "{world}", "{year}-{month}/{world}").
                       Available variables: {world}, {year}, {month}, {day}, {width}, {height}
```

### Examples

```bash
# Dry run - see what would happen
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --dry-run

# Organize a specific folder
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --single-folder

# Scan all historical month folders
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --scan-all-months

# Watch mode with custom interval
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --watch --interval 60

# Organize into "YYYY-MM/World Name (WidthxHeight)" folders using a template
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --template "{year}-{month}/{world} ({width}x{height})"
```

### Folder Structure

Before organizing:
```
VRChat/
├── 2025-01/
│   ├── screenshot_1.png
│   ├── screenshot_2.png
│   ├── screenshot_3.png
│   └── ...
├── 2025-02/
│   ├── screenshot_1.png
│   └── ...
```

After organizing:
```
VRChat/
├── 2025-01/
│   ├── Black Cat/
│   │   ├── screenshot_1.png
│   │   └── screenshot_2.png
│   ├── Home Sweet Home/
│   │   └── screenshot_3.png
│   ├── Prints/
│   │   └── print_2048x1440.png
│   └── ...
├── 2025-02/
│   └── ...
```

## 🔧 Scripts

### `organize_vrchat.py`
Main organization script. Can run in normal mode, dry-run mode, or watch mode.

### `preview_vrchat.py`
Preview what will be organized without making any changes.

### `gui_vrchat_organizer.py`
Graphical user interface for easier use without command-line knowledge.

### `debug_metadata.py`
Utility script to inspect EXIF metadata in image files for troubleshooting.

### `build_appimage.sh`
Build script to create a standalone Linux AppImage executable.

## 📁 Project Structure

```
VRChat-Organizer/
├── src/                          # Source code directory
│   ├── organize_vrchat.py        # Main organizer class
│   ├── gui_vrchat_organizer.py   # GUI interface
│   ├── preview_vrchat.py         # Preview tool
│   └── debug_metadata.py         # Metadata debugging
├── scripts/                       # Build and utility scripts
│   └── build_appimage.sh         # AppImage builder
├── docs/                          # Documentation
├── icons/                         # Application icons
├── requirements.txt               # Python dependencies
├── LICENSE                        # MIT License
├── CONTRIBUTING.md                # Contribution guidelines
└── README.md                      # This file
```

## 🐛 Troubleshooting

### ⚠️ Critical: VRCX Not Running or Metadata Disabled
**Symptom**: All screenshots show "No world metadata found"
- **Solution 1**: Ensure VRCX is installed from [github.com/vrcx-team/VRCX](https://github.com/vrcx-team/VRCX)
- **Solution 2**: Open VRCX Settings → Enable "Screenshot Metadata" checkbox
- **Solution 3**: Make sure VRCX is running in the background while taking screenshots
- **Solution 4**: Use `debug_metadata.py` on a recent screenshot to verify metadata is being embedded

### ⚠️ Watch Mode Not Working
**Symptom**: Watch mode doesn't organize new screenshots
- Verify VRCX is running in the background
- Verify "Screenshot Metadata" is enabled in VRCX Settings
- Try increasing the interval: `--watch --interval 60`
- Check file permissions on the VRChat pictures folder

### "No world metadata found"
- Verify VRCX is running and Screenshot Metadata is enabled (see above)
- Some old screenshots may not have VRCX metadata embedded
- These images will remain in the month folder root
- Use `debug_metadata.py` to inspect specific images and confirm metadata presence

### Import errors
- Dependencies are automatically installed on first run
- If issues persist, manually install: `pip3 install Pillow`

### Permission denied errors
- Ensure you have read/write permissions on the VRChat pictures directory
- Try running with appropriate permissions

### GUI not appearing
- Ensure tkinter is installed: `sudo apt install python3-tk` (Ubuntu/Debian)
- Or `brew install python-tk` (macOS)

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## 📄 License

This project is licensed under the MIT License - see [LICENSE](LICENSE) for details.

## ⚠️ Final Disclaimer

This tool modifies your file system by moving your screenshot files. **Use at your own risk!**

**Critical Safety Steps:**
- ✅ **Always run `--dry-run` first** - See what would happen before any changes
- ✅ **Always backup before running** - Keep copies of your important screenshots
- ✅ **Review `--dry-run` output carefully** - Make sure you agree with what it will do
- ✅ **Only run if you're comfortable** - Don't proceed if you have doubts

**The author assumes NO responsibility for data loss or damage.** This is provided as-is. While the tool is designed to be safe and careful, **you use it at your own risk.** If you don't wish to proceed after reviewing the `--dry-run` output, simply do not run the command without the `--dry-run` flag

## 🙏 Acknowledgments

- [Pillow](https://python-pillow.org/) - Python Imaging Library
- [VRCX](https://github.com/vrcx-team/VRCX) - VRChat Companion
- VRChat Community

## 📞 Support

For issues, questions, or suggestions:
- Open an [issue](../../issues) on GitHub
- Check existing documentation in the [docs](docs/) folder

---

**Made with ❤️ for the VRChat community**
