#!/bin/bash
# Build VRChat Organizer as an AppImage
# Requires: cargo and appimagetool installed

set -e
set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

echo "=== Building VRChat Organizer AppImage ==="
command -v cargo >/dev/null || { echo "cargo is required" >&2; exit 1; }
command -v sha256sum >/dev/null || { echo "sha256sum is required" >&2; exit 1; }

VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' src-tauri/Cargo.toml | head -n 1)"
[ -n "$VERSION" ] || { echo "Unable to determine application version" >&2; exit 1; }
RELEASE_DIR="$SCRIPT_DIR/../release/v${VERSION}"
mkdir -p "$RELEASE_DIR"
rm -f "$RELEASE_DIR"/VRChatOrganizer-"$VERSION"-x86_64.AppImage \
  "$RELEASE_DIR"/SHA256SUMS

# Step 1: Build the Tauri app in release mode
echo ">>> Building Tauri app (release)..."
cargo build --release -p app

# Step 2: Build the desktop binary (CLI version)
echo ">>> Building desktop CLI binary..."
cargo build --release -p desktop

# Step 3: Prepare AppDir structure
APPDIR="./target/VRChatOrganizer.AppDir"
rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin"
mkdir -p "$APPDIR/usr/share/applications"
mkdir -p "$APPDIR/usr/share/icons/hicolor/256x256/apps"

# Copy binaries
echo ">>> Copying binaries..."
cp ./target/release/app "$APPDIR/usr/bin/vrchat-organizer"
cp ./target/release/desktop "$APPDIR/usr/bin/vrchat-organizer-cli"

# Copy desktop entry
cp "$SCRIPT_DIR/packaging/linux/com.vrchat.organizer.desktop" \
  "$APPDIR/usr/share/applications/com.vrchat.organizer.desktop"

# Copy the release icon
cp "$SCRIPT_DIR/src-tauri/icons/256x256.png" \
  "$APPDIR/usr/share/icons/hicolor/256x256/apps/vrchat-organizer.png"

mkdir -p "$APPDIR/usr/share/metainfo"
cp "$SCRIPT_DIR/packaging/linux/com.vrchat.organizer.appdata.xml" \
  "$APPDIR/usr/share/metainfo/com.vrchat.organizer.appdata.xml"

# Symlinks for AppImage
ln -sf "usr/bin/vrchat-organizer" "$APPDIR/AppRun"
ln -sf "usr/share/applications/com.vrchat.organizer.desktop" "$APPDIR/com.vrchat.organizer.desktop"
ln -sf "usr/share/icons/hicolor/256x256/apps/vrchat-organizer.png" "$APPDIR/vrchat-organizer.png"

APPIMAGE_NAME="VRChatOrganizer-${VERSION}-x86_64.AppImage"
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
  "$APPIMAGETOOL" "$APPDIR" "${RELEASE_DIR}/${APPIMAGE_NAME}"
  sha256sum "${RELEASE_DIR}/${APPIMAGE_NAME}" > "${RELEASE_DIR}/SHA256SUMS"
  echo "✅ AppImage created at: ${RELEASE_DIR}/${APPIMAGE_NAME}"
else
  echo "appimagetool is required to create a release AppImage." >&2
  exit 1
fi

echo "=== Build complete ==="
echo ""
echo "Tauri GUI app:    ./target/release/app"
echo "CLI tool:         ./target/release/desktop"
echo "AppImage:         ${RELEASE_DIR}/${APPIMAGE_NAME}"
