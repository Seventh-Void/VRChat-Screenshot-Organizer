# VRChat Screenshot Organizer

> Automatically organize your VRChat screenshots by world while maintaining your year/month folder structure

[![License: MIT](https://img.shields.io/badge/License-MIT-yellow.svg)](https://opensource.org/licenses/MIT)

## 🎯 Features

- ✨ **Automatic World Detection** - Reads world metadata from VRChat screenshot EXIF/PNG text chunks
- 📁 **Smart Organization** - Creates world-named subfolders within each month folder
- 🛡️ **Safe Operations** - Automatically handles duplicate filenames and prevents overwriting
- 🎨 **Print Handling** - Automatically separates 2048×1440 prints into a dedicated "Prints" folder
- 📅 **Bulk Scanning** - Option to scan all historical month folders at once
- 🖥️ **CLI & Tauri GUI** - Use either command-line tools or the Tauri desktop app
- 📦 **Standalone Binary** - Single executable, no runtime dependencies needed
- ⚡ **Pure Rust** - Fast, memory-safe, no Python or external runtime required

## 📋 Requirements

- **[VRCX](https://github.com/vrcx-team/VRCX)** installed and running
  - Required to embed world metadata in screenshots
  - Must enable **"Screenshot Metadata"** in VRCX settings
  - Must be running in background for organization to work

### ⚠️ Important: VRCX Setup Required

This tool reads world metadata that **VRCX embeds into your screenshots**. Without VRCX:

1. **No metadata embedded** → screenshots won't be organized
2. **VRCX must be running** → for proper metadata embedding
3. **Enable in settings** → Go to VRCX Settings → Enable "Screenshot Metadata"

If you see "No world metadata found" messages, verify VRCX is running and Screenshot Metadata is enabled.

## 🤖 AI NOTICE & DISCLAIMER

This project has been developed with assistance from an AI coding tool. While the AI helped generate code and documentation, it may still have bugs, edge cases, or inaccuracies.

**RUN AT YOUR OWN RISK!** This tool modifies your file system by moving files.

### Always Do This Before Running:

1. **Always run with `--dry-run` first:**
   ```bash
   ./vrchat-organizer ~/Pictures/VRChat/VRChat --dry-run
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

### Download

Grab the latest AppImage from the [releases page](https://github.com/yourusername/VRChat-Screenshot-Organizer/releases) or build from source.

### Before You Start

1. **Install VRCX**: Download from [github.com/vrcx-team/VRCX](https://github.com/vrcx-team/VRCX)
2. **Enable Screenshot Metadata**: In VRCX Settings → Media → Enable "Screenshot Metadata"
3. **Keep VRCX Running** — metadata is embedded at screenshot time

### Usage

```bash
# Make the AppImage executable
chmod +x VRChatOrganizer-x86_64.AppImage

# Dry run — preview what will happen (no actual changes)
./VRChatOrganizer-x86_64.AppImage ~/Pictures/VRChat/VRChat --dry-run

# Run organization on your VRChat screenshots folder
./VRChatOrganizer-x86_64.AppImage ~/Pictures/VRChat/VRChat

# Use a custom subfolder naming template
./VRChatOrganizer-x86_64.AppImage ~/Pictures/VRChat/VRChat --template "{year}-{month}/{world}"

# Scan all historical month folders, not just the latest
./VRChatOrganizer-x86_64.AppImage ~/Pictures/VRChat/VRChat --scan-all-months

# Treat the path as a single folder (no YYYY-MM structure)
./VRChatOrganizer-x86_64.AppImage ~/Pictures/VRChat/Screenshots --single-folder
```

### Building from Source

```bash
# Clone the repository
git clone https://github.com/yourusername/VRChat-Screenshot-Organizer
cd VRChat-Screenshot-Organizer/rust-rewrite

# Build the desktop CLI
cargo build -p desktop --release

# Or build the Tauri desktop app
cargo build -p app --release

# Build the full workspace (all crates)
cargo build --release
```

## 📖 Usage Guide

### Command Line Options

```
Usage: vrchat-organizer [OPTIONS] [PATH]

Arguments:
  [PATH]  Path to VRChat pictures directory or specific folder
          [default: ~/Pictures/VRChat/VRChat]

Options:
  -d, --dry-run            Show what would be done without making changes
  -s, --scan-all-months    Scan all month folders instead of just the latest one
  -f, --single-folder      Treat path as a single folder to organize
                           (not as a root with YYYY-MM folders)
  -t, --template <TEMPLATE>  Custom subfolder naming template
                           [default: {world}]
                           Variables: {world}, {year}, {month}, {day}, {width}, {height}
  -h, --help               Print help
  -V, --version            Print version
```

### Environment Variables

| Variable | Description |
|---|---|
| `VRCHAT_DRY_RUN` | Set to any value to enable dry-run mode |
| `VRCHAT_SCAN_ALL_MONTHS` | Set to any value to scan all month folders |
| `VRCHAT_SINGLE_FOLDER` | Set to any value to treat path as a single folder |

### Examples

```bash
# Dry run — see what would happen
./vrchat-organizer ~/Pictures/VRChat/VRChat --dry-run

# Organize a specific folder
./vrchat-organizer ~/Pictures/VRChat/VRChat --single-folder

# Scan all historical month folders
./vrchat-organizer ~/Pictures/VRChat/VRChat --scan-all-months

# Organize into "{year}-{month}/World Name (WidthxHeight)" folders
./vrchat-organizer ~/Pictures/VRChat/VRChat --template "{year}-{month}/{world} ({width}x{height})"
```

### Folder Structure

Before organizing:
```
VRChat/
├── 2025-01/
│   ├── VRChat_2025-01-12_12-34-56.xyz_1920x1080.png
│   ├── VRChat_2025-01-12_13-00-00.xyz_1920x1080.png
│   ├── VRChat_2025-01-13_10-00-00.xyz_2048x1440.png
│   └── ...
├── 2025-02/
│   └── ...
```

After organizing:
```
VRChat/
├── 2025-01/
│   ├── Black Cat/
│   │   ├── VRChat_2025-01-12_12-34-56.xyz_1920x1080.png
│   │   └── VRChat_2025-01-12_13-00-00.xyz_1920x1080.png
│   ├── Prints/
│   │   └── VRChat_2025-01-13_10-00-00.xyz_2048x1440.png
│   └── ...
├── 2025-02/
│   └── ...
```

## 🐛 Troubleshooting

### ⚠️ Critical: VRCX Not Running or Metadata Disabled
**Symptom**: All screenshots show "No world metadata found"
- **Solution 1**: Ensure VRCX is installed from [github.com/vrcx-team/VRCX](https://github.com/vrcx-team/VRCX)
- **Solution 2**: Open VRCX Settings → Enable "Screenshot Metadata" checkbox
- **Solution 3**: Make sure VRCX is running in the background while taking screenshots

### "No world metadata found"
- Verify VRCX is running and Screenshot Metadata is enabled (see above)
- Some old screenshots may not have VRCX metadata embedded
- These images will remain in the month folder root

### Permission denied errors
- Ensure you have read/write permissions on the VRChat pictures directory
- Make the AppImage executable: `chmod +x VRChatOrganizer-x86_64.AppImage`

## 📁 Project Structure

```
VRChat-Screenshot-Organizer/
├── rust-rewrite/                 # Rust workspace (main codebase)
│   ├── Cargo.toml                # Workspace manifest
│   ├── crates/
│   │   ├── organizer-core/       # Core library: metadata extraction + file organization
│   │   └── desktop/              # CLI binary entrypoint
│   ├── src-tauri/                # Tauri desktop app shell
│   └── README.md                 # Rust-specific build instructions
├── docs/                         # Documentation
├── icons/                        # Application icons
├── README.md                     # This file
└── LICENSE                       # MIT License
```

## 🤝 Contributing

Contributions are welcome! Please see [CONTRIBUTING.md](CONTRIBUTING.md) for guidelines.

## 📄 License

This project is licensed under the MIT License - see [LICENSE](LICENSE) for details.

## ⚠️ Final Disclaimer

This tool modifies your file system by moving your screenshot files. **Use at your own risk!**

**Critical Safety Steps:**
- ✅ **Always run `--dry-run` first** — See what would happen before any changes
- ✅ **Always backup before running** — Keep copies of your important screenshots
- ✅ **Review `--dry-run` output carefully** — Make sure you agree with what it will do
- ✅ **Only run if you're comfortable** — Don't proceed if you have doubts

**The author assumes NO responsibility for data loss or damage.** This is provided as-is. While the tool is designed to be safe and careful, **you use it at your own risk.** If you don't wish to proceed after reviewing the `--dry-run` output, simply do not run the command without the `--dry-run` flag

## 🙏 Acknowledgments

- [image-rs](https://github.com/image-rs/image) — Rust Imaging Library
- [kamadak-exif](https://github.com/kamadak/exif-rs) — Rust EXIF Reader
- [VRCX](https://github.com/vrcx-team/VRCX) — VRChat Companion
- [Tauri](https://tauri.app/) — Desktop Application Framework
- VRChat Community

## 📞 Support

For issues, questions, or suggestions:
- Open an [issue](../../issues) on GitHub
- Check existing documentation in the [docs](docs/) folder

---

**Made with ❤️ for the VRChat community**
