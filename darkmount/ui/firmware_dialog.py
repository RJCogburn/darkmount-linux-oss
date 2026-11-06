"""Firmware update dialog: confirmation, progress, result."""
from __future__ import annotations

from PySide6.QtCore import QObject, Qt, QThread, Signal
from PySide6.QtWidgets import (QDialog, QHBoxLayout, QLabel, QProgressBar, QPushButton,
                               QVBoxLayout)

from .. import firmware as FW


class _Worker(QObject):
    progress = Signal(str, float, str)
    finished = Signal(bool, str)

    def __init__(self, pkg, mcus):
        super().__init__()
        self.pkg, self.mcus = pkg, mcus

    def run(self):
        try:
            FW.update_with_backup(self.pkg, lambda s, p, m: self.progress.emit(s, p, m))
            self.finished.emit(True, "")
        except Exception as e:  # report every failure to the UI
            self.finished.emit(False, str(e))


STAGES = {"prepare": "Preparing", "transfer": "Transferring firmware",
          "validate": "Validating", "install": "Installing", "restart": "Restarting keyboard",
          "done": "Done"}


class FirmwareDialog(QDialog):
    def __init__(self, parent, pkg, installed: str):
        super().__init__(parent)
        self.setWindowTitle("Firmware update")
        self.setModal(True)
        self.setMinimumWidth(520)
        self.pkg = pkg
        self.ok = False
        lay = QVBoxLayout(self)
        lay.setSpacing(12)
        title = QLabel(f"Update firmware {installed} → {pkg.version}", objectName="PageTitle")
        lay.addWidget(title)
        notes = "".join(f"<li>{n}</li>" for n in (pkg.notes or []))
        self.info = QLabel(
            (f"<b>What's new</b><ul>{notes}</ul>" if notes else "") +
            "Keyboard, media dock and numpad are updated together. This takes a few "
            "minutes.<br><br><b style='color:#ee7f00'>Do not unplug the keyboard, close this "
            "window or let the computer sleep until it finishes.</b> The keyboard can't be "
            "used for typing while it updates.")
        self.info.setWordWrap(True)
        self.info.setTextFormat(Qt.RichText)
        lay.addWidget(self.info)
        self.stage = QLabel("", objectName="Section")
        self.bar = QProgressBar()
        self.bar.setRange(0, 1000)
        self.bar.setTextVisible(False)
        self.bar.setStyleSheet("QProgressBar{background:#2c2e31;border:none;height:8px}"
                               "QProgressBar::chunk{background:#ee7f00}")
        self.detail = QLabel("", objectName="Muted")
        for w in (self.stage, self.bar, self.detail):
            w.hide()
            lay.addWidget(w)
        row = QHBoxLayout()
        row.addStretch()
        self.cancel = QPushButton("Cancel")
        self.cancel.clicked.connect(self.reject)
        self.start = QPushButton("Update now", objectName="Primary")
        self.start.clicked.connect(self.begin)
        row.addWidget(self.cancel)
        row.addWidget(self.start)
        lay.addLayout(row)
        self.running = False

    def begin(self):
        self.running = True
        self.start.setEnabled(False)
        self.cancel.setEnabled(False)
        for w in (self.stage, self.bar, self.detail):
            w.show()
        self.stage.setText("PREPARING")
        self.thread = QThread(self)
        self.worker = _Worker(self.pkg, [0, 1, 2])
        self.worker.moveToThread(self.thread)
        self.thread.started.connect(self.worker.run)
        self.worker.progress.connect(self.on_progress)
        self.worker.finished.connect(self.on_finished)
        self.worker.finished.connect(self.thread.quit)
        self.thread.start()

    def on_progress(self, stage, pct, msg):
        self.stage.setText(STAGES.get(stage, stage).upper())
        if stage == "transfer":
            self.bar.setValue(int(pct * 10))
        elif stage in ("validate", "install", "restart", "done"):
            self.bar.setValue(1000)
        if msg:
            self.detail.setText(msg)

    def on_finished(self, ok, err):
        self.running = False
        self.ok = ok
        self.cancel.setEnabled(True)
        self.cancel.setText("Close")
        if ok:
            self.stage.setText("UPDATE COMPLETE")
            self.detail.setText(f"Your keyboard now runs firmware {self.pkg.version}.")
        else:
            self.stage.setText("UPDATE FAILED")
            self.stage.setStyleSheet("color:#fa5343")
            self.detail.setText(f"{err}\n\nIf the keyboard stays in update mode, open this "
                                "window again and press “Update now” to resume.")
            self.detail.setWordWrap(True)

    def reject(self):
        if self.running:
            return  # can't cancel mid-update
        super().reject()

    def closeEvent(self, e):
        if self.running:
            e.ignore()
        else:
            super().closeEvent(e)
