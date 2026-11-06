"""QLink transport for be quiet! devices over Linux hidraw.

Reconstructed from the IO Center web app's protocol code. Frame layout for
64-byte packets:

    [0]  payload length (data length + 6)
    [1]  sequence id (bit 7 = more frames follow)
    [2]  session id
    [3]  status
    [4]  request id
    [5]  feature id
    [6]  command id
    [7:] data
    [62:64] CRC-16/Modbus over bytes 0..61, little-endian

Continuation frames (multi-frame transfers) only carry bytes 0..2 and put
data from byte 3 onwards.
"""
from __future__ import annotations

import glob
import os
import random
import select
import struct
import threading
import time
from dataclasses import dataclass
from enum import IntEnum

VID_BEQUIET = 0x373F
PACKET_SIZE = 64
VENDOR_USAGE_PAGE = 0xFF00


class Feature(IntEnum):
    ROOT = 1
    DFU = 2
    DEVICE_INFO = 3
    STORAGE = 4
    HUB = 5
    USB_DEVICE = 6
    KEYBOARD = 7
    MOUSE = 8
    BATTERY = 9
    PAIRING = 10
    LIGHTINGS = 16
    BINDINGS = 17
    MACROS = 18
    NUMPAD_MODULE = 32
    MEDIA_DOCK_MODULE = 33


class Status(IntEnum):
    SUCCESS = 0
    INVALID_SESSION_ID = 1
    INVALID_FEATURE_ID = 2
    INVALID_COMMAND_ID = 3
    INVALID_REQUEST_ID = 4
    INVALID_PARAMETER = 5
    TIMEOUT = 6
    BUSY = 7
    NOT_AUTHORIZED = 8
    NOT_ACTIVE = 9
    INVALID_STATE = 10
    INVALID_SIZE = 11
    INSUFFICIENT_RESOURCES = 12
    NOT_SUPPORTED = 13


class RootCmd(IntEnum):
    OpenSession = 1
    CloseSession = 2
    KeepAlive = 3
    GetSupportedFeatures = 4
    GetQLinkVersion = 5
    GetActiveSessionInfo = 6
    RequestStateChange = 7
    SendStateChangeDecision = 8


class RootNotify(IntEnum):
    SessionStateChanged = 1
    ActiveSessionChanged = 2
    StateChangeRequested = 3
    StateChangeDecision = 4


class ClientType(IntEnum):
    Unspecified = 0
    QControl = 1  # the Windows IO Center app
    Web = 2


class QLinkError(Exception):
    pass


def crc16_modbus(data: bytes) -> int:
    crc = 0xFFFF
    for b in data:
        crc ^= b
        for _ in range(8):
            crc = (crc >> 1) ^ 0xA001 if crc & 1 else crc >> 1
    return crc


@dataclass
class Frame:
    raw: bytes

    @property
    def length(self) -> int:
        return self.raw[0]

    @property
    def sequence_id(self) -> int:
        return self.raw[1] & 0x7F

    @property
    def more(self) -> bool:
        return bool(self.raw[1] >> 7)

    @property
    def session_id(self) -> int:
        return self.raw[2]

    @property
    def status(self) -> int:
        return self.raw[3]

    @property
    def request_id(self) -> int:
        return self.raw[4]

    @property
    def feature(self) -> int:
        return self.raw[5]

    @property
    def command(self) -> int:
        return self.raw[6]

    @property
    def is_notification(self) -> bool:
        return self.request_id == 0 and self.sequence_id == 0

    @property
    def data(self) -> bytes:
        # Mirrors the web app: first frame data starts at 7, continuation at 3.
        start = 3 if self.sequence_id > 0 else 7
        return self.raw[start:self.length + 1]

    def crc_ok(self) -> bool:
        got = self.raw[PACKET_SIZE - 2] | (self.raw[PACKET_SIZE - 1] << 8)
        return got == crc16_modbus(self.raw[:PACKET_SIZE - 2])


@dataclass
class Response:
    status: int
    feature: int
    command: int
    data: bytes


def build_frames(session: int, request_id: int, feature: int, command: int,
                 payload: bytes = b"") -> list[bytes]:
    """Build one or more 64-byte frames for a request.

    Mirrors the official app byte for byte, including how it computes the
    "more frames" flag: the expected frame count is ceil(len / 55) even though
    continuation frames carry 59 bytes, so for some large payloads the last
    frame still has bit 7 set. The firmware is tested against that behaviour.
    """
    first_max = PACKET_SIZE - 9
    if len(payload) <= first_max:
        chunks = [payload]
        total = 1
    else:
        chunks = [payload[:first_max]]
        rest = payload[first_max:]
        while rest:
            chunks.append(rest[:first_max + 4])
            rest = rest[first_max + 4:]
        total = -(-len(payload) // first_max)  # what the official app uses
    frames = []
    for idx, chunk in enumerate(chunks):
        p = bytearray(PACKET_SIZE)
        p[0] = (len(chunk) + 6) & 0xFF
        p[1] = (idx | ((0 if total - 1 == idx else 1) << 7)) if total > 1 else 0
        p[2] = session
        if idx == 0:
            p[3] = Status.SUCCESS
            p[4] = request_id
            p[5] = feature
            p[6] = command
            p[7:7 + len(chunk)] = chunk
        else:
            p[3:3 + len(chunk)] = chunk
        crc = crc16_modbus(bytes(p[:PACKET_SIZE - 2]))
        p[PACKET_SIZE - 2] = crc & 0xFF
        p[PACKET_SIZE - 1] = crc >> 8
        frames.append(bytes(p))
    return frames


@dataclass
class HidrawNode:
    path: str
    vid: int
    pid: int
    name: str
    usage_page: int | None
    uses_report_ids: bool


def _parse_descriptor(desc: bytes) -> tuple[int | None, bool]:
    """Return (first usage page, uses report ids) from a HID report descriptor."""
    i = 0
    usage_page = None
    report_ids = False
    while i < len(desc):
        prefix = desc[i]
        if prefix == 0xFE:  # long item
            i += 3 + desc[i + 1]
            continue
        size = (0, 1, 2, 4)[prefix & 0x3]
        tag = prefix & 0xFC
        val = int.from_bytes(desc[i + 1:i + 1 + size], "little")
        if tag == 0x04 and usage_page is None:
            usage_page = val
        elif tag == 0x84:
            report_ids = True
        i += 1 + size
    return usage_page, report_ids


def list_nodes(vid: int = VID_BEQUIET) -> list[HidrawNode]:
    nodes = []
    for sys_path in sorted(glob.glob("/sys/class/hidraw/hidraw*")):
        try:
            uevent = open(os.path.join(sys_path, "device/uevent")).read()
        except OSError:
            continue
        props = dict(line.split("=", 1) for line in uevent.splitlines() if "=" in line)
        hid_id = props.get("HID_ID", "")
        try:
            _bus, v, p = hid_id.split(":")
            v, p = int(v, 16), int(p, 16)
        except ValueError:
            continue
        if v != vid:
            continue
        try:
            desc = open(os.path.join(sys_path, "device/report_descriptor"), "rb").read()
            page, rids = _parse_descriptor(desc)
        except OSError:
            page, rids = None, False
        nodes.append(HidrawNode("/dev/" + os.path.basename(sys_path), v, p,
                                props.get("HID_NAME", ""), page, rids))
    return nodes


def find_control_node(pid: int | None = None) -> HidrawNode:
    nodes = [n for n in list_nodes() if n.usage_page == VENDOR_USAGE_PAGE]
    if pid is not None:
        nodes = [n for n in nodes if n.pid == pid]
    if not nodes:
        raise QLinkError("No be quiet! QLink device found (vendor 0x373F, usage page 0xFF00). "
                         "Is the keyboard plugged in?")
    return nodes[0]


@dataclass
class Session:
    session_id: int = 0
    connection_id: int = 0
    state: int = 0
    timeout: int = 0


class QLinkDevice:
    """A QLink connection over a hidraw node."""

    def __init__(self, node: HidrawNode, client_type: ClientType = ClientType.Web,
                 debug: bool = False):
        self.node = node
        self.client_type = client_type
        self.debug = debug
        self.fd: int | None = None
        self.session = Session()
        self._req_id = 1
        self._lock = threading.RLock()
        self._keepalive_stop = threading.Event()
        self._keepalive_thread: threading.Thread | None = None
        self.notifications: list[Frame] = []
        self.foreign_seen = 0.0  # last time we saw traffic for another session

    # -- low level -------------------------------------------------------
    def open(self):
        try:
            self.fd = os.open(self.node.path, os.O_RDWR)
        except PermissionError as e:
            raise QLinkError(f"Permission denied opening {self.node.path}. Install the udev rule "
                             "(see README) and replug the keyboard.") from e

    def close(self):
        self.stop_keepalive()
        if self.fd is not None:
            try:
                if self.session.session_id:
                    self.request(Feature.ROOT, RootCmd.CloseSession, bytes([self.session.session_id]))
            except QLinkError:
                pass
            os.close(self.fd)
            self.fd = None

    def __enter__(self):
        self.open()
        return self

    def __exit__(self, *exc):
        self.close()

    def _next_request_id(self) -> int:
        self._req_id = (self._req_id + 1) % 256 or 1
        return self._req_id

    def _write(self, frame: bytes):
        if self.debug:
            print(">>", frame.hex(" "))
        prefix = b"" if self.node.uses_report_ids else b"\x00"
        os.write(self.fd, prefix + frame)

    def _read(self, timeout: float) -> Frame | None:
        r, _, _ = select.select([self.fd], [], [], timeout)
        if not r:
            return None
        raw = os.read(self.fd, 256)
        if self.node.uses_report_ids:
            raw = raw[1:]
        raw = raw[:PACKET_SIZE].ljust(PACKET_SIZE, b"\x00")
        if self.debug:
            print("<<", raw.hex(" "))
        f = Frame(raw)
        if not f.is_notification and f.session_id != self.session.session_id:
            self.foreign_seen = time.monotonic()  # a reply to another client's request
        return f

    def request(self, feature: int, command: int, payload: bytes = b"",
                timeout: float = 1.0, session: int | None = None) -> Response:
        """Send a request and return the (reassembled) response frame."""
        with self._lock:
            sess = self.session.session_id if session is None else session
            frames = build_frames(sess, self._next_request_id(), feature, command, payload)
            for f in frames:
                self._write(f)
            deadline = time.monotonic() + timeout
            while True:
                remaining = deadline - time.monotonic()
                if remaining <= 0:
                    raise QLinkError(f"Timeout waiting for {Feature(feature).name}:{command}")
                resp = self._read(remaining)
                if resp is None:
                    continue
                if resp.is_notification:
                    # never a reply (notifications can share feature+command ids)
                    self.notifications.append(resp)
                    continue
                if resp.feature == feature and resp.command == command:
                    break
            data = bytearray(resp.data)
            last = resp
            while last.sequence_id > 0 or last.more:
                nxt = self._read(max(0.05, deadline - time.monotonic()))
                if nxt is None:
                    raise QLinkError("Timeout waiting for continuation frame")
                data += nxt.data
                last = nxt
                if not nxt.more:
                    break
            if resp.status != Status.SUCCESS:
                try:
                    name = Status(resp.status).name
                except ValueError:
                    name = str(resp.status)
                raise QLinkError(f"{Feature(feature).name}:{command} failed: {name}")
            return Response(resp.status, resp.feature, resp.command, bytes(data))

    def next_notification(self, timeout: float) -> Frame | None:
        """Wait up to `timeout` seconds for an unsolicited frame from the device.

        Reads in short slices under the request lock so keep-alives and other
        requests can interleave safely.
        """
        deadline = time.monotonic() + timeout
        while True:
            if self.notifications:
                return self.notifications.pop(0)
            remaining = deadline - time.monotonic()
            if remaining <= 0:
                return None
            with self._lock:
                f = self._read(min(0.2, remaining))
            if f is not None and f.is_notification:  # skip stray replies
                return f

    # -- session ---------------------------------------------------------
    def open_session(self, keepalive: bool = True) -> Session:
        nonce = random.randint(10000, 99999)
        payload = struct.pack("<I", nonce) + bytes([self.client_type])
        resp = self.request(Feature.ROOT, RootCmd.OpenSession, payload, session=0)
        d = resp.data
        self.session = Session(
            connection_id=struct.unpack_from("<I", d, 0)[0],
            session_id=d[4],
            state=d[5] if len(d) > 5 else 0,
            timeout=d[6] if len(d) > 6 else 0,
        )
        if not 0 < self.session.session_id < 255:
            raise QLinkError(f"Device refused session (id={self.session.session_id})")
        if self.session.timeout and keepalive:
            self.start_keepalive(self.session.timeout)
        return self.session

    def close_session(self):
        self.stop_keepalive()
        if self.session.session_id:
            try:
                self.request(Feature.ROOT, RootCmd.CloseSession,
                             bytes([self.session.session_id]))
            except QLinkError:
                pass
        self.session = Session()

    def ensure_active(self, timeout: float = 4.0) -> bool:
        """Become the keyboard's active session (needed for writes on firmware >= 1.31).

        Only the oldest open session may write. Other darkmount processes (the
        background service) step aside when they see our traffic, so we re-open our
        session until it comes up active.
        """
        if self.session.state == 1:
            return True
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            self.close_session()
            time.sleep(0.4)
            self.open_session()
            if self.session.state == 1:
                return True
        return False

    def start_keepalive(self, timeout_s: int):
        self.stop_keepalive()
        self._keepalive_stop.clear()

        def loop():
            while not self._keepalive_stop.wait(timeout_s / 2):
                try:
                    self.request(Feature.ROOT, RootCmd.KeepAlive)
                except (QLinkError, OSError):
                    pass

        self._keepalive_thread = threading.Thread(target=loop, daemon=True)
        self._keepalive_thread.start()

    def stop_keepalive(self):
        self._keepalive_stop.set()
        if self._keepalive_thread:
            self._keepalive_thread.join(timeout=1)
            self._keepalive_thread = None
