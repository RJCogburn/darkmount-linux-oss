#!/usr/bin/env bash
# Build darkmount-linux as an AppImage: the desktop app and the `darkmount` CLI.
# Needs: cargo, rsync, curl and internet the first time (to fetch appimagetool).
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
BUILD="$ROOT/build"
APPDIR="$BUILD/AppDir"
DIST="$ROOT/dist"
NAME="darkmount-linux"

echo "==> cargo build --release"
(cd "$ROOT" && cargo build --release --locked)

rm -rf "$APPDIR"
mkdir -p "$APPDIR/usr/bin" "$APPDIR/usr/share/$NAME" "$DIST" "$BUILD"
install -m755 "$ROOT/target/release/$NAME" "$APPDIR/usr/bin/$NAME"
install -m755 "$ROOT/target/release/darkmount" "$APPDIR/usr/bin/darkmount"

echo "==> assets"
rsync -a "$ROOT/assets/" "$APPDIR/usr/share/$NAME/assets/"

cat > "$APPDIR/AppRun" <<'RUN'
#!/bin/sh
HERE="$(dirname "$(readlink -f "$0")")"
# the command-line tool: through a link named "darkmount", or "<AppImage> darkmount ..."
# (or "--cli ..." / "--daemon", as in the Python version)
if [ "$(basename "${ARGV0:-$0}")" = "darkmount" ]; then
    exec "$HERE/usr/bin/darkmount" "$@"
fi
case "${1:-}" in
    darkmount|--cli) shift; exec "$HERE/usr/bin/darkmount" "$@" ;;
    --daemon) shift; exec "$HERE/usr/bin/darkmount" daemon --interval 600 "$@" ;;
esac
exec "$HERE/usr/bin/darkmount-linux" "$@"
RUN
chmod +x "$APPDIR/AppRun"

cat > "$APPDIR/$NAME.desktop" <<DESK
[Desktop Entry]
Type=Application
Name=darkmount-linux
GenericName=Keyboard Settings
Comment=Unofficial offline settings app for the be quiet! Dark Mount keyboard
Exec=$NAME
Icon=$NAME
Categories=Settings;HardwareSettings;
Keywords=keyboard;rgb;lighting;dark mount;be quiet;
StartupWMClass=$NAME
DESK
install -m644 "$ROOT/packaging/$NAME.png" "$APPDIR/$NAME.png"
cp "$APPDIR/$NAME.png" "$APPDIR/.DirIcon"

echo "==> appimagetool"
TOOL="$BUILD/appimagetool-x86_64.AppImage"
if [ ! -x "$TOOL" ]; then
    curl -sSL -o "$TOOL" \
      https://github.com/AppImage/appimagetool/releases/download/continuous/appimagetool-x86_64.AppImage
    chmod +x "$TOOL"
fi
OUT="$DIST/$NAME-x86_64.AppImage"
ARCH=x86_64 APPIMAGE_EXTRACT_AND_RUN=1 "$TOOL" --no-appstream "$APPDIR" "$OUT" >/dev/null
(cd "$DIST" && sha256sum "$(basename "$OUT")" > "$(basename "$OUT").sha256")
echo "==> built $OUT ($(du -h "$OUT" | cut -f1))"
