"""Qt GUI for the be quiet! Dark Mount keyboard."""
from __future__ import annotations

import datetime
import sys

from PySide6.QtCore import Qt, QTimer, Signal
from PySide6.QtGui import QColor, QIcon
from PySide6.QtWidgets import (QApplication, QCheckBox, QColorDialog, QComboBox, QDialog,
                               QDialogButtonBox, QFileDialog, QFormLayout, QGroupBox,
                               QHBoxLayout, QHeaderView, QLabel, QLineEdit, QMainWindow,
                               QMessageBox, QPlainTextEdit, QPushButton, QSlider,
                               QStackedWidget, QTableWidget, QTableWidgetItem, QTabWidget,
                               QToolButton, QVBoxLayout, QWidget)

from .device import (ClockMode, ColorMode, DarkMount, Direction, Effect, GameModeFlags,
                     LightingConfig, LightingMode, ScreensaverMode, fmt_color)
from .qlink import QLinkError

# Options per effect, as offered by the official app.
EFFECTS = {
    Effect.Static: ([ColorMode.SingleColor], []),
    Effect.ColorWave: ([ColorMode.SingleColor, ColorMode.DualColor, ColorMode.Gradient],
                       [Direction.Up, Direction.Down, Direction.Left, Direction.Right]),
    Effect.Tornado: ([ColorMode.SingleColor, ColorMode.DualColor, ColorMode.Gradient],
                     [Direction.Clockwise, Direction.Counterclockwise]),
    Effect.Breathing: ([ColorMode.SingleColor, ColorMode.DualColor, ColorMode.Gradient], []),
    Effect.Reactive: ([ColorMode.SingleColor, ColorMode.DualColor, ColorMode.Gradient], []),
    Effect.Matrix: ([ColorMode.DualColor, ColorMode.Gradient],
                    [Direction.Up, Direction.Down, Direction.Left, Direction.Right]),
}
DEFAULT_DIRECTION = {Effect.ColorWave: Direction.Right, Effect.Tornado: Direction.Clockwise,
                     Effect.Matrix: Direction.Down}
COLOR_MODE_LABEL = {ColorMode.SingleColor: "Single color", ColorMode.DualColor: "Two colors",
                    ColorMode.Gradient: "Gradient"}
RAINBOW = [(255, 0, 0), (255, 200, 0), (0, 255, 0), (0, 200, 255), (0, 0, 255), (255, 0, 255)]
GAME_FLAGS = [("Windows key", GameModeFlags.WIN), ("Alt+Tab", GameModeFlags.ALT_TAB),
              ("Alt+F4", GameModeFlags.ALT_F4), ("Shift+Tab", GameModeFlags.SHIFT_TAB),
              ("Caps Lock", GameModeFlags.CAPS_LOCK)]


class Link:
    """Keeps a session to the keyboard, reconnecting when it is replugged."""

    def __init__(self):
        self.kb: DarkMount | None = None

    def connect(self) -> bool:
        if self.kb:
            return True
        try:
            kb = DarkMount()
            kb.__enter__()
            self.kb = kb
            return True
        except (QLinkError, OSError):
            self.kb = None
            return False

    def drop(self):
        if self.kb:
            try:
                self.kb.__exit__(None, None, None)
            except Exception:
                pass
        self.kb = None


class ColorButton(QToolButton):
    changed = Signal()

    def __init__(self, rgb=(255, 40, 0)):
        super().__init__()
        self.setFixedSize(44, 28)
        self.rgb = tuple(rgb)
        self.clicked.connect(self._pick)
        self._paint()

    def set_rgb(self, rgb):
        self.rgb = tuple(rgb)
        self._paint()

    def _paint(self):
        self.setStyleSheet(f"QToolButton {{ background: {fmt_color(self.rgb)}; "
                           "border: 1px solid #888; border-radius: 4px; }")
        self.setToolTip(fmt_color(self.rgb))

    def _pick(self):
        c = QColorDialog.getColor(QColor(*self.rgb), self, "Pick color")
        if c.isValid():
            self.set_rgb((c.red(), c.green(), c.blue()))
            self.changed.emit()


def labeled_slider(on_change):
    row = QHBoxLayout()
    s = QSlider(Qt.Horizontal)
    s.setRange(0, 100)
    val = QLabel("0")
    val.setMinimumWidth(28)
    s.valueChanged.connect(lambda v: val.setText(str(v)))
    s.valueChanged.connect(on_change)
    row.addWidget(s)
    row.addWidget(val)
    return s, row


class LightingTab(QWidget):
    def __init__(self, win: "MainWindow"):
        super().__init__()
        self.win = win
        self.loading = False
        self.apply_timer = QTimer(self, singleShot=True, interval=250, timeout=self.apply)

        self.enabled = QCheckBox("Backlight on")
        self.enabled.toggled.connect(self.toggle_enabled)

        self.effect = QComboBox()
        for e in EFFECTS:
            self.effect.addItem(e.name, e)
        self.effect.currentIndexChanged.connect(self.effect_changed)

        self.color_mode = QComboBox()
        self.color_mode.currentIndexChanged.connect(self.color_mode_changed)

        self.colors_box = QHBoxLayout()
        self.color_buttons: list[ColorButton] = []
        self.add_btn = QToolButton(text="+")
        self.del_btn = QToolButton(text="−")
        self.add_btn.clicked.connect(lambda: self._gradient_count(+1))
        self.del_btn.clicked.connect(lambda: self._gradient_count(-1))
        colors_row = QHBoxLayout()
        colors_row.addLayout(self.colors_box)
        colors_row.addWidget(self.add_btn)
        colors_row.addWidget(self.del_btn)
        colors_row.addStretch()

        self.direction = QComboBox()
        self.direction.currentIndexChanged.connect(self.changed)
        self.brightness, b_row = labeled_slider(self.changed)
        self.speed, s_row = labeled_slider(self.changed)

        form = QFormLayout()
        form.addRow(self.enabled)
        form.addRow("Effect", self.effect)
        form.addRow("Colors", self.color_mode)
        form.addRow("", colors_row)
        form.addRow("Direction", self.direction)
        form.addRow("Brightness", b_row)
        form.addRow("Speed", s_row)
        self.form = form
        box = QGroupBox("Backlight")
        box.setLayout(form)
        lay = QVBoxLayout(self)
        lay.addWidget(box)
        hint = QLabel("Changes are sent to the keyboard immediately and saved on it.")
        hint.setStyleSheet("color: gray")
        lay.addWidget(hint)
        lay.addStretch()

    # -- populate from device --------------------------------------------------
    def load(self, kb: DarkMount):
        self.loading = True
        try:
            mode = kb.lighting_mode()
            cfg = kb.lighting()
            self.enabled.setChecked(mode != LightingMode.Off and cfg.effect != Effect.Off)
            eff = cfg.effect if cfg.effect in EFFECTS else Effect.Static
            self.effect.setCurrentIndex(list(EFFECTS).index(eff))
            self._fill_effect_options(eff)
            if cfg.color_mode in EFFECTS[eff][0]:
                self.color_mode.setCurrentIndex(EFFECTS[eff][0].index(cfg.color_mode))
            self._set_colors(cfg)
            dirs = EFFECTS[eff][1]
            if cfg.direction in dirs:
                self.direction.setCurrentIndex(dirs.index(cfg.direction))
            self.brightness.setValue(cfg.brightness)
            self.speed.setValue(cfg.speed)
        finally:
            self.loading = False
        self._update_enabled_state()

    def _fill_effect_options(self, eff: Effect):
        modes, dirs = EFFECTS[eff]
        self.color_mode.blockSignals(True)
        self.color_mode.clear()
        for m in modes:
            self.color_mode.addItem(COLOR_MODE_LABEL[m], m)
        self.color_mode.blockSignals(False)
        self.direction.blockSignals(True)
        self.direction.clear()
        for d in dirs:
            self.direction.addItem(d.name, d)
        self.direction.blockSignals(False)
        self.form.setRowVisible(self.direction, bool(dirs))
        self.speed.setEnabled(eff != Effect.Static)

    def _clear_color_buttons(self):
        for b in self.color_buttons:
            self.colors_box.removeWidget(b)
            b.deleteLater()
        self.color_buttons = []

    def _add_color_button(self, rgb):
        b = ColorButton(rgb)
        b.changed.connect(self.changed)
        self.colors_box.addWidget(b)
        self.color_buttons.append(b)

    def _set_colors(self, cfg: LightingConfig):
        self._clear_color_buttons()
        mode = self.color_mode.currentData()
        if mode == ColorMode.Gradient:
            cols = [c for c, _ in cfg.colors] if cfg.color_mode == ColorMode.Gradient else []
            if len(cols) < 2:
                cols = RAINBOW
        else:
            cols = [c for c in cfg.colors] if cfg.color_mode in (ColorMode.SingleColor,
                                                                 ColorMode.DualColor) else \
                [c for c, _ in cfg.colors]
            need = 1 if mode == ColorMode.SingleColor else 2
            cols = (cols + [(255, 40, 0), (0, 120, 255)])[:need]
            if need == 2 and len(cfg.colors) < 2:
                cols[1] = (0, 120, 255)
        for c in cols:
            self._add_color_button(c)
        self._update_gradient_buttons()

    def _update_gradient_buttons(self):
        grad = self.color_mode.currentData() == ColorMode.Gradient
        self.add_btn.setVisible(grad)
        self.del_btn.setVisible(grad)
        self.add_btn.setEnabled(len(self.color_buttons) < 8)
        self.del_btn.setEnabled(len(self.color_buttons) > 2)

    def _gradient_count(self, delta):
        if delta > 0:
            self._add_color_button(RAINBOW[len(self.color_buttons) % len(RAINBOW)])
        elif len(self.color_buttons) > 2:
            b = self.color_buttons.pop()
            self.colors_box.removeWidget(b)
            b.deleteLater()
        self._update_gradient_buttons()
        self.changed()

    # -- events --------------------------------------------------------------
    def effect_changed(self):
        if self.loading:
            return
        eff = self.effect.currentData()
        current = self.current_config()
        self.loading = True
        self._fill_effect_options(eff)
        if eff in DEFAULT_DIRECTION:
            self.direction.setCurrentIndex(EFFECTS[eff][1].index(DEFAULT_DIRECTION[eff]))
        self._set_colors(current)
        self.loading = False
        self.changed()

    def color_mode_changed(self):
        if self.loading:
            return
        current = self.current_config()
        self._set_colors(current)
        self.changed()

    def toggle_enabled(self, on):
        self._update_enabled_state()
        if self.loading:
            return
        if on:
            self.changed()
        else:
            self.win.run(lambda kb: kb.set_lighting_mode(LightingMode.Off))

    def _update_enabled_state(self):
        for w in (self.effect, self.color_mode, self.direction, self.brightness, self.speed,
                  self.add_btn, self.del_btn, *self.color_buttons):
            w.setEnabled(self.enabled.isChecked())
        if self.enabled.isChecked():
            self.speed.setEnabled(self.effect.currentData() != Effect.Static)
            self._update_gradient_buttons()

    def changed(self, *_):
        if not self.loading and self.enabled.isChecked():
            self.apply_timer.start()

    def current_config(self) -> LightingConfig:
        mode = self.color_mode.currentData() or ColorMode.SingleColor
        cols = [b.rgb for b in self.color_buttons] or [(255, 40, 0)]
        if mode == ColorMode.Gradient:
            n = len(cols)
            cols = [(c, round(i * 100 / (n - 1))) if n > 1 else (c, 0) for i, c in enumerate(cols)]
        eff = self.effect.currentData()
        direction = self.direction.currentData()
        return LightingConfig(effect=eff,
                              direction=int(direction) if direction is not None else 0,
                              brightness=self.brightness.value(), speed=self.speed.value(),
                              color_mode=mode, colors=cols)

    def apply(self):
        cfg = self.current_config()
        self.win.run(lambda kb: kb.set_lighting(cfg))


class KeyboardTab(QWidget):
    def __init__(self, win: "MainWindow"):
        super().__init__()
        self.win = win
        self.loading = False
        self.game_mode = QCheckBox("Game mode on")
        self.game_mode.toggled.connect(self.game_toggled)
        self.flag_boxes = []
        flags_box = QGroupBox("While game mode is on, disable:")
        fl = QVBoxLayout(flags_box)
        for label, flag in GAME_FLAGS:
            cb = QCheckBox(label)
            cb.toggled.connect(self.flags_changed)
            fl.addWidget(cb)
            self.flag_boxes.append((cb, flag))
        hint = QLabel("Fn + Pause also toggles game mode on the keyboard.")
        hint.setStyleSheet("color: gray")

        gm = QGroupBox("Game mode")
        gl = QVBoxLayout(gm)
        gl.addWidget(self.game_mode)
        gl.addWidget(flags_box)
        gl.addWidget(hint)

        self.info = QLabel()
        self.info.setTextInteractionFlags(Qt.TextSelectableByMouse)
        info_box = QGroupBox("Device")
        QVBoxLayout(info_box).addWidget(self.info)

        lay = QVBoxLayout(self)
        lay.addWidget(gm)
        lay.addWidget(info_box)
        lay.addStretch()

    def load(self, kb: DarkMount):
        self.loading = True
        try:
            self.game_mode.setChecked(kb.game_mode())
            flags = kb.game_mode_flags()
            for cb, flag in self.flag_boxes:
                cb.setChecked(bool(flags & flag))
            info = kb.device_info()
            phys, vis = kb.layout()
            np_conn, np_pos = kb.numpad_state()
            md_conn, md_pos = kb.media_dock_state()
            fw = "<br>".join(f"&nbsp;&nbsp;{n}: {v}" for n, v in info["firmware"])
            self.info.setText(
                f"Serial: {kb.serial_number()}<br>Layout: {phys} / {vis}<br>"
                f"Polling rate: {kb.polling_rate()} Hz<br>"
                f"Numpad: {'connected (' + np_pos + ')' if np_conn else 'not connected'}<br>"
                f"Media dock: {'connected (' + md_pos + ')' if md_conn else 'not connected'}<br>"
                f"Firmware:<br>{fw}")
        finally:
            self.loading = False

    def game_toggled(self, on):
        if not self.loading:
            self.win.run(lambda kb: kb.set_game_mode(on))

    def flags_changed(self):
        if self.loading:
            return
        flags = GameModeFlags.NONE
        for cb, flag in self.flag_boxes:
            if cb.isChecked():
                flags |= flag
        self.win.run(lambda kb: kb.set_game_mode_flags(flags))


class DockTab(QWidget):
    TIMEOUTS = [("Never", 0), ("15 s", 15), ("30 s", 30), ("1 min", 60), ("5 min", 300),
                ("10 min", 600), ("15 min", 900), ("30 min", 1800)]

    def __init__(self, win: "MainWindow"):
        super().__init__()
        self.win = win
        self.loading = False
        self.apply_timer = QTimer(self, singleShot=True, interval=300, timeout=self.apply)

        self.status = QLabel()
        self.clock = QLabel()
        sync = QPushButton("Sync clock now")
        sync.clicked.connect(self.sync_clock)
        clock_row = QHBoxLayout()
        clock_row.addWidget(self.clock)
        clock_row.addStretch()
        clock_row.addWidget(sync)

        self.clock_mode = QComboBox()
        self.clock_mode.addItem("12-hour", int(ClockMode.Clock12h))
        self.clock_mode.addItem("24-hour", int(ClockMode.Clock24h))
        self.menu_color = ColorButton()
        self.screensaver = QComboBox()
        for m in ScreensaverMode:
            self.screensaver.addItem(m.name, int(m))
        self.ss_timeout = QComboBox()
        self.off_timeout = QComboBox()
        for label, v in self.TIMEOUTS[1:]:
            self.ss_timeout.addItem(label, v)
        for label, v in self.TIMEOUTS:
            self.off_timeout.addItem(label, v)
        for w in (self.clock_mode, self.screensaver, self.ss_timeout, self.off_timeout):
            w.currentIndexChanged.connect(self.changed)
        self.menu_color.changed.connect(self.changed)

        form = QFormLayout()
        form.addRow("Status", self.status)
        form.addRow("Clock", clock_row)
        form.addRow("Clock format", self.clock_mode)
        form.addRow("Menu color", self.menu_color)
        form.addRow("Screensaver", self.screensaver)
        form.addRow("Screensaver after", self.ss_timeout)
        form.addRow("Screen off after", self.off_timeout)
        self.box = QGroupBox("Media dock")
        self.box.setLayout(form)
        lay = QVBoxLayout(self)
        lay.addWidget(self.box)
        hint = QLabel("The background service (darkmount-clock) keeps the clock synced automatically.")
        hint.setStyleSheet("color: gray")
        hint.setWordWrap(True)
        lay.addWidget(hint)
        lay.addStretch()

    @staticmethod
    def _select(combo: QComboBox, value):
        idx = combo.findData(int(value))
        if idx < 0:
            combo.addItem(f"{value} s", value)
            idx = combo.count() - 1
        combo.setCurrentIndex(idx)

    def load(self, kb: DarkMount):
        self.loading = True
        try:
            conn, pos = kb.media_dock_state()
            self.status.setText(f"connected ({pos})" if conn else "not connected")
            for w in (self.clock_mode, self.menu_color, self.screensaver, self.ss_timeout,
                      self.off_timeout):
                w.setEnabled(conn)
            if not conn:
                self.clock.setText("—")
                return
            cfg = kb.media_dock_config()
            self._select(self.clock_mode, cfg.clock_mode)
            self.menu_color.set_rgb(cfg.menu_color)
            self._select(self.screensaver, cfg.screensaver_mode)
            self._select(self.ss_timeout, cfg.screensaver_timeout)
            self._select(self.off_timeout, cfg.screen_off_timeout)
            self.show_clock(kb)
        finally:
            self.loading = False

    def show_clock(self, kb):
        t = kb.media_dock_time()
        self.clock.setText(datetime.datetime.fromtimestamp(t, datetime.timezone.utc)
                           .strftime("%Y-%m-%d %H:%M:%S"))

    def changed(self, *_):
        if not self.loading:
            self.apply_timer.start()

    def apply(self):
        def do(kb):
            cfg = kb.media_dock_config()
            cfg.clock_mode = self.clock_mode.currentData()
            cfg.menu_color = self.menu_color.rgb
            cfg.screensaver_mode = self.screensaver.currentData()
            cfg.screensaver_timeout = self.ss_timeout.currentData()
            cfg.screen_off_timeout = self.off_timeout.currentData()
            kb.set_media_dock_config(cfg)
        self.win.run(do)

    def sync_clock(self):
        self.win.run(lambda kb: (kb.sync_media_dock_time(), self.show_clock(kb)))


DISPLAY_KEY_ICONS = {"Image0": "⌨ keyboard", "Image1": "🌐 internet", "Image2": "📁 folder",
                     "Image3": "✉ email", "Image4": "✂ scissors", "Image5": "〰 wave",
                     "Image6": "🔒 lock", "Image7": "🌙 moon"}
RUN_MARKER = "darkmount:run"  # OpenFile target used for "run a Linux command" bindings


def key_display(name: str) -> str:
    if name in DISPLAY_KEY_ICONS:
        return f"Display key {name[-1]} ({DISPLAY_KEY_ICONS[name]})"
    return {"DockMute": "Dock: mute", "DockPlayPause": "Dock: play/pause",
            "DockPrev": "Dock: previous", "DockNext": "Dock: next",
            "DockLeft": "Dock: left", "DockRight": "Dock: right"}.get(name, name)


class BindingDialog(QDialog):
    TYPES = ["Send key / shortcut", "Media", "Mouse button", "Mouse scroll", "Backlight",
             "System shortcut", "Run Linux command", "Open file", "Open folder", "Open URL",
             "Disable key"]

    def __init__(self, parent, key_name=None, layer=0, binding=None, command=""):
        super().__init__(parent)
        from . import bindings as B
        self.B = B
        self.original = binding
        self.setWindowTitle("Edit key")
        self.key = QComboBox()
        for name in B.KEY_IDS:
            if name != "Fn":
                self.key.addItem(key_display(name), name)
        if key_name:
            self.key.setCurrentIndex(self.key.findData(key_name))
            self.key.setEnabled(False)
        self.fn = QCheckBox("Fn layer (Fn + key)")
        self.fn.setChecked(bool(layer))
        self.fn.setEnabled(key_name is None)

        self.type = QComboBox()
        self.type.addItems(self.TYPES)
        self.stack = QStackedWidget()
        self.combo = QLineEdit(placeholderText="e.g. Escape, F13, ctrl+shift+T, super+E")
        self.media = self._enum_combo(B.Media)
        self.mouse = self._enum_combo(B.MouseButton)
        self.scroll = self._enum_combo(B.MouseScroll)
        self.backlight = self._enum_combo(B.BacklightAction, skip={B.BacklightAction.SelectEffect})
        self.host = self._enum_combo(B.HostAction)
        self.command = QPlainTextEdit()
        self.command.setPlaceholderText("Shell command, e.g. konsole  or  flatpak run org.gimp.GIMP")
        self.command.setMaximumHeight(90)
        self.path = QLineEdit()
        browse = QPushButton("Browse…")
        browse.clicked.connect(self._browse)
        path_row = QWidget()
        pl = QHBoxLayout(path_row)
        pl.setContentsMargins(0, 0, 0, 0)
        pl.addWidget(self.path)
        pl.addWidget(browse)
        self.url = QLineEdit(placeholderText="https://…")
        self.pages = [self.combo, self.media, self.mouse, self.scroll, self.backlight, self.host,
                      self.command, path_row, path_row, self.url, QLabel("The key will do nothing.")]
        for w in dict.fromkeys(self.pages):
            self.stack.addWidget(w)
        self.type.currentIndexChanged.connect(
            lambda i: (self.stack.setCurrentWidget(self.pages[i]), self._hint()))
        self.hint = QLabel(wordWrap=True)
        self.hint.setStyleSheet("color: gray")

        form = QFormLayout(self)
        form.addRow("Key", self.key)
        form.addRow("", self.fn)
        form.addRow("Action", self.type)
        form.addRow("", self.stack)
        form.addRow(self.hint)
        bb = QDialogButtonBox(QDialogButtonBox.Ok | QDialogButtonBox.Cancel)
        bb.accepted.connect(self._accept)
        bb.rejected.connect(self.reject)
        form.addRow(bb)
        if binding is not None:
            self._load(binding, command)
        self._hint()

    def _enum_combo(self, enum, skip=()):
        c = QComboBox()
        for e in enum:
            if e not in skip:
                c.addItem(e.name, int(e))
        return c

    def _browse(self):
        if self.type.currentText() == "Open folder":
            p = QFileDialog.getExistingDirectory(self, "Folder")
        else:
            p, _ = QFileDialog.getOpenFileName(self, "File")
        if p:
            self.path.setText(p)

    def _hint(self):
        t = self.type.currentText()
        self.hint.setText(
            "Runs via the darkmount background service." if t in
            ("Run Linux command", "Open file", "Open folder", "Open URL") else
            "Sent by the keyboard itself as a Windows-style shortcut "
            "(e.g. Lock = Win+L, Explorer = Win+E)." if t == "System shortcut" else "")

    def _load(self, b, command):
        B = self.B
        idx = {B.BindType.StandardKey: 0, B.BindType.Media: 1, B.BindType.Backlight: 4,
               B.BindType.WindowsShortcut: 5, B.BindType.OpenFile: 7,
               B.BindType.OpenFolder: 8, B.BindType.OpenBrowser: 9,
               B.BindType.Disable: 10}.get(b.type)
        if b.type == B.BindType.Mouse:
            idx = 3 if b.extra.get("scroll") else 2
        if command or (b.type == B.BindType.OpenFile and b.text == RUN_MARKER):
            idx = 6
        if idx is None:
            return
        self.type.setCurrentIndex(idx)
        if idx == 0:
            mods = [m.name.lower().replace("l", "", 1) if m.name.startswith("L") else
                    m.name.lower() for m in B.Modifier if m and b.modifiers & m]
            mods = [{"gui": "super", "rgui": "rsuper"}.get(m, m) for m in mods]
            self.combo.setText("+".join(mods + [B.HID_NAMES.get(b.usage, "")]))
        for i, w in ((1, self.media), (2, self.mouse), (3, self.scroll), (4, self.backlight),
                     (5, self.host)):
            if idx == i:
                w.setCurrentIndex(max(0, w.findData(b.action)))
        if idx == 6:
            self.command.setPlainText(command)
        if idx in (7, 8):
            self.path.setText(b.text)
        if idx == 9:
            self.url.setText(b.text)

    def _accept(self):
        try:
            self.result_record, self.result_command = self._build()
        except ValueError as e:
            QMessageBox.warning(self, "Edit key", str(e))
            return
        self.accept()

    @property
    def label(self) -> str:
        return ("Fn+" if self.fn.isChecked() else "") + self.key.currentData()

    def _build(self):
        B = self.B
        key = B.KEY_IDS[self.key.currentData()]
        layer = 1 if self.fn.isChecked() else 0
        i = self.type.currentIndex()
        if i == 0:
            mods, usage = B.parse_combo(self.combo.text())
            return B.encode_standard_key(key, layer, usage, mods), None
        if i == 1:
            return B.encode_media(key, layer, B.Media(self.media.currentData())), None
        if i == 2:
            return B.encode_mouse_button(key, layer, B.MouseButton(self.mouse.currentData())), None
        if i == 3:
            return B.encode_mouse_scroll(key, layer, B.MouseScroll(self.scroll.currentData())), None
        if i == 4:
            return B.encode_backlight(key, layer,
                                      B.BacklightAction(self.backlight.currentData())), None
        if i == 5:
            return B.encode_host_action(key, layer, B.HostAction(self.host.currentData())), None
        if i == 6:
            cmd = self.command.toPlainText().strip()
            if not cmd:
                raise ValueError("Enter a command")
            o = self.original
            if o is not None and o.type in (B.BindType.OpenFile, B.BindType.OpenFolder,
                                            B.BindType.OpenBrowser):
                # Keep the existing on-keyboard binding (e.g. a Windows path used by
                # IO Center); the Linux command overrides it via the config file.
                return o.data, cmd
            return B.encode_open(key, layer, B.BindType.OpenFile, RUN_MARKER), cmd
        if i in (7, 8):
            if not self.path.text().strip():
                raise ValueError("Choose a path")
            kind = B.BindType.OpenFile if i == 7 else B.BindType.OpenFolder
            return B.encode_open(key, layer, kind, self.path.text().strip()), None
        if i == 9:
            if not self.url.text().strip():
                raise ValueError("Enter a URL")
            return B.encode_open(key, layer, B.BindType.OpenBrowser, self.url.text().strip()), None
        return B.encode_disable(key, layer), None


class KeysTab(QWidget):
    def __init__(self, win: "MainWindow"):
        super().__init__()
        self.win = win
        self.bindings = []
        self.table = QTableWidget(0, 2)
        self.table.setHorizontalHeaderLabels(["Key", "Action"])
        self.table.horizontalHeader().setSectionResizeMode(0, QHeaderView.ResizeToContents)
        self.table.horizontalHeader().setStretchLastSection(True)
        self.table.verticalHeader().hide()
        self.table.setSelectionBehavior(QTableWidget.SelectRows)
        self.table.setSelectionMode(QTableWidget.SingleSelection)
        self.table.setEditTriggers(QTableWidget.NoEditTriggers)
        self.table.doubleClicked.connect(self.edit)

        add = QPushButton(QIcon.fromTheme("list-add"), "Remap a key…")
        edit = QPushButton(QIcon.fromTheme("document-edit"), "Edit…")
        reset = QPushButton(QIcon.fromTheme("edit-undo"), "Reset to default")
        add.clicked.connect(self.add)
        edit.clicked.connect(self.edit)
        reset.clicked.connect(self.reset)
        row = QHBoxLayout()
        for b in (add, edit, reset):
            row.addWidget(b)
        row.addStretch()
        hint = QLabel("Keys not listed use their default function. Changes are saved on the "
                      "keyboard; “Run Linux command” needs the darkmount background service.")
        hint.setWordWrap(True)
        hint.setStyleSheet("color: gray")
        lay = QVBoxLayout(self)
        lay.addWidget(self.table)
        lay.addLayout(row)
        lay.addWidget(hint)

    def load(self, kb: DarkMount):
        from .config import read_actions
        acts = {k.lower(): v for k, v in read_actions().items()}
        self.bindings = sorted(kb.bindings(), key=lambda b: (b.layer, b.key))
        self.commands = {}
        self.table.setRowCount(len(self.bindings))
        for r, b in enumerate(self.bindings):
            label = ("Fn+" if b.layer else "") + b.key_name
            cmd = acts.get(label.lower(), "")
            self.commands[r] = cmd
            desc = b.describe()
            if cmd:
                first = cmd.strip().splitlines()[0] if cmd.strip() else cmd
                desc = f"run: {first}" + (" …" if "\n" in cmd.strip() else "")
            self.table.setItem(r, 0, QTableWidgetItem(
                ("Fn + " if b.layer else "") + key_display(b.key_name)))
            self.table.setItem(r, 1, QTableWidgetItem(desc))

    def _selected(self):
        rows = self.table.selectionModel().selectedRows()
        return rows[0].row() if rows else None

    def _apply(self, dlg: BindingDialog):
        from .config import set_action
        rec, cmd = dlg.result_record, dlg.result_command

        def do(kb):
            kb.set_binding(rec)
            if cmd is not None or dlg.type.currentIndex() != 6:
                set_action(dlg.label, cmd)
            self.load(kb)
        self.win.run(do)

    def add(self):
        dlg = BindingDialog(self)
        if dlg.exec():
            self._apply(dlg)

    def edit(self, *_):
        r = self._selected()
        if r is None:
            return self.add()
        b = self.bindings[r]
        dlg = BindingDialog(self, b.key_name, b.layer, b, self.commands.get(r, ""))
        if dlg.exec():
            self._apply(dlg)

    def reset(self):
        from .config import set_action
        r = self._selected()
        if r is None:
            return
        b = self.bindings[r]
        label = ("Fn+" if b.layer else "") + b.key_name
        if QMessageBox.question(self, "Reset key",
                                f"Reset {key_display(b.key_name)}"
                                f"{' (Fn layer)' if b.layer else ''} to its default?") \
                != QMessageBox.Yes:
            return

        def do(kb):
            kb.clear_binding(b.key, b.layer)
            set_action(label, None)
            self.load(kb)
        self.win.run(do)


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle("Dark Mount")
        self.resize(520, 560)
        self.link = Link()

        self.header = QLabel()
        self.header.setStyleSheet("font-size: 15pt; font-weight: 600; padding: 6px;")
        self.tabs = QTabWidget()
        self.lighting = LightingTab(self)
        self.keyboard = KeyboardTab(self)
        self.dock = DockTab(self)
        self.keys = KeysTab(self)
        self.tabs.addTab(self.lighting, "Lighting")
        self.tabs.addTab(self.keys, "Keys")
        self.tabs.addTab(self.keyboard, "Keyboard")
        self.tabs.addTab(self.dock, "Media dock")

        central = QWidget()
        lay = QVBoxLayout(central)
        lay.addWidget(self.header)
        lay.addWidget(self.tabs)
        self.setCentralWidget(central)

        self.reconnect_timer = QTimer(self, interval=2000, timeout=self.try_connect)
        self.try_connect()

    def set_connected(self, ok: bool):
        self.tabs.setEnabled(ok)
        self.header.setText("be quiet! Dark Mount" if ok else
                            "Dark Mount — not connected (waiting for keyboard…)")
        if ok:
            self.reconnect_timer.stop()
        else:
            self.reconnect_timer.start()

    def try_connect(self):
        if not self.link.connect():
            self.set_connected(False)
            return
        try:
            kb = self.link.kb
            self.lighting.load(kb)
            self.keys.load(kb)
            self.keyboard.load(kb)
            self.dock.load(kb)
            self.set_connected(True)
        except (QLinkError, OSError) as e:
            self.link.drop()
            self.set_connected(False)
            self.statusBar().showMessage(f"Error reading keyboard: {e}", 8000)

    def run(self, fn):
        """Run fn(kb) against the device, handling disconnects."""
        if not self.link.kb and not self.link.connect():
            self.set_connected(False)
            return
        try:
            fn(self.link.kb)
            self.statusBar().showMessage("Saved to keyboard", 1500)
        except OSError:
            self.link.drop()
            self.set_connected(False)
        except QLinkError as e:
            # Session may have expired: reconnect once and retry.
            self.link.drop()
            if self.link.connect():
                try:
                    fn(self.link.kb)
                    self.statusBar().showMessage("Saved to keyboard", 1500)
                    return
                except (QLinkError, OSError) as e2:
                    e = e2
            QMessageBox.warning(self, "Dark Mount", f"Command failed:\n{e}")

    def closeEvent(self, ev):
        self.link.drop()
        super().closeEvent(ev)


def main():
    app = QApplication(sys.argv)
    app.setApplicationName("Dark Mount")
    app.setDesktopFileName("darkmount")
    app.setWindowIcon(QIcon.fromTheme("input-keyboard"))
    w = MainWindow()
    w.show()
    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
