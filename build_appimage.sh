#!/usr/bin/env bash
set -euo pipefail

APP_NAME="VRChat Organizer"
APP_DIR="VRChatOrganizer.AppDir"
OUTPUT="VRChatOrganizer.AppImage"
ICON_NAME="vrchat-organizer"
SCRIPT="organize_vrchat.py"

rm -rf "$APP_DIR"
mkdir -p "$APP_DIR/usr/bin"
mkdir -p "$APP_DIR/usr/lib/python3"
mkdir -p "$APP_DIR/usr/share/applications"
mkdir -p "$APP_DIR/usr/share/icons/hicolor/scalable/apps"
mkdir -p "$APP_DIR/usr/share/icons/hicolor/256x256/apps"

cp "$SCRIPT" "$APP_DIR/usr/bin/$SCRIPT"
cp "vrchat-organizer.desktop" "$APP_DIR/"
cp "vrchat-organizer.desktop" "$APP_DIR/usr/share/applications/"
cp "icons/256x256/apps/$ICON_NAME.svg" "$APP_DIR/usr/share/icons/hicolor/scalable/apps/"
cp "gui_vrchat_organizer.py" "$APP_DIR/usr/bin/gui_vrchat_organizer.py"
cp "icons/256x256/apps/$ICON_NAME.svg" "$APP_DIR/$ICON_NAME.svg"

BUILD_VENV=".appimage-build-venv"

if ! python3 -m venv "$BUILD_VENV" >/dev/null 2>&1; then
  echo "Error: Unable to create a Python virtual environment. Ensure python3-venv is installed."
  exit 1
fi

source "$BUILD_VENV/bin/activate"
python -m pip install --upgrade pip setuptools wheel
python -m pip install --target "$APP_DIR/usr/lib/python3" Pillow

# Clean up build virtual environment after packaging.
rm -rf "$BUILD_VENV"

cat > "$APP_DIR/AppRun" <<'EOF'
#!/usr/bin/env bash
HERE="$(dirname "$(readlink -f "${0}")")"
export PYTHONPATH="$HERE/usr/lib/python3:$PYTHONPATH"
exec python3 "$HERE/usr/bin/gui_vrchat_organizer.py" "$@"
EOF

chmod +x "$APP_DIR/AppRun"

if command -v appimagetool >/dev/null 2>&1; then
  appimagetool "$APP_DIR" "$OUTPUT"
else
  echo "appimagetool not found. Downloading AppImageKit appimagetool..."
  wget -qO appimagetool-x86_64.AppImage "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
  chmod +x appimagetool-x86_64.AppImage
  ./appimagetool-x86_64.AppImage "$APP_DIR" "$OUTPUT"
fi

echo "Built $OUTPUT"
