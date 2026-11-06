# darkmount-linux

Unofficial, **offline** Linux control for the **be quiet! Dark Mount** keyboard:
lighting, key remapping (including the Fn layer and the numpad display keys),
game mode, media dock settings, dock clock sync and **firmware updates**. There's a command-line tool,
a background helper service and a desktop app, which you can also get as an
AppImage.

The vendor's own configuration software is Windows-only, or a web app that needs
an internet connection. This project talks to the keyboard directly over
`/dev/hidraw` and never goes online.

> **Not affiliated with be quiet!** “be quiet!” and “Dark Mount” are trademarks
> of their respective owner and are used here only to say which hardware this
> software supports. See [Notice](#notice).

Tested on firmware 1.19.0 and 1.31.0 (main board, media dock and numpad), ANSI/US
layout, including a full 1.19.0 → 1.31.0 firmware update.

## Setup

Give your user access to the keyboard (once):

```bash
sudo cp 70-bequiet-darkmount.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
# then unplug and re-plug the keyboard
```

The desktop app can also install this rule for you (it asks for your password).

## Desktop app

| Section | What you can do |
|---|---|
| Illumination | effect (Static, Color Wave, Tornado, Breathing, Reactive, Matrix), single/dual/spectrum colours, brightness, speed, direction. Animated live preview |
| Key Binding | click a key on the rendered keyboard and assign a key or shortcut, media, mouse, lighting control, system shortcut, **Linux command**, open file/folder/website, or disable. Separate Fn layer |
| Media Dock | menu colour, idle screen (image/clock), 12/24 h clock, screen-off timer, clock sync |
| Product Settings | firmware versions and updates, game mode and blocked keys, layout, helper service on/off |

**AppImage** (bundled Python and Qt, ~60 MB):

```bash
packaging/build-appimage.sh                     # -> dist/darkmount-linux-x86_64.AppImage
./darkmount-linux-x86_64.AppImage               # desktop app
./darkmount-linux-x86_64.AppImage --cli info    # same commands as `darkmount`
./darkmount-linux-x86_64.AppImage --daemon      # background service
```

**From source** (needs PySide6, e.g. `pacman -S pyside6`): `python3 -m darkmount.ui`

## Command line

Python 3.10+ standard library only. Run it with `python3 -m darkmount …`, or install
it with `pipx install .` or `uv tool install .`.

```bash
darkmount info                      # serial, firmware, layout, current settings

darkmount light set --effect static --color ff2800 --brightness 80
darkmount light set --effect colorwave --gradient ff0000,00ff00,0000ff --direction right
darkmount light off | on

darkmount keys list                          # current custom bindings
darkmount keys set CapsLock --to Escape      # remap (combos: ctrl+shift+T, super+E, …)
darkmount keys set Fn+F12 --media MicMute    # Fn layer
darkmount keys set Image0 --open-url https://example.com
darkmount keys reset CapsLock
darkmount keys names                         # all key and action names

darkmount gamemode on | off
darkmount gamemode block win,alt-tab,alt-f4

darkmount dock set --clock 24 --menu-color dc4d00 --screensaver clock
darkmount dock sync-time

darkmount firmware status           # installed vs. downloaded firmware
darkmount firmware download         # fetch the latest from the vendor's server (online)
darkmount firmware install          # back up settings, update, restore settings
darkmount firmware install --from DIR   # offline: folder with index.json + .bin files

darkmount backup save | list | restore [FILE]
```

`--debug` prints every raw 64-byte frame.

## Background service

```bash
cp systemd/darkmount-clock.service ~/.config/systemd/user/   # adjust ExecStart if needed
systemctl --user enable --now darkmount-clock.service
```

(or toggle **Linux helper service** in the app's Product Settings.)

The service:

* syncs the media dock clock on connect, after suspend/resume and every 10 minutes;
* performs **open file / open folder / open website** key actions. The keyboard
  only reports these to host software. Everything else (media keys, Win+E,
  Win+L, Sleep and so on) is sent by the keyboard itself as normal key presses;
* runs your own commands from `~/.config/darkmount/config.toml`:

```toml
[actions]
Image0 = "flatpak run org.gimp.GIMP"
"Fn+F12" = "konsole"
```

The numpad display keys are `Image0`–`Image7`, left to right, top row first.
**Run Linux command** in the app sets up both the key binding and the config entry.

## Firmware updates

`darkmount firmware install` (or **Check for updates → Install** in the app) follows
the vendor's update sequence: the keyboard reboots into its bootloader (USB PID
`0009`), receives the images for the main board, media dock and numpad (32-byte
chunks, ~3 minutes), validates and commits them, and reboots. Firmware is downloaded
from the vendor's own update server (it is not included in this repository) and
every image is checked against the SHA-512 in the vendor manifest before flashing.

* Updates **reset the keyboard's settings to factory defaults**. The tool backs them
  up first and restores them afterwards (`~/.local/share/darkmount/backups/`).
* If an update is interrupted, the keyboard stays in its bootloader: it can't type,
  but it isn't bricked. Run the install again (or press *Finish firmware update* in
  the app) and it resumes where it stopped.
* Use at your own risk. Don't unplug the keyboard or suspend the computer while it
  updates.

Firmware 1.31 only accepts changes from the oldest open session. The background
service steps aside automatically whenever the app or CLI talks to the keyboard and
reconnects ~5 s after they're done. Display-key Linux actions pause while the app is
open.

## Protocol notes

The keyboard speaks a vendor protocol ("QLink") over 64-byte HID reports on the
vendor collection (usage page `0xFF00`), with no report ID:

| byte | field |
|------|-------|
| 0 | payload length (data + 6) |
| 1 | sequence id, bit 7 = more frames |
| 2 | session id |
| 3 | status (0 = success) |
| 4 | request id |
| 5 | feature (1 root, 3 device info, 6 usb, 7 keyboard, 16 lighting, 17 bindings, 18 macros, 32 numpad, 33 media dock) |
| 6 | command |
| 7.. | data |
| 62–63 | CRC-16/Modbus of bytes 0–61, little-endian |

* Sessions: `ROOT/OpenSession` (session 0, data = random u32 + client type
  1 = desktop app, 2 = web), kept alive with `ROOT/KeepAlive` every timeout/2 s.
  Several sessions can coexist, and inactive ones can still write.
* `BINDINGS/ActionFired` notifications (`[key, layer, handled_by_device, type,
  payload…]`) are only sent to client-type-1 sessions.
* Binding records: `[key id, layer (0x00 / 0x80 = Fn), type, data…]`. The Fn
  layer must be `0x80`; `0x01` is rejected on 1.19.0. `GetBindings` returns
  stale data for ~1 s after a write.

See `darkmount/qlink.py`, `device.py` and `bindings.py` for the details.

## Notice

This is an independent, unofficial project. It is not affiliated with, endorsed by
or supported by be quiet! or Listan GmbH. Use at your own risk.

The keyboard renders and layout data in `darkmount/assets/dark_mount/` originate
from the vendor's configuration software. The vendor logo has been removed
(`packaging/remove_logo.py`), and the assets are included only to depict the
hardware this software controls. If you are the rights holder and want them
removed, please open an issue.

The Inter font (`darkmount/assets/fonts/`) is © The Inter Project Authors and
licensed under the SIL Open Font License 1.1.
