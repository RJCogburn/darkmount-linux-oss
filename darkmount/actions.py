"""Host-side handling of key actions the keyboard can't perform itself.

The keyboard performs "Windows shortcut" bindings (Explorer, Lock, Sleep, …) on its
own by sending normal HID shortcuts. Open-file / open-folder / open-URL bindings,
however, only send a QLink ActionFired notification to an IO Center ("QControl")
session; this module performs them on Linux.

Config: ~/.config/darkmount/config.toml

    [actions]
    # key name (see `darkmount keys names`), optionally prefixed with "Fn+"
    Image0 = "flatpak run com.example.App"
    "Fn+F12" = "konsole"

A configured command runs for that key whenever the keyboard reports it, and
replaces the default behaviour for open-file/folder/URL bindings.
"""
from __future__ import annotations

import os
import shutil
import subprocess
import sys
import tomllib
from pathlib import Path

from .bindings import KEY_NAMES, BindType

CONFIG_PATH = Path(os.environ.get("XDG_CONFIG_HOME", Path.home() / ".config")) / \
    "darkmount" / "config.toml"


def load_actions() -> dict[str, str]:
    try:
        with open(CONFIG_PATH, "rb") as f:
            cfg = tomllib.load(f)
    except FileNotFoundError:
        return {}
    except tomllib.TOMLDecodeError as e:
        log(f"config error in {CONFIG_PATH}: {e}")
        return {}
    return {k.lower(): str(v) for k, v in cfg.get("actions", {}).items()}


def log(msg: str):
    print(msg, file=sys.stderr, flush=True)


def notify(summary: str, body: str = ""):
    if shutil.which("notify-send"):
        subprocess.Popen(["notify-send", "-a", "Dark Mount", "-i", "input-keyboard",
                          summary, body], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def spawn(argv: list[str] | str, shell: bool = False):
    subprocess.Popen(argv, shell=shell, start_new_session=True, stdin=subprocess.DEVNULL,
                     stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)


def parse_action_fired(data: bytes) -> dict:
    """ActionFired payload: [key, layer, handled_by_device, type, action/payload…]."""
    d = {"key": data[0], "layer": (data[1] >> 7) & 1, "device_handled": bool(data[2]),
         "type": data[3], "text": ""}
    if d["type"] in (BindType.OpenFile, BindType.OpenFolder, BindType.OpenBrowser) \
            and len(data) > 4:
        n = data[4]
        d["text"] = bytes(data[5:5 + n]).decode("latin-1")
    return d


def key_label(key: int, layer: int) -> str:
    name = KEY_NAMES.get(key, f"key{key}")
    return f"Fn+{name}" if layer else name


def handle_action_fired(data: bytes, actions: dict[str, str]):
    ev = parse_action_fired(data)
    label = key_label(ev["key"], ev["layer"])
    cmd = actions.get(label.lower())
    if cmd:
        log(f"{label}: running {cmd!r}")
        spawn(cmd, shell=True)
        return
    if ev["device_handled"]:
        return  # keyboard already sent the shortcut itself
    t, target = ev["type"], ev["text"]
    if t == BindType.OpenBrowser and target:
        log(f"{label}: opening URL {target}")
        spawn(["xdg-open", target])
    elif t in (BindType.OpenFile, BindType.OpenFolder) and target:
        path = Path(os.path.expanduser(target))
        if path.exists():
            log(f"{label}: opening {path}")
            if t == BindType.OpenFile and os.access(path, os.X_OK) and path.is_file():
                spawn([str(path)])
            else:
                spawn(["xdg-open", str(path)])
        else:
            log(f"{label}: {target!r} does not exist on this system")
            notify(f"{label}: nothing to open",
                   f"Bound to “{target}”, which doesn't exist here.\n"
                   f"Set a Linux command for {label} in {CONFIG_PATH}")
    else:
        log(f"{label}: unhandled action {data.hex(' ')}")
