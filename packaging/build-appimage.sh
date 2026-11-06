#!/usr/bin/env bash
# Build darkmount-linux as a self-contained AppImage.
# Needs: uv, curl, internet (first build only, to fetch Python/PySide6/appimagetool).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
APPDIR="$BUILD/AppDir"
DIST="$ROOT/dist"
PYVER="${PYVER:-3.12}"
NAME="darkmount-linux"

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr" "$DIST" "$BUILD"

echo "==> Python $PYVER (standalone)"
uv python install "$PYVER" >/dev/null
PYBIN="$(uv python find "$PYVER")"
PYHOME="$(cd "$(dirname "$PYBIN")/.." && pwd)"
cp -a "$PYHOME" "$APPDIR/usr/python"
# drop bits we don't need
rm -rf "$APPDIR/usr/python/lib/python$PYVER/test" \
       "$APPDIR/usr/python/lib/python$PYVER/idlelib" \
       "$APPDIR/usr/python/lib/python$PYVER/tkinter" \
       "$APPDIR/usr/python/lib/python$PYVER/ensurepip" \
       "$APPDIR/usr/python/share" 2>/dev/null || true
find "$APPDIR/usr/python" -name "__pycache__" -prune -exec rm -rf {} +
rm -f "$APPDIR/usr/python/lib/python$PYVER/EXTERNALLY-MANAGED"

echo "==> PySide6-Essentials"
uv pip install --quiet --python "$APPDIR/usr/python/bin/python3" \
    --target "$APPDIR/usr/site" "PySide6-Essentials>=6.7"
QT="$APPDIR/usr/site/PySide6"
# trim modules this app never uses (keeps Core/Gui/Widgets/Svg/DBus/OpenGL/Wayland/XCB)
for m in Qml Quick QuickWidgets QuickControls2 QuickTest Designer Help Pdf PdfWidgets \
         Sql Test Xml Concurrent NetworkAuth PrintSupport UiTools StateMachine \
         SpatialAudio Multimedia MultimediaWidgets RemoteObjects Scxml Sensors SerialPort \
         SerialBus TextToSpeech WebChannel WebSockets HttpServer Positioning Nfc \
         Bluetooth 3D Charts DataVisualization Graphs Location; do
    rm -f "$QT"/Qt$m.*.so "$QT"/Qt$m.pyi "$QT"/Qt$m.abi3.so 2>/dev/null || true
done
rm -rf "$QT"/{designer,assistant,linguist,lupdate,lrelease,qmllint,qmlformat,qmlls,qmltyperegistrar} \
       "$QT"/Qt/qml "$QT"/Qt/translations "$QT"/include "$QT"/typesystems "$QT"/glue \
       "$QT"/scripts "$QT"/examples 2>/dev/null || true
find "$APPDIR/usr/site" -name "__pycache__" -prune -exec rm -rf {} +

echo "==> app"
mkdir -p "$APPDIR/usr/app"
rsync -a --exclude "__pycache__" "$ROOT/darkmount" "$APPDIR/usr/app/"

cat > "$APPDIR/AppRun" <<'EOF'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
export PYTHONPATH="$HERE/usr/app:$HERE/usr/site"
export PYTHONNOUSERSITE=1
unset PYTHONHOME
exec "$HERE/usr/python/bin/python3" -s -m darkmount.ui "$@"
EOF
chmod +x "$APPDIR/AppRun"

cat > "$APPDIR/darkmount-linux.desktop" <<EOF
[Desktop Entry]
Type=Application
Name=darkmount-linux
GenericName=Keyboard Settings
Comment=Unofficial offline settings app for the be quiet! Dark Mount keyboard
Exec=$NAME
Icon=darkmount-linux
Categories=Settings;HardwareSettings;Qt;
Keywords=keyboard;rgb;lighting;dark mount;
StartupWMClass=darkmount-linux
EOF

echo "==> icon"
QT_QPA_PLATFORM=offscreen PYTHONPATH="$APPDIR/usr/app:$APPDIR/usr/site" \
    "$APPDIR/usr/python/bin/python3" -s -c "
import sys
from PySide6.QtWidgets import QApplication
app = QApplication([])
from darkmount.ui.theme import app_icon
app_icon().pixmap(256, 256).save(sys.argv[1])
" "$APPDIR/darkmount-linux.png"
cp "$APPDIR/darkmount-linux.png" "$APPDIR/.DirIcon"

echo "==> appimagetool"
TOOL="$BUILD/appimagetool-x86_64.AppImage"
if [ ! -x "$TOOL" ]; then
    curl -sSL -o "$TOOL" \
      https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$TOOL"
fi
OUT="$DIST/$NAME-x86_64.AppImage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT" >/dev/null
echo "==> built $OUT ($(du -h "$OUT" | cut -f1))"
