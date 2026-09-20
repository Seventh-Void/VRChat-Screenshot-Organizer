# Building VRChat Organizer for Windows

> **Note for Linux users:** You can cross-compile a Windows EXE from Linux using `mingw-w64` without needing a Windows machine. See Option 2 below.

## Prerequisites

### Option 1: Native Windows Build

1. **Windows 10/11** with:
   - Rust from https://rustup.rs
   - Visual Studio Build Tools or Visual Studio with "Desktop development with C++" workload
   - Node.js (for Tauri)

2. **Install Tauri prerequisites** (run in PowerShell as Admin):
   ```powershell
   # WebView2 is already included in Windows 10 (version 1803+) and Windows 11
   ```

### Option 2: Cross-Compile from Linux (for the CLI binary only)

No Windows machine needed! Cross-compile the CLI binary directly from Linux:

```bash
# 1. Add the Windows target
rustup target add x86_64-pc-windows-gnu

# 2. Install the mingw-w64 cross-compiler
# Debian/Ubuntu:
sudo apt install mingw-w64

# Arch Linux:
# sudo pacman -S mingw-w64-gcc

# 3. Build the CLI binary for Windows
cd rust-rewrite
cargo build --release -p desktop --target x86_64-pc-windows-gnu
```

The output will be at `rust-rewrite/target/x86_64-pc-windows-gnu/release/desktop.exe` — a standalone Windows executable you can upload as a release artifact.

> ⚠️ **Limitation:** The Tauri GUI app (`-p app`) cannot be cross-compiled from Linux because it requires native Windows WebView2. For the full GUI app on Windows, build natively on Windows (Option 1) or use GitHub Actions.

## Build Steps

### Option 1: Tauri GUI App (native Windows, recommended for full app)

```powershell
# 1. Navigate to the project
cd rust-rewrite

# 2. Install the Tauri CLI
cargo install tauri-cli --version '^2'

# 3. Build the portable GUI executable
cargo tauri build --bundles none

# 4. Build installers
cargo tauri build --bundles nsis,msi
```

The native Cargo output is `target/release/app.exe`. The release workflow
renames it to `vrchat-organizer-2.0.0-portable.exe`. Installers are written to
`target/release/bundle/nsis/` and `target/release/bundle/msi/`.

### Option 2: CLI Binary Only (cross-compile from Linux)

```bash
# On Linux with mingw-w64 (see Prerequisites Option 2 above):
rustup target add x86_64-pc-windows-gnu
sudo apt install mingw-w64
cargo build --release -p desktop --target x86_64-pc-windows-gnu
```

The output will be at `target/x86_64-pc-windows-gnu/release/desktop.exe`.

### Option 3: Automated via GitHub Actions (easiest!)

Push a tag to GitHub and the [release workflow](../.github/workflows/release.yml) will automatically build:
- **AppImage** (Linux)
- **desktop.exe** (Windows CLI, cross-compiled from Linux runner)
- **app.exe** (Windows GUI source binary, built on Windows runner)

No local builds needed. The workflow publishes the AppImage, portable Windows
executable, NSIS installer, and MSI when a GitHub release is published.

## Creating an Installer

To create a Windows installer, you can use:

1. **WiX Toolset** — Tauri supports bundling with WiX:
   ```powershell
   cargo tauri build --bundles msi
   ```

2. **NSIS** — Create a simple NSIS installer script

3. **Simply distribute the .exe** — the app is portable and doesn't require installation
