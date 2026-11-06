"""Firmware update (DFU feature) for the Dark Mount.

Follows the official app's sequence exactly:

  normal mode (PID 0x0001)
    DFU/Sync(upgrade_id, mcus)        -> keyboard reboots into its bootloader
  bootloader (PID 0x0009)
    DFU/Sync(upgrade_id, mcus)        -> resume point per MCU (resumable!)
    for each MCU whose resume point is Transfer:
      DFU/Transfer(mcu, crc16, size)
        <- DataRequest(mcu, chunk, offset, length)   (repeated)
        -> DFU/WriteData(mcu, bytes)                 (chunk by chunk)
        <- TransferComplete(mcu)
      DFU/Validate(mcu)
        <- ValidationComplete(mcu)
    <- ResumePointChanged(mcu, Ready) for all  -> DFU/Ready
    <- ResumePointChanged(mcu, Commit) for all -> DFU/Commit
    <- CommitComplete                          -> keyboard reboots with new firmware

Firmware images come from the vendor's update server (cached under
~/.cache/darkmount/firmware/) or from a local folder, and are verified against
the SHA-512 "integrity" values in the manifest before anything is written.
"""
from __future__ import annotations

import base64
import hashlib
import json
import os
import shutil
import struct
import subprocess
import time
import urllib.request
from dataclasses import dataclass
from enum import IntEnum
from pathlib import Path
from typing import Callable

from .qlink import (ClientType, Feature, Frame, QLinkDevice, QLinkError, crc16_modbus,
                    list_nodes, VENDOR_USAGE_PAGE)

PID_NORMAL = 0x0001
PID_BOOTLOADER = 0x0009
BASE_URL = "https://dfu-release.bequiet.com/fw/dark_mount"
CACHE = Path(os.environ.get("XDG_CACHE_HOME", Path.home() / ".cache")) / "darkmount" / "firmware"
UPGRADE_ID = 5648 + 1  # DFUState.UpgradeID + 1 in the official app
MCU_NAMES = {0: "Main", 1: "MediaModule", 2: "NumpadModule"}
MCU_LABELS = {0: "Keyboard", 1: "Media Dock", 2: "Numpad"}


class DfuCmd(IntEnum):
    Sync = 1
    Transfer = 2
    WriteData = 3
    Validate = 4
    Ready = 5
    Commit = 6
    Abort = 7


class DfuNotify(IntEnum):
    ErrorOccurred = 1
    DataRequest = 2
    TransferComplete = 3
    ValidationComplete = 4
    ReadyComplete = 5
    CommitComplete = 6
    Aborted = 7
    ResumePointChanged = 8


class ResumePoint(IntEnum):
    Transfer = 0
    Validate = 1
    Ready = 2
    Commit = 3
    WriteData = 4


class DfuError(IntEnum):
    InvalidState = 1
    InvalidLength = 2
    WriteFailed = 3
    LoaderError = 4
    ValidationFailed = 5
    ForceSync = 6


class FirmwareError(Exception):
    pass


# ----------------------------------------------------------------------------- packages
def _ver(s: str) -> tuple[int, ...]:
    return tuple(int(x) for x in s.split(".")[:3])


@dataclass
class Image:
    mcu: int
    name: str
    version: str
    source: str
    integrity: str
    data: bytes | None = None

    def verify(self) -> bool:
        if self.data is None:
            return False
        digest = base64.b64encode(hashlib.sha512(self.data).digest()).decode()
        return digest == self.integrity


@dataclass
class Package:
    version: str
    images: dict[int, Image]
    folder: Path
    release_date: str = ""
    notes: list[str] | None = None


def select_package(manifest: dict) -> list[dict]:
    """Pick the newest firmware set, as the official app does (devices + devices_v2)."""
    best = manifest["devices"][0]
    for cand in manifest.get("devices_v2", []) or []:
        if _ver(cand["firmwares"][0]["version"]) > _ver(best["firmwares"][0]["version"]):
            best = cand
    return best["firmwares"]


def _package_from(manifest: dict, folder: Path, load: bool = True) -> Package:
    order = {v: k for k, v in MCU_NAMES.items()}
    images = {}
    fws = select_package(manifest)
    for i, fw in enumerate(fws):
        mcu = order.get(fw.get("mcu"), i)
        img = Image(mcu, fw.get("mcu", f"MCU{mcu}"), fw["version"], fw["source"],
                    fw["integrity"])
        if load and (folder / fw["source"]).exists():
            img.data = (folder / fw["source"]).read_bytes()
        images[mcu] = img
    notes = None
    pn = folder / "patch-notes.json"
    if pn.exists():
        try:
            notes = [n for item in json.loads(pn.read_text()).get("notes", [])
                     for n in item.get("content", {}).get("en", [])]
        except (ValueError, AttributeError):
            notes = None
    return Package(fws[0]["version"], images, folder, fws[0].get("releaseDate", ""), notes)


def _get(url: str, timeout: float = 30) -> bytes:
    req = urllib.request.Request(url, headers={"User-Agent": "darkmount-linux"})
    with urllib.request.urlopen(req, timeout=timeout) as r:
        return r.read()


def download_latest(progress: Callable[[str], None] = print) -> Package:
    """Fetch the newest firmware into the cache (needs internet). Returns the package."""
    manifest = json.loads(_get(f"{BASE_URL}/index.json?v={int(time.time())}"))
    fws = select_package(manifest)
    version = fws[0]["version"]
    folder = CACHE / version
    folder.mkdir(parents=True, exist_ok=True)
    (folder / "index.json").write_text(json.dumps(manifest, indent=1))
    try:
        (folder / "patch-notes.json").write_bytes(_get(f"{BASE_URL}/patch-notes.json"))
    except OSError:
        pass
    for fw in fws:
        path = folder / fw["source"]
        if not path.exists():
            progress(f"downloading {fw['source']}")
            path.write_bytes(_get(f"{BASE_URL}/{fw['source']}", timeout=120))
    pkg = _package_from(manifest, folder)
    for img in pkg.images.values():
        if not img.verify():
            (folder / img.source).unlink(missing_ok=True)
            raise FirmwareError(f"{img.source}: checksum mismatch after download")
    return pkg


def load_package(folder: Path) -> Package:
    """Load a firmware set from a folder containing index.json and the .bin files."""
    folder = Path(folder).expanduser()
    try:
        manifest = json.loads((folder / "index.json").read_text())
    except (OSError, ValueError) as e:
        raise FirmwareError(f"{folder}: no readable index.json ({e})")
    pkg = _package_from(manifest, folder)
    for img in pkg.images.values():
        if img.data is None:
            raise FirmwareError(f"{folder / img.source} is missing")
        if not img.verify():
            raise FirmwareError(f"{img.source}: SHA-512 does not match the manifest")
    return pkg


def cached_packages() -> list[Package]:
    out = []
    if CACHE.exists():
        for d in sorted(CACHE.iterdir(), key=lambda p: _ver(p.name) if p.name[0].isdigit()
                        else (0,), reverse=True):
            try:
                out.append(load_package(d))
            except FirmwareError:
                continue
    return out


# ----------------------------------------------------------------------------- updater
def _find(pid: int):
    for n in list_nodes():
        if n.pid == pid and n.usage_page == VENDOR_USAGE_PAGE:
            return n
    return None


def _wait_for(pid: int, timeout: float):
    end = time.monotonic() + timeout
    while time.monotonic() < end:
        n = _find(pid)
        if n:
            try:
                fd = os.open(n.path, os.O_RDWR)
                os.close(fd)
                return n
            except OSError:
                pass  # udev may not have applied permissions yet
        time.sleep(0.3)
    return None


def service_active() -> bool:
    if not shutil.which("systemctl"):
        return False
    r = subprocess.run(["systemctl", "--user", "is-active", "darkmount-clock.service"],
                       capture_output=True, text=True)
    return r.stdout.strip() == "active"


def set_service(running: bool):
    if shutil.which("systemctl"):
        subprocess.run(["systemctl", "--user", "start" if running else "stop",
                        "darkmount-clock.service"], capture_output=True)


def update_with_backup(pkg: Package, report: Callable[[str, float, str], None],
                       restore_settings: bool = True, debug: bool = False):
    """Back up settings (if the keyboard is running), update, then restore them.

    Firmware updates reset the keyboard to factory settings.
    """
    from . import backup
    from .device import DarkMount
    snap = None
    if _find(PID_NORMAL):
        try:
            with DarkMount() as kb:
                snap = backup.snapshot(kb)
            path = backup.save(snap)
            report("prepare", 0, f"settings backed up to {path}")
        except (QLinkError, OSError) as e:
            report("prepare", 0, f"warning: couldn't back up settings ({e})")
    elif restore_settings:
        snap = backup.latest()
        if snap:
            report("prepare", 0, f"will restore settings saved {snap.get('time')}")
    Updater(pkg, [0, 1, 2], report, debug=debug).run()
    if restore_settings and snap:
        time.sleep(1.0)
        try:
            with DarkMount() as kb:
                backup.restore(kb, snap, lambda m: report("done", 100, m))
        except (QLinkError, OSError) as e:
            report("done", 100, f"warning: couldn't restore settings ({e}); "
                                "run `darkmount backup restore`")


class Updater:
    """Runs a firmware update. `report(stage, percent, message)` gets progress."""

    def __init__(self, package: Package | None, mcus: list[int] | None = None,
                 report: Callable[[str, float, str], None] | None = None,
                 debug: bool = False):
        self.pkg = package
        self.mcus = sorted(mcus) if mcus else None
        self.report = report or (lambda s, p, m: print(f"[{s}] {p:5.1f}% {m}"))
        self.debug = debug
        self.dev: QLinkDevice | None = None
        self.rp: list[int] = []
        self.written: dict[int, int] = {}
        self.active: int | None = None     # MCU currently transferring
        self.started: set[int] = set()     # MCUs we've sent Transfer for

    # -- helpers ---------------------------------------------------------------
    def _req(self, cmd: DfuCmd, payload: bytes = b"", timeout: float = 3.0):
        return self.dev.request(Feature.DFU, cmd, payload, timeout=timeout).data

    def _open(self, node, keepalive: bool):
        self.dev = QLinkDevice(node, client_type=ClientType.Web, debug=self.debug)
        self.dev.open()
        self.dev.open_session(keepalive=keepalive)

    def _close(self):
        if self.dev:
            try:
                self.dev.close()
            except Exception:
                pass
            self.dev = None

    def _sync(self) -> list[int]:
        payload = struct.pack("<I", UPGRADE_ID) + bytes([len(self.mcus)] + self.mcus)
        d = self._req(DfuCmd.Sync, payload, timeout=5)
        count = d[4]
        return list(d[5:5 + count])

    def _progress(self, stage="transfer", msg=""):
        total = sum(len(self.pkg.images[m].data) for m in self.mcus)
        done = sum(self.written.get(m, 0) for m in self.mcus)
        self.report(stage, 100.0 * done / max(1, total), msg)

    def _start_next(self) -> bool:
        nxt = next((i for i, r in enumerate(self.rp) if r == ResumePoint.Transfer
                    and self.mcus[i] not in self.started), None)
        if nxt is None:
            return False
        self._transfer(nxt)
        return True

    def _transfer(self, idx: int):
        mcu = self.mcus[idx]
        self.active = mcu
        self.started.add(mcu)
        img = self.pkg.images[mcu].data
        crc = crc16_modbus(img)
        self.report("transfer", 0, f"sending {MCU_LABELS.get(mcu, mcu)} firmware "
                                   f"({len(img)} bytes)")
        self._req(DfuCmd.Transfer, bytes([mcu, crc & 0xFF, crc >> 8]) +
                  struct.pack("<I", len(img)))

    def _write(self, data: bytes):
        mcu, chunk = data[0], data[1] | (data[2] << 8)
        offset = struct.unpack_from("<I", data, 3)[0]
        length = struct.unpack_from("<I", data, 7)[0]
        if mcu not in self.mcus:
            raise FirmwareError(f"keyboard requested data for unexpected MCU {mcu}")
        idx = self.mcus.index(mcu)
        self.rp[idx] = ResumePoint.WriteData
        img = self.pkg.images[mcu].data
        if chunk <= 0:
            return
        end = min(offset + length, len(img))
        p = offset
        while p < end:
            self._req(DfuCmd.WriteData, bytes([mcu]) + img[p:min(p + chunk, end)], timeout=5)
            p += chunk
            self.written[mcu] = min(len(img), p)
            self._progress(msg=f"{MCU_LABELS.get(mcu, mcu)}: {self.written[mcu]}/{len(img)}")
            time.sleep(0.002)

    # -- main --------------------------------------------------------------------------
    def run(self):
        if self.pkg is None:
            raise FirmwareError("no firmware package")
        for img in self.pkg.images.values():
            if not img.verify():
                raise FirmwareError(f"{img.source}: checksum mismatch")
        restart_service = service_active()
        if restart_service:
            self.report("prepare", 0, "pausing the darkmount background service")
            set_service(False)
            time.sleep(1)
        try:
            self._run()
        finally:
            self._close()
            if restart_service:
                set_service(True)

    def _run(self):
        node = _find(PID_NORMAL) or _find(PID_BOOTLOADER)
        if node is None:
            raise FirmwareError("keyboard not found")
        if self.mcus is None:
            self.mcus = sorted(self.pkg.images)
        missing = [m for m in self.mcus if m not in self.pkg.images]
        if missing:
            raise FirmwareError(f"package has no image for MCU {missing}")

        if node.pid == PID_NORMAL:
            self.report("prepare", 0, "asking the keyboard to enter update mode")
            self._open(node, keepalive=True)
            try:
                self._sync()
            except (QLinkError, OSError) as e:
                # The keyboard usually reboots into its bootloader right away, without
                # answering (the USB device disappears -> EIO / timeout). Only a real
                # status error means it refused.
                if isinstance(e, QLinkError) and "failed:" in str(e):
                    raise FirmwareError(f"keyboard refused update mode: {e}")
            if self.dev:
                self.dev.stop_keepalive()
                try:
                    os.close(self.dev.fd)
                except OSError:
                    pass
                self.dev = None
            self.report("prepare", 0, "waiting for the bootloader")
            node = _wait_for(PID_BOOTLOADER, 30)
            if node is None:
                raise FirmwareError("keyboard did not come back in bootloader mode within 30 s "
                                    "(it is still running its old firmware)")
            time.sleep(0.5)
        else:
            self.report("prepare", 0, "keyboard is already in bootloader mode — resuming")

        self._open(node, keepalive=False)
        self.rp = self._sync()
        if len(self.rp) != len(self.mcus):
            raise FirmwareError(f"bootloader reports {len(self.rp)} MCUs, expected "
                                f"{len(self.mcus)}")
        self.report("transfer", 0, "resume points: " + ", ".join(
            f"{MCU_LABELS.get(m, m)}={ResumePoint(r).name}" for m, r in zip(self.mcus, self.rp)))
        # Mirrors DFU_Run: act on the first MCU's resume point.
        first = self.rp[0]
        if first == ResumePoint.Transfer:
            self._transfer(0)
        elif first == ResumePoint.Validate:
            self._req(DfuCmd.Validate, bytes([self.mcus[0]]))
        elif first == ResumePoint.Ready:
            self._req(DfuCmd.Ready)
        elif first == ResumePoint.Commit:
            try:
                self._req(DfuCmd.Commit)
            except (QLinkError, OSError):
                pass
            self.rp = []  # nothing else to do; wait for the restart below

        last_event = time.monotonic()
        committed = False
        while self.rp:
            try:
                f = self.dev.next_notification(1.0)
            except OSError:
                # device vanished: fine if it was installing (reboot), an error otherwise
                if all(r in (ResumePoint.Ready, ResumePoint.Commit) for r in self.rp):
                    break
                raise FirmwareError("keyboard disconnected during the update — run the "
                                    "update again to resume")
            if f is None:
                idle = time.monotonic() - last_event
                if idle > 5 and self.active is None and not committed:
                    # stall guard: previous MCU is done, next one hasn't been started
                    if self._start_next():
                        last_event = time.monotonic()
                        continue
                if idle > 60:
                    raise FirmwareError("no response from the bootloader for 60 s")
                continue
            last_event = time.monotonic()
            if f.feature != Feature.DFU:
                continue
            d = bytes(f.data)
            ev = f.command
            if self.debug:
                print("DFU event", ev, d.hex(" "))
            if ev == DfuNotify.DataRequest:
                self._write(d)
            elif ev == DfuNotify.TransferComplete:
                mcu = d[0]
                self.active = None
                self.report("validate", 100.0 if mcu == self.mcus[-1] else 0,
                            f"validating {MCU_LABELS.get(mcu, mcu)}")
                self._req(DfuCmd.Validate, bytes([mcu]))
            elif ev == DfuNotify.ValidationComplete:
                # official app: only move on if this MCU isn't still marked WriteData
                idx = self.mcus.index(d[0]) if d[0] in self.mcus else 0
                if self.rp[idx] != ResumePoint.WriteData:
                    self._start_next()
            elif ev == DfuNotify.ResumePointChanged:
                if d[0] not in self.mcus:
                    continue
                idx = self.mcus.index(d[0])
                new = d[1]
                if self.rp[idx] != new:
                    self.rp[idx] = new
                    if all(r == ResumePoint.Ready for r in self.rp):
                        self.report("install", 100, "all images validated — installing")
                        self._req(DfuCmd.Ready)
                    elif all(r == ResumePoint.Commit for r in self.rp):
                        self.report("install", 100, "committing")
                        try:
                            self._req(DfuCmd.Commit)
                        except (QLinkError, OSError):
                            pass  # the keyboard reboots immediately, often without replying
                        break
                    elif committed and all(r == ResumePoint.Transfer for r in self.rp):
                        break
            elif ev == DfuNotify.CommitComplete:
                break
            elif ev == DfuNotify.ReadyComplete:
                pass
            elif ev == DfuNotify.Aborted:
                raise FirmwareError("the bootloader aborted the update")
            elif ev == DfuNotify.ErrorOccurred:
                try:
                    name = DfuError(d[0]).name
                except (ValueError, IndexError):
                    name = d.hex(" ")
                raise FirmwareError(f"bootloader error: {name}")

        self._close()
        self.report("restart", 100, "keyboard is restarting")
        node = _wait_for(PID_NORMAL, 60)
        if node is None:
            raise FirmwareError("keyboard did not restart in normal mode within 60 s")
        time.sleep(1.5)
        self.report("done", 100, "update complete")
