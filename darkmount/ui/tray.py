"""System tray icon, single-instance handling and start-at-login."""
from __future__ import annotations

import os
import shlex
import sys
from pathlib import Path

from PySide6.QtCore import QByteArray, QRectF, Qt
from PySide6.QtGui import QAction, QActionGroup, QIcon, QPainter, QPixmap
from PySide6.QtNetwork import QLocalServer, QLocalSocket
from PySide6.QtSvg import QSvgRenderer
from PySide6.QtWidgets import QMenu, QSystemTrayIcon

from ..device import (ColorMode, DarkMount, Direction, Effect, LightingMode)
from ..qlink import QLinkError
from .theme import ACCENT

SERVER_NAME = f"darkmount-ui-{os.getuid()}"
AUTOSTART_DIR = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / "autostart"


# --------------------------------------------------------------------- single instance
def notify_running_instance() -> bool:
    """If another copy is running, ask it to show its window and return True."""
    s = QLocalSocket()
    s.connectToServer(SERVER_NAME)
    if not s.waitForConnected(300):
        return False
    s.write(b"show\n")
    s.flush()
    s.waitForBytesWritten(300)
    s.disconnectFromServer()
    return True


def listen_for_instances(on_show) -> QLocalServer:
    QLocalServer.removeServer(SERVER_NAME)  # clean up after a crash
    server = QLocalServer()
    server.listen(SERVER_NAME)

    def accept():
        c = server.nextPendingConnection()
        c.readyRead.connect(lambda: (c.readAll(), on_show()))
        c.disconnected.connect(c.deleteLater)
    server.newConnection.connect(accept)
    return server


# --------------------------------------------------------------------- autostart
def launch_command(extra: str = "") -> str:
    appimage = os.environ.get("APPIMAGE")
    if appimage:
        return f"{shlex.quote(appimage)} {extra}".strip()
    root = Path(__file__).resolve().parents[2]
    return (f"env PYTHONPATH={shlex.quote(str(root))} {shlex.quote(sys.executable)} "
            f"-m darkmount.ui {extra}").strip()


def autostart_path(desktop_id: str) -> Path:
    return AUTOSTART_DIR / f"{desktop_id}.desktop"


def autostart_enabled(desktop_id: str) -> bool:
    return autostart_path(desktop_id).exists()


def set_autostart(desktop_id: str, name: str, on: bool):
    p = autostart_path(desktop_id)
    if not on:
        p.unlink(missing_ok=True)
        return
    AUTOSTART_DIR.mkdir(parents=True, exist_ok=True)
    p.write_text(f"""[Desktop Entry]
Type=Application
Name={name}
Comment=Dark Mount keyboard settings (system tray)
Exec={launch_command('--tray')}
Icon={desktop_id}
Terminal=false
X-GNOME-Autostart-enabled=true
X-KDE-autostart-after=panel
""")


# --------------------------------------------------------------------- tray
def tray_icon() -> QIcon:
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" '
           f'stroke="{ACCENT}" stroke-width="2" stroke-linecap="round" stroke-linejoin="round">'
           '<rect x="1.5" y="5.5" width="21" height="13" rx="2"/>'
           '<path d="M5.5 9.5h1M9 9.5h1M12.5 9.5h1M16 9.5h1M5.5 12.5h1M9 12.5h1M12.5 12.5h1'
           'M16 12.5h2.5M7 15.5h10"/></svg>')
    r = QSvgRenderer(QByteArray(svg.encode()))
    ic = QIcon()
    for size in (16, 22, 24, 32, 48, 64):
        pm = QPixmap(size, size)
        pm.fill(Qt.transparent)
        p = QPainter(pm)
        r.render(p, QRectF(0, 0, size, size))
        p.end()
        ic.addPixmap(pm)
    return ic


EFFECTS = [("Static", Effect.Static), ("Color Wave", Effect.ColorWave),
           ("Tornado", Effect.Tornado), ("Breathing", Effect.Breathing),
           ("Reactive", Effect.Reactive), ("Matrix", Effect.Matrix)]
DEFAULT_DIR = {Effect.ColorWave: Direction.Right, Effect.Tornado: Direction.Clockwise,
               Effect.Matrix: Direction.Down}


class Tray(QSystemTrayIcon):
    def __init__(self, window, app_name: str):
        super().__init__(tray_icon(), window)
        self.w = window
        self.setToolTip(f"{app_name} — Dark Mount")
        m = QMenu()
        m.addAction(f"Open {app_name}", self.w.show_from_tray)
        m.addSeparator()
        self.light_on = QAction("Backlight", m, checkable=True)
        self.light_on.triggered.connect(self.toggle_light)
        m.addAction(self.light_on)
        eff = m.addMenu("Effect")
        self.effect_group = QActionGroup(eff)
        for label, e in EFFECTS:
            a = QAction(label, eff, checkable=True)
            a.setData(int(e))
            self.effect_group.addAction(a)
            eff.addAction(a)
        self.effect_group.triggered.connect(lambda a: self.set_effect(Effect(a.data())))
        bri = m.addMenu("Brightness")
        self.bri_group = QActionGroup(bri)
        for v in (10, 30, 50, 70, 100):
            a = QAction(f"{v} %", bri, checkable=True)
            a.setData(v)
            self.bri_group.addAction(a)
            bri.addAction(a)
        self.bri_group.triggered.connect(lambda a: self.set_brightness(a.data()))
        self.game = QAction("Game mode", m, checkable=True)
        self.game.triggered.connect(self.toggle_game)
        m.addAction(self.game)
        m.addSeparator()
        m.addAction("Sync dock clock", lambda: self.quick(lambda kb: kb.sync_media_dock_time(),
                                                          "Dock clock synced"))
        m.addSeparator()
        m.addAction("Quit", self.w.quit_app)
        m.aboutToShow.connect(self.refresh)
        self.menu = m
        self.setContextMenu(m)
        self.activated.connect(self._activated)

    def _activated(self, reason):
        if reason in (QSystemTrayIcon.Trigger, QSystemTrayIcon.DoubleClick):
            if self.w.isVisible() and not self.w.isMinimized():
                self.w.hide_to_tray()
            else:
                self.w.show_from_tray()

    # -- device access: reuse the window's session, else connect briefly --------------
    def quick(self, fn, ok_msg: str | None = None):
        if self.w.isVisible() and self.w.kb:
            self.w.run(fn)
            self.w.reload_pages()
            return True
        try:
            with DarkMount() as kb:
                fn(kb)
            if ok_msg:
                self.showMessage("Dark Mount", ok_msg, self.icon(), 2000)
            return True
        except (QLinkError, OSError) as e:
            self.showMessage("Dark Mount", f"Keyboard not available: {e}", self.icon(), 4000)
            return False

    def refresh(self):
        state = {}

        def read(kb):
            state["mode"] = kb.lighting_mode()
            state["cfg"] = kb.lighting()
            state["game"] = kb.game_mode()
        ok = self.quick(read)
        for a in (self.light_on, self.game, *self.effect_group.actions(),
                  *self.bri_group.actions()):
            a.setEnabled(ok)
        if not ok:
            return
        cfg = state["cfg"]
        self.light_on.setChecked(state["mode"] != LightingMode.Off and cfg.effect != Effect.Off)
        for a in self.effect_group.actions():
            a.setChecked(a.data() == int(cfg.effect) if isinstance(cfg.effect, Effect) else False)
        for a in self.bri_group.actions():
            a.setChecked(a.data() == cfg.brightness)
        self.game.setChecked(state["game"])

    def toggle_light(self, on):
        self.quick(lambda kb: kb.set_lighting_mode(LightingMode.General if on
                                                   else LightingMode.Off))

    def set_brightness(self, v):
        def do(kb):
            cfg = kb.lighting()
            cfg.brightness = v
            kb.set_lighting(cfg)
        self.quick(do)

    def set_effect(self, e: Effect):
        def do(kb):
            cfg = kb.lighting()
            if cfg.effect == Effect.Off or not cfg.colors:
                cfg.colors = [(255, 40, 0)]
                cfg.color_mode = ColorMode.SingleColor
            cfg.effect = e
            if e == Effect.Static and cfg.color_mode != ColorMode.SingleColor:
                first = cfg.colors[0]
                cfg.colors = [first[0] if isinstance(first[0], tuple) else first]
                cfg.color_mode = ColorMode.SingleColor
            if e == Effect.Matrix and cfg.color_mode == ColorMode.SingleColor:
                cfg.color_mode = ColorMode.DualColor
                cfg.colors = [cfg.colors[0], (255, 255, 255)]
            if e in DEFAULT_DIR:
                cfg.direction = int(DEFAULT_DIR[e])
            if not cfg.speed:
                cfg.speed = 50
            kb.set_lighting(cfg)
        self.quick(do)

    def toggle_game(self, on):
        self.quick(lambda kb: kb.set_game_mode(on))
