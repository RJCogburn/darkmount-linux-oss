//! darkmount: command-line control for the be quiet! Dark Mount keyboard.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::io::Write;
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant, SystemTime};

use clap::{Parser, Subcommand, ValueEnum};
use darkmount_core::darkmount::bindings::{self as b, bind_type};
use darkmount_core::darkmount::device::{bindings_notify, color_mode, direction, effect, fmt_color,
                                  game_flags, lighting_mode, parse_color, LightingConfig};
use darkmount_core::darkmount::qlink::{self, feature, ClientType};
use darkmount_core::darkmount::{actions, firmware as fw, DarkMount};

type Res = Result<ExitCode, Box<dyn std::error::Error>>;

#[derive(Parser)]
#[command(name = "darkmount", about = "Command-line control for the be quiet! Dark Mount keyboard")]
struct Cli {
    /// print raw HID frames
    #[arg(long, global = true)]
    debug: bool,
    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// list be quiet! hidraw nodes (no session)
    List,
    /// show device info and current settings
    Info {
        /// also list QLink features
        #[arg(long)]
        features: bool,
    },
    /// lighting control
    Light {
        action: LightAction,
        #[arg(long, default_value_t = 0, hide = true)]
        zone: u8,
        /// static, colorwave, tornado, breathing, reactive, matrix, ripple
        #[arg(long)]
        effect: Option<String>,
        /// RRGGBB
        #[arg(long)]
        color: Option<String>,
        /// second color (dual-color effects)
        #[arg(long)]
        color2: Option<String>,
        /// comma-separated colors, e.g. ff0000,00ff00,0000ff
        #[arg(long)]
        gradient: Option<String>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        brightness: Option<u8>,
        #[arg(long, value_parser = clap::value_parser!(u8).range(0..=100))]
        speed: Option<u8>,
        /// up, down, left, right, clockwise, counterclockwise, ...
        #[arg(long)]
        direction: Option<String>,
        /// (show) dump raw layer bytes
        #[arg(long)]
        raw: bool,
    },
    /// game mode
    Gamemode {
        action: GameAction,
        /// (block) comma list: shift-tab,alt-f4,win,alt-tab,caps-lock; empty clears
        keys: Option<String>,
    },
    /// media dock
    Dock {
        action: DockAction,
        /// RRGGBB
        #[arg(long)]
        menu_color: Option<String>,
        #[arg(long, value_parser = ["12", "24"])]
        clock: Option<String>,
        #[arg(long, value_parser = ["disabled", "clock", "image"])]
        screensaver: Option<String>,
        /// seconds
        #[arg(long)]
        screensaver_timeout: Option<u16>,
        /// seconds, 0 = never
        #[arg(long)]
        screen_off: Option<u16>,
    },
    /// key bindings / remapping. KEY may be prefixed with Fn+ for the Fn layer,
    /// e.g. Fn+F1, Image0, CapsLock. See `darkmount keys names`.
    Keys {
        action: KeysAction,
        /// physical key, e.g. CapsLock, Fn+F12, Image0
        key: Option<String>,
        #[command(flatten)]
        to: KeyTarget,
    },
    /// firmware status, download and update
    Firmware {
        action: FwAction,
        /// use firmware from DIR (index.json + .bin files) instead of the cache
        #[arg(long = "from", value_name = "DIR")]
        from_dir: Option<PathBuf>,
        /// don't ask for confirmation
        #[arg(long)]
        yes: bool,
        /// reinstall even if not newer
        #[arg(long)]
        force: bool,
        /// don't restore your settings afterwards (updates reset them)
        #[arg(long)]
        no_restore: bool,
    },
    /// save / restore on-keyboard settings
    Backup {
        action: BackupAction,
        /// (restore) backup file; default: the latest
        file: Option<PathBuf>,
    },
    /// background service: dock clock + key actions
    Daemon {
        /// seconds
        #[arg(long, default_value_t = 300)]
        interval: u64,
    },
}

#[derive(clap::Args)]
#[group(multiple = false)]
struct KeyTarget {
    /// send a key/combo, e.g. Escape or ctrl+shift+T
    #[arg(long, value_name = "COMBO")]
    to: Option<String>,
    /// VolumeUp, VolumeDown, Mute, MicMute, PlayPause, NextTrack, …
    #[arg(long)]
    media: Option<String>,
    /// Left, Right, Middle, Forward, Back
    #[arg(long)]
    mouse: Option<String>,
    /// ScrollUp, ScrollDown, ScrollLeft, ScrollRight
    #[arg(long)]
    scroll: Option<String>,
    /// NextEffect, PrevEffect, BrightnessUp, BrightnessDown
    #[arg(long)]
    backlight: Option<String>,
    /// keyboard-performed shortcut: LockPC, SleepPC, Calculator, …
    #[arg(long)]
    host: Option<String>,
    /// opened by `darkmount daemon` (or the app)
    #[arg(long, value_name = "PATH")]
    open_file: Option<String>,
    /// opened by `darkmount daemon` (or the app)
    #[arg(long, value_name = "PATH")]
    open_folder: Option<String>,
    /// opened by `darkmount daemon` (or the app)
    #[arg(long, value_name = "URL")]
    open_url: Option<String>,
    /// make the key do nothing
    #[arg(long)]
    disable: bool,
}

#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum LightAction { Show, Set, On, Off }
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum GameAction { Status, On, Off, Block }
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum DockAction { Show, Set, SyncTime }
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum KeysAction { List, Set, Reset, Names }
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum FwAction { Status, Download, Install }
#[derive(Clone, Copy, PartialEq, ValueEnum)]
enum BackupAction { Save, Restore, List }

const FLAG_NAMES: &[(&str, u8)] = &[("shift-tab", game_flags::SHIFT_TAB), ("alt-f4", game_flags::ALT_F4),
    ("win", game_flags::WIN), ("alt-tab", game_flags::ALT_TAB), ("caps-lock", game_flags::CAPS_LOCK)];

fn fail(msg: impl Into<String>) -> Box<dyn std::error::Error> {
    msg.into().into()
}

fn flags_str(f: u8) -> String {
    let on: Vec<&str> = FLAG_NAMES.iter().filter(|(_, b)| f & b != 0).map(|(n, _)| *n).collect();
    if on.is_empty() { "(none)".into() } else { on.join(", ") }
}

/// Index in a name table, matched case-insensitively and ignoring '-'/'_'.
fn pick(table: &[&str], s: &str, what: &str) -> Result<usize, Box<dyn std::error::Error>> {
    let norm = |x: &str| x.to_lowercase().replace(['-', '_'], "");
    table.iter().position(|n| norm(n) == norm(s))
        .ok_or_else(|| fail(format!("unknown {what} {s:?}; choose from: {}", table.join(", "))))
}

fn utc(t: u32) -> String {
    // civil date from days since 1970 (Howard Hinnant's algorithm)
    let (days, secs) = ((t / 86400) as i64, t % 86400);
    let z = days + 719_468;
    let (era, doe) = (z.div_euclid(146_097), z.rem_euclid(146_097));
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + (m <= 2) as i64;
    format!("{y:04}-{m:02}-{d:02} {:02}:{:02}:{:02}", secs / 3600, secs / 60 % 60, secs % 60)
}

fn describe(c: &LightingConfig) -> String {
    let dir = direction::NAMES.get(c.direction as usize).copied().unwrap_or("?");
    let cols: Vec<String> = c.colors.iter().map(|&x| fmt_color(x)).collect();
    let mode = ["single", "dual", "gradient", "oriented gradient"].get(c.color_mode as usize).copied().unwrap_or("?");
    format!("{} brightness={}% speed={}% direction={dir} colors({mode})={}",
            c.effect_name(), c.brightness, c.speed, cols.join(","))
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match run(cli) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}

fn run(cli: Cli) -> Res {
    let debug = cli.debug;
    let open = || DarkMount::open(ClientType::Web, debug);
    match cli.cmd {
        Cmd::List => cmd_list(),
        Cmd::Keys { action: KeysAction::Names, .. } => cmd_key_names(),
        Cmd::Daemon { interval } => cmd_daemon(interval, debug),
        Cmd::Firmware { action, from_dir, yes, force, no_restore } =>
            cmd_firmware(action, from_dir, yes, force, no_restore),
        Cmd::Backup { action, file } => cmd_backup(action, file),
        Cmd::Info { features } => cmd_info(&open()?, features),
        Cmd::Light { action, zone, effect, color, color2, gradient, brightness, speed, direction, raw } => {
            let kb = open()?;
            match action {
                LightAction::Show => {
                    let c = kb.lighting(zone)?;
                    println!("mode={}  {}", lighting_name(kb.lighting_mode()?), describe(&c));
                    if raw {
                        println!("raw: {}", qlink::hex(&c.raw));
                    }
                }
                LightAction::Off => kb.set_lighting_mode(lighting_mode::OFF)?,
                LightAction::On => kb.set_lighting_mode(lighting_mode::GENERAL)?,
                LightAction::Set => {
                    let mut c = kb.lighting(zone)?;
                    if let Some(e) = effect {
                        c.effect = pick(&effect::NAMES, &e, "effect")? as u8;
                    }
                    if let Some(v) = brightness { c.brightness = v; }
                    if let Some(v) = speed { c.speed = v; }
                    if let Some(d) = direction {
                        c.direction = pick(&direction::NAMES, &d, "direction")? as u8;
                    }
                    let col = |s: &str| parse_color(s).ok_or_else(|| fail(format!("bad color {s:?} (use RRGGBB)")));
                    if let Some(g) = gradient {
                        let cols = g.split(',').map(col).collect::<Result<Vec<_>, _>>()?;
                        if cols.len() < 2 {
                            return Err(fail("--gradient needs at least two colors"));
                        }
                        let step = 100.0 / (cols.len() - 1) as f32;
                        c.color_mode = color_mode::GRADIENT;
                        c.stops = (0..cols.len()).map(|i| (i as f32 * step).round() as u8).collect();
                        c.colors = cols;
                    } else if let (Some(a), Some(b2)) = (&color, &color2) {
                        c.color_mode = color_mode::DUAL;
                        c.colors = vec![col(a)?, col(b2)?];
                    } else if let Some(a) = &color {
                        c.color_mode = color_mode::SINGLE;
                        c.colors = vec![col(a)?];
                    }
                    kb.set_lighting(&c, zone)?;
                    println!("set: {}", describe(&kb.lighting(zone)?));
                }
            }
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Gamemode { action, keys } => {
            let kb = open()?;
            match action {
                GameAction::On | GameAction::Off => kb.set_game_mode(action == GameAction::On)?,
                GameAction::Block => {
                    let mut flags = 0u8;
                    for n in keys.unwrap_or_default().split(',').map(str::trim).filter(|s| !s.is_empty()) {
                        flags |= FLAG_NAMES.iter().find(|(k, _)| k.eq_ignore_ascii_case(n)).map(|f| f.1)
                            .ok_or_else(|| fail(format!("unknown key {n:?}; choose from shift-tab, alt-f4, \
                                                         win, alt-tab, caps-lock (or pass nothing to clear)")))?;
                    }
                    kb.set_game_mode_flags(flags)?;
                }
                GameAction::Status => {}
            }
            println!("Game mode: {}  (blocks: {})", if kb.game_mode()? { "ON" } else { "off" },
                     flags_str(kb.game_mode_flags()?));
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Dock { action, menu_color, clock, screensaver, screensaver_timeout, screen_off } => {
            let kb = open()?;
            let (conn, pos) = kb.media_dock_state()?;
            if !conn {
                println!("Media dock is not connected.");
                return Ok(ExitCode::from(1));
            }
            if action == DockAction::SyncTime {
                println!("Dock clock set to {}", utc(kb.sync_media_dock_time()?));
                return Ok(ExitCode::SUCCESS);
            }
            let mut cfg = kb.media_dock_config()?;
            if action == DockAction::Set {
                if let Some(c) = menu_color {
                    cfg.menu_color = parse_color(&c).ok_or_else(|| fail("bad --menu-color (use RRGGBB)"))?;
                }
                if let Some(c) = clock { cfg.clock_mode = (c == "24") as u8; }
                if let Some(s) = screensaver {
                    cfg.screensaver_mode = ["disabled", "clock", "image"].iter().position(|x| *x == s).unwrap_or(0) as u8;
                }
                if let Some(v) = screensaver_timeout { cfg.screensaver_timeout = v; }
                if let Some(v) = screen_off { cfg.screen_off_timeout = v; }
                kb.set_media_dock_config(&cfg)?;
                cfg = kb.media_dock_config()?;
            }
            let ss = ["Disabled", "Clock", "Image"].get(cfg.screensaver_mode as usize)
                .map_or(cfg.screensaver_mode.to_string(), |s| s.to_string());
            println!("Position:            {pos}");
            println!("Clock:               {} ({})", utc(kb.media_dock_time()?),
                     if cfg.clock_mode == 1 { "24h" } else { "12h" });
            println!("Menu color:          {}", fmt_color(cfg.menu_color));
            println!("Screensaver:         {ss} after {}s", cfg.screensaver_timeout);
            println!("Screen off after:    {}s{}", cfg.screen_off_timeout,
                     if cfg.screen_off_timeout == 0 { " (never)" } else { "" });
            Ok(ExitCode::SUCCESS)
        }
        Cmd::Keys { action, key, to } => cmd_keys(&mut open()?, action, key, to),
    }
}

fn lighting_name(m: u8) -> &'static str {
    ["Off", "General", "Custom", "Realtime"].get(m as usize).copied().unwrap_or("?")
}

fn cmd_list() -> Res {
    let nodes = qlink::list_nodes(qlink::VID_BEQUIET);
    if nodes.is_empty() {
        println!("No be quiet! devices found.");
        return Ok(ExitCode::from(1));
    }
    for n in nodes {
        let page = n.usage_page.map_or("?".into(), |p| format!("0x{p:04x}"));
        let tag = if n.usage_page == Some(qlink::VENDOR_USAGE_PAGE) { "  <- control interface" } else { "" };
        println!("{}  {:04x}:{:04x}  usage_page={page}  {}{tag}", n.path.display(), n.vid, n.pid, n.name);
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_info(kb: &DarkMount, features: bool) -> Res {
    let info = kb.device_info()?;
    println!("Serial:        {}", kb.serial_number()?);
    println!("Model id:      0x{:04x} rev {}", info.model_id, info.revision);
    for (name, ver) in &info.firmware {
        println!("Firmware:      {name:10} {ver}");
    }
    println!("QLink:         {}", kb.qlink_version()?);
    let (phys, vis) = kb.layout()?;
    println!("Layout:        {phys} / {vis}");
    println!("Polling rate:  {} Hz", kb.polling_rate()?);
    println!("Game mode:     {}  (blocks: {})", if kb.game_mode()? { "ON" } else { "off" },
             flags_str(kb.game_mode_flags()?));
    println!("Lighting:      {}: {}", lighting_name(kb.lighting_mode()?), describe(&kb.lighting(0)?));
    for (label, st) in [("Numpad:", kb.numpad_state()), ("Media dock:", kb.media_dock_state())] {
        match st {
            Ok((true, pos)) => println!("{label:15}connected ({pos})"),
            Ok((false, _)) => println!("{label:15}not connected"),
            Err(e) => println!("{label:15}? ({e})"),
        }
    }
    if features {
        let f: Vec<String> = kb.supported_features()?.into_iter().map(|(n, v)| format!("{n} v{v}")).collect();
        println!("Features:      {}", f.join(", "));
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_key_names() -> Res {
    let keys: Vec<&str> = b::KEY_IDS.iter().map(|(n, _)| *n).collect();
    println!("Physical keys (KEY):\n  {}", keys.join(" "));
    let sends: Vec<String> = b::hid_table().into_iter().map(|(n, _)| n).collect();
    println!("\nKeys to send (--to, combine with ctrl/shift/alt/super/rctrl/…):\n  {}", sends.join(" "));
    for (t, table) in [("--media", b::MEDIA_ACTIONS), ("--mouse", b::MOUSE_BUTTONS), ("--scroll", b::MOUSE_SCROLL),
                       ("--backlight", b::BACKLIGHT_ACTIONS), ("--host", b::HOST_ACTIONS)] {
        println!("\n{t}:\n  {}", table.join(" "));
    }
    Ok(ExitCode::SUCCESS)
}

fn cmd_keys(kb: &mut DarkMount, action: KeysAction, key: Option<String>, to: KeyTarget) -> Res {
    if action == KeysAction::List {
        let mut bs = kb.bindings()?;
        bs.sort_by_key(|x| (x.layer, x.key));
        if !kb.bindings_enabled()? {
            println!("(custom bindings are DISABLED on the keyboard)");
        }
        if bs.is_empty() {
            println!("No custom bindings; all keys use their defaults.");
        }
        for x in bs {
            let label = format!("{}{}", if x.layer != 0 { "Fn+" } else { "" }, x.key_name());
            println!("{label:16} {}", x.describe());
        }
        return Ok(ExitCode::SUCCESS);
    }
    let name = key.ok_or_else(|| fail("which key? (see `darkmount keys names`)"))?;
    let (layer, name) = match name.get(..3) {
        Some(p) if p.eq_ignore_ascii_case("fn+") => (1u8, &name[3..]),
        _ => (0u8, name.as_str()),
    };
    let k = b::key_id(name).ok_or_else(|| fail(format!("unknown key {name:?} (see `darkmount keys names`)")))?;
    let label = format!("{}{}", if layer != 0 { "Fn+" } else { "" }, b::key_name(k));
    if action == KeysAction::Reset {
        kb.clear_binding(k, layer)?;
        println!("{label}: reset to default");
        return Ok(ExitCode::SUCCESS);
    }
    let v = |t: &[&str], s: &str, w: &str| pick(t, s, w).map(|i| i as u8 + 1);
    let rec = if let Some(c) = &to.to {
        let (mods, usage) = b::parse_combo(c).map_err(fail)?;
        b::encode_standard_key(k, layer, usage, mods)
    } else if let Some(s) = &to.media {
        b::encode_media(k, layer, v(b::MEDIA_ACTIONS, s, "media action")?)
    } else if let Some(s) = &to.mouse {
        b::encode_mouse_button(k, layer, v(b::MOUSE_BUTTONS, s, "mouse button")?)
    } else if let Some(s) = &to.scroll {
        b::encode_mouse_scroll(k, layer, v(b::MOUSE_SCROLL, s, "scroll direction")?)
    } else if let Some(s) = &to.backlight {
        b::encode_backlight(k, layer, v(b::BACKLIGHT_ACTIONS, s, "backlight action")?, 0)
    } else if let Some(s) = &to.host {
        b::encode_host_action(k, layer, v(b::HOST_ACTIONS, s, "host action")?)
    } else if let Some(p) = &to.open_file {
        b::encode_open(k, layer, bind_type::OPEN_FILE, p).map_err(fail)?
    } else if let Some(p) = &to.open_folder {
        b::encode_open(k, layer, bind_type::OPEN_FOLDER, p).map_err(fail)?
    } else if let Some(p) = &to.open_url {
        b::encode_open(k, layer, bind_type::OPEN_BROWSER, p).map_err(fail)?
    } else if to.disable {
        b::encode_disable(k, layer)
    } else {
        return Err(fail("nothing to set: use --to, --media, --mouse, --scroll, --backlight, \
                         --host, --open-file, --open-folder, --open-url or --disable"));
    };
    kb.set_binding(&rec)?;
    let now = kb.bindings()?.into_iter().find(|x| x.key == k && x.layer == layer);
    println!("{label}: {}", now.map_or("(not confirmed)".into(), |x| x.describe()));
    Ok(ExitCode::SUCCESS)
}

fn cmd_backup(action: BackupAction, file: Option<PathBuf>) -> Res {
    match action {
        BackupAction::List => {
            let mut files: Vec<PathBuf> = std::fs::read_dir(fw::backups_dir()).into_iter().flatten().flatten()
                .map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "json")).collect();
            files.sort();
            if files.is_empty() {
                println!("no backups yet");
            }
            for f in files {
                println!("{}", f.display());
            }
        }
        BackupAction::Save => {
            let kb = DarkMount::open(ClientType::Web, false)?;
            println!("saved {}", fw::save_snapshot(&fw::snapshot(&kb).map_err(fail)?).map_err(fail)?.display());
        }
        BackupAction::Restore => {
            let snap = match file {
                Some(f) => serde_json::from_str(&std::fs::read_to_string(f)?)?,
                None => fw::latest_snapshot().ok_or_else(|| fail("no backup found"))?,
            };
            let mut kb = DarkMount::open(ClientType::Web, false)?;
            fw::restore_snapshot(&mut kb, &snap, &|m| println!("{m}")).map_err(fail)?;
        }
    }
    Ok(ExitCode::SUCCESS)
}

struct Current {
    fw: Vec<(String, String)>,
    numpad: bool,
    dock: bool,
}

/// Installed versions (None if not found) and whether the keyboard is in its bootloader.
fn current_versions() -> Result<(Option<Current>, bool), Box<dyn std::error::Error>> {
    if fw::find(fw::PID_NORMAL).is_none() {
        return Ok((None, fw::find(fw::PID_BOOTLOADER).is_some()));
    }
    let kb = DarkMount::open(ClientType::Web, false)?;
    Ok((Some(Current { fw: kb.device_info()?.firmware, numpad: kb.numpad_state()?.0,
                       dock: kb.media_dock_state()?.0 }), false))
}

fn cmd_firmware(action: FwAction, from_dir: Option<PathBuf>, yes: bool, force: bool, no_restore: bool) -> Res {
    if action == FwAction::Download {
        let pkg = fw::download_latest(&|m| println!("{m}")).map_err(fail)?;
        println!("Firmware {} downloaded and verified: {}", pkg.version, pkg.folder.display());
        return Ok(ExitCode::SUCCESS);
    }
    let pkg = match from_dir {
        Some(d) => Some(fw::load_package(&d).map_err(fail)?),
        None => fw::cached_packages().into_iter().next(),
    };
    let (cur, in_bootloader) = current_versions()?;
    if action == FwAction::Status {
        if in_bootloader {
            println!("Keyboard is in BOOTLOADER mode (an update was interrupted).");
            println!("Run `darkmount firmware install` to finish it.");
        } else if let Some(c) = &cur {
            for (n, v) in &c.fw {
                println!("Installed  {n:10} {v}");
            }
            let s = |b: bool| if b { "connected" } else { "not connected" };
            println!("Numpad: {}   Media dock: {}", s(c.numpad), s(c.dock));
        } else {
            println!("Keyboard not found.");
        }
        match &pkg {
            Some(p) => {
                println!("Available  {}  ({})", p.version, p.folder.display());
                for n in &p.notes {
                    println!("  - {n}");
                }
            }
            None => println!("No firmware downloaded yet: `darkmount firmware download` (needs internet)."),
        }
        return Ok(ExitCode::SUCCESS);
    }
    let pkg = pkg.ok_or_else(|| fail("no firmware available: run `darkmount firmware download` or pass --from DIR"))?;
    if let Some(c) = cur.as_ref().filter(|_| !in_bootloader) {
        if !(c.numpad && c.dock) {
            return Err(fail("Attach both the numpad and the media dock before updating \
                             (all three parts are updated together)."));
        }
        let installed = c.fw.first().map(|x| x.1.clone()).unwrap_or_default();
        println!("Installed firmware: {installed}   ->   new firmware: {}", pkg.version);
        if fw::version_key(&pkg.version) <= fw::version_key(&installed) && !force {
            println!("Already up to date (use --force to reinstall).");
            return Ok(ExitCode::SUCCESS);
        }
    }
    println!("\n  Do NOT unplug the keyboard or put the computer to sleep during the update.");
    println!("  The keyboard will restart into update mode and back; typing won't work meanwhile.\n");
    if !yes {
        print!("Type 'update' to start: ");
        std::io::stdout().flush()?;
        let mut line = String::new();
        if std::io::stdin().read_line(&mut line)? == 0 || line.trim().to_lowercase() != "update" {
            println!("Cancelled.");
            return Ok(ExitCode::from(1));
        }
    }
    let last = RefCell::new(String::new());
    let report = |stage: &str, pct: f32, msg: &str| {
        let mut l = last.borrow_mut();
        if stage == "transfer" && msg.contains('/') && *l == "transfer" {
            print!("\r  {pct:5.1}%  {msg:60}");
            let _ = std::io::stdout().flush();
        } else {
            if *l == "transfer" {
                println!();
            }
            println!("[{stage}] {msg}");
        }
        *l = stage.to_string();
    };
    if let Err(e) = fw::update_with_backup(&pkg, &report, !no_restore) {
        eprintln!("\nUPDATE FAILED: {e}");
        eprintln!("If the keyboard stays in update mode, run `darkmount firmware install` again to resume.");
        return Ok(ExitCode::from(2));
    }
    println!("\nDone. New versions:");
    if let (Some(c), _) = current_versions()? {
        for (n, v) in c.fw {
            println!("  {n:10} {v}");
        }
    }
    Ok(ExitCode::SUCCESS)
}

/// Background service: keeps the media dock clock in sync (on connect, after resume,
/// every `interval`) and performs open-file/folder/URL key actions. Connects as the
/// IO Center client type, which is what makes the keyboard report key actions; steps
/// aside while another client (the app) is talking to the keyboard.
fn cmd_daemon(interval: u64, debug: bool) -> Res {
    println!("darkmount daemon: dock clock sync every {interval}s; key actions from {}",
             actions::config_path().display());
    loop {
        if qlink::find_control_node(None).is_err() {
            std::thread::sleep(Duration::from_secs(3));
            continue;
        }
        std::thread::sleep(Duration::from_secs(1)); // let the keyboard finish enumerating
        if let Err(e) = daemon_session(interval, debug) {
            eprintln!("keyboard disconnected: {e}");
            std::thread::sleep(Duration::from_secs(2));
        }
    }
}

fn daemon_session(interval: u64, debug: bool) -> Result<(), qlink::Error> {
    let kb = DarkMount::open(ClientType::QControl, debug)?;
    println!("keyboard connected");
    let mut last_sync: Option<Instant> = None;
    let mut last_wall = SystemTime::now();
    let mut yielded = false;
    loop {
        let now = SystemTime::now();
        let jumped = now.duration_since(last_wall).map_or(true, |d| d > Duration::from_secs(30));
        last_wall = now;
        // firmware >= 1.31 lets only the oldest session write: step aside while another
        // client talks to the keyboard, come back once it has been quiet for a while
        let quiet = kb.dev.foreign_seen().map_or(Duration::MAX, |t| t.elapsed());
        if !yielded && quiet < Duration::from_secs(2) {
            kb.dev.close_session();
            yielded = true;
            println!("another client is active; pausing");
        } else if yielded && quiet > Duration::from_secs(5) {
            kb.dev.open_session(true)?;
            yielded = false;
            last_sync = None;
            println!("resumed");
        }
        if yielded {
            kb.dev.next_notification(Duration::from_secs(1))?;
            continue;
        }
        if jumped || last_sync.is_none_or(|t| t.elapsed() >= Duration::from_secs(interval)) {
            if kb.media_dock_state()?.0 {
                let t = kb.sync_media_dock_time()?;
                if last_sync.is_none() || jumped {
                    println!("synced dock clock to {}", utc(t));
                }
            }
            last_sync = Some(Instant::now());
        }
        let Some(f) = kb.dev.next_notification(Duration::from_secs(1))? else { continue };
        if f.feature() != feature::BINDINGS || f.command() != bindings_notify::ACTION_FIRED {
            continue;
        }
        let Some(ev) = actions::parse_action_fired(f.data()) else { continue };
        let acts: BTreeMap<String, String> = actions::read_actions(&actions::config_path())
            .into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect();
        let launch = actions::decide(&ev, &acts);
        let label = actions::label(&b::key_name(ev.key), ev.layer);
        println!("{label}: {launch:?}");
        if let Err(e) = actions::spawn(&launch) {
            eprintln!("{label}: couldn't start: {e}");
        }
    }
}
