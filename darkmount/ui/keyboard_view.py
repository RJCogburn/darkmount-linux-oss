"""Rendered Dark Mount (main board + numpad + media dock) with lighting preview and
clickable keys, composed from the vendor's layout data (logos removed)."""
from __future__ import annotations

import json
import math
import random
import time
from dataclasses import dataclass

from PySide6.QtCore import QPointF, QRectF, Qt, QTimer, Signal
from PySide6.QtGui import QColor, QImage, QPainter, QPainterPath, QPen, QPixmap
from PySide6.QtWidgets import QWidget

from ..bindings import KEY_NAMES
from .theme import ACCENT, DM, N100

_MAP = json.load(open(DM / "dark_mount_keys_mapping.json"))
_JSON_TO_ID = dict(_MAP["base"])
for _d in _MAP["diff"].values():
    _JSON_TO_ID.update(_d)


def json_key_to_name(jid: str) -> str | None:
    kid = _JSON_TO_ID.get(jid)
    return KEY_NAMES.get(kid) if kid is not None else None


def _img(src: str) -> QImage:
    return QImage(str(DM / src.replace("qrc:/manifests/dark_mount/", "")))


@dataclass
class KeyGeom:
    name: str
    rect: QRectF
    radius: float


@dataclass
class Mask:
    image: QImage
    rect: QRectF


class KeyboardView(QWidget):
    keyClicked = Signal(str)

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setMouseTracking(True)
        self.setMinimumSize(500, 260)
        self.mode = "lighting"  # lighting | keys | plain
        self.numpad_pos = "Right"
        self.dock_pos = "Left"
        self.layout_name = "US"
        self.keys: list[KeyGeom] = []
        self.masks: list[Mask] = []
        self.base: QPixmap | None = None
        self.bounds = QRectF()
        self.hover: str | None = None
        self.selected: str | None = None
        self.highlighted: set[str] = set()
        self.dim_keys: set[str] = set()
        # lighting preview state
        self.light_on = True
        self.effect = "Static"
        self.colors: list[QColor] = [QColor("#ff2800")]
        self.brightness = 100
        self.speed = 50
        self.direction = "Right"
        self._t0 = time.monotonic()
        self._drops: dict[int, float] = {}
        self._flashes: dict[str, float] = {}
        self.timer = QTimer(self, interval=40, timeout=self.update)
        self.rebuild()

    # -- configuration ---------------------------------------------------------
    def set_modules(self, numpad: str | None, dock: str | None):
        numpad = numpad if numpad in ("Left", "Right") else None
        dock = dock if dock in ("Left", "Right") else None
        if (numpad, dock) != (self.numpad_pos, self.dock_pos):
            self.numpad_pos, self.dock_pos = numpad, dock
            self.rebuild()

    def set_mode(self, mode: str):
        if mode != self.mode:
            self.mode = mode
            self.rebuild()
            self._update_timer()

    def set_lighting(self, on: bool, effect: str, colors: list[QColor], brightness: int,
                     speed: int, direction: str):
        self.light_on, self.effect, self.colors = on, effect, colors or [QColor("#ff2800")]
        self.brightness, self.speed, self.direction = brightness, speed, direction
        self._update_timer()
        self.update()

    def _update_timer(self):
        animated = self.mode == "lighting" and self.light_on and self.effect != "Static"
        if animated and not self.timer.isActive():
            self.timer.start()
        elif not animated:
            self.timer.stop()

    # -- composition ---------------------------------------------------------------
    def rebuild(self):
        section = {"keys": "KeyBinding"}.get(self.mode, "Default")
        main = json.load(open(DM / "dark_mount_main_ansi.json"))
        num = json.load(open(DM / "dark_mount_numpad.json"))
        med = json.load(open(DM / "dark_mount_media.json"))
        parts = []  # (image, x offset)
        keys: list[KeyGeom] = []
        masks: list[Mask] = []

        def add_keys(lst, ox):
            for k in lst:
                g = k["geometry"]
                name = json_key_to_name(k["id"])
                if name:
                    keys.append(KeyGeom(name, QRectF(g["x"] + ox, g["y"], g["width"],
                                                     g["height"]), g.get("rx", 3)))

        def add_masks(lst, ox, disabled=()):
            for m in lst or []:
                if m["id"] in disabled:
                    continue
                r = m["mask"]["rect"]
                masks.append(Mask(_img(m["mask"]["source"]),
                                  QRectF(r["x"] + ox, r["y"], r["width"], r["height"])))

        m = main["Default"]
        parts.append((_img(m["layoutImages"].get(self.layout_name, m["layoutImages"]["US"])), 0))
        add_keys(m["keys"], 0)
        np_disabled = set()
        if self.numpad_pos:
            nsec = num.get(section) or num["Default"]
            ox = nsec["positions"][self.numpad_pos]["physicalGeometryOffset"]["x"]
            parts.append((_img(nsec["images"][self.numpad_pos]["Default"]), ox))
            add_keys(num["KeyBinding"]["keys"], ox)
            np_disabled = set(num["Default"].get("disabledKeys", {}).get(self.numpad_pos, []))
            add_masks(num["Default"].get("ledMasks"), ox)
        add_masks(m.get("ledMasks"), 0, np_disabled)
        if self.dock_pos:
            dsec = med["KeyBinding"] if self.mode == "keys" else med["Default"]
            ox = dsec["positions"][self.dock_pos]["physicalGeometryOffset"]["x"]
            parts.append((_img(dsec["images"][self.dock_pos]["Default"]), ox))
            if self.mode == "keys":
                add_keys(dsec.get("keys", []), ox)

        x0 = min(ox for _, ox in parts)
        x1 = max(ox + im.width() for im, ox in parts)
        h = max(im.height() for im, _ in parts)
        base = QImage(int(x1 - x0), h, QImage.Format_ARGB32_Premultiplied)
        base.fill(Qt.transparent)
        p = QPainter(base)
        # draw main first, then modules on top (as the official app does)
        for im, ox in parts:
            p.drawImage(int(ox - x0), 0, im)
        p.end()
        for k in keys:
            k.rect.translate(-x0, 0)
        for mk in masks:
            mk.rect.translate(-x0, 0)
            # pre-whiten mask alpha for tinting
        self.base = QPixmap.fromImage(base)
        self.keys, self.masks = keys, masks
        # crop vertical empty space (images are 670 tall with transparent margins)
        self.bounds = self._content_bounds(base)
        self.update()

    @staticmethod
    def _content_bounds(img: QImage) -> QRectF:
        w, h = img.width(), img.height()
        top, bottom = 0, h - 1
        step = max(1, w // 200)

        def row_empty(y):
            return all(img.pixelColor(x, y).alpha() == 0 for x in range(0, w, step))
        while top < h and row_empty(top):
            top += 4
        while bottom > top and row_empty(bottom):
            bottom -= 4
        pad = 30
        return QRectF(0, max(0, top - pad), w, min(h, bottom - top + 2 * pad))

    # -- geometry helpers ----------------------------------------------------------------
    def _transform(self):
        b = self.bounds
        s = min((self.width() - 40) / b.width(), (self.height() - 40) / b.height())
        s = max(0.1, s)
        ox = (self.width() - b.width() * s) / 2 - b.x() * s
        oy = (self.height() - b.height() * s) / 2 - b.y() * s
        return s, ox, oy

    def _key_at(self, pos: QPointF) -> str | None:
        s, ox, oy = self._transform()
        x, y = (pos.x() - ox) / s, (pos.y() - oy) / s
        for k in self.keys:
            if k.rect.adjusted(-2, -2, 2, 2).contains(x, y):
                return k.name
        return None

    # -- lighting colors -------------------------------------------------------------------
    def _lerp(self, a: QColor, b: QColor, t: float) -> QColor:
        return QColor(int(a.red() + (b.red() - a.red()) * t),
                      int(a.green() + (b.green() - a.green()) * t),
                      int(a.blue() + (b.blue() - a.blue()) * t))

    def _palette(self, u: float) -> QColor:
        """Cyclic gradient through self.colors, u in [0,1)."""
        cs = self.colors
        if len(cs) == 1:
            return cs[0]
        u = (u % 1.0) * len(cs)
        i = int(u)
        return self._lerp(cs[i % len(cs)], cs[(i + 1) % len(cs)], u - i)

    def _color_at(self, nx: float, ny: float, key: str | None, now: float) -> QColor | None:
        """nx, ny in [0,1] across the board. Returns None for 'off'."""
        if not self.light_on:
            return None
        spd = 0.05 + self.speed / 100 * 0.6
        t = (now - self._t0) * spd
        e = self.effect
        if e == "Static":
            c = self.colors[0]
        elif e == "Breathing":
            phase = (t * 0.6) % len(self.colors)
            base = self.colors[int(phase) % len(self.colors)]
            k = 0.5 - 0.5 * math.cos((phase % 1.0) * 2 * math.pi)
            c = QColor(int(base.red() * k), int(base.green() * k), int(base.blue() * k))
        elif e == "ColorWave":
            d = self.direction
            pos = {"Right": -nx, "Left": nx, "Down": -ny, "Up": ny}.get(d, -nx)
            c = self._palette(pos * 1.0 + t)
        elif e == "Tornado":
            ang = math.atan2(ny - 0.5, (nx - 0.5) * 2.6) / (2 * math.pi)
            sign = -1 if self.direction == "Counterclockwise" else 1
            c = self._palette(ang * sign + t)
        elif e == "Reactive":
            # idle: second color dim; random keys flash with the first color
            if key and random.random() < 0.0015:
                self._flashes[key] = now
            f = self._flashes.get(key or "", -10)
            k = max(0.0, 1 - (now - f) / (0.4 + (100 - self.speed) / 100))
            idle = self.colors[1] if len(self.colors) > 1 else QColor(0, 0, 0)
            c = self._lerp(idle, self.colors[0], k)
        elif e == "Matrix":
            col = int(nx * 40)
            if col not in self._drops or self._drops[col] > 1.4:
                self._drops[col] = -random.random() * 1.5
            dist = self._drops[col] - ny
            k = 1.0 if 0 <= dist < 0.06 else max(0.0, 0.6 - dist * 2.5) if dist >= 0.06 else 0
            head = self.colors[1] if len(self.colors) > 1 else QColor("#ffffff")
            c = head if 0 <= dist < 0.06 else self._lerp(QColor(0, 0, 0), self.colors[0], k)
        else:
            c = self.colors[0]
        b = self.brightness / 100
        return QColor(int(c.red() * b), int(c.green() * b), int(c.blue() * b))

    # -- events ------------------------------------------------------------------------------
    def mouseMoveEvent(self, ev):
        if self.mode != "keys":
            return
        k = self._key_at(ev.position())
        if k != self.hover:
            self.hover = k
            self.setCursor(Qt.PointingHandCursor if k else Qt.ArrowCursor)
            self.update()

    def leaveEvent(self, _):
        self.hover = None
        self.update()

    def mousePressEvent(self, ev):
        if self.mode == "keys" and ev.button() == Qt.LeftButton:
            k = self._key_at(ev.position())
            if k and k not in self.dim_keys:
                self.selected = k
                self.update()
                self.keyClicked.emit(k)

    # -- painting -------------------------------------------------------------------------------
    def paintEvent(self, _):
        if not self.base:
            return
        p = QPainter(self)
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        p.setRenderHint(QPainter.Antialiasing)
        s, ox, oy = self._transform()
        p.translate(ox, oy)
        p.scale(s, s)
        p.drawPixmap(0, 0, self.base)
        W, H = self.base.width(), self.base.height()
        now = time.monotonic()

        if self.mode == "lighting":
            if self.effect == "Matrix" and self.light_on:
                for c in list(self._drops):
                    self._drops[c] += 0.01 * (0.3 + self.speed / 60)
            # underglow
            p.setCompositionMode(QPainter.CompositionMode_Screen)
            for mk in self.masks:
                c = self._color_at(mk.rect.center().x() / W, mk.rect.center().y() / H, None, now)
                if c is None:
                    continue
                tint = QImage(mk.image.size(), QImage.Format_ARGB32_Premultiplied)
                tint.fill(c)
                tp = QPainter(tint)
                tp.setCompositionMode(QPainter.CompositionMode_DestinationIn)
                tp.drawImage(0, 0, mk.image)
                tp.end()
                p.drawImage(mk.rect, tint)
            # keycaps
            for k in self.keys:
                c = self._color_at(k.rect.center().x() / W, k.rect.center().y() / H, k.name, now)
                if c is None:
                    continue
                path = QPainterPath()
                path.addRoundedRect(k.rect.adjusted(1, 1, -1, -1), k.radius, k.radius)
                c.setAlpha(200)
                p.fillPath(path, c)
            p.setCompositionMode(QPainter.CompositionMode_SourceOver)

        elif self.mode == "keys":
            for k in self.keys:
                path = QPainterPath()
                path.addRoundedRect(k.rect.adjusted(-1, -1, 1, 1), k.radius + 1, k.radius + 1)
                if k.name in self.dim_keys:
                    p.fillPath(path, QColor(0, 0, 0, 150))
                    continue
                if k.name == self.selected:
                    c = QColor(ACCENT)
                    c.setAlpha(170)
                    p.fillPath(path, c)
                elif k.name in self.highlighted:
                    c = QColor(ACCENT)
                    c.setAlpha(70)
                    p.fillPath(path, c)
                    p.setPen(QPen(QColor(ACCENT), 1.6))
                    p.drawPath(path)
                if k.name == self.hover:
                    p.setPen(QPen(QColor(N100), 1.4))
                    p.drawPath(path)
        p.end()
