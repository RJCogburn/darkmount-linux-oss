"""darkmount: command-line control for the be quiet! Dark Mount keyboard."""
from __future__ import annotations

import argparse
import datetime
import sys
import time

from .device import (ClockMode, ColorMode, DarkMount, Direction, Effect, GameModeFlags,
                     LightingMode, ScreensaverMode, fmt_color, parse_color)
from .qlink import QLinkError, find_control_node, list_nodes

FLAG_NAMES = {
    "shift-tab": GameModeFlags.SHIFT_TAB,
    "alt-f4": GameModeFlags.ALT_F4,
    "win": GameModeFlags.WIN,
    "alt-tab": GameModeFlags.ALT_TAB,
    "caps-lock": GameModeFlags.CAPS_LOCK,
}


def _enum_arg(enum):
    names = {e.name.lower(): e for e in enum}

    def conv(s):
        try:
            return names[s.lower()]
        except KeyError:
            raise argparse.ArgumentTypeError(f"choose from: {', '.join(names)}")
    conv.__name__ = enum.__name__
    return conv


def _pct(s):
    v = int(s)
    if not 0 <= v <= 100:
        raise argparse.ArgumentTypeError("must be 0-100")
    return v


def _flags_str(flags: GameModeFlags) -> str:
    on = [n for n, f in FLAG_NAMES.items() if flags & f]
    return ", ".join(on) if on else "(none)"


def cmd_list(args):
    nodes = list_nodes()
    if not nodes:
        print("No be quiet! devices found.")
        return 1
    for n in nodes:
        page = f"0x{n.usage_page:04x}" if n.usage_page is not None else "?"
        tag = "  <- control interface" if n.usage_page == 0xFF00 else ""
        print(f"{n.path}  {n.vid:04x}:{n.pid:04x}  usage_page={page}  {n.name}{tag}")
    return 0


def cmd_info(kb: DarkMount, args):
    info = kb.device_info()
    print(f"Serial:        {kb.serial_number()}")
    print(f"Model id:      0x{info['model_id']:04x} rev {info['revision']}")
    for name, ver in info["firmware"]:
        print(f"Firmware:      {name:10s} {ver}")
    print(f"QLink:         {kb.qlink_version()}")
    phys, vis = kb.layout()
    print(f"Layout:        {phys} / {vis}")
    print(f"Polling rate:  {kb.polling_rate()} Hz")
    print(f"Game mode:     {'ON' if kb.game_mode() else 'off'}  (blocks: {_flags_str(kb.game_mode_flags())})")
    print(f"Lighting:      {kb.lighting_mode().name}: {kb.lighting().describe()}")
    for label, fn in (("Numpad", kb.numpad_state), ("Media dock", kb.media_dock_state)):
        try:
            conn, pos = fn()
            print(f"{label + ':':15s}{'connected (' + pos + ')' if conn else 'not connected'}")
        except QLinkError as e:
            print(f"{label + ':':15s}? ({e})")
    if args.features:
        print("Features:      " + ", ".join(f"{n} v{v}" for n, v in kb.supported_features()))
    return 0


def cmd_light(kb: DarkMount, args):
    if args.action == "show":
        print(f"mode={kb.lighting_mode().name}  {kb.lighting(args.zone).describe()}")
        if args.raw:
            print("raw:", kb.lighting(args.zone).raw.hex(" "))
        return 0
    if args.action == "off":
        kb.set_lighting_mode(LightingMode.Off)
        return 0
    if args.action == "on":
        kb.set_lighting_mode(LightingMode.General)
        return 0

    # set: start from current config and override what was given
    cfg = kb.lighting(args.zone)
    if args.effect is not None:
        cfg.effect = args.effect
    if args.brightness is not None:
        cfg.brightness = args.brightness
    if args.speed is not None:
        cfg.speed = args.speed
    if args.direction is not None:
        cfg.direction = args.direction
    if args.gradient:
        cols = [parse_color(c) for c in args.gradient.split(",")]
        if len(cols) < 2:
            raise SystemExit("--gradient needs at least two colors")
        step = 100 / (len(cols) - 1)
        cfg.color_mode = ColorMode.Gradient
        cfg.colors = [(c, round(i * step)) for i, c in enumerate(cols)]
    elif args.color and args.color2:
        cfg.color_mode = ColorMode.DualColor
        cfg.colors = [parse_color(args.color), parse_color(args.color2)]
    elif args.color:
        cfg.color_mode = ColorMode.SingleColor
        cfg.colors = [parse_color(args.color)]
    kb.set_lighting(cfg, args.zone)
    print("set:", kb.lighting(args.zone).describe())
    return 0


def cmd_gamemode(kb: DarkMount, args):
    if args.action in ("on", "off"):
        kb.set_game_mode(args.action == "on")
    elif args.action == "block":
        flags = GameModeFlags.NONE
        for name in filter(None, (args.keys or "").split(",")):
            try:
                flags |= FLAG_NAMES[name.strip().lower()]
            except KeyError:
                raise SystemExit(f"unknown key {name!r}; choose from {', '.join(FLAG_NAMES)} "
                                 "(or pass nothing to clear)")
        kb.set_game_mode_flags(flags)
    print(f"Game mode: {'ON' if kb.game_mode() else 'off'}  (blocks: {_flags_str(kb.game_mode_flags())})")
    return 0


def cmd_dock(kb: DarkMount, args):
    conn, pos = kb.media_dock_state()
    if not conn:
        print("Media dock is not connected.")
        return 1
    if args.action == "sync-time":
        t = kb.sync_media_dock_time()
        print("Dock clock set to", datetime.datetime.fromtimestamp(t, datetime.timezone.utc)
              .strftime("%Y-%m-%d %H:%M:%S"))
        return 0
    cfg = kb.media_dock_config()
    if args.action == "set":
        if args.menu_color:
            cfg.menu_color = parse_color(args.menu_color)
        if args.clock:
            cfg.clock_mode = ClockMode.Clock24h if args.clock == "24" else ClockMode.Clock12h
        if args.screensaver:
            cfg.screensaver_mode = ScreensaverMode[args.screensaver.capitalize()]
        if args.screensaver_timeout is not None:
            cfg.screensaver_timeout = args.screensaver_timeout
        if args.screen_off is not None:
            cfg.screen_off_timeout = args.screen_off
        kb.set_media_dock_config(cfg)
        cfg = kb.media_dock_config()
    t = kb.media_dock_time()
    try:
        ss = ScreensaverMode(cfg.screensaver_mode).name
    except ValueError:
        ss = str(cfg.screensaver_mode)
    print(f"Position:            {pos}")
    print(f"Clock:               {datetime.datetime.fromtimestamp(t, datetime.timezone.utc):%Y-%m-%d %H:%M:%S}"
          f" ({'24h' if cfg.clock_mode == ClockMode.Clock24h else '12h'})")
    print(f"Menu color:          {fmt_color(cfg.menu_color)}")
    print(f"Screensaver:         {ss} after {cfg.screensaver_timeout}s")
    print(f"Screen off after:    {cfg.screen_off_timeout}s" + (" (never)" if not cfg.screen_off_timeout else ""))
    return 0


def cmd_key_names(args):
    from .bindings import HID_USAGE, KEY_IDS, BacklightAction, HostAction, Media, MouseButton, \
        MouseScroll
    print("Physical keys (KEY):\n  " + " ".join(KEY_IDS))
    print("\nKeys to send (--to, combine with ctrl/shift/alt/super/rctrl/…):\n  " +
          " ".join(HID_USAGE))
    for title, enum in (("--media", Media), ("--mouse", MouseButton), ("--scroll", MouseScroll),
                        ("--backlight", BacklightAction), ("--host", HostAction)):
        print(f"\n{title}:\n  " + " ".join(e.name for e in enum))
    return 0


def cmd_keys(kb: DarkMount, args):
    from . import bindings as B

    if args.action == "list":
        bs = sorted(kb.bindings(), key=lambda b: (b.layer, b.key))
        if not kb.bindings_enabled():
            print("(custom bindings are DISABLED on the keyboard)")
        if not bs:
            print("No custom bindings; all keys use their defaults.")
        for b in bs:
            print(f"{('Fn+' if b.layer else '') + b.key_name:16s} {b.describe()}")
        return 0

    if not args.key:
        raise SystemExit("which key? (see `darkmount keys names`)")
    name = args.key
    layer = B.Layer.Common
    if name.lower().startswith("fn+"):
        layer, name = B.Layer.Fn, name[3:]
    try:
        key = B.parse_key_name(name)
    except ValueError as e:
        raise SystemExit(str(e))
    label = ("Fn+" if layer else "") + B.KEY_NAMES[key]

    def enum_val(enum, s):
        for e in enum:
            if e.name.lower() == s.lower():
                return e
        raise SystemExit(f"unknown value {s!r}; choose from: {', '.join(e.name for e in enum)}")

    if args.action == "reset":
        kb.clear_binding(key, layer)
        print(f"{label}: reset to default")
        return 0

    # set
    if args.to:
        try:
            mods, usage = B.parse_combo(args.to)
        except ValueError as e:
            raise SystemExit(str(e))
        rec = B.encode_standard_key(key, layer, usage, mods)
    elif args.media:
        rec = B.encode_media(key, layer, enum_val(B.Media, args.media))
    elif args.mouse:
        rec = B.encode_mouse_button(key, layer, enum_val(B.MouseButton, args.mouse))
    elif args.scroll:
        rec = B.encode_mouse_scroll(key, layer, enum_val(B.MouseScroll, args.scroll))
    elif args.backlight:
        rec = B.encode_backlight(key, layer, enum_val(B.BacklightAction, args.backlight))
    elif args.host:
        rec = B.encode_host_action(key, layer, enum_val(B.HostAction, args.host))
    elif args.open_file:
        rec = B.encode_open(key, layer, B.BindType.OpenFile, args.open_file)
    elif args.open_folder:
        rec = B.encode_open(key, layer, B.BindType.OpenFolder, args.open_folder)
    elif args.open_url:
        rec = B.encode_open(key, layer, B.BindType.OpenBrowser, args.open_url)
    elif args.disable:
        rec = B.encode_disable(key, layer)
    else:
        raise SystemExit("nothing to set: use --to, --media, --mouse, --scroll, --backlight, "
                         "--host, --open-file, --open-folder, --open-url or --disable")
    kb.set_binding(rec)
    now = [b for b in kb.bindings() if b.key == key and b.layer == layer]
    print(f"{label}: {now[0].describe() if now else '(not confirmed)'}")
    return 0


def cmd_daemon(args):
    """Background service.

    * keeps the media dock clock in sync (on connect, after resume, every --interval)
    * performs open-file/folder/URL key actions (and [actions] from the config file)

    Connects as the IO Center client type, which is what makes the keyboard report
    key actions. Other tools (CLI, GUI) can connect at the same time.
    """
    from .actions import CONFIG_PATH, handle_action_fired, load_actions
    from .device import BindingsNotify
    from .qlink import ClientType, Feature

    def stamp(t):
        return datetime.datetime.fromtimestamp(t, datetime.timezone.utc).strftime("%Y-%m-%d %H:%M:%S")

    print(f"darkmount daemon: dock clock sync every {args.interval}s; key actions from "
          f"{CONFIG_PATH}", flush=True)
    while True:
        try:
            find_control_node()
        except QLinkError:
            time.sleep(3)
            continue
        time.sleep(1)  # let the keyboard finish enumerating
        try:
            with DarkMount(client_type=ClientType.QControl, debug=args.debug) as kb:
                print("keyboard connected", flush=True)
                actions = load_actions()
                cfg_mtime = CONFIG_PATH.stat().st_mtime if CONFIG_PATH.exists() else 0
                last_sync = 0.0
                last_tick = time.time()
                while True:
                    now = time.time()
                    jumped = abs(now - last_tick) > 30  # suspend/resume or clock change
                    last_tick = now
                    if jumped or now - last_sync >= args.interval:
                        if kb.media_dock_state()[0]:
                            t = kb.sync_media_dock_time()
                            if not last_sync or jumped:
                                print("synced dock clock to", stamp(t), flush=True)
                        last_sync = now
                    m = CONFIG_PATH.stat().st_mtime if CONFIG_PATH.exists() else 0
                    if m != cfg_mtime:
                        actions, cfg_mtime = load_actions(), m
                        print("reloaded config", flush=True)
                    f = kb.dev.next_notification(1.0)
                    if f and f.feature == Feature.BINDINGS and \
                            f.command == BindingsNotify.ActionFired:
                        handle_action_fired(f.data, actions)
        except (QLinkError, OSError) as e:
            print("keyboard disconnected:", e, file=sys.stderr, flush=True)
            time.sleep(2)


def build_parser():
    p = argparse.ArgumentParser(prog="darkmount", description=__doc__)
    p.add_argument("--debug", action="store_true", help="print raw HID frames")
    sub = p.add_subparsers(dest="cmd", required=True)

    sub.add_parser("list", help="list be quiet! hidraw nodes (no session)")

    s = sub.add_parser("info", help="show device info and current settings")
    s.add_argument("--features", action="store_true", help="also list QLink features")

    s = sub.add_parser("light", help="lighting control")
    s.add_argument("action", choices=["show", "set", "on", "off"])
    s.add_argument("--zone", type=int, default=0, help=argparse.SUPPRESS)
    s.add_argument("--effect", type=_enum_arg(Effect),
                   help="static, colorwave, tornado, breathing, reactive, matrix, ripple")
    s.add_argument("--color", help="RRGGBB")
    s.add_argument("--color2", help="second color (dual-color effects)")
    s.add_argument("--gradient", help="comma-separated colors, e.g. ff0000,00ff00,0000ff")
    s.add_argument("--brightness", type=_pct)
    s.add_argument("--speed", type=_pct)
    s.add_argument("--direction", type=_enum_arg(Direction),
                   help="up, down, left, right, clockwise, counterclockwise, ...")
    s.add_argument("--raw", action="store_true", help="(show) dump raw layer bytes")

    s = sub.add_parser("gamemode", help="game mode")
    s.add_argument("action", choices=["status", "on", "off", "block"])
    s.add_argument("keys", nargs="?",
                   help="(block) comma list: shift-tab,alt-f4,win,alt-tab,caps-lock; empty clears")

    s = sub.add_parser("dock", help="media dock")
    s.add_argument("action", choices=["show", "set", "sync-time"])
    s.add_argument("--menu-color", help="RRGGBB")
    s.add_argument("--clock", choices=["12", "24"])
    s.add_argument("--screensaver", choices=["disabled", "clock", "image"])
    s.add_argument("--screensaver-timeout", type=int, help="seconds")
    s.add_argument("--screen-off", type=int, help="seconds, 0 = never")

    s = sub.add_parser("keys", help="key bindings / remapping",
                       description="Remap keys. KEY may be prefixed with Fn+ for the Fn layer, "
                                   "e.g. Fn+F1, Image0, CapsLock. See `darkmount keys names`.")
    s.add_argument("action", choices=["list", "set", "reset", "names"])
    s.add_argument("key", nargs="?", help="physical key, e.g. CapsLock, Fn+F12, Image0")
    g = s.add_mutually_exclusive_group()
    g.add_argument("--to", metavar="COMBO", help="send a key/combo, e.g. Escape or ctrl+shift+T")
    g.add_argument("--media", help="VolumeUp, VolumeDown, Mute, MicMute, PlayPause, NextTrack, …")
    g.add_argument("--mouse", help="Left, Right, Middle, Forward, Back")
    g.add_argument("--scroll", help="ScrollUp, ScrollDown, ScrollLeft, ScrollRight")
    g.add_argument("--backlight", help="NextEffect, PrevEffect, BrightnessUp, BrightnessDown")
    g.add_argument("--host", help="keyboard-performed shortcut: LockPC, SleepPC, Calculator, …")
    g.add_argument("--open-file", metavar="PATH", help="opened by `darkmount daemon`")
    g.add_argument("--open-folder", metavar="PATH", help="opened by `darkmount daemon`")
    g.add_argument("--open-url", metavar="URL", help="opened by `darkmount daemon`")
    g.add_argument("--disable", action="store_true", help="make the key do nothing")

    s = sub.add_parser("daemon", help="background service: dock clock + key actions")
    s.add_argument("--interval", type=int, default=300, help="seconds (default 300)")
    return p


def main(argv=None):
    args = build_parser().parse_args(argv)
    if args.cmd == "list":
        return cmd_list(args)
    if args.cmd == "keys" and args.action == "names":
        return cmd_key_names(args)
    try:
        if args.cmd == "daemon":
            return cmd_daemon(args)
        with DarkMount(debug=args.debug) as kb:
            return {"info": cmd_info, "light": cmd_light, "gamemode": cmd_gamemode,
                    "dock": cmd_dock, "keys": cmd_keys}[args.cmd](kb, args)
    except QLinkError as e:
        print("error:", e, file=sys.stderr)
        return 2
    except KeyboardInterrupt:
        return 130


if __name__ == "__main__":
    sys.exit(main())
