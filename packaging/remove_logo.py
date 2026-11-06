#!/usr/bin/env python3
"""Remove the vendor logo from the top plate of the main keyboard renders.

The plate is horizontally brushed metal, so the logo box is replaced row by row
with texture from the same rows further left, then each row is corrected with a
linear ramp so it meets the original pixels on both sides seamlessly.

Usage: remove_logo.py IMAGE [IMAGE...]   (files are rewritten in place)
"""
import sys

from PySide6.QtGui import QColor, QGuiApplication, QImage

BOX = (424, 200, 92, 30)   # x, y, w, h — covers the logo on all main_*.webp renders
SRC_DX = -110              # take texture from this far to the left


def clean(path: str):
    img = QImage(path).convertToFormat(QImage.Format_ARGB32)
    x0, y0, w, h = BOX
    for y in range(y0, y0 + h):
        # boundary colours (original image) just outside the box
        L = img.pixelColor(x0 - 1, y)
        R = img.pixelColor(x0 + w, y)
        # texture source and its own boundary colours
        sL = img.pixelColor(x0 - 1 + SRC_DX, y)
        sR = img.pixelColor(x0 + w + SRC_DX, y)
        for i in range(w):
            t = (i + 0.5) / w
            s = img.pixelColor(x0 + i + SRC_DX, y)
            out = []
            for ch in ("red", "green", "blue"):
                base = getattr(L, ch)() * (1 - t) + getattr(R, ch)() * t
                sbase = getattr(sL, ch)() * (1 - t) + getattr(sR, ch)() * t
                out.append(max(0, min(255, round(base + (getattr(s, ch)() - sbase)))))
            img.setPixelColor(x0 + i, y, QColor(*out, img.pixelColor(x0 + i, y).alpha()))
    if not img.save(path, "WEBP", 95):
        raise SystemExit(f"failed to save {path}")
    print("cleaned", path)


if __name__ == "__main__":
    app = QGuiApplication([])
    for p in sys.argv[1:]:
        clean(p)
