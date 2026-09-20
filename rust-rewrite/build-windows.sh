#!/bin/bash
# Build VRChat Organizer for Windows (cross-compilation)
# Builds the portable CLI on Linux. The Tauri GUI is built natively by CI.
#
# For a native Windows build, see BUILDING_WINDOWS.md

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building VRChat Organizer for Windows ==="
command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 1; }
command -v rustup >/dev/null || { echo "rustup is required" >&2; exit 1; }

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -n 1)"
[ -n "$VERSION" ] || { echo "Unable to determine application version" >&2; exit 1; }
RELEASE_DIR="$SCRIPT_DIR/../release/v${VERSION}"
mkdir -p "$RELEASE_DIR"
WINDOWS_BINARY="${RELEASE_DIR}/VRChatOrganizer-${VERSION}-windows-x86_64.exe"

# Check if we have the Windows target
if rustup target list --installed | grep -q "x86_64-pc-windows-gnu"; then
  echo ">>> Building Windows binary (x86_64-pc-windows-gnu)..."
  
  # Build the organizer-core library for Windows
  cargo build --release -p organizer-core --target x86_64-pc-windows-gnu
  
  # Build the desktop CLI binary for Windows
  cargo build --release -p desktop --target x86_64-pc-windows-gnu
  cp ./target/x86_64-pc-windows-gnu/release/desktop.exe "$WINDOWS_BINARY"
  find "$RELEASE_DIR" -maxdepth 1 -type f \( -name '*.AppImage' -o -name '*.exe' -o -name '*.msi' \) \
    -print0 | sort -z | xargs -0 sha256sum > "$RELEASE_DIR/SHA256SUMS"

  echo "✅ Windows binaries built!"
  echo "   CLI: ${WINDOWS_BINARY}"
else
  echo ""
  echo "⚠️  Windows cross-compilation target not available."
  echo ""
  echo "To set up cross-compilation:"
  echo "  1. Install mingw-w64:  sudo apt install mingw-w64 (Debian/Ubuntu) or pacman -S mingw-w64 (Arch)"
  echo "  2. Add target:         rustup target add x86_64-pc-windows-gnu"
  echo "  3. Run this script again."
  echo ""
  echo "Alternatively, for a native Windows build:"
  echo "  1. Install Rust on Windows from https://rustup.rs"
  echo "  2. Clone this repo on Windows"
  echo "  3. Run: cargo build --release -p app"
  echo "  4. Build the GUI and installers on Windows with:"
  echo "     cargo tauri build --bundles nsis,msi"
  echo ""
  echo "See BUILDING_WINDOWS.md for detailed instructions."
  exit 1
fi
