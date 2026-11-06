"""darkmount-linux desktop app for the be quiet! Dark Mount keyboard (offline, unofficial)."""
from __future__ import annotations

import os
import subprocess
import sys

from PySide6.QtCore import QRectF, QSize, Qt, QTimer
from PySide6.QtGui import QColor, QIcon, QImage, QPainter, QPixmap
from PySide6.QtSvg import QSvgRenderer
from PySide6.QtWidgets import (QApplication, QButtonGroup, QHBoxLayout, QLabel, QMainWindow,
                               QMessageBox, QPushButton, QStackedWidget, QToolButton,
                               QVBoxLayout, QWidget)

from ..device import DarkMount
from ..qlink import QLinkError, list_nodes
from .keyboard_view import KeyboardView
from .pages import IlluminationPage, KeyBindingPage, MediaDockPage, SettingsPage
from .theme import ACCENT, DM, N300, app_icon, device_render, icon, load_fonts, stylesheet

APP_NAME = "darkmount-linux"

UDEV_RULE = """# be quiet! Dark Mount: give the logged-in user access to the hidraw nodes.
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="373f", ATTRS{idProduct}=="0001", TAG+="uaccess"
SUBSYSTEM=="hidraw", ATTRS{idVendor}=="373f", ATTRS{idProduct}=="0009", TAG+="uaccess"
"""


class DockView(QWidget):
    """Large media dock render; the menu tile is lit in the menu color."""
    MASK_RECT = QRectF(2083, 1173, 316, 347)  # from dark_mount_media.json

    def __init__(self):
        super().__init__()
        self.src = QPixmap(str(DM / "images/media_dock/media.webp"))
        self.mask = QSvgRenderer(str(DM / "images/media_dock/masks/led-menu.svg"))
        self.color = QColor("#dc4d00")
        self.setMinimumSize(300, 200)

    def set_color(self, c: QColor):
        self.color = QColor(c)
        self.update()

    def paintEvent(self, _):
        p = QPainter(self)
        p.setRenderHint(QPainter.SmoothPixmapTransform)
        p.setRenderHint(QPainter.Antialiasing)
        sw, sh = self.src.width(), self.src.height()
        s = min(self.width() * 0.85 / sw, self.height() * 0.85 / sh)
        p.translate((self.width() - sw * s) / 2, (self.height() - sh * s) / 2)
        p.scale(s, s)
        p.drawPixmap(0, 0, self.src)
        r = self.MASK_RECT
        tile = QImage(int(r.width()), int(r.height()), QImage.Format_ARGB32_Premultiplied)
        tile.fill(Qt.transparent)
        tp = QPainter(tile)
        self.mask.render(tp)
        tp.setCompositionMode(QPainter.CompositionMode_SourceIn)
        tp.fillRect(tile.rect(), self.color)
        tp.end()
        p.fillRect(r.adjusted(-6, -6, 6, 6), QColor(40, 40, 40))
        p.drawImage(r, tile)
        p.end()


class MainWindow(QMainWindow):
    def __init__(self):
        super().__init__()
        self.setWindowTitle(f"{APP_NAME} — Dark Mount")
        self.resize(1440, 900)
        self.kb: DarkMount | None = None

        root = QWidget(objectName="Root")
        v = QVBoxLayout(root)
        v.setContentsMargins(0, 0, 0, 0)
        v.setSpacing(0)
        v.addWidget(self._top_bar())
        v.addWidget(self._device_bar())

        body = QHBoxLayout()
        body.setSpacing(0)
        self.left = QStackedWidget(objectName="LeftPanel")
        self.left.setFixedWidth(381)
        self.left.setAttribute(Qt.WA_StyledBackground)
        self.illum = IlluminationPage(self)
        self.keys = KeyBindingPage(self)
        self.dock = MediaDockPage(self)
        self.settings = SettingsPage(self)
        for p in (self.illum, self.keys, self.dock, self.settings):
            self.left.addWidget(p)
        body.addWidget(self.left)

        self.stage = QStackedWidget(objectName="Stage")
        self.stage.setAttribute(Qt.WA_StyledBackground)
        self.view = KeyboardView()
        self.view.keyClicked.connect(self.keys.select)
        self.dock_view = DockView()
        self.stage.addWidget(self.view)
        self.stage.addWidget(self.dock_view)
        self.offline = self._offline_panel()
        self.stage.addWidget(self.offline)
        body.addWidget(self.stage, 1)
        v.addLayout(body, 1)
        self.setCentralWidget(root)

        self.illum.preview.connect(self._preview)
        self.dock.picker.colorChanged.connect(self.dock_view.set_color)
        self.reconnect_timer = QTimer(self, interval=2000, timeout=self.connect_device)
        self.section(0)
        self.connect_device()

    # -- chrome ------------------------------------------------------------------------
    def _top_bar(self):
        bar = QWidget(objectName="TopBar")
        bar.setAttribute(Qt.WA_StyledBackground)
        bar.setFixedHeight(64)
        h = QHBoxLayout(bar)
        h.setContentsMargins(0, 0, 24, 0)
        h.setSpacing(0)
        for name, active in (("home", False), ("keyboard", True)):
            b = QToolButton(objectName="TopIcon")
            b.setIcon(icon(name, N300 if not active else ACCENT))
            b.setIconSize(QSize(24, 24))
            b.setFixedSize(80, 64)
            b.setCheckable(True)
            b.setChecked(active)
            b.setEnabled(active)
            b.setToolTip("Dark Mount" if active else "Only the Dark Mount is supported")
            h.addWidget(b)
        h.addStretch()
        t1 = QLabel("darkmount", objectName="Brand")
        t2 = QLabel("linux", objectName="Brand2")
        h.addWidget(t1)
        h.addWidget(t2)
        h.addSpacing(26)
        self.find_btn = QPushButton("Find Device", objectName="Primary")
        self.find_btn.setFixedSize(124, 32)
        self.find_btn.clicked.connect(self.connect_device)
        h.addWidget(self.find_btn)
        h.addSpacing(26)
        gear = QToolButton(objectName="TopIcon")
        gear.setIcon(icon("gear"))
        gear.setIconSize(QSize(22, 22))
        gear.setFixedSize(48, 64)
        gear.setToolTip("About")
        gear.clicked.connect(self.about)
        h.addWidget(gear)
        return bar

    def _device_bar(self):
        bar = QWidget(objectName="DeviceBar")
        bar.setAttribute(Qt.WA_StyledBackground)
        bar.setFixedHeight(65)
        outer = QHBoxLayout(bar)
        outer.setContentsMargins(0, 0, 0, 0)
        outer.setSpacing(0)
        head = QWidget()
        head.setFixedWidth(381)
        h = QHBoxLayout(head)
        h.setContentsMargins(18, 0, 0, 0)
        h.setSpacing(0)
        outer.addWidget(head)
        thumb = QLabel()
        thumb.setPixmap(device_render(56, 34))
        h.addWidget(thumb)
        h.addSpacing(26)
        tv = QVBoxLayout()
        tv.setSpacing(2)
        tv.addStretch()
        tv.addWidget(QLabel("KEYBOARD", objectName="Kicker"))
        self.dev_name = QLabel("DARK MOUNT", objectName="DeviceName")
        tv.addWidget(self.dev_name)
        tv.addStretch()
        h.addLayout(tv)
        h.addStretch()
        sep = QWidget()
        sep.setFixedWidth(1)
        sep.setStyleSheet("background:#000")
        outer.addWidget(sep)
        h = outer
        # sections
        self.sections = QButtonGroup(self)
        sub = QHBoxLayout()
        sub.setContentsMargins(24, 0, 0, 0)
        sub.setSpacing(40)
        for i, (ic, tip) in enumerate((("bulb", "Illumination"), ("keybind", "Key Binding"),
                                       ("media", "Media Dock"), ("wrench", "Product Settings"))):
            b = QToolButton(objectName="SubIcon")
            b.setIcon(icon(ic))
            b.setIconSize(QSize(26, 26))
            b.setCheckable(True)
            b.setToolTip(tip)
            b.setCursor(Qt.PointingHandCursor)
            self.sections.addButton(b, i)
            sub.addWidget(b)
        self.sections.idClicked.connect(self.section)
        h.addLayout(sub)
        h.addStretch(4)
        return bar

    def _offline_panel(self):
        w = QWidget()
        lay = QVBoxLayout(w)
        lay.addStretch()
        self.offline_title = QLabel("SEARCHING FOR DARK MOUNT…", objectName="Big")
        self.offline_title.setAlignment(Qt.AlignCenter)
        lay.addWidget(self.offline_title)
        self.offline_msg = QLabel("", objectName="Muted")
        self.offline_msg.setAlignment(Qt.AlignCenter)
        self.offline_msg.setWordWrap(True)
        lay.addWidget(self.offline_msg)
        self.perm_btn = QPushButton("Grant access (installs a udev rule)", objectName="Primary")
        self.perm_btn.clicked.connect(self.install_udev)
        self.perm_btn.hide()
        lay.addWidget(self.perm_btn, 0, Qt.AlignHCenter)
        self.resume_btn = QPushButton("Finish firmware update", objectName="Primary")
        self.resume_btn.clicked.connect(self.resume_firmware_update)
        self.resume_btn.hide()
        lay.addWidget(self.resume_btn, 0, Qt.AlignHCenter)
        lay.addStretch()
        return w

    # -- navigation --------------------------------------------------------------------
    def section(self, i):
        self.sections.button(i).setChecked(True)
        self.left.setCurrentIndex(i)
        if not self.kb:
            self.stage.setCurrentWidget(self.offline)
            return
        if i == 2:
            self.stage.setCurrentWidget(self.dock_view)
        else:
            self.stage.setCurrentWidget(self.view)
            self.view.set_mode({0: "lighting", 1: "keys"}.get(i, "plain"))
            if i == 1:
                self.keys._update_view()
            if i != 1:
                self.view.selected = None
            self._preview()

    def _preview(self):
        if self.view.mode == "lighting":
            self.view.set_lighting(*self.illum.preview_args())

    # -- device --------------------------------------------------------------------------
    def connect_device(self):
        if self.kb:
            return
        try:
            kb = DarkMount()
            kb.__enter__()
        except QLinkError as e:
            self._offline(str(e))
            return
        self.kb = kb
        try:
            np_conn, np_pos = kb.numpad_state()
            md_conn, md_pos = kb.media_dock_state()
            self.view.set_modules(np_pos if np_conn else None, md_pos if md_conn else None)
            for p in (self.illum, self.keys, self.dock, self.settings):
                p.load(kb)
        except (QLinkError, OSError) as e:
            self.drop()
            self._offline(str(e))
            return
        self.reconnect_timer.stop()
        self.left.setEnabled(True)
        self.find_btn.setText("Connected")
        self.section(self.sections.checkedId())

    def _offline(self, msg):
        self.left.setEnabled(False)
        self.find_btn.setText("Find Device")
        self.stage.setCurrentWidget(self.offline)
        perm = "Permission denied" in msg
        boot = "bootloader" in msg
        self.perm_btn.setVisible(perm)
        self.resume_btn.setVisible(boot)
        present = any(n.vid == 0x373F for n in list_nodes())
        self.offline_title.setText("NO ACCESS TO KEYBOARD" if perm else
                                   "FIRMWARE UPDATE NOT FINISHED" if boot else
                                   "SEARCHING FOR DARK MOUNT…")
        if boot:
            msg = ("The keyboard is waiting in update mode. Press the button to finish "
                   "installing the firmware.")
        self.offline_msg.setText(msg if (perm or present) else
                                 "Connect the keyboard via USB.")
        if not self.reconnect_timer.isActive():
            self.reconnect_timer.start()

    def drop(self):
        if self.kb:
            try:
                self.kb.__exit__(None, None, None)
            except Exception:
                pass
        self.kb = None

    def run(self, fn):
        if not self.kb:
            self.connect_device()
            if not self.kb:
                return
        try:
            fn(self.kb)
            self.statusBar().showMessage("Saved to keyboard", 1500)
        except OSError:
            self.drop()
            self._offline("Keyboard disconnected")
        except QLinkError as e:
            self.drop()
            self.connect_device()
            if self.kb:
                try:
                    fn(self.kb)
                    return
                except (QLinkError, OSError) as e2:
                    e = e2
            QMessageBox.warning(self, "Dark Mount", f"Command failed:\n{e}")

    # -- firmware ---------------------------------------------------------------------
    def begin_firmware_update(self, pkg, installed: str):
        from .firmware_dialog import FirmwareDialog
        self.reconnect_timer.stop()
        self.drop()  # release our session; the updater needs the device to itself
        self.left.setEnabled(False)
        dlg = FirmwareDialog(self, pkg, installed)
        dlg.exec()
        self.find_btn.setText("Find Device")
        self.connect_device()
        if not self.kb:
            self.reconnect_timer.start()

    def resume_firmware_update(self):
        from .. import firmware as FW
        cached = FW.cached_packages()
        if not cached:
            QMessageBox.warning(self, "Firmware", "No downloaded firmware found in "
                                f"{FW.CACHE}.\nRun `darkmount firmware download` while online.")
            return
        self.begin_firmware_update(cached[0], "?")

    def install_udev(self):
        cmd = ("cat > /etc/udev/rules.d/70-bequiet-darkmount.rules <<'EOF'\n" + UDEV_RULE +
               "EOF\nudevadm control --reload-rules && udevadm trigger")
        r = subprocess.run(["pkexec", "sh", "-c", cmd], capture_output=True, text=True)
        if r.returncode != 0:
            QMessageBox.warning(self, "Grant access", "Couldn't install the rule:\n" +
                                (r.stderr or "cancelled"))
            return
        QMessageBox.information(self, "Grant access", "Done. Unplug and re-plug the keyboard.")

    def about(self):
        QMessageBox.about(self, APP_NAME,
                          f"<b>{APP_NAME}</b><br>Unofficial, offline Linux control for the "
                          "be quiet! Dark Mount keyboard.<br><br>"
                          "This project is not affiliated with, endorsed by or supported by "
                          "be quiet!. “be quiet!” and “Dark Mount” are trademarks of their "
                          "respective owner. Device renders and layout data originate from "
                          "the vendor's software (logos removed) and are used only to "
                          "depict the supported hardware.<br><br>"
                          "Command line: <code>darkmount --help</code>")

    def closeEvent(self, e):
        self.drop()
        super().closeEvent(e)


def main(argv=None):
    argv = sys.argv if argv is None else argv
    if len(argv) > 1 and argv[1] in ("--daemon", "--cli"):
        from ..cli import main as cli_main
        rest = argv[2:]
        return cli_main((["daemon"] if argv[1] == "--daemon" else []) + rest)
    os.environ.setdefault("QT_QPA_PLATFORMTHEME", "")
    app = QApplication(argv)
    app.setApplicationName(APP_NAME)
    app.setDesktopFileName(APP_NAME)
    app.setStyle("Fusion")
    app.setStyleSheet(stylesheet(load_fonts()))
    app.setWindowIcon(app_icon())
    w = MainWindow()
    w.show()
    return app.exec()


if __name__ == "__main__":
    sys.exit(main())
