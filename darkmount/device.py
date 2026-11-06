"""High-level API for the be quiet! Dark Mount keyboard."""
from __future__ import annotations

import struct
import time
from dataclasses import dataclass, field
from enum import IntEnum, IntFlag

from .qlink import (ClientType, Feature, QLinkDevice, QLinkError, RootCmd,
                    find_control_node)

PID_DARK_MOUNT = 0x0001
PID_DARK_MOUNT_BOOTLOADER = 0x0009


class DeviceInfoCmd(IntEnum):
    GetDeviceInfo = 1
    GetSerialNumber = 2


class KeyboardCmd(IntEnum):
    GetLayout = 1
    GetConfig = 2
    SetConfig = 3
    GetState = 4
    SetState = 5


class UsbCmd(IntEnum):
    GetPollingRate = 1
    SetPollingRate = 2


class LightingCmd(IntEnum):
    GetLightingMode = 1
    SetLightingMode = 2
    GetLayersLayout = 3
    GetLayerConfig = 5
    SetLayerConfig = 6


class BindingsCmd(IntEnum):
    GetBindings = 1
    SetBinding = 2
    ClearBinding = 3
    GetConfig = 4
    SetConfig = 5
    GetBinding = 6


class BindingsNotify(IntEnum):
    BindingChanged = 1
    ActionFired = 2
    ConfigChanged = 3


class ModuleCmd(IntEnum):  # shared by numpad + media dock for GetState
    GetState = 1


class MediaDockCmd(IntEnum):
    GetState = 1
    GetConfig = 2
    SetConfig = 3
    GetDateTime = 4
    SetDateTime = 5


class LightingMode(IntEnum):
    Off = 0
    General = 1
    Custom = 2
    Realtime = 3


class Effect(IntEnum):
    Static = 0
    ColorWave = 1
    Tornado = 2
    Breathing = 3
    Reactive = 4
    Matrix = 5
    Off = 6
    Ripple = 7


class ColorMode(IntEnum):
    SingleColor = 0
    DualColor = 1
    Gradient = 2
    OrientedGradient = 3


class Direction(IntEnum):
    Up = 0
    Down = 1
    Left = 2
    Right = 3
    Clockwise = 4
    Counterclockwise = 5
    Omnidirectional = 6
    Horizontal = 7
    Vertical = 8
    Cross = 9


class GameModeFlags(IntFlag):
    NONE = 0
    SHIFT_TAB = 1
    ALT_F4 = 2
    WIN = 4
    ALT_TAB = 8
    CAPS_LOCK = 16


class ScreensaverMode(IntEnum):
    Disabled = 0
    Clock = 1
    Image = 2


class ClockMode(IntEnum):
    Clock12h = 0
    Clock24h = 1


RGB = tuple[int, int, int]


def parse_color(s: str) -> RGB:
    s = s.strip().lstrip("#")
    if len(s) != 6:
        raise ValueError(f"bad color {s!r}, expected RRGGBB")
    return int(s[0:2], 16), int(s[2:4], 16), int(s[4:6], 16)


def fmt_color(c: RGB) -> str:
    return "#%02x%02x%02x" % c


@dataclass
class LightingConfig:
    effect: Effect = Effect.Static
    direction: int = Direction.Down
    brightness: int = 100
    speed: int = 50
    color_mode: ColorMode = ColorMode.SingleColor
    # SingleColor -> [c]; DualColor -> [c1, c2]; Gradient -> [(c, pos), ...]
    colors: list = field(default_factory=lambda: [(255, 40, 0)])
    gradient_type: int = 0
    gradient_angle: int = 90
    ripple: tuple[int, int, int] | None = None  # width, range, fade_out
    raw: bytes = b""

    @classmethod
    def parse(cls, t: bytes) -> "LightingConfig":
        eff = Effect.Off if t[0] > 100 else Effect(t[0]) if t[0] in Effect._value2member_map_ else t[0]
        cm = t[4]
        colors: list = []
        gtype, gangle = 0, 90
        if cm == ColorMode.SingleColor:
            colors = [tuple(t[5:8])]
        elif cm == ColorMode.DualColor:
            colors = [tuple(t[5:8]), tuple(t[8:11])]
        elif cm == ColorMode.Gradient:
            n = t[5]
            colors = [(tuple(t[6 + i * 4:9 + i * 4]), t[9 + i * 4]) for i in range(n)]
        elif cm == ColorMode.OrientedGradient:
            gtype = t[5]
            gangle = t[6] | (t[7] << 8)
            n = t[8]
            colors = [(tuple(t[9 + i * 4:12 + i * 4]), t[12 + i * 4]) for i in range(n)]
        ripple = tuple(t[-3:]) if eff == Effect.Ripple else None
        return cls(eff, t[1], t[2], t[3], ColorMode(cm) if cm in ColorMode._value2member_map_ else cm,
                   colors, gtype, gangle, ripple, bytes(t))

    def encode(self, zone: int) -> bytes:
        out = [zone, int(self.effect), int(self.direction), self.brightness, self.speed,
               int(self.color_mode)]
        if self.color_mode == ColorMode.SingleColor:
            out += list(self.colors[0])
        elif self.color_mode == ColorMode.DualColor:
            out += list(self.colors[0]) + list(self.colors[1])
        elif self.color_mode in (ColorMode.Gradient, ColorMode.OrientedGradient):
            if self.color_mode == ColorMode.OrientedGradient:
                out += [self.gradient_type, self.gradient_angle & 0xFF, self.gradient_angle >> 8]
            out.append(len(self.colors))
            for c, pos in self.colors:
                out += list(c) + [round(pos)]
        if self.effect == Effect.Ripple:
            out += list(self.ripple or (5, 7, 0))
        return bytes(out)

    def describe(self) -> str:
        eff = self.effect.name if isinstance(self.effect, Effect) else str(self.effect)
        try:
            d = Direction(self.direction).name
        except ValueError:
            d = str(self.direction)
        cm = self.color_mode.name if isinstance(self.color_mode, ColorMode) else str(self.color_mode)
        if self.color_mode in (ColorMode.Gradient, ColorMode.OrientedGradient):
            cols = ", ".join(f"{fmt_color(c)}@{p}" for c, p in self.colors)
        else:
            cols = ", ".join(fmt_color(c) for c in self.colors)
        return (f"effect={eff} brightness={self.brightness} speed={self.speed} "
                f"direction={d} colors[{cm}]={cols}")


@dataclass
class MediaDockConfig:
    menu_color: RGB
    clock_mode: int
    screensaver_mode: int
    screensaver_timeout: int
    screen_off_timeout: int

    @classmethod
    def parse(cls, t: bytes) -> "MediaDockConfig":
        return cls(tuple(t[0:3]), t[3], t[4],
                   t[5] | (t[6] << 8), t[7] | (t[8] << 8))

    def encode(self) -> bytes:
        r, g, b = self.menu_color
        return bytes([r, g, b, self.clock_mode, self.screensaver_mode,
                      self.screensaver_timeout & 0xFF, self.screensaver_timeout >> 8,
                      self.screen_off_timeout & 0xFF, self.screen_off_timeout >> 8])


def _bcdish(x: int) -> int:
    s = f"{x:x}"
    return int(s) if s.isdigit() else x


class DarkMount:
    def __init__(self, debug: bool = False, client_type: ClientType = ClientType.Web):
        node = find_control_node()
        if node.pid == PID_DARK_MOUNT_BOOTLOADER:
            raise QLinkError("Keyboard is in bootloader (firmware update) mode. "
                             "Run `darkmount firmware install` to finish the update.")
        self.dev = QLinkDevice(node, client_type=client_type, debug=debug)

    def __enter__(self):
        self.dev.open()
        self.dev.open_session()
        return self

    def __exit__(self, *exc):
        self.dev.close()

    def _req(self, feature, cmd, payload=b""):
        try:
            return self.dev.request(feature, cmd, bytes(payload)).data
        except QLinkError as e:
            # Firmware >= 1.31 only accepts writes from the active session: ask for it
            # (the darkmount service hands it over) and retry once.
            if "NOT_ACTIVE" not in str(e) or not self.dev.ensure_active():
                raise
            return self.dev.request(feature, cmd, bytes(payload)).data

    # -- info --------------------------------------------------------------
    def qlink_version(self) -> str:
        d = self._req(Feature.ROOT, RootCmd.GetQLinkVersion)
        return f"{d[3]}.{d[2]}.{(d[1] << 8) + d[0]}"

    def supported_features(self) -> list[tuple[str, int]]:
        d = self._req(Feature.ROOT, RootCmd.GetSupportedFeatures, [0])
        out = []
        for i in range(d[0]):
            fid, ver = d[1 + i * 2], d[2 + i * 2]
            try:
                name = Feature(fid).name
            except ValueError:
                name = f"0x{fid:02x}"
            out.append((name, ver))
        return out

    def serial_number(self) -> str:
        d = self._req(Feature.DEVICE_INFO, DeviceInfoCmd.GetSerialNumber)
        return d[1:1 + d[0]].decode("ascii", "replace")

    def device_info(self) -> dict:
        t = self._req(Feature.DEVICE_INFO, DeviceInfoCmd.GetDeviceInfo)
        mcus = []
        names = ["Main", "MediaDock", "Numpad"]
        for i in range(t[3]):
            r = 4 + i * 4
            major = _bcdish(t[r + 3])
            middle = int(f"{_bcdish(t[r + 1])}{_bcdish(t[r + 2])}")
            minor = _bcdish(t[r])
            mcus.append((names[i] if i < len(names) else f"MCU{i}", f"{major}.{middle}.{minor}"))
        return {"model_id": t[0] | (t[1] << 8), "revision": t[2], "firmware": mcus}

    # -- keyboard ------------------------------------------------------------
    def layout(self) -> tuple[str, str]:
        d = self._req(Feature.KEYBOARD, KeyboardCmd.GetLayout)
        phys = ["ANSI", "ISO", "JIS"]
        vis = ["US", "DE", "UK", "FR", "NO"]
        return (phys[d[0]] if d[0] < 3 else str(d[0]), vis[d[1]] if d[1] < 5 else str(d[1]))

    def game_mode(self) -> bool:
        return bool(self._req(Feature.KEYBOARD, KeyboardCmd.GetState)[0] & 1)

    def set_game_mode(self, on: bool):
        self._req(Feature.KEYBOARD, KeyboardCmd.SetState, [1 if on else 0])

    def game_mode_flags(self) -> GameModeFlags:
        return GameModeFlags(self._req(Feature.KEYBOARD, KeyboardCmd.GetConfig)[0] & 0x1F)

    def set_game_mode_flags(self, flags: GameModeFlags):
        self._req(Feature.KEYBOARD, KeyboardCmd.SetConfig, [int(flags)])

    def polling_rate(self) -> int:
        d = self._req(Feature.USB_DEVICE, UsbCmd.GetPollingRate)
        return d[0] | (d[1] << 8)

    # -- lighting --------------------------------------------------------------
    def lighting_mode(self) -> LightingMode:
        return LightingMode(self._req(Feature.LIGHTINGS, LightingCmd.GetLightingMode)[0])

    def set_lighting_mode(self, mode: LightingMode):
        self._req(Feature.LIGHTINGS, LightingCmd.SetLightingMode, [int(mode)])

    def lighting(self, zone: int = 0) -> LightingConfig:
        return LightingConfig.parse(self._req(Feature.LIGHTINGS, LightingCmd.GetLayerConfig, [zone]))

    def set_lighting(self, cfg: LightingConfig, zone: int = 0):
        if self.lighting_mode() != LightingMode.General:
            self.set_lighting_mode(LightingMode.General)
        self._req(Feature.LIGHTINGS, LightingCmd.SetLayerConfig, cfg.encode(zone))
        time.sleep(0.1)  # the web app waits 100 ms after each layer write

    # -- key bindings --------------------------------------------------------------
    def bindings_enabled(self) -> bool:
        return bool(self._req(Feature.BINDINGS, BindingsCmd.GetConfig)[0])

    _last_binding_write = 0.0
    BINDING_SETTLE_S = 1.2  # list reads right after a write return stale/empty data

    def bindings(self) -> list:
        from .bindings import parse_bindings
        wait = self._last_binding_write + self.BINDING_SETTLE_S - time.monotonic()
        if wait > 0:
            time.sleep(wait)
        d = self.dev.request(Feature.BINDINGS, BindingsCmd.GetBindings, b"\x00\x00",
                             timeout=3).data
        return parse_bindings(d)

    def set_binding(self, record: bytes):
        """Write a binding record built by one of the bindings.encode_* helpers."""
        if len(record) == 2:
            self._req(Feature.BINDINGS, BindingsCmd.ClearBinding, record)
        else:
            self._req(Feature.BINDINGS, BindingsCmd.SetBinding, record)
        self._last_binding_write = time.monotonic()

    def clear_binding(self, key: int, layer: int = 0):
        from .bindings import encode_clear
        self.set_binding(encode_clear(key, layer))

    # -- modules -----------------------------------------------------------------
    def numpad_state(self) -> tuple[bool, str]:
        d = self._req(Feature.NUMPAD_MODULE, ModuleCmd.GetState)
        return bool(d[0]), ["None", "Left", "Right"][d[1]] if d[1] < 3 else str(d[1])

    def media_dock_state(self) -> tuple[bool, str]:
        d = self._req(Feature.MEDIA_DOCK_MODULE, MediaDockCmd.GetState)
        return bool(d[0]), ["None", "Left", "Right"][d[1]] if d[1] < 3 else str(d[1])

    def media_dock_config(self) -> MediaDockConfig:
        return MediaDockConfig.parse(self._req(Feature.MEDIA_DOCK_MODULE, MediaDockCmd.GetConfig))

    def set_media_dock_config(self, cfg: MediaDockConfig):
        self._req(Feature.MEDIA_DOCK_MODULE, MediaDockCmd.SetConfig, cfg.encode())

    def media_dock_time(self) -> int:
        d = self._req(Feature.MEDIA_DOCK_MODULE, MediaDockCmd.GetDateTime)
        return struct.unpack_from("<I", d.ljust(4, b"\0"))[0]

    def sync_media_dock_time(self) -> int:
        """Set the dock clock to local wall-clock time (as the official app does)."""
        now = time.time()
        local = int(now + time.localtime(now).tm_gmtoff)
        self._req(Feature.MEDIA_DOCK_MODULE, MediaDockCmd.SetDateTime, struct.pack("<I", local))
        return local
