# Building VRChat Organizer for Windows

## Prerequisites

1. **Windows 10/11** with:
   - Rust from https://rustup.rs
   - Visual Studio Build Tools or Visual Studio with "Desktop development with C++" workload
   - Node.js (for Tauri)

2. **Install Tauri prerequisites** (run in PowerShell as Admin):
   ```powershell
   # WebView2 is already included in Windows 10 (version 1803+) and Windows 11
   ```

## Build Steps

### Option 1: Tauri GUI App (recommended)

```powershell
# 1. Navigate to the project
cd rust-rewrite

# 2. Build the Tauri app
cargo build --release -p app

# 3. The binary will be at:
#    target/release/vrchat-organizer.exe
```

### Option 2: Standalone Binary (from Linux cross-compile)

```bash
# On Linux with mingw-w64:
rustup target add x86_64-pc-windows-gnu
sudo apt install mingw-w64   # or: pacman -S mingw-w64-gcc
cargo build --release -p desktop --target x86_64-pc-windows-gnu
```

The output will be at `target/x86_64-pc-windows-gnu/release/desktop.exe`.

## Creating an Installer

To create a Windows installer, you can use:

1. **WiX Toolset** — Tauri supports bundling with WiX:
   ```powershell
   cargo tauri build --bundles msi
   ```

2. **NSIS** — Create a simple NSIS installer script

3. **Simply distribute the .exe** — the app is portable and doesn't require installation

