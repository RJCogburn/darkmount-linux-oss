#!/usr/bin/env bash
# Build the .tar.gz (any distribution, with install.sh) and, when nfpm is installed,
# the .deb and .rpm, from target/release. Run `cargo build --release` (or
# build-appimage.sh) first.
set -euo pipefail

ROOT="$(cd "$(dirname "$0")/.." && pwd)"
VERSION="$(grep -m1 '^version' "$ROOT/Cargo.toml" | cut -d'"' -f2)"
DIST="$ROOT/dist"
NAME="darkmount-linux-$VERSION-x86_64"
STAGE="$ROOT/build/$NAME"
mkdir -p "$DIST"

echo "==> $NAME.tar.gz"
rm -rf "$STAGE"
mkdir -p "$STAGE/bin" "$STAGE/share/darkmount-linux" "$STAGE/share/applications" \
         "$STAGE/share/icons/hicolor/256x256/apps"
install -m755 "$ROOT/target/release/darkmount-linux" "$ROOT/target/release/darkmount" "$STAGE/bin/"
cp -r "$ROOT/assets" "$STAGE/share/darkmount-linux/"
install -m644 "$ROOT/packaging/darkmount-linux.desktop" "$STAGE/share/applications/"
install -m644 "$ROOT/packaging/darkmount-linux.png" "$STAGE/share/icons/hicolor/256x256/apps/"
install -m644 "$ROOT/70-bequiet-darkmount.rules" "$ROOT/LICENSE" "$ROOT/README.md" "$STAGE/"
install -m755 "$ROOT/packaging/install.sh" "$STAGE/"
tar -C "$(dirname "$STAGE")" -czf "$DIST/$NAME.tar.gz" "$NAME"

if command -v nfpm >/dev/null 2>&1; then
    for fmt in deb rpm; do
        echo "==> .$fmt"
        (cd "$ROOT" && VERSION="$VERSION" nfpm package -f packaging/nfpm.yaml -p "$fmt" -t "$DIST/")
    done
else
    echo "==> nfpm not installed: skipping .deb and .rpm"
fi
ls -la "$DIST"
