# Project Structure

This document explains the VRChat Organizer project structure and where each component is located.

## Directory Layout

```
VRChat-Organizer/
│
├── 📄 README.md                    # Main project documentation
├── 📄 LICENSE                      # MIT License
├── 📄 CONTRIBUTING.md              # Contribution guidelines
├── 📄 setup.py                     # Python package setup
├── 📄 pyproject.toml               # Modern Python project config
├── 📄 requirements.txt             # Python dependencies
├── 📄 .gitignore                   # Git ignore file
│
├── 📁 docs/                        # Documentation
│   ├── INSTALLATION.md             # Installation guide
│   ├── ADVANCED.md                 # Advanced usage guide
│   ├── build_windows.ps1           # Windows EXE builder
│   └── FAQ.md                      # Frequently asked questions
│
├── 📁 src/                         # Source code (to be used)
│   └── (Python files go here)
│
├── 📁 scripts/                     # Build scripts
│   └── build_appimage.sh           # AppImage builder
│
├── 📁 icons/                       # Application icons
│   └── 256x256/
│       └── apps/
│           └── vrchat-organizer.svg
│
├── 🐍 organize_vrchat.py           # Main organizer script
├── 🐍 gui_vrchat_organizer.py      # GUI interface
├── 🐍 preview_vrchat.py            # Preview tool
└── 🐍 debug_metadata.py            # Metadata debugging tool
```

## File Descriptions

### Core Application Files

#### `organize_vrchat.py`
- **Purpose**: Main VRChat screenshot organizer
- **Type**: CLI (Command-Line Interface)
- **Usage**: `python3 organize_vrchat.py [PATH] [OPTIONS]`
- **Features**:
  - Organize screenshots by world
  - Watch mode for continuous monitoring
  - Dry-run mode for previewing changes
  - Automatic dependency installation
  - Detailed logging

#### `gui_vrchat_organizer.py`
- **Purpose**: Graphical user interface for non-technical users
- **Type**: GUI (Tkinter-based)
- **Usage**: `python3 gui_vrchat_organizer.py`
- **Features**:
  - Point-and-click folder selection
  - Real-time log display
  - Watch mode toggle
  - Interval settings

#### `preview_vrchat.py`
- **Purpose**: Preview what would be organized without making changes
- **Type**: CLI tool
- **Usage**: `python3 preview_vrchat.py [PATH]`
- **Features**:
  - Shows all changes that would be made
  - Categorizes images by destination
  - Statistics summary

#### `debug_metadata.py`
- **Purpose**: Inspect metadata in image files
- **Type**: CLI utility
- **Usage**: `python3 debug_metadata.py [IMAGE_PATH]`
- **Features**:
  - Displays image info and EXIF data
  - Shows embedded metadata
  - Helps troubleshoot metadata issues

### Configuration Files

#### `setup.py`
- **Purpose**: Python package installation configuration
- **Used by**: `pip install -e .` or `python3 setup.py install`
- **Installs**: Console scripts and dependencies

#### `pyproject.toml`
- **Purpose**: Modern Python project configuration (PEP 517)
- **Contains**: Build system, project metadata, tool configurations
- **Used by**: `pip install .` and other modern Python tools

#### `requirements.txt`
- **Purpose**: List of Python package dependencies
- **Used by**: `pip install -r requirements.txt`
- **Content**: `Pillow>=9.0.0`

#### `.gitignore`
- **Purpose**: Tell Git which files to ignore
- **Excludes**:
  - Python cache and compiled files
  - Virtual environments
  - IDE configuration
  - Build artifacts
  - Log files

### Documentation

#### `README.md`
- Main documentation file
- Features overview
- Quick start guide
- Command reference
- Troubleshooting

#### `docs/INSTALLATION.md`
- Step-by-step installation instructions
- System requirements
- Troubleshooting installation issues

#### `docs/ADVANCED.md`
- Advanced command-line options
- Use case examples
- Debugging tips
- Performance optimization

#### `docs/FAQ.md`
- Frequently asked questions
- Common issues and solutions
- Technical explanations

### Build & Distribution

#### `build_appimage.sh`
- **Purpose**: Create a standalone Linux AppImage
- **Usage**: `./build_appimage.sh`
- **Output**: `VRChatOrganizer.AppImage`
- **Requirements**: appimagetool installed

#### `vrchat-organizer.desktop`
- **Purpose**: Linux desktop application entry
- **Used by**: Desktop environments (GNOME, KDE, etc.)
- **Enables**: Application menu integration

### Media Assets

#### `icons/256x256/apps/`
- Application icon in SVG format
- Used by AppImage and desktop environment

## How to Navigate the Project

### For Users
1. Start with [README.md](../README.md)
2. Follow [Installation Guide](INSTALLATION.md)
3. Refer to [FAQ](FAQ.md) for common questions

### For Developers
1. Clone the repository
2. Install: `pip install -r requirements.txt`
3. Run tests: `python3 organize_vrchat.py --dry-run`
4. See [CONTRIBUTING.md](../CONTRIBUTING.md) for contribution guidelines

### For Packagers
1. Look at `setup.py` and `pyproject.toml`
2. Use `build_appimage.sh` for Linux AppImage
3. Python packaging: `pip install -e .`

## Adding New Features

When adding new features:

1. Create your feature in the appropriate file
2. Add docstrings and type hints
3. Update relevant documentation
4. Test with `--dry-run` first
5. Follow the existing code style
6. Submit a pull request (see [CONTRIBUTING.md](../CONTRIBUTING.md))

## File Size Reference

- `organize_vrchat.py`: ~15 KB (core logic)
- `gui_vrchat_organizer.py`: ~8 KB (GUI)
- `preview_vrchat.py`: ~7 KB (preview tool)
- `debug_metadata.py`: ~6 KB (debugging)
- Total Python: ~36 KB
- Pillow dependency: ~2 MB (installed separately)

## Dependencies Map

```
VRChat Organizer
├── Python 3.7+ (standard library)
│   ├── json
│   ├── os
│   ├── shutil
│   ├── sys
│   ├── subprocess
│   ├── time
│   ├── threading
│   ├── pathlib
│   ├── logging
│   ├── typing
│   ├── argparse
│   └── tkinter (GUI only)
│
└── Pillow >=9.0.0 (image processing)
    └── PIL.Image
    └── PIL.ExifTags
```

## Common Tasks

### Running the organizer
```bash
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat
```

### Preview changes first
```bash
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --dry-run
```

### Use the GUI
```bash
python3 gui_vrchat_organizer.py
```

### Watch for new screenshots
```bash
python3 organize_vrchat.py ~/Pictures/VRChat/VRChat --watch
```

### Debug a specific image
```bash
python3 debug_metadata.py ~/Pictures/VRChat/VRChat/2025-01/screenshot.png
```

### Build AppImage
```bash
./build_appimage.sh
./VRChatOrganizer.AppImage
```

---

For more details, see the individual documentation files in the `docs/` folder.
