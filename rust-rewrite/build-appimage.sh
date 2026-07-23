#!/bin/bash
# Build VRChat Organizer as an AppImage
# Requires: cargo, npm, and appimagetool installed

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building VRChat Organizer AppImage ==="

# Step 1: Build the Tauri app in release mode
echo ">>> Building Tauri app (release)..."
cargo build --release -p app

# Step 2: Build the desktop binary (CLI version)
echo ">>> Building desktop CLI binary..."
cargo build --release -p desktop

# The Tauri binary is at ./target/release/app
# The desktop CLI binary is at ./target/release/desktop

# Step 3: Prepare AppDir structure
APPDIR="./target/VRChatOrganizer.AppDir"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# Copy binaries
echo ">>> Copying binaries..."
cp ./target/release/app "$APPDIR/usr/bin/vrchat-organizer"
cp ./target/release/desktop "$APPDIR/usr/bin/vrchat-organizer-cli"

# Copy desktop entry
cat > "$APPDIR/usr/share/applications/vrchat-organizer.desktop" << 'DESKTOP'
[Desktop Entry]
Name=VRChat Organizer
Comment=Organize VRChat screenshots by world name
Exec=vrchat-organizer
Icon=vrchat-organizer
Terminal=false
Type=Application
Categories=Utility;
DESKTOP

# Copy icon
cp "../icons/256x256/apps/vrchat-organizer.svg" "$APPDIR/usr/share/icons/hicolor/256x256/apps/vrchat-organizer.svg" 2>/dev/null || true

# Create AppStream metadata
mkdir -p "$APPDIR/usr/share/metainfo"
cat > "$APPDIR/usr/share/metainfo/vrchat-organizer.metainfo.xml" << 'METAINFO'
<?xml version="1.0" encoding="UTF-8"?>
<component type="desktop-application">
  <id>vrchat-organizer</id>
  <name>VRChat Organizer</name>
  <summary>Organize VRChat screenshots by world name</summary>
  <description>
    <p>Automatically sorts your VRChat screenshots into folders based on the world name embedded in the image metadata.</p>
  </description>
  <categories>
    <category>Utility</category>
  </categories>
</component>
METAINFO

# Symlinks for AppImage
ln -sf "usr/bin/vrchat-organizer" "$APPDIR/AppRun"
ln -sf "usr/share/applications/vrchat-organizer.desktop" "$APPDIR/vrchat-organizer.desktop"
ln -sf "usr/share/icons/hicolor/256x256/apps/vrchat-organizer.svg" "$APPDIR/vrchat-organizer.svg"

echo ">>> Determining AppImage version..."
# Scan existing release AppImages for versioning
VERSION=1
for f in "$SCRIPT_DIR"/target/VRChatOrganizer-v*-x86_64.AppImage; do
  if [ -f "$f" ]; then
    basename=$(basename "$f")
    # Extract version number from pattern VRChatOrganizer-v{N}-x86_64.AppImage
    num=$(echo "$basename" | sed 's/^VRChatOrganizer-v\([0-9]*\)-x86_64\.AppImage$/\1/')
    if [ -n "$num" ] && [ "$num" -ge "$VERSION" ] 2>/dev/null; then
      VERSION=$((num + 1))
    fi
  fi
done
APPIMAGE_NAME="VRChatOrganizer-v${VERSION}-x86_64.AppImage"
echo ">>> Building AppImage as: ${APPIMAGE_NAME}..."

# Locate appimagetool — try PATH first, then common locations
APPIMAGETOOL=""
if command -v appimagetool &> /dev/null; then
  APPIMAGETOOL="appimagetool"
elif [ -f /tmp/appimagetool ]; then
  APPIMAGETOOL="/tmp/appimagetool"
elif [ -f /usr/bin/appimagetool ]; then
  APPIMAGETOOL="/usr/bin/appimagetool"
fi

if [ -n "$APPIMAGETOOL" ]; then
  $APPIMAGETOOL "$APPDIR" "${SCRIPT_DIR}/target/${APPIMAGE_NAME}"
  echo "✅ AppImage created at: ./target/${APPIMAGE_NAME}"
else
  echo "⚠️  appimagetool not found. To create AppImage, install appimagetool and run:"
  echo "   appimagetool $APPDIR ${SCRIPT_DIR}/target/${APPIMAGE_NAME}"
  echo "AppDir is ready at: $APPDIR"
fi

echo "=== Build complete ==="
echo ""
echo "Tauri GUI app:    ./target/release/app"
echo "CLI tool:         ./target/release/desktop"
echo "AppImage:         ./target/${APPIMAGE_NAME} (if appimagetool was available)"

