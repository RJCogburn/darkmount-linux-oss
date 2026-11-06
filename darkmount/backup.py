"""Save / restore on-keyboard settings (used around firmware updates, which reset them)."""
from __future__ import annotations

import json
import os
import time
from pathlib import Path

from .device import DarkMount, GameModeFlags, LightingConfig, LightingMode, MediaDockConfig

BACKUPS = Path(os.environ.get("XDG_DATA_HOME", Path.home() / ".local/share")) / \
    "darkmount" / "backups"


def snapshot(kb: DarkMount) -> dict:
    info = kb.device_info()
    snap = {
        "time": time.strftime("%Y-%m-%d %H:%M:%S"),
        "firmware": info["firmware"][0][1] if info["firmware"] else "?",
        "bindings": [b.data.hex() for b in kb.bindings()],
        "lighting_mode": int(kb.lighting_mode()),
        "lighting": kb.lighting().encode(0).hex(),
        "game_flags": int(kb.game_mode_flags()),
    }
    if kb.media_dock_state()[0]:
        snap["dock"] = kb.media_dock_config().encode().hex()
    return snap


def save(snap: dict) -> Path:
    BACKUPS.mkdir(parents=True, exist_ok=True)
    p = BACKUPS / f"{time.strftime('%Y%m%d-%H%M%S')}-fw{snap.get('firmware', '?')}.json"
    p.write_text(json.dumps(snap, indent=1))
    return p


def latest() -> dict | None:
    files = sorted(BACKUPS.glob("*.json")) if BACKUPS.exists() else []
    return json.loads(files[-1].read_text()) if files else None


def restore(kb: DarkMount, snap: dict, report=print):
    from .device import BindingsCmd
    from .qlink import Feature
    n = 0
    for rec in snap.get("bindings", []):
        kb.set_binding(bytes.fromhex(rec))
        n += 1
    report(f"restored {n} key bindings")
    if "lighting" in snap:
        raw = bytes.fromhex(snap["lighting"])
        mode = LightingMode(snap.get("lighting_mode", 1))
        if mode == LightingMode.Off:
            kb.set_lighting_mode(LightingMode.Off)
        else:
            kb.set_lighting_mode(LightingMode.General)
            kb._req(Feature.LIGHTINGS, 6, raw)  # SetLayerConfig with the saved record
        report("restored lighting")
    if "game_flags" in snap:
        kb.set_game_mode_flags(GameModeFlags(snap["game_flags"]))
        report("restored game mode keys")
    if "dock" in snap and kb.media_dock_state()[0]:
        kb.set_media_dock_config(MediaDockConfig.parse(bytes.fromhex(snap["dock"])))
        report("restored media dock settings")
