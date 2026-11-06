"""Left-panel pages: Illumination, Key Binding, Media Dock, Product Settings."""
from __future__ import annotations

import datetime
import os
import shutil
import subprocess
from pathlib import Path

from PySide6.QtCore import QSize, Qt, QTimer, Signal
from PySide6.QtGui import QColor
from PySide6.QtWidgets import (QApplication, QButtonGroup, QCheckBox, QColorDialog, QComboBox,
                               QFileDialog,
                               QFrame, QGridLayout, QHBoxLayout, QLabel, QLineEdit,
                               QListWidget, QMessageBox, QPlainTextEdit, QPushButton,
                               QScrollArea, QSlider, QSpinBox, QStackedWidget, QToolButton,
                               QVBoxLayout, QWidget)

from .. import bindings as B
from ..device import (ClockMode, ColorMode, DarkMount, Direction, Effect, GameModeFlags,
                      LightingConfig, LightingMode, ScreensaverMode, fmt_color)
from .theme import ACCENT, N100, N300, PRESETS, Swatch, Toggle, icon

RUN_MARKER = "darkmount:run"
DISPLAY_ICONS = {"Image0": "⌨ keyboard", "Image1": "🌐 internet", "Image2": "📁 folder",
                 "Image3": "✉ email", "Image4": "✂ scissors", "Image5": "〰 wave",
                 "Image6": "🔒 lock", "Image7": "🌙 moon"}


def key_title(name: str) -> str:
    if name in DISPLAY_ICONS:
        return f"Display key {name[-1]} ({DISPLAY_ICONS[name]})"
    return {"DockMute": "Media dock: top left", "DockPlayPause": "Media dock: top right",
            "DockPrev": "Media dock: bottom left", "DockNext": "Media dock: bottom right",
            "Grave": "` ~", "Minus": "- _", "Equals": "= +", "BracketLeft": "[ {",
            "BracketRight": "] }", "Backslash": "\\ |", "Semicolon": "; :",
            "Apostrophe": "' \"", "Comma": ", <", "Period": ". >", "Slash": "/ ?",
            "LeftGui": "Left Win", "RightGui": "Right Win"}.get(name, name)


def label(text, name="Section"):
    w = QLabel(text)
    w.setObjectName(name)
    return w


def info_label(text, tip):
    row = QHBoxLayout()
    row.setSpacing(6)
    row.addWidget(label(text))
    i = QLabel()
    i.setPixmap(icon("info", N300, None, 14).pixmap(14, 14))
    i.setToolTip(tip)
    row.addWidget(i)
    row.addStretch()
    return row


class Page(QWidget):
    """Scrollable left-panel page with a title row (optionally with a Toggle)."""
    changed = Signal()

    def __init__(self, title: str, toggle: bool = False):
        super().__init__()
        outer = QVBoxLayout(self)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)
        head = QHBoxLayout()
        head.setContentsMargins(16, 24, 16, 12)
        head.addWidget(label(title, "PageTitle"))
        head.addStretch()
        self.toggle = Toggle() if toggle else None
        if self.toggle:
            head.addWidget(self.toggle)
        outer.addLayout(head)
        scroll = QScrollArea()
        scroll.setWidgetResizable(True)
        scroll.setFrameShape(QFrame.NoFrame)
        body = QWidget()
        self.body = QVBoxLayout(body)
        self.body.setContentsMargins(16, 8, 16, 16)
        self.body.setSpacing(10)
        scroll.setWidget(body)
        outer.addWidget(scroll, 1)
        self.footer = QVBoxLayout()
        self.footer.setContentsMargins(16, 8, 16, 12)
        outer.addLayout(self.footer)
        self.loading = False

    def footer_row(self, text, button_text, slot):
        row = QHBoxLayout()
        row.addWidget(label(text))
        row.addStretch()
        b = QPushButton(button_text)
        b.setFixedWidth(124)
        b.clicked.connect(slot)
        row.addWidget(b)
        self.footer.addLayout(row)
        return b


class ColorPicker(QWidget):
    """'COLOR ... CHANGE' + big color bar + presets row."""
    colorChanged = Signal(QColor)

    def __init__(self, title="COLOR"):
        super().__init__()
        lay = QVBoxLayout(self)
        lay.setContentsMargins(0, 0, 0, 0)
        lay.setSpacing(8)
        head = QHBoxLayout()
        head.addWidget(label(title))
        head.addStretch()
        ch = QPushButton("CHANGE")
        ch.setObjectName("Link")
        ch.setCursor(Qt.PointingHandCursor)
        ch.clicked.connect(self._dialog)
        head.addWidget(ch)
        lay.addLayout(head)
        self.bar = Swatch("#ff2800", 349, 48)
        self.bar.setSizePolicy(self.bar.sizePolicy().horizontalPolicy(),
                               self.bar.sizePolicy().verticalPolicy())
        self.bar.setMinimumWidth(100)
        self.bar.setMaximumWidth(16777215)
        self.bar.setStyleSheet("")
        self.bar.clicked.connect(self._dialog)
        lay.addWidget(self.bar)
        lay.addSpacing(6)
        lay.addLayout(info_label("PRESETS", "Click a preset to apply it."))
        row = QHBoxLayout()
        row.setSpacing(12)
        for c in PRESETS:
            s = Swatch(c, 22, 22)
            s.picked.connect(lambda c: self.set_color(QColor(c), emit=True))
            row.addWidget(s)
        row.addStretch()
        lay.addLayout(row)

    def color(self) -> QColor:
        return QColor(self.bar.color)

    def set_color(self, c: QColor, emit=False):
        self.bar.set_color(c.name())
        if emit:
            self.colorChanged.emit(c)

    def _dialog(self):
        c = QColorDialog.getColor(self.color(), self, "Choose color")
        if c.isValid():
            self.set_color(c, emit=True)


def slider_row(title):
    box = QVBoxLayout()
    box.setSpacing(6)
    box.addWidget(label(title))
    s = QSlider(Qt.Horizontal)
    s.setRange(0, 100)
    box.addWidget(s)
    return box, s


def segmented(options: list[str]) -> tuple[QWidget, QButtonGroup]:
    w = QWidget()
    lay = QHBoxLayout(w)
    lay.setContentsMargins(0, 0, 0, 0)
    lay.setSpacing(0)
    g = QButtonGroup(w)
    for i, o in enumerate(options):
        b = QPushButton(o)
        b.setObjectName("Seg")
        b.setCheckable(True)
        b.setCursor(Qt.PointingHandCursor)
        g.addButton(b, i)
        lay.addWidget(b)
    return w, g


# --------------------------------------------------------------------------- Illumination
EFFECT_UI = [("Static", "static", Effect.Static), ("Color Wave", "wave", Effect.ColorWave),
             ("Tornado", "tornado", Effect.Tornado), ("Breathing", "breathing", Effect.Breathing),
             ("Reactive", "reactive", Effect.Reactive), ("Matrix", "matrix", Effect.Matrix)]
EFFECT_DIRS = {Effect.ColorWave: [Direction.Left, Direction.Right, Direction.Up, Direction.Down],
               Effect.Tornado: [Direction.Clockwise, Direction.Counterclockwise],
               Effect.Matrix: [Direction.Down, Direction.Up, Direction.Left, Direction.Right]}
EFFECT_MODES = {Effect.Static: [ColorMode.SingleColor],
                Effect.ColorWave: [ColorMode.Gradient, ColorMode.SingleColor, ColorMode.DualColor],
                Effect.Tornado: [ColorMode.Gradient, ColorMode.SingleColor, ColorMode.DualColor],
                Effect.Breathing: [ColorMode.Gradient, ColorMode.SingleColor, ColorMode.DualColor],
                Effect.Reactive: [ColorMode.DualColor, ColorMode.SingleColor, ColorMode.Gradient],
                Effect.Matrix: [ColorMode.Gradient, ColorMode.DualColor]}
MODE_NAMES = {ColorMode.SingleColor: "SINGLE", ColorMode.DualColor: "DUAL",
              ColorMode.Gradient: "SPECTRUM"}
RAINBOW = ["#ff0000", "#ffc800", "#00ff00", "#00c8ff", "#0000ff", "#ff00ff"]


class IlluminationPage(Page):
    preview = Signal()

    def __init__(self, app):
        super().__init__("ILLUMINATION", toggle=True)
        self.app = app
        self.apply_timer = QTimer(self, singleShot=True, interval=250, timeout=self.apply)
        self.toggle.toggled.connect(self._toggled)

        self.body.addWidget(label("EFFECTS"))
        box = QFrame()
        box.setObjectName("TileBox")
        grid = QGridLayout(box)
        grid.setContentsMargins(4, 4, 4, 4)
        grid.setSpacing(0)
        self.effects = QButtonGroup(self)
        for i, (text, ic, eff) in enumerate(EFFECT_UI):
            b = QToolButton()
            b.setObjectName("Tile")
            b.setText(text)
            b.setIcon(icon(ic))
            b.setIconSize(QSize(20, 20))
            b.setToolButtonStyle(Qt.ToolButtonTextUnderIcon)
            b.setCheckable(True)
            b.setFixedSize(85, 56)
            b.setCursor(Qt.PointingHandCursor)
            self.effects.addButton(b, int(eff))
            grid.addWidget(b, i // 4, i % 4)
        self.effects.idClicked.connect(self._effect_clicked)
        self.body.addWidget(box)
        self.body.addSpacing(8)

        self.mode_label = label("COLOR MODE")
        self.body.addWidget(self.mode_label)
        self.mode_w, self.mode_g = segmented(["SINGLE", "DUAL", "SPECTRUM"])
        self.mode_g.idClicked.connect(self._mode_clicked)
        self.body.addWidget(self.mode_w)

        self.picker = ColorPicker("COLOR")
        self.picker.colorChanged.connect(lambda c: self._set_color(0, c))
        self.body.addWidget(self.picker)
        self.picker2 = ColorPicker("SECOND COLOR")
        self.picker2.colorChanged.connect(lambda c: self._set_color(1, c))
        self.body.addWidget(self.picker2)

        self.spectrum_box = QWidget()
        sl = QVBoxLayout(self.spectrum_box)
        sl.setContentsMargins(0, 0, 0, 0)
        sh = QHBoxLayout()
        sh.addWidget(label("SPECTRUM"))
        sh.addStretch()
        self.add_stop = QPushButton("+")
        self.del_stop = QPushButton("−")
        for b in (self.add_stop, self.del_stop):
            b.setFixedSize(26, 22)
            sh.addWidget(b)
        self.add_stop.clicked.connect(lambda: self._stops(+1))
        self.del_stop.clicked.connect(lambda: self._stops(-1))
        sl.addLayout(sh)
        self.stops_row = QHBoxLayout()
        self.stops_row.setSpacing(6)
        sl.addLayout(self.stops_row)
        self.body.addWidget(self.spectrum_box)
        self.stop_swatches: list[Swatch] = []

        self.dir_label = label("DIRECTION")
        self.body.addWidget(self.dir_label)
        self.direction = QComboBox()
        self.direction.currentIndexChanged.connect(self._changed)
        self.body.addWidget(self.direction)

        b_box, self.brightness = slider_row("BRIGHTNESS")
        s_box, self.speed = slider_row("SPEED")
        self.speed_box = QWidget()
        s_box.setContentsMargins(0, 6, 0, 0)
        self.speed_box.setLayout(s_box)
        self.brightness.valueChanged.connect(self._changed)
        self.speed.valueChanged.connect(self._changed)
        self.body.addLayout(b_box)
        self.body.addWidget(self.speed_box)
        self.body.addStretch()
        self.colors = [QColor("#ff2800")]
        self.color_mode = ColorMode.SingleColor

    # -- state ----------------------------------------------------------------
    def load(self, kb: DarkMount):
        self.loading = True
        try:
            mode = kb.lighting_mode()
            cfg = kb.lighting()
            on = mode != LightingMode.Off and cfg.effect != Effect.Off
            self.toggle.setChecked(on)
            eff = cfg.effect if cfg.effect in EFFECT_MODES else Effect.Static
            self.effects.button(int(eff)).setChecked(True)
            self.color_mode = cfg.color_mode if cfg.color_mode in EFFECT_MODES[eff] \
                else EFFECT_MODES[eff][0]
            if cfg.color_mode in (ColorMode.Gradient, ColorMode.OrientedGradient):
                self.colors = [QColor(*c) for c, _ in cfg.colors] or [QColor(c) for c in RAINBOW]
            else:
                self.colors = [QColor(*c) for c in cfg.colors]
            if eff == Effect.Off or not self.colors or all(c.name() == "#000000"
                                                          for c in self.colors):
                self.colors = [QColor("#ff2800")]
            self._fill_dirs(eff, cfg.direction)
            self.brightness.setValue(cfg.brightness)
            self.speed.setValue(cfg.speed)
            self._refresh()
        finally:
            self.loading = False
        self.preview.emit()

    def effect(self) -> Effect:
        return Effect(self.effects.checkedId()) if self.effects.checkedId() >= 0 else Effect.Static

    def _fill_dirs(self, eff, current=None):
        self.direction.blockSignals(True)
        self.direction.clear()
        for d in EFFECT_DIRS.get(eff, []):
            self.direction.addItem(d.name, int(d))
        if current is not None and self.direction.findData(int(current)) >= 0:
            self.direction.setCurrentIndex(self.direction.findData(int(current)))
        self.direction.blockSignals(False)

    def _refresh(self):
        eff = self.effect()
        modes = EFFECT_MODES[eff]
        for i, m in enumerate([ColorMode.SingleColor, ColorMode.DualColor, ColorMode.Gradient]):
            b = self.mode_g.button(i)
            b.setVisible(m in modes)
            b.setChecked(m == self.color_mode)
        multi = len(modes) > 1
        self.mode_label.setVisible(multi)
        self.mode_w.setVisible(multi)
        grad = self.color_mode == ColorMode.Gradient
        self.picker.setVisible(not grad)
        self.picker2.setVisible(self.color_mode == ColorMode.DualColor)
        self.spectrum_box.setVisible(grad)
        if grad and len(self.colors) < 2:
            self.colors = [QColor(c) for c in RAINBOW]
        if self.color_mode == ColorMode.DualColor and len(self.colors) < 2:
            self.colors = self.colors[:1] + [QColor("#0078ff")]
        self.picker.set_color(self.colors[0])
        if len(self.colors) > 1:
            self.picker2.set_color(self.colors[1])
        for s in self.stop_swatches:
            s.deleteLater()
        self.stop_swatches = []
        if grad:
            for i, c in enumerate(self.colors):
                s = Swatch(c.name(), 36, 28)
                s.clicked.connect(lambda _=False, i=i: self._pick_stop(i))
                self.stops_row.addWidget(s)
                self.stop_swatches.append(s)
        self.add_stop.setEnabled(len(self.colors) < 8)
        self.del_stop.setEnabled(len(self.colors) > 2)
        has_dir = bool(EFFECT_DIRS.get(eff))
        self.dir_label.setVisible(has_dir)
        self.direction.setVisible(has_dir)
        self.speed_box.setVisible(eff != Effect.Static)
        on = self.toggle.isChecked()
        for w in (self.effects.buttons()):
            w.setEnabled(on)

    def preview_args(self):
        eff = self.effect()
        cols = self.colors[:1] if self.color_mode == ColorMode.SingleColor else \
            self.colors[:2] if self.color_mode == ColorMode.DualColor else self.colors
        d = self.direction.currentData()
        return (self.toggle.isChecked(), eff.name, cols, self.brightness.value(),
                self.speed.value(), Direction(d).name if d is not None else "Right")

    # -- events ------------------------------------------------------------------
    def _toggled(self, on):
        self._refresh()
        self.preview.emit()
        if self.loading:
            return
        if on:
            self.apply_timer.start()
        else:
            self.app.run(lambda kb: kb.set_lighting_mode(LightingMode.Off))

    def _effect_clicked(self, eid):
        eff = Effect(eid)
        if self.color_mode not in EFFECT_MODES[eff]:
            self.color_mode = EFFECT_MODES[eff][0]
        self._fill_dirs(eff, {Effect.ColorWave: Direction.Right, Effect.Tornado:
                              Direction.Clockwise, Effect.Matrix: Direction.Down}.get(eff))
        self._refresh()
        self._changed()

    def _mode_clicked(self, i):
        self.color_mode = [ColorMode.SingleColor, ColorMode.DualColor, ColorMode.Gradient][i]
        self._refresh()
        self._changed()

    def _set_color(self, i, c):
        while len(self.colors) <= i:
            self.colors.append(QColor(c))
        self.colors[i] = c
        self._changed()

    def _pick_stop(self, i):
        c = QColorDialog.getColor(self.colors[i], self, "Choose color")
        if c.isValid():
            self.colors[i] = c
            self._refresh()
            self._changed()

    def _stops(self, d):
        if d > 0:
            self.colors.append(QColor(RAINBOW[len(self.colors) % len(RAINBOW)]))
        elif len(self.colors) > 2:
            self.colors.pop()
        self._refresh()
        self._changed()

    def _changed(self, *_):
        self.preview.emit()
        if not self.loading and self.toggle.isChecked():
            self.apply_timer.start()

    def config(self) -> LightingConfig:
        on, eff, cols, bri, spd, _ = self.preview_args()
        rgb = [(c.red(), c.green(), c.blue()) for c in cols]
        if self.color_mode == ColorMode.Gradient:
            n = len(rgb)
            rgb = [(c, round(i * 100 / (n - 1))) for i, c in enumerate(rgb)]
        d = self.direction.currentData()
        return LightingConfig(effect=self.effect(), direction=int(d) if d is not None else 0,
                              brightness=bri, speed=spd, color_mode=self.color_mode, colors=rgb)

    def apply(self):
        cfg = self.config()
        self.app.run(lambda kb: kb.set_lighting(cfg))


# --------------------------------------------------------------------------- Key binding
ACTION_TYPES = ["Default (no remap)", "Keyboard key / shortcut", "Media", "Mouse button",
                "Mouse scroll", "Lighting", "System shortcut", "Run Linux command",
                "Open file", "Open folder", "Open website", "Disable key"]


class KeyBindingPage(Page):
    def __init__(self, app):
        super().__init__("KEY BINDING", toggle=False)
        self.app = app
        self.layer = 0
        self.bindings: list[B.Binding] = []
        self.commands: dict[str, str] = {}
        self.key: str | None = None

        seg, self.layer_g = segmented(["REMAP", "FN LAYER"])
        self.layer_g.button(0).setChecked(True)
        self.layer_g.idClicked.connect(self._layer)
        self.body.addWidget(seg)

        self.stack = QStackedWidget()
        # empty state
        empty = QWidget()
        el = QVBoxLayout(empty)
        el.addStretch()
        ic = QLabel()
        ic.setPixmap(icon("keybind", ACCENT, None, 56).pixmap(56, 56))
        el.addWidget(ic, 0, Qt.AlignHCenter)
        t = label("SELECT KEY\nTO SET UP A\nFUNCTION", "Big")
        el.addWidget(t, 0, Qt.AlignHCenter)
        el.addStretch()
        self.stack.addWidget(empty)
        # editor
        ed = QWidget()
        lay = QVBoxLayout(ed)
        lay.setContentsMargins(0, 8, 0, 0)
        lay.setSpacing(10)
        self.key_kicker = label("", "Kicker")
        self.key_name = label("", "DeviceName")
        lay.addWidget(self.key_kicker)
        lay.addWidget(self.key_name)
        self.current = label("", "Muted")
        self.current.setWordWrap(True)
        lay.addWidget(self.current)
        lay.addWidget(label("ASSIGN FUNCTION"))
        self.types = QListWidget()
        self.types.addItems(ACTION_TYPES)
        self.types.setFixedHeight(len(ACTION_TYPES) * 31)
        self.types.currentRowChanged.connect(self._type_changed)
        lay.addWidget(self.types)
        self.params = QStackedWidget()
        self._build_params()
        lay.addWidget(self.params)
        self.hint = label("", "Muted")
        self.hint.setWordWrap(True)
        lay.addWidget(self.hint)
        row = QHBoxLayout()
        self.apply_btn = QPushButton("Apply")
        self.apply_btn.setObjectName("Primary")
        self.apply_btn.clicked.connect(self.apply)
        row.addStretch()
        row.addWidget(self.apply_btn)
        lay.addLayout(row)
        lay.addStretch()
        self.stack.addWidget(ed)
        self.body.addWidget(self.stack, 1)
        self.footer_row("RESET ALL KEYS TO DEFAULT", "Reset", self.reset_all)

    def _build_params(self):
        P = self.params
        P.addWidget(QWidget())  # default
        self.combo = QLineEdit(placeholderText="e.g.  Escape   F13   ctrl+shift+T   super+E")
        P.addWidget(self.combo)
        self.media = QComboBox()
        for e in B.Media:
            self.media.addItem(e.name, int(e))
        P.addWidget(self.media)
        self.mouse = QComboBox()
        for e in B.MouseButton:
            self.mouse.addItem(e.name, int(e))
        P.addWidget(self.mouse)
        self.scroll = QComboBox()
        for e in B.MouseScroll:
            self.scroll.addItem(e.name, int(e))
        P.addWidget(self.scroll)
        self.light = QComboBox()
        for e in B.BacklightAction:
            if e != B.BacklightAction.SelectEffect:
                self.light.addItem(e.name, int(e))
        P.addWidget(self.light)
        self.host = QComboBox()
        for e in B.HostAction:
            self.host.addItem(e.name, int(e))
        P.addWidget(self.host)
        self.command = QPlainTextEdit()
        self.command.setPlaceholderText("Shell command, e.g.  konsole")
        self.command.setFixedHeight(80)
        P.addWidget(self.command)
        pw = QWidget()
        pl = QHBoxLayout(pw)
        pl.setContentsMargins(0, 0, 0, 0)
        self.path = QLineEdit()
        br = QPushButton("…")
        br.setFixedWidth(34)
        br.clicked.connect(self._browse)
        pl.addWidget(self.path)
        pl.addWidget(br)
        P.addWidget(pw)
        self.path_page = pw
        self.url = QLineEdit(placeholderText="https://…")
        P.addWidget(self.url)
        P.addWidget(QWidget())  # disable
        self.param_index = [0, 1, 2, 3, 4, 5, 6, 7, 8, 8, 9, 10]

    def _browse(self):
        if self.types.currentRow() == 9:
            p = QFileDialog.getExistingDirectory(self, "Folder")
        else:
            p, _ = QFileDialog.getOpenFileName(self, "File")
        if p:
            self.path.setText(p)

    def _type_changed(self, row):
        if row < 0:
            return
        self.params.setCurrentIndex(self.param_index[row])
        self.hint.setText({
            6: "Sent by the keyboard itself as a Windows-style shortcut "
               "(Explorer = Win+E, Lock = Win+L, Task Manager = Ctrl+Shift+Esc…).",
            7: "Runs through the darkmount background service.",
            8: "Opened by the darkmount background service.",
            9: "Opened by the darkmount background service.",
            10: "Opened by the darkmount background service.",
        }.get(row, ""))

    def _layer(self, i):
        self.layer = i
        self.key = None
        self.app.view.selected = None
        self.stack.setCurrentIndex(0)
        self._update_view()

    def label_for(self, name):
        return ("Fn+" if self.layer else "") + name

    def load(self, kb: DarkMount):
        from ..config import read_actions
        self.bindings = kb.bindings()
        self.commands = {k.lower(): v for k, v in read_actions().items()}
        self._update_view()
        if self.key:
            self.select(self.key)

    def _update_view(self):
        v = self.app.view
        v.highlighted = {b.key_name for b in self.bindings if b.layer == self.layer}
        v.dim_keys = {"Fn"} | ({"Image0", "Image1", "Image2", "Image3", "Image4", "Image5",
                                 "Image6", "Image7", "DockMute", "DockPlayPause", "DockPrev",
                                 "DockNext"} if self.layer else set())
        v.update()

    def binding_for(self, name):
        for b in self.bindings:
            if b.key_name == name and b.layer == self.layer:
                return b
        return None

    def select(self, name: str):
        self.key = name
        self.stack.setCurrentIndex(1)
        self.key_kicker.setText("FN LAYER" if self.layer else "KEY")
        self.key_name.setText(key_title(name))
        b = self.binding_for(name)
        cmd = self.commands.get(self.label_for(name).lower(), "")
        if cmd:
            first = cmd.strip().splitlines()[0]
            self.current.setText(f"Current: run “{first}”" + (" …" if "\n" in cmd.strip() else ""))
        else:
            self.current.setText("Current: " + (b.describe() if b else "default"))
        row = 0
        if b:
            row = {B.BindType.StandardKey: 1, B.BindType.Media: 2, B.BindType.Backlight: 5,
                   B.BindType.WindowsShortcut: 6, B.BindType.OpenFile: 8,
                   B.BindType.OpenFolder: 9, B.BindType.OpenBrowser: 10,
                   B.BindType.Disable: 11}.get(b.type, 0)
            if b.type == B.BindType.Mouse:
                row = 4 if b.extra.get("scroll") else 3
            if b.type == B.BindType.OpenFile and b.text == RUN_MARKER:
                row = 7
        if cmd:
            row = 7
        self.types.setCurrentRow(row)
        self.combo.clear()
        self.command.setPlainText(cmd)
        self.path.clear()
        self.url.clear()
        if b and row == 1:
            mods = []
            for m in B.Modifier:
                if m and b.modifiers & m:
                    n = m.name.lower()
                    n = n[1:] if n.startswith("l") else n
                    mods.append({"gui": "super", "rgui": "rsuper"}.get(n, n))
            self.combo.setText("+".join(mods + [B.HID_NAMES.get(b.usage, "")]))
        for r, w in ((2, self.media), (3, self.mouse), (4, self.scroll), (5, self.light),
                     (6, self.host)):
            if b and row == r:
                w.setCurrentIndex(max(0, w.findData(b.action)))
        if b and row in (8, 9):
            self.path.setText(b.text)
        if b and row == 10:
            self.url.setText(b.text)

    def _record(self):
        key = B.KEY_IDS[self.key]
        L = self.layer
        row = self.types.currentRow()
        if row == 0:
            return B.encode_clear(key, L), None
        if row == 1:
            mods, usage = B.parse_combo(self.combo.text())
            return B.encode_standard_key(key, L, usage, mods), None
        if row == 2:
            return B.encode_media(key, L, B.Media(self.media.currentData())), None
        if row == 3:
            return B.encode_mouse_button(key, L, B.MouseButton(self.mouse.currentData())), None
        if row == 4:
            return B.encode_mouse_scroll(key, L, B.MouseScroll(self.scroll.currentData())), None
        if row == 5:
            return B.encode_backlight(key, L, B.BacklightAction(self.light.currentData())), None
        if row == 6:
            return B.encode_host_action(key, L, B.HostAction(self.host.currentData())), None
        if row == 7:
            cmd = self.command.toPlainText().strip()
            if not cmd:
                raise ValueError("Enter a command")
            b = self.binding_for(self.key)
            if b and b.type in (B.BindType.OpenFile, B.BindType.OpenFolder,
                                B.BindType.OpenBrowser):
                return b.data, cmd  # keep existing on-keyboard binding (e.g. for Windows)
            return B.encode_open(key, L, B.BindType.OpenFile, RUN_MARKER), cmd
        if row in (8, 9):
            if not self.path.text().strip():
                raise ValueError("Choose a path")
            kind = B.BindType.OpenFile if row == 8 else B.BindType.OpenFolder
            return B.encode_open(key, L, kind, self.path.text().strip()), None
        if row == 10:
            if not self.url.text().strip():
                raise ValueError("Enter a URL")
            return B.encode_open(key, L, B.BindType.OpenBrowser, self.url.text().strip()), None
        return B.encode_disable(key, L), None

    def apply(self):
        from ..config import set_action
        try:
            rec, cmd = self._record()
        except ValueError as e:
            QMessageBox.warning(self, "Key binding", str(e))
            return
        lbl = self.label_for(self.key)

        def do(kb):
            kb.set_binding(rec)
            set_action(lbl, cmd)
            self.load(kb)
        self.app.run(do)

    def reset_all(self):
        if QMessageBox.question(self, "Reset key bindings",
                                "Reset ALL keys (both layers) to their default functions?\n"
                                "This also removes your display key actions.") \
                != QMessageBox.Yes:
            return
        from ..config import set_action

        def do(kb):
            for b in kb.bindings():
                kb.clear_binding(b.key, b.layer)
                set_action(("Fn+" if b.layer else "") + b.key_name, None)
            self.load(kb)
        self.app.run(do)


# --------------------------------------------------------------------------- Media dock
class MediaDockPage(Page):
    def __init__(self, app):
        super().__init__("MEDIA DOCK")
        self.app = app
        self.apply_timer = QTimer(self, singleShot=True, interval=300, timeout=self.apply)
        self.picker = ColorPicker("MENU COLOR")
        self.picker.colorChanged.connect(self._changed)
        self.body.addWidget(self.picker)
        self.body.addSpacing(6)

        self.body.addLayout(info_label("IDLE GRAPHIC", "What the dock shows when idle."))
        r = QHBoxLayout()
        self.idle_on = QCheckBox("Show idle screen after")
        self.idle_s = QSpinBox()
        self.idle_s.setRange(5, 3600)
        self.idle_s.setSuffix(" s")
        self.idle_s.setFixedWidth(110)
        r.addWidget(self.idle_on)
        r.addStretch()
        r.addWidget(self.idle_s)
        self.body.addLayout(r)

        v = QHBoxLayout()
        v.addLayout(info_label("VIEW", "Idle screen content."))
        self.view_w, self.view_g = segmented(["IMAGE", "CLOCK"])
        self.view_w.setFixedWidth(220)
        v.addWidget(self.view_w)
        self.body.addLayout(v)

        c = QHBoxLayout()
        c.addLayout(info_label("CLOCK TYPE", "12 or 24 hour clock."))
        self.clock_w, self.clock_g = segmented(["12H", "24H"])
        self.clock_w.setFixedWidth(220)
        c.addWidget(self.clock_w)
        self.body.addLayout(c)

        r2 = QHBoxLayout()
        self.off_on = QCheckBox("Turn off screen after")
        self.off_s = QSpinBox()
        self.off_s.setRange(5, 36000)
        self.off_s.setSuffix(" s")
        self.off_s.setFixedWidth(110)
        r2.addWidget(self.off_on)
        r2.addStretch()
        r2.addWidget(self.off_s)
        self.body.addLayout(r2)

        self.body.addSpacing(10)
        t = QHBoxLayout()
        t.addLayout(info_label("CLOCK", "The dock clock is synced automatically by the "
                                        "darkmount background service."))
        self.clock_lbl = label("—", "Muted")
        t.addWidget(self.clock_lbl)
        sync = QPushButton("Sync now")
        sync.clicked.connect(self.sync)
        t.addWidget(sync)
        self.body.addLayout(t)
        self.body.addStretch()
        self.footer_row("RESET SCREEN SETTINGS", "Reset", self.reset)

        for w in (self.idle_on, self.off_on):
            w.toggled.connect(self._changed)
        for w in (self.idle_s, self.off_s):
            w.valueChanged.connect(self._changed)
        self.view_g.idClicked.connect(self._changed)
        self.clock_g.idClicked.connect(self._changed)
        self.connected = False

    def load(self, kb: DarkMount):
        self.loading = True
        try:
            conn, _ = kb.media_dock_state()
            self.connected = conn
            self.setEnabled(conn)
            if not conn:
                return
            cfg = kb.media_dock_config()
            self.picker.set_color(QColor(*cfg.menu_color))
            self.app.dock_view.set_color(QColor(*cfg.menu_color))
            self.idle_on.setChecked(cfg.screensaver_mode != ScreensaverMode.Disabled)
            self.idle_s.setValue(max(5, cfg.screensaver_timeout or 30))
            self.view_g.button(0 if cfg.screensaver_mode != ScreensaverMode.Clock else 1) \
                .setChecked(True)
            self.clock_g.button(0 if cfg.clock_mode == ClockMode.Clock12h else 1).setChecked(True)
            self.off_on.setChecked(cfg.screen_off_timeout > 0)
            self.off_s.setValue(max(5, cfg.screen_off_timeout or 60))
            self.show_clock(kb)
        finally:
            self.loading = False

    def show_clock(self, kb):
        t = kb.media_dock_time()
        self.clock_lbl.setText(datetime.datetime.fromtimestamp(t, datetime.timezone.utc)
                               .strftime("%H:%M  %d %b"))

    def _changed(self, *_):
        self.idle_s.setEnabled(self.idle_on.isChecked())
        self.off_s.setEnabled(self.off_on.isChecked())
        if not self.loading:
            self.apply_timer.start()

    def apply(self):
        c = self.picker.color()

        def do(kb):
            cfg = kb.media_dock_config()
            cfg.menu_color = (c.red(), c.green(), c.blue())
            if not self.idle_on.isChecked():
                cfg.screensaver_mode = ScreensaverMode.Disabled
            else:
                cfg.screensaver_mode = ScreensaverMode.Image if self.view_g.checkedId() == 0 \
                    else ScreensaverMode.Clock
            cfg.screensaver_timeout = self.idle_s.value()
            cfg.clock_mode = ClockMode.Clock12h if self.clock_g.checkedId() == 0 \
                else ClockMode.Clock24h
            cfg.screen_off_timeout = self.off_s.value() if self.off_on.isChecked() else 0
            kb.set_media_dock_config(cfg)
        self.app.run(do)

    def sync(self):
        self.app.run(lambda kb: (kb.sync_media_dock_time(), self.show_clock(kb)))

    def reset(self):
        from ..device import MediaDockConfig

        def do(kb):
            kb.set_media_dock_config(MediaDockConfig((220, 77, 0), ClockMode.Clock12h,
                                                     ScreensaverMode.Image, 30, 60))
            self.load(kb)
        self.app.run(do)


# --------------------------------------------------------------------------- Product settings
GAME_FLAGS = [("Disable SHIFT + TAB", GameModeFlags.SHIFT_TAB),
              ("Disable ALT + F4", GameModeFlags.ALT_F4),
              ("Disable Windows Key", GameModeFlags.WIN),
              ("Disable ALT + TAB", GameModeFlags.ALT_TAB),
              ("Disable CAPS LOCK", GameModeFlags.CAPS_LOCK)]
SERVICE = "darkmount-clock.service"


class SettingsPage(Page):
    def __init__(self, app):
        super().__init__("PRODUCT SETTINGS")
        self.app = app
        fh = QHBoxLayout()
        fh.addWidget(label("FIRMWARE VERSION"))
        fh.addStretch()
        self.check_btn = QPushButton("Check for updates", objectName="Primary")
        self.check_btn.clicked.connect(self.check_updates)
        fh.addWidget(self.check_btn)
        self.body.addLayout(fh)
        self.fw = QVBoxLayout()
        self.fw.setSpacing(4)
        self.body.addLayout(self.fw)
        ur = QHBoxLayout()
        self.update_lbl = label("", "Muted")
        self.update_lbl.setWordWrap(True)
        ur.addWidget(self.update_lbl, 1)
        self.install_btn = QPushButton("Install", objectName="Primary")
        self.install_btn.clicked.connect(self.install_update)
        self.install_btn.hide()
        ur.addWidget(self.install_btn)
        self.body.addLayout(ur)
        self.body.addSpacing(8)
        self.installed = "0.0.0"
        self.modules_ok = False

        gm = QHBoxLayout()
        gm.addLayout(info_label("GAME MODE", "Also toggled on the keyboard with Fn + Pause."))
        self.game = Toggle()
        self.game.toggled.connect(self._game)
        gm.addWidget(self.game)
        self.body.addLayout(gm)
        self.flags = []
        for text, flag in GAME_FLAGS:
            r = QHBoxLayout()
            r.addWidget(QLabel(text))
            r.addStretch()
            t = Toggle()
            t.toggled.connect(self._flags)
            r.addWidget(t)
            self.body.addLayout(r)
            self.flags.append((t, flag))
        self.body.addSpacing(8)
        lay = QHBoxLayout()
        lay.addLayout(info_label("LAYOUT", "Reported by the keyboard."))
        self.layout_lbl = QLabel()
        lay.addWidget(self.layout_lbl)
        self.body.addLayout(lay)
        self.info = label("", "Muted")
        self.info.setWordWrap(True)
        self.info.setTextInteractionFlags(Qt.TextSelectableByMouse)
        self.body.addWidget(self.info)

        self.body.addSpacing(8)
        svc = QHBoxLayout()
        svc.addLayout(info_label("LINUX HELPER SERVICE",
                                 "Background service that keeps the dock clock synced and runs "
                                 "open-file / open-folder / website / Linux-command key "
                                 "actions."))
        self.svc = Toggle()
        self.svc.toggled.connect(self._service)
        svc.addWidget(self.svc)
        self.body.addLayout(svc)
        self.svc_state = label("", "Muted")
        self.svc_state.setWordWrap(True)
        self.body.addWidget(self.svc_state)

        self.body.addSpacing(8)
        at = QHBoxLayout()
        at.addLayout(info_label("START IN TRAY AT LOGIN",
                                "Start this app hidden in the system tray when you log in. "
                                "Closing the window always keeps it in the tray; use Quit "
                                "in the tray menu to exit."))
        self.autostart = Toggle()
        self.autostart.toggled.connect(self._autostart)
        at.addWidget(self.autostart)
        self.body.addLayout(at)
        self.body.addStretch()

    def load(self, kb: DarkMount):
        self.loading = True
        try:
            while self.fw.count():
                it = self.fw.takeAt(0)
                if it.widget():
                    it.widget().deleteLater()
            names = {"Main": "Keyboard", "MediaDock": "Media Dock", "Numpad": "Numpad"}
            fw = kb.device_info()["firmware"]
            for n, v in fw:
                w = QLabel(f"{names.get(n, n)}<br><span style='color:{N300}'>v {v}</span>")
                self.fw.addWidget(w)
            self.installed = fw[0][1] if fw else "0.0.0"
            self.modules_ok = kb.numpad_state()[0] and kb.media_dock_state()[0]
            self._show_available()
            self.game.setChecked(kb.game_mode())
            flags = kb.game_mode_flags()
            for t, f in self.flags:
                t.setChecked(bool(flags & f))
            phys, vis = kb.layout()
            self.layout_lbl.setText(f"{phys} {vis}")
            self.info.setText(f"Serial {kb.serial_number()} · polling {kb.polling_rate()} Hz")
            self._service_status()
            from .tray import autostart_enabled
            self.autostart.setChecked(autostart_enabled(self.app.DESKTOP_ID))
        finally:
            self.loading = False

    # -- firmware ------------------------------------------------------------------------
    def _show_available(self):
        from .. import firmware as FW
        cached = FW.cached_packages()
        pkg = cached[0] if cached else None
        self.pkg = pkg
        if pkg and FW._ver(pkg.version) > FW._ver(self.installed):
            self.update_lbl.setText(f"Firmware <b style='color:{ACCENT}'>{pkg.version}</b> is "
                                    "ready to install." + ("" if self.modules_ok else
                                    "<br>Attach the numpad and media dock first."))
            self.install_btn.setVisible(True)
            self.install_btn.setEnabled(self.modules_ok)
        elif pkg:
            self.update_lbl.setText(f"Up to date (latest downloaded: {pkg.version}).")
            self.install_btn.hide()
        else:
            self.update_lbl.setText("Press “Check for updates” to download the latest "
                                    "firmware (needs internet once).")
            self.install_btn.hide()

    def check_updates(self):
        from .. import firmware as FW
        self.check_btn.setEnabled(False)
        self.update_lbl.setText("Checking…")
        QApplication.processEvents()
        try:
            FW.download_latest(progress=lambda m: (self.update_lbl.setText(m),
                                                   QApplication.processEvents()))
        except Exception as e:  # network / server errors
            QMessageBox.warning(self, "Check for updates", f"Couldn't download firmware:\n{e}")
        finally:
            self.check_btn.setEnabled(True)
        self._show_available()

    def install_update(self):
        if not self.pkg:
            return
        self.app.begin_firmware_update(self.pkg, self.installed)

    def _autostart(self, on):
        if self.loading:
            return
        from .tray import set_autostart
        set_autostart(self.app.DESKTOP_ID, self.app.APP_NAME, on)

    def _game(self, on):
        if not self.loading:
            self.app.run(lambda kb: kb.set_game_mode(on))

    def _flags(self, *_):
        if self.loading:
            return
        flags = GameModeFlags.NONE
        for t, f in self.flags:
            if t.isChecked():
                flags |= f
        self.app.run(lambda kb: kb.set_game_mode_flags(flags))

    # -- helper service ----------------------------------------------------------------
    def _systemctl(self, *args) -> subprocess.CompletedProcess:
        return subprocess.run(["systemctl", "--user", *args], capture_output=True, text=True)

    def _service_status(self):
        if not shutil.which("systemctl"):
            self.svc.setEnabled(False)
            self.svc_state.setText("systemd not available")
            return
        active = self._systemctl("is-active", SERVICE).stdout.strip() == "active"
        enabled = self._systemctl("is-enabled", SERVICE).stdout.strip() == "enabled"
        self.svc.blockSignals(True)
        self.svc.setChecked(active)
        self.svc.blockSignals(False)
        self.svc_state.setText(("Running" if active else "Stopped") +
                               (", starts at login" if enabled else ""))

    def _service(self, on):
        unit_dir = Path.home() / ".config/systemd/user"
        unit = unit_dir / SERVICE
        if on and not unit.exists():
            exe = os.environ.get("APPIMAGE") or shutil.which("darkmount")
            if not exe:
                QMessageBox.warning(self, "Helper service", "Couldn't find the darkmount "
                                    "command to run as a service.")
                self._service_status()
                return
            args = "--daemon" if os.environ.get("APPIMAGE") else "daemon"
            unit_dir.mkdir(parents=True, exist_ok=True)
            unit.write_text(f"""[Unit]
Description=be quiet! Dark Mount helper (dock clock sync, key actions)
After=graphical-session.target

[Service]
ExecStart="{exe}" {args} --interval 600
Restart=on-failure
RestartSec=10

[Install]
WantedBy=default.target
""")
            self._systemctl("daemon-reload")
        if on:
            self._systemctl("enable", "--now", SERVICE)
        else:
            self._systemctl("disable", "--now", SERVICE)
        self._service_status()
