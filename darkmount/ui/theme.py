"""Look & feel: dark neutrals, orange accent, Inter font."""
from __future__ import annotations

from pathlib import Path

from PySide6.QtCore import QByteArray, QRectF, QSize, Qt, Signal
from PySide6.QtGui import QColor, QFontDatabase, QIcon, QPainter, QPixmap
from PySide6.QtSvg import QSvgRenderer
from PySide6.QtWidgets import QAbstractButton, QWidget

ASSETS = Path(__file__).resolve().parent.parent / "assets"
DM = ASSETS / "dark_mount"

ACCENT = "#ee7f00"
ACCENT_HOVER = "#f19933"
N0 = "#ffffff"
N100 = "#e1e1e1"
N300 = "#929395"
N400 = "#76777a"
N500 = "#4a4c51"
N600 = "#2c2e31"
N700 = "#1d1d1d"
N800 = "#121212"
N900 = "#000000"
PANEL = "#1b1b1b"
STAGE = "#1e1e1e"

PRESETS = ["#ff2800", "#ffe03c", "#00ff00", "#00ffff", "#0000ff", "#a020f0", "#ff00aa",
           "#ff0000"]


def load_fonts() -> str:
    family = None
    for f in sorted((ASSETS / "fonts").glob("*.ttf")):
        fid = QFontDatabase.addApplicationFont(str(f))
        fams = QFontDatabase.applicationFontFamilies(fid)
        if fams:
            family = fams[0]
    return family or "Sans Serif"


def stylesheet(font: str) -> str:
    return f"""
* {{ font-family: "{font}"; color: {N100}; font-size: 10pt; }}
QMainWindow, #Root {{ background: {N800}; }}
#TopBar {{ background: {N900}; }}
#DeviceBar {{ background: {N700}; border-bottom: 1px solid {N900}; }}
#LeftPanel {{ background: {PANEL}; border-right: 1px solid {N900}; }}
#Stage {{ background: {STAGE}; }}
#PageTitle {{ font-size: 13pt; font-weight: 700; color: {N0}; }}
#Section {{ font-size: 8pt; font-weight: 700; color: {N100}; }}
#Muted {{ color: {N300}; font-size: 9pt; }}
#Kicker {{ color: {ACCENT}; font-size: 8pt; }}
#DeviceName {{ color: {N0}; font-weight: 700; font-size: 11pt; }}
#Brand {{ color: {N0}; font-weight: 700; font-size: 12pt; }}
#Brand2 {{ color: {ACCENT}; font-weight: 700; font-size: 12pt; }}
#Big {{ font-size: 18pt; font-weight: 800; color: {N0}; }}
QPushButton {{ background: {N500}; border: none; padding: 6px 14px; color: {N100}; }}
QPushButton:hover {{ background: #5a5c61; }}
QPushButton:disabled {{ color: {N400}; background: {N600}; }}
QPushButton#Primary {{ background: {ACCENT}; color: {N900}; }}
QPushButton#Primary:hover {{ background: {ACCENT_HOVER}; }}
QPushButton#Link {{ background: transparent; color: {ACCENT}; font-weight: 700; font-size: 8pt;
                    padding: 0; }}
QPushButton#Link:hover {{ color: {ACCENT_HOVER}; }}
QPushButton#Seg {{ background: {N800}; color: {N100}; font-size: 8pt; padding: 9px; }}
QPushButton#Seg:checked {{ color: {ACCENT}; background: {N900}; }}
QToolButton#Tile {{ background: transparent; border: none; color: {N300}; font-size: 8pt;
                    padding: 6px 2px; }}
QToolButton#Tile:hover {{ color: {N100}; }}
QToolButton#Tile:checked {{ background: {N800}; color: {ACCENT}; }}
QFrame#TileBox {{ background: {N600}; }}
QToolButton#TopIcon, QToolButton#SubIcon {{ background: transparent; border: none; }}
QToolButton#TopIcon:checked {{ background: {N700}; }}
QSlider::groove:horizontal {{ height: 3px; background: {N600}; }}
QSlider::sub-page:horizontal {{ background: {N300}; }}
QSlider::handle:horizontal {{ background: {N100}; width: 8px; margin: -7px 0; }}
QComboBox, QLineEdit, QSpinBox, QPlainTextEdit {{ background: {N600}; border: none;
    padding: 6px 8px; color: {N100}; selection-background-color: {ACCENT}; }}
QComboBox::drop-down {{ border: none; width: 22px; }}
QSpinBox::up-button, QSpinBox::down-button {{ width: 0; border: none; }}
QComboBox QAbstractItemView {{ background: {N600}; border: 1px solid {N500};
    selection-background-color: {ACCENT}; selection-color: {N900}; outline: none; }}
QCheckBox {{ spacing: 8px; }}
QCheckBox::indicator {{ width: 14px; height: 14px; border: 1px solid {N400}; background: transparent; }}
QCheckBox::indicator:checked {{ background: {ACCENT}; border: 1px solid {ACCENT}; image: none; }}
QScrollArea, QScrollArea > QWidget > QWidget {{ background: transparent; border: none; }}
QScrollBar:vertical {{ background: {PANEL}; width: 6px; }}
QScrollBar::handle:vertical {{ background: {N500}; min-height: 30px; }}
QScrollBar::add-line, QScrollBar::sub-line {{ height: 0; }}
QToolTip {{ background: {N600}; color: {N100}; border: 1px solid {N500}; padding: 4px; }}
QMessageBox, QDialog {{ background: {N700}; }}
QStatusBar {{ background: {N900}; color: {N300}; }}
QListWidget {{ background: {N800}; border: none; outline: none; }}
QListWidget::item {{ padding: 7px 8px; color: {N100}; }}
QListWidget::item:selected {{ background: {N900}; color: {ACCENT}; }}
QListWidget::item:hover {{ background: {N600}; }}
"""


# Simple line icons (24x24, stroke = currentColor placeholder)
ICONS = {
    "home": '<path d="M3 11l9-7 9 7"/><path d="M5 10v10h5v-6h4v6h5V10"/>',
    "keyboard": '<rect x="2" y="6" width="20" height="12" rx="1.5"/>'
                '<path d="M6 10h1M9 10h1M12 10h1M15 10h1M18 10h0.5M6 13h1M9 13h1M12 13h1'
                'M15 13h1M18 13h0.5M7 16h10"/>',
    "bulb": '<path d="M9 18h6M10 21h4"/><path d="M12 3a6 6 0 0 0-3.5 10.9c.6.5 1 1.2 1 2V16h5'
            'v-.1c0-.8.4-1.5 1-2A6 6 0 0 0 12 3z"/>',
    "keybind": '<path d="M5 20l2-12h10l2 12z"/><path d="M8 8l1-4h6l1 4"/><path d="M3 20h18"/>',
    "media": '<rect x="3" y="5" width="18" height="14" rx="1"/><path d="M6 8h12v8H6z"/>'
             '<path d="M10.5 10.5v3l2.5-1.5z"/>',
    "wrench": '<path d="M14.5 6.5a4 4 0 0 0 5 5L21 13l-2 2-1.5-1.5-8.5 8.5a2.1 2.1 0 0 1-3-3'
              'l8.5-8.5L13 9l2-2z" transform="translate(-1 -1)"/>',
    "gear": '<circle cx="12" cy="12" r="3"/><path d="M19.4 15a1.6 1.6 0 0 0 .3 1.8l.1.1a2 2 0 1 1'
            '-2.8 2.8l-.1-.1a1.6 1.6 0 0 0-1.8-.3 1.6 1.6 0 0 0-1 1.5V21a2 2 0 1 1-4 0v-.1a1.6 '
            '1.6 0 0 0-1-1.5 1.6 1.6 0 0 0-1.8.3l-.1.1a2 2 0 1 1-2.8-2.8l.1-.1a1.6 1.6 0 0 0 .3'
            '-1.8 1.6 1.6 0 0 0-1.5-1H3a2 2 0 1 1 0-4h.1a1.6 1.6 0 0 0 1.5-1 1.6 1.6 0 0 0-.3-'
            '1.8l-.1-.1a2 2 0 1 1 2.8-2.8l.1.1a1.6 1.6 0 0 0 1.8.3H9a1.6 1.6 0 0 0 1-1.5V3a2 2 '
            '0 1 1 4 0v.1a1.6 1.6 0 0 0 1 1.5 1.6 1.6 0 0 0 1.8-.3l.1-.1a2 2 0 1 1 2.8 2.8l-.1'
            '.1a1.6 1.6 0 0 0-.3 1.8V9a1.6 1.6 0 0 0 1.5 1H21a2 2 0 1 1 0 4h-.1a1.6 1.6 0 0 0-'
            '1.5 1z"/>',
    "refresh": '<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/>',
    "static": '<rect x="6" y="6" width="12" height="12"/>',
    "wave": '<path d="M3 9c3-3 6 3 9 0s6 3 9 0M3 14c3-3 6 3 9 0s6 3 9 0M3 19c3-3 6 3 9 0s6 3 9 0"/>',
    "tornado": '<path d="M3 5h18M5 9h14M8 13h9M10 17h5M12 21h1"/>',
    "breathing": '<circle cx="12" cy="12" r="3"/><circle cx="12" cy="12" r="7" stroke-dasharray'
                 '="2 2"/><circle cx="12" cy="12" r="10" opacity=".5"/>',
    "reactive": '<path d="M12 12v.01"/><path d="M8.5 15.5a5 5 0 0 1 0-7M15.5 8.5a5 5 0 0 1 0 7'
                'M5.6 18.4a9 9 0 0 1 0-12.8M18.4 5.6a9 9 0 0 1 0 12.8"/>',
    "matrix": '<path d="M6 4v3M6 10v1M12 3v5M12 11v2M12 16v1M18 5v2M18 10v4M6 14v4M18 17v3"/>',
    "plus": '<rect x="4" y="3" width="16" height="18" rx="1"/><path d="M12 8v8M8 12h8"/>',
    "info": '<circle cx="12" cy="12" r="10" fill="currentColor" stroke="none"/>'
            '<path d="M12 11v6M12 7.5v.5" stroke="#1b1b1b" stroke-width="2.4"/>',
}


def icon_pixmap(name: str, color: str, size: int = 24) -> QPixmap:
    body = ICONS[name].replace("currentColor", color)
    svg = (f'<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" '
           f'stroke="{color}" stroke-width="1.4" stroke-linecap="round" '
           f'stroke-linejoin="round">{body}</svg>')
    r = QSvgRenderer(QByteArray(svg.encode()))
    pm = QPixmap(size * 2, size * 2)
    pm.setDevicePixelRatio(2)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    r.render(p, QRectF(0, 0, size, size))
    p.end()
    return pm


def icon(name: str, color: str = N300, active: str | None = ACCENT, size: int = 24) -> QIcon:
    ic = QIcon()
    ic.addPixmap(icon_pixmap(name, color, size), QIcon.Normal, QIcon.Off)
    if active:
        ic.addPixmap(icon_pixmap(name, active, size), QIcon.Normal, QIcon.On)
        ic.addPixmap(icon_pixmap(name, N100, size), QIcon.Active, QIcon.Off)
    return ic


class Toggle(QAbstractButton):
    """Small square on/off switch."""

    def __init__(self, parent=None):
        super().__init__(parent)
        self.setCheckable(True)
        self.setCursor(Qt.PointingHandCursor)
        self.setFixedSize(24, 14)

    def sizeHint(self):
        return QSize(24, 14)

    def paintEvent(self, _):
        p = QPainter(self)
        p.fillRect(0, 3, 24, 8, QColor(N500))
        if self.isChecked():
            p.fillRect(12, 1, 12, 12, QColor(ACCENT))
        else:
            p.fillRect(0, 1, 12, 12, QColor(N100 if self.isEnabled() else N400))
        p.end()


class Swatch(QAbstractButton):
    """Clickable color block."""
    picked = Signal(str)

    def __init__(self, color: str, w=20, h=20, parent=None):
        super().__init__(parent)
        self.color = color
        self.setFixedSize(w, h)
        self.setCursor(Qt.PointingHandCursor)
        self.clicked.connect(lambda: self.picked.emit(self.color))

    def set_color(self, c: str):
        self.color = c
        self.update()

    def paintEvent(self, _):
        p = QPainter(self)
        p.fillRect(self.rect(), QColor(self.color))
        if self.isChecked() or self.underMouse():
            p.setPen(QColor(N0))
            p.drawRect(self.rect().adjusted(0, 0, -1, -1))
        p.end()


def device_render(width: int, height: int) -> QPixmap:
    """Keyboard + numpad composed from the (logo-free) renders, fitted into w x h."""
    from PySide6.QtGui import QImage
    main = QImage(str(DM / "images/default/main_us.webp"))
    num = QImage(str(DM / "images/default/numpad_right.webp"))
    canvas = QImage(825 + num.width(), main.height(), QImage.Format_ARGB32_Premultiplied)
    canvas.fill(Qt.transparent)
    p = QPainter(canvas)
    p.drawImage(0, 0, main)
    p.drawImage(825, 0, num)
    p.end()
    # crop transparent margins
    top, bottom = 0, canvas.height() - 1
    while top < bottom and canvas.pixelColor(canvas.width() // 2, top).alpha() == 0:
        top += 1
    while bottom > top and canvas.pixelColor(canvas.width() // 2, bottom).alpha() == 0:
        bottom -= 1
    img = canvas.copy(0, top, canvas.width(), bottom - top + 1)
    return QPixmap.fromImage(img).scaled(width, height, Qt.KeepAspectRatio,
                                         Qt.SmoothTransformation)


def app_icon() -> QIcon:
    pm = QPixmap(256, 256)
    pm.fill(Qt.transparent)
    p = QPainter(pm)
    p.setRenderHint(QPainter.Antialiasing)
    p.setBrush(QColor(N800))
    p.setPen(QColor(ACCENT))
    p.drawRoundedRect(4, 4, 248, 248, 40, 40)
    r = device_render(224, 224)
    p.drawPixmap((256 - r.width()) // 2, (256 - r.height()) // 2, r)
    p.end()
    return QIcon(pm)


def hline(parent: QWidget | None = None):
    from PySide6.QtWidgets import QFrame
    f = QFrame(parent)
    f.setFixedHeight(1)
    f.setStyleSheet(f"background: {N600};")
    return f
