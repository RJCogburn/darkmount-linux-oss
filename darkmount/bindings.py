"""Key bindings for the Dark Mount (BINDINGS feature, 0x11)."""
from __future__ import annotations

from dataclasses import dataclass, field
from enum import IntEnum, IntFlag

# Physical key ids used by the keyboard (from be quiet!'s Dark Mount key map).
KEY_IDS: dict[str, int] = {
    "Grave": 1, "1": 2, "2": 3, "3": 4, "4": 5, "5": 6, "6": 7, "7": 8, "8": 9, "9": 10,
    "0": 11, "Minus": 12, "Equals": 13, "Backspace": 14, "Tab": 15, "Q": 16, "W": 17,
    "E": 18, "R": 19, "T": 20, "Y": 21, "U": 22, "I": 23, "O": 24, "P": 25,
    "BracketLeft": 26, "BracketRight": 27, "Backslash": 28, "CapsLock": 29, "A": 30,
    "S": 31, "D": 32, "F": 33, "G": 34, "H": 35, "J": 36, "K": 37, "L": 38,
    "Semicolon": 39, "Apostrophe": 40, "Enter": 41, "LeftShift": 42, "Z": 43, "X": 44,
    "C": 45, "V": 46, "B": 47, "N": 48, "M": 49, "Comma": 50, "Period": 51, "Slash": 52,
    "RightShift": 53, "LeftCtrl": 54, "Fn": 55, "LeftAlt": 56, "Space": 57, "RightAlt": 58,
    "RightCtrl": 59, "Insert": 60, "Delete": 61, "Left": 62, "Home": 63, "End": 64,
    "Up": 65, "Down": 66, "PageUp": 67, "PageDown": 68, "Right": 69, "NumLock": 70,
    "Num7": 71, "Num4": 72, "Num1": 73, "NumSlash": 74, "Num8": 75, "Num5": 76, "Num2": 77,
    "Num0": 78, "NumAsterisk": 79, "Num9": 80, "Num6": 81, "Num3": 82, "NumPeriod": 83,
    "NumMinus": 84, "NumPlus": 85, "NumEnter": 86, "Escape": 87, "F1": 88, "F2": 89,
    "F3": 90, "F4": 91, "F5": 92, "F6": 93, "F7": 94, "F8": 95, "F9": 96, "F10": 97,
    "F11": 98, "F12": 99, "PrintScreen": 100, "ScrollLock": 101, "Pause": 102,
    "LeftGui": 103, "RightGui": 104, "NonUsBackslash": 105,
    "Image0": 109, "Image1": 110, "Image2": 111, "Image3": 112, "Image4": 113,
    "Image5": 114, "Image6": 115, "Image7": 116,
    "DockMute": 117, "DockPlayPause": 118, "DockPrev": 119, "DockNext": 120,
    "DockLeft": 121, "DockRight": 128,
}
KEY_NAMES = {v: k for k, v in KEY_IDS.items()}

# USB HID keyboard usage ids, for "send this key" bindings.
HID_USAGE: dict[str, int] = {
    **{chr(ord("A") + i): 0x04 + i for i in range(26)},
    **{str((i + 1) % 10): 0x1E + i for i in range(10)},
    "Enter": 0x28, "Escape": 0x29, "Backspace": 0x2A, "Tab": 0x2B, "Space": 0x2C,
    "Minus": 0x2D, "Equals": 0x2E, "BracketLeft": 0x2F, "BracketRight": 0x30,
    "Backslash": 0x31, "Semicolon": 0x33, "Apostrophe": 0x34, "Grave": 0x35,
    "Comma": 0x36, "Period": 0x37, "Slash": 0x38, "CapsLock": 0x39,
    **{f"F{i}": 0x3A + i - 1 for i in range(1, 13)},
    "PrintScreen": 0x46, "ScrollLock": 0x47, "Pause": 0x48, "Insert": 0x49, "Home": 0x4A,
    "PageUp": 0x4B, "Delete": 0x4C, "End": 0x4D, "PageDown": 0x4E, "Right": 0x4F,
    "Left": 0x50, "Down": 0x51, "Up": 0x52, "NumLock": 0x53, "NumSlash": 0x54,
    "NumAsterisk": 0x55, "NumMinus": 0x56, "NumPlus": 0x57, "NumEnter": 0x58,
    **{f"Num{i}": 0x59 + i - 1 for i in range(1, 10)}, "Num0": 0x62, "NumPeriod": 0x63,
    "NonUsBackslash": 0x64, "Menu": 0x65,
    **{f"F{i}": 0x68 + i - 13 for i in range(13, 25)},
    "LeftCtrl": 0xE0, "LeftShift": 0xE1, "LeftAlt": 0xE2, "LeftGui": 0xE3,
    "RightCtrl": 0xE4, "RightShift": 0xE5, "RightAlt": 0xE6, "RightGui": 0xE7,
}
HID_NAMES = {v: k for k, v in HID_USAGE.items()}


class Layer(IntEnum):
    Common = 0
    Fn = 1


class BindType(IntEnum):
    Default = 0
    StandardKey = 1
    Media = 2
    Mouse = 3
    OpenFolder = 4
    OpenFile = 5
    OpenBrowser = 6
    WindowsShortcut = 7
    Profile = 8
    Backlight = 9
    Macro = 10
    AltCode = 11
    Disable = 12


class Modifier(IntFlag):
    NONE = 0
    LCtrl = 1
    LShift = 2
    LAlt = 4
    LGui = 8
    RCtrl = 16
    RShift = 32
    RAlt = 64
    RGui = 128


MOD_ALIASES = {"ctrl": Modifier.LCtrl, "shift": Modifier.LShift, "alt": Modifier.LAlt,
               "super": Modifier.LGui, "win": Modifier.LGui, "meta": Modifier.LGui,
               "rctrl": Modifier.RCtrl, "rshift": Modifier.RShift, "ralt": Modifier.RAlt,
               "altgr": Modifier.RAlt, "rsuper": Modifier.RGui}


class Media(IntEnum):
    VolumeUp = 1
    VolumeDown = 2
    Mute = 3
    MicMute = 4
    NextTrack = 5
    PrevTrack = 6
    PlayPause = 7
    Stop = 8


class MouseButton(IntEnum):
    Left = 1
    Right = 2
    Middle = 3
    Forward = 4
    Back = 5


class MouseScroll(IntEnum):
    ScrollUp = 1
    ScrollDown = 2
    ScrollLeft = 3
    ScrollRight = 4


class HostAction(IntEnum):
    """'WindowsShortcut' actions: executed by the host app, not the keyboard."""
    FileExplorer = 1
    Calculator = 2
    TaskManager = 3
    LockPC = 4
    ShutDownPC = 5
    SleepPC = 6
    HibernatePC = 7
    SnippingTool = 8
    Notepad = 9
    Paint = 10
    AirplaneMode = 11
    XboxGameBar = 12
    SystemSettings = 13
    Refresh = 14
    TabbingApp = 15
    CloseApp = 16
    Copy = 17
    Paste = 18
    Cut = 19
    InternetBrowser = 20
    EmailReader = 21


class BacklightAction(IntEnum):
    SelectEffect = 1
    NextEffect = 2
    PrevEffect = 3
    BrightnessUp = 4
    BrightnessDown = 5


# Types whose action is performed by host software when the key is pressed.
HOST_TYPES = {BindType.OpenFolder, BindType.OpenFile, BindType.OpenBrowser,
              BindType.WindowsShortcut, BindType.Profile}


@dataclass
class Binding:
    key: int
    layer: int
    type: BindType
    data: bytes = b""
    # decoded convenience fields
    modifiers: int = 0
    usage: int = 0
    action: int = 0
    text: str = ""
    extra: dict = field(default_factory=dict)

    @property
    def key_name(self) -> str:
        return KEY_NAMES.get(self.key, f"key{self.key}")

    def describe(self) -> str:
        t = self.type
        if t == BindType.StandardKey:
            mods = [m.name for m in Modifier if m and self.modifiers & m]
            key = HID_NAMES.get(self.usage, f"0x{self.usage:02x}")
            return "key " + "+".join(mods + [key])
        if t == BindType.Media:
            return "media " + _name(Media, self.action)
        if t == BindType.Mouse:
            if self.extra.get("scroll"):
                return "mouse " + _name(MouseScroll, self.action)
            return "mouse " + _name(MouseButton, self.action) + \
                (f" ({self.extra['mode']})" if self.extra.get("mode") else "")
        if t == BindType.WindowsShortcut:
            return "system shortcut: " + _name(HostAction, self.action)
        if t in (BindType.OpenFile, BindType.OpenFolder, BindType.OpenBrowser):
            return f"{t.name} {self.text!r}"
        if t == BindType.Backlight:
            s = "backlight " + _name(BacklightAction, self.action)
            if self.action == BacklightAction.SelectEffect:
                s += f" {self.extra.get('effect')}"
            return s
        if t == BindType.Macro:
            return f"macro {self.extra.get('macro_id')}"
        if t == BindType.AltCode:
            return f"alt-code {self.extra.get('code')}"
        if t in (BindType.Disable, BindType.Default):
            return "disabled"
        return f"{t.name} {self.data.hex(' ')}"


def _name(enum, v):
    try:
        return enum(v).name
    except ValueError:
        return str(v)


def parse_bindings(t: bytes) -> list[Binding]:
    """Parse a GetBindings response (mirrors the official app's parser)."""
    total = t[0] + t[1]
    i = t[2:]
    out: list[Binding] = []
    a = 0
    for _ in range(total):
        if a + 3 > len(i):
            break
        key = i[a]
        layer = (i[a + 1] >> 7) & 1
        typ = i[a + 2]
        if typ == BindType.Default:
            typ = BindType.Disable
        try:
            typ = BindType(typ)
        except ValueError:
            break  # unknown record type: can't know its size, stop parsing
        p = a + 3
        b = Binding(key, layer, typ)
        if typ == BindType.StandardKey:
            b.modifiers, b.usage = i[p], i[p + 1]
            size = 5
        elif typ == BindType.AltCode:
            b.extra["code"] = i[p + 1] | (i[p + 2] << 8)
            size = 6
        elif typ == BindType.Media:
            b.action = i[p]
            if b.action == 10:  # specific sound device: length-prefixed id
                n = i[p + 1]
                b.text = bytes(i[p + 2:p + 2 + n]).decode("latin-1")
                size = 5 + n
            else:
                size = 4
        elif typ == BindType.Mouse:
            if i[p] == 2:
                b.extra["scroll"] = True
                b.action = i[p + 1] + 1
                size = 5
            else:
                v = i[p + 1]
                b.action = (v & 31) + 1
                b.extra["mode"] = ("double-click" if v >> 7 & 1 else
                                   "while-pressed" if v >> 6 & 1 else
                                   f"auto-fire {i[p + 2]}" if i[p + 2] else "")
                size = 6
        elif typ in (BindType.OpenFile, BindType.OpenFolder, BindType.OpenBrowser):
            n = i[p]
            b.text = bytes(i[p + 1:p + 1 + n]).decode("latin-1")
            size = 4 + n
        elif typ == BindType.WindowsShortcut:
            b.action = i[p]
            size = 4
        elif typ == BindType.Profile:
            b.action = i[p]
            size = 20 if b.action == 1 else 4
        elif typ == BindType.Backlight:
            b.action = i[p]
            if b.action == BacklightAction.SelectEffect:
                b.extra["effect"] = i[p + 1]
                size = 5
            else:
                size = 4
        elif typ == BindType.Macro:
            b.action = i[p]
            b.extra["macro_id"] = i[p + 1]
            size = 5
        else:  # Disable
            size = 3
        b.data = bytes(i[a:a + size])
        out.append(b)
        a += size
        if a >= len(i):
            break
    return out


def _keyref(key: int, layer: int) -> list[int]:
    # The keyboard expects the Fn layer as bit 7 of the second byte (verified on
    # firmware 1.19.0: 0x01 is rejected by GetBinding; 0x80 round-trips).
    return [key & 0xFF, 0x80 if layer else 0x00]


def encode_standard_key(key: int, layer: int, usage: int, modifiers: int = 0) -> bytes:
    return bytes(_keyref(key, layer) + [BindType.StandardKey, modifiers & 0xFF, usage])


def encode_media(key: int, layer: int, action: Media) -> bytes:
    return bytes(_keyref(key, layer) + [BindType.Media, int(action)])


def encode_mouse_button(key: int, layer: int, button: MouseButton) -> bytes:
    return bytes(_keyref(key, layer) + [BindType.Mouse, 1, (int(button) - 1) & 31, 0])


def encode_mouse_scroll(key: int, layer: int, direction: MouseScroll) -> bytes:
    return bytes(_keyref(key, layer) + [BindType.Mouse, 2, int(direction) - 1])


def encode_host_action(key: int, layer: int, action: HostAction) -> bytes:
    return bytes(_keyref(key, layer) + [BindType.WindowsShortcut, int(action)])


def encode_backlight(key: int, layer: int, action: BacklightAction, effect: int = 0) -> bytes:
    extra = [effect] if action == BacklightAction.SelectEffect else []
    return bytes(_keyref(key, layer) + [BindType.Backlight, int(action)] + extra)


def encode_open(key: int, layer: int, kind: BindType, target: str) -> bytes:
    """OpenFile / OpenFolder / OpenBrowser: performed by the host when pressed."""
    if kind not in (BindType.OpenFile, BindType.OpenFolder, BindType.OpenBrowser):
        raise ValueError(kind)
    raw = target.encode("latin-1")
    if len(raw) > 200:
        raise ValueError("target too long (max 200 characters)")
    return bytes(_keyref(key, layer) + [kind, len(raw)]) + raw


def encode_disable(key: int, layer: int) -> bytes:
    # The official app sends type 0 for "disable".
    return bytes(_keyref(key, layer) + [0])


def encode_clear(key: int, layer: int) -> bytes:
    return bytes(_keyref(key, layer))


def parse_key_name(name: str) -> int:
    for k, v in KEY_IDS.items():
        if k.lower() == name.lower():
            return v
    raise ValueError(f"unknown key {name!r}")


def parse_combo(combo: str) -> tuple[int, int]:
    """'ctrl+shift+Esc' -> (modifiers, usage)"""
    parts = [p for p in combo.replace(" ", "").split("+") if p]
    mods = 0
    usage = None
    for p in parts:
        if p.lower() in MOD_ALIASES:
            mods |= MOD_ALIASES[p.lower()]
            continue
        for k, v in HID_USAGE.items():
            if k.lower() == p.lower():
                usage = v
                break
        else:
            raise ValueError(f"unknown key {p!r}")
    if usage is None:
        raise ValueError("combo needs a non-modifier key")
    return mods, usage
