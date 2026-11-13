#!/bin/sh
# Install darkmount-linux from this folder (the release .tar.gz), on any distribution.
#
#   ./install.sh               for your user: ~/.local (no root, except for the udev rule)
#   ./install.sh --system      for everyone:  /usr/local (uses sudo)
#   ./install.sh --uninstall   remove it again (add --system for a system install)
#
# The programs also run straight from this folder: ./bin/darkmount-linux
set -eu

HERE="$(cd "$(dirname "$0")" && pwd)"
PREFIX="$HOME/.local"
SUDO=""
UNINSTALL=0
for arg in "$@"; do
    case "$arg" in
        --system) PREFIX="/usr/local"; SUDO="sudo" ;;
        --uninstall) UNINSTALL=1 ;;
        -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
        *) echo "unknown option: $arg (see --help)" >&2; exit 1 ;;
    esac
done
# root (e.g. a container) needs no sudo; everyone else needs it for the udev rule
ASROOT="sudo"
if [ "$(id -u)" = 0 ]; then SUDO=""; ASROOT=""; fi

RULE=/etc/udev/rules.d/70-bequiet-darkmount.rules
DESKTOP="$PREFIX/share/applications/darkmount-linux.desktop"
ICON="$PREFIX/share/icons/hicolor/256x256/apps/darkmount-linux.png"

if [ "$UNINSTALL" = 1 ]; then
    $SUDO rm -f "$PREFIX/bin/darkmount-linux" "$PREFIX/bin/darkmount" "$DESKTOP" "$ICON"
    $SUDO rm -rf "$PREFIX/share/darkmount-linux"
    echo "Removed darkmount-linux from $PREFIX."
    echo "Your settings stay in ~/.config/darkmount; the udev rule stays in $RULE"
    echo "(remove it with: sudo rm $RULE)."
    exit 0
fi

echo "==> Installing to $PREFIX"
$SUDO mkdir -p "$PREFIX/bin" "$PREFIX/share/darkmount-linux" "$(dirname "$DESKTOP")" "$(dirname "$ICON")"
$SUDO install -m755 "$HERE/bin/darkmount-linux" "$HERE/bin/darkmount" "$PREFIX/bin/"
$SUDO rm -rf "$PREFIX/share/darkmount-linux/assets"
$SUDO cp -r "$HERE/share/darkmount-linux/assets" "$PREFIX/share/darkmount-linux/"
# the menu entry runs the program by its full path ($PREFIX/bin may not be on PATH)
sed "s|^Exec=darkmount-linux|Exec=$PREFIX/bin/darkmount-linux|" \
    "$HERE/share/applications/darkmount-linux.desktop" | $SUDO tee "$DESKTOP" >/dev/null
$SUDO install -m644 "$HERE/share/icons/hicolor/256x256/apps/darkmount-linux.png" "$ICON"
command -v update-desktop-database >/dev/null 2>&1 && $SUDO update-desktop-database -q "$(dirname "$DESKTOP")" 2>/dev/null || true

if [ -f "$RULE" ] || [ -f /usr/lib/udev/rules.d/70-bequiet-darkmount.rules ]; then
    echo "==> udev rule already installed"
else
    echo "==> Installing the udev rule (gives your user access to the keyboard; asks for your password)"
    if $ASROOT install -m644 "$HERE/70-bequiet-darkmount.rules" "$RULE"; then
        { $ASROOT udevadm control --reload-rules && $ASROOT udevadm trigger --subsystem-match=hidraw; } 2>/dev/null || true
    else
        echo "   skipped: install it later from the app (Grant access) or with"
        echo "   sudo cp 70-bequiet-darkmount.rules /etc/udev/rules.d/"
    fi
fi

echo "==> Done. Start \"darkmount-linux\" from your application menu, or run:"
echo "   $PREFIX/bin/darkmount-linux        (desktop app)"
echo "   $PREFIX/bin/darkmount info         (command line)"
case ":$PATH:" in *":$PREFIX/bin:"*) ;; *) echo "   (add $PREFIX/bin to your PATH to type just 'darkmount')" ;; esac
