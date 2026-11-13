//! High-level API for the be quiet! Dark Mount keyboard.

use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use super::bindings::{self, Binding};
use super::qlink::{self, feature, root, ClientType, Error, QLinkDevice, Result};

pub const PID_DARK_MOUNT: u16 = 0x0001;
pub const PID_DARK_MOUNT_BOOTLOADER: u16 = 0x0009;

mod cmd {
    pub const INFO_GET_DEVICE_INFO: u8 = 1;
    pub const INFO_GET_SERIAL: u8 = 2;
    pub const KB_GET_LAYOUT: u8 = 1;
    pub const KB_GET_CONFIG: u8 = 2;
    pub const KB_SET_CONFIG: u8 = 3;
    pub const KB_GET_STATE: u8 = 4;
    pub const KB_SET_STATE: u8 = 5;
    pub const USB_GET_POLLING: u8 = 1;
    pub const LIGHT_GET_MODE: u8 = 1;
    pub const LIGHT_SET_MODE: u8 = 2;
    pub const LIGHT_GET_LAYER: u8 = 5;
    pub const LIGHT_SET_LAYER: u8 = 6;
    pub const BIND_GET_ALL: u8 = 1;
    pub const BIND_SET: u8 = 2;
    pub const BIND_CLEAR: u8 = 3;
    pub const BIND_GET_CONFIG: u8 = 4;
    pub const MODULE_GET_STATE: u8 = 1;
    pub const DOCK_GET_CONFIG: u8 = 2;
    pub const DOCK_SET_CONFIG: u8 = 3;
    pub const DOCK_GET_TIME: u8 = 4;
    pub const DOCK_SET_TIME: u8 = 5;
}

/// Bindings notifications (feature BINDINGS).
pub mod bindings_notify {
    pub const BINDING_CHANGED: u8 = 1;
    pub const ACTION_FIRED: u8 = 2;
    pub const CONFIG_CHANGED: u8 = 3;
}

pub mod lighting_mode {
    pub const OFF: u8 = 0;
    pub const GENERAL: u8 = 1;
    pub const CUSTOM: u8 = 2;
    pub const REALTIME: u8 = 3;
}

/// Effects (the keyboard reports unknown/"off" values above 100 as Off).
pub mod effect {
    pub const STATIC: u8 = 0;
    pub const COLOR_WAVE: u8 = 1;
    pub const TORNADO: u8 = 2;
    pub const BREATHING: u8 = 3;
    pub const REACTIVE: u8 = 4;
    pub const MATRIX: u8 = 5;
    pub const OFF: u8 = 6;
    pub const RIPPLE: u8 = 7;
    pub const NAMES: [&str; 8] = ["Static", "ColorWave", "Tornado", "Breathing", "Reactive",
                                  "Matrix", "Off", "Ripple"];
}

pub mod color_mode {
    pub const SINGLE: u8 = 0;
    pub const DUAL: u8 = 1;
    pub const GRADIENT: u8 = 2;
    pub const ORIENTED_GRADIENT: u8 = 3;
}

pub mod direction {
    pub const NAMES: [&str; 10] = ["Up", "Down", "Left", "Right", "Clockwise",
        "Counterclockwise", "Omnidirectional", "Horizontal", "Vertical", "Cross"];
}

/// Game mode blocks these shortcuts.
pub mod game_flags {
    pub const SHIFT_TAB: u8 = 1;
    pub const ALT_F4: u8 = 2;
    pub const WIN: u8 = 4;
    pub const ALT_TAB: u8 = 8;
    pub const CAPS_LOCK: u8 = 16;
}

pub type Rgb = [u8; 3];

pub fn parse_color(s: &str) -> Option<Rgb> {
    let s = s.trim().trim_start_matches('#');
    if s.len() != 6 || !s.is_ascii() {
        return None;
    }
    let c = |i: usize| u8::from_str_radix(&s[i..i + 2], 16).ok();
    Some([c(0)?, c(2)?, c(4)?])
}

pub fn fmt_color(c: Rgb) -> String {
    format!("#{:02x}{:02x}{:02x}", c[0], c[1], c[2])
}

/// One lighting layer (zone).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LightingConfig {
    pub effect: u8,
    pub direction: u8,
    pub brightness: u8,
    pub speed: u8,
    pub color_mode: u8,
    /// Single: [c]; Dual: [c1, c2]; gradients: colors with positions in `stops`
    pub colors: Vec<Rgb>,
    /// gradient positions (0..100), same length as `colors` for gradients
    pub stops: Vec<u8>,
    pub gradient_type: u8,
    pub gradient_angle: u16,
    /// Ripple: width, range, fade-out
    pub ripple: Option<[u8; 3]>,
    pub raw: Vec<u8>,
}

impl Default for LightingConfig {
    fn default() -> Self {
        LightingConfig {
            effect: effect::STATIC,
            direction: 1,
            brightness: 100,
            speed: 50,
            color_mode: color_mode::SINGLE,
            colors: vec![[255, 40, 0]],
            stops: Vec::new(),
            gradient_type: 0,
            gradient_angle: 90,
            ripple: None,
            raw: Vec::new(),
        }
    }
}

impl LightingConfig {
    pub fn parse(t: &[u8]) -> LightingConfig {
        let at = |i: usize| t.get(i).copied().unwrap_or(0);
        let rgb = |i: usize| [at(i), at(i + 1), at(i + 2)];
        let eff = if at(0) > 100 { effect::OFF } else { at(0) };
        let cm = at(4);
        let mut c = LightingConfig {
            effect: eff, direction: at(1), brightness: at(2), speed: at(3), color_mode: cm,
            colors: Vec::new(), stops: Vec::new(), gradient_type: 0, gradient_angle: 90,
            ripple: None, raw: t.to_vec(),
        };
        match cm {
            color_mode::SINGLE => c.colors = vec![rgb(5)],
            color_mode::DUAL => c.colors = vec![rgb(5), rgb(8)],
            color_mode::GRADIENT => {
                for i in 0..at(5) as usize {
                    c.colors.push(rgb(6 + i * 4));
                    c.stops.push(at(9 + i * 4));
                }
            }
            color_mode::ORIENTED_GRADIENT => {
                c.gradient_type = at(5);
                c.gradient_angle = at(6) as u16 | (at(7) as u16) << 8;
                for i in 0..at(8) as usize {
                    c.colors.push(rgb(9 + i * 4));
                    c.stops.push(at(12 + i * 4));
                }
            }
            _ => {}
        }
        if eff == effect::RIPPLE && t.len() >= 3 {
            let n = t.len();
            c.ripple = Some([t[n - 3], t[n - 2], t[n - 1]]);
        }
        c
    }

    pub fn encode(&self, zone: u8) -> Vec<u8> {
        let mut out = vec![zone, self.effect, self.direction, self.brightness, self.speed,
                           self.color_mode];
        let first = self.colors.first().copied().unwrap_or([255, 40, 0]);
        match self.color_mode {
            color_mode::SINGLE => out.extend(first),
            color_mode::DUAL => {
                out.extend(first);
                out.extend(self.colors.get(1).copied().unwrap_or(first));
            }
            color_mode::GRADIENT | color_mode::ORIENTED_GRADIENT => {
                if self.color_mode == color_mode::ORIENTED_GRADIENT {
                    out.extend([self.gradient_type, (self.gradient_angle & 0xFF) as u8,
                                (self.gradient_angle >> 8) as u8]);
                }
                out.push(self.colors.len() as u8);
                for (i, c) in self.colors.iter().enumerate() {
                    out.extend(c);
                    out.push(self.stops.get(i).copied().unwrap_or(0));
                }
            }
            _ => {}
        }
        if self.effect == effect::RIPPLE {
            out.extend(self.ripple.unwrap_or([5, 7, 0]));
        }
        out
    }

    pub fn effect_name(&self) -> String {
        effect::NAMES.get(self.effect as usize).map_or_else(|| self.effect.to_string(),
                                                            |s| s.to_string())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MediaDockConfig {
    pub menu_color: Rgb,
    /// 0 = 12 h, 1 = 24 h
    pub clock_mode: u8,
    /// 0 = disabled, 1 = clock, 2 = image
    pub screensaver_mode: u8,
    pub screensaver_timeout: u16,
    pub screen_off_timeout: u16,
}

impl MediaDockConfig {
    pub fn parse(t: &[u8]) -> MediaDockConfig {
        let at = |i: usize| t.get(i).copied().unwrap_or(0);
        MediaDockConfig {
            menu_color: [at(0), at(1), at(2)],
            clock_mode: at(3),
            screensaver_mode: at(4),
            screensaver_timeout: at(5) as u16 | (at(6) as u16) << 8,
            screen_off_timeout: at(7) as u16 | (at(8) as u16) << 8,
        }
    }

    pub fn encode(&self) -> Vec<u8> {
        let [r, g, b] = self.menu_color;
        vec![r, g, b, self.clock_mode, self.screensaver_mode,
             (self.screensaver_timeout & 0xFF) as u8, (self.screensaver_timeout >> 8) as u8,
             (self.screen_off_timeout & 0xFF) as u8, (self.screen_off_timeout >> 8) as u8]
    }
}

/// Version bytes the firmware stores as hex digits ("0x19" means 19).
fn bcdish(x: u8) -> u32 {
    let s = format!("{x:x}");
    if s.chars().all(|c| c.is_ascii_digit()) { s.parse().unwrap_or(x as u32) } else { x as u32 }
}

#[derive(Debug, Clone)]
pub struct DeviceInfo {
    pub model_id: u16,
    pub revision: u8,
    /// (MCU name, version)
    pub firmware: Vec<(String, String)>,
}

const BINDING_SETTLE: Duration = Duration::from_millis(1200);
const T1: Duration = Duration::from_secs(1);

/// An open session with the Dark Mount.
pub struct DarkMount {
    pub dev: QLinkDevice,
    last_binding_write: Option<Instant>,
}

impl DarkMount {
    /// Find the keyboard, open its control node and a session.
    pub fn open(client_type: ClientType, debug: bool) -> Result<DarkMount> {
        let node = qlink::find_control_node(None)?;
        if node.pid == PID_DARK_MOUNT_BOOTLOADER {
            return Err(Error::Other("Keyboard is in bootloader (firmware update) mode. \
                                     Finish the firmware update first.".into()));
        }
        let dev = QLinkDevice::open(node, client_type, debug)?;
        dev.open_session(true)?;
        Ok(DarkMount { dev, last_binding_write: None })
    }

    fn req(&self, f: u8, c: u8, payload: &[u8]) -> Result<Vec<u8>> {
        match self.dev.request(f, c, payload, T1, None) {
            // firmware >= 1.31 only accepts writes from the active session
            Err(e) if e.is_not_active() && self.dev.ensure_active(Duration::from_secs(4)) => {
                Ok(self.dev.request(f, c, payload, T1, None)?.data)
            }
            r => Ok(r?.data),
        }
    }

    // -- info -----------------------------------------------------------------------
    pub fn qlink_version(&self) -> Result<String> {
        let d = self.req(feature::ROOT, root::GET_QLINK_VERSION, &[])?;
        let at = |i: usize| d.get(i).copied().unwrap_or(0) as u32;
        Ok(format!("{}.{}.{}", at(3), at(2), (at(1) << 8) + at(0)))
    }

    pub fn supported_features(&self) -> Result<Vec<(String, u8)>> {
        let d = self.req(feature::ROOT, root::GET_SUPPORTED_FEATURES, &[0])?;
        let n = d.first().copied().unwrap_or(0) as usize;
        Ok((0..n).filter_map(|i| {
            Some((feature::name(*d.get(1 + i * 2)?), *d.get(2 + i * 2)?))
        }).collect())
    }

    pub fn serial_number(&self) -> Result<String> {
        let d = self.req(feature::DEVICE_INFO, cmd::INFO_GET_SERIAL, &[])?;
        let n = d.first().copied().unwrap_or(0) as usize;
        Ok(String::from_utf8_lossy(d.get(1..(1 + n).min(d.len())).unwrap_or(&[])).into_owned())
    }

    pub fn device_info(&self) -> Result<DeviceInfo> {
        let t = self.req(feature::DEVICE_INFO, cmd::INFO_GET_DEVICE_INFO, &[])?;
        let at = |i: usize| t.get(i).copied().unwrap_or(0);
        let names = ["Main", "MediaDock", "Numpad"];
        let firmware = (0..at(3) as usize).map(|i| {
            let r = 4 + i * 4;
            // the middle part is two version bytes joined, then read as a number
            let middle: u32 = format!("{}{}", bcdish(at(r + 1)), bcdish(at(r + 2)))
                .parse().unwrap_or(0);
            let version = format!("{}.{middle}.{}", bcdish(at(r + 3)), bcdish(at(r)));
            (names.get(i).map_or_else(|| format!("MCU{i}"), |s| s.to_string()), version)
        }).collect();
        Ok(DeviceInfo { model_id: at(0) as u16 | (at(1) as u16) << 8, revision: at(2), firmware })
    }

    // -- keyboard -------------------------------------------------------------------
    pub fn layout(&self) -> Result<(String, String)> {
        let d = self.req(feature::KEYBOARD, cmd::KB_GET_LAYOUT, &[])?;
        let phys = ["ANSI", "ISO", "JIS"];
        let vis = ["US", "DE", "UK", "FR", "NO"];
        let a = d.first().copied().unwrap_or(0);
        let b = d.get(1).copied().unwrap_or(0);
        Ok((phys.get(a as usize).map_or_else(|| a.to_string(), |s| s.to_string()),
            vis.get(b as usize).map_or_else(|| b.to_string(), |s| s.to_string())))
    }

    pub fn game_mode(&self) -> Result<bool> {
        Ok(self.req(feature::KEYBOARD, cmd::KB_GET_STATE, &[])?.first().is_some_and(|b| b & 1 != 0))
    }

    pub fn set_game_mode(&self, on: bool) -> Result<()> {
        self.req(feature::KEYBOARD, cmd::KB_SET_STATE, &[on as u8]).map(drop)
    }

    pub fn game_mode_flags(&self) -> Result<u8> {
        Ok(self.req(feature::KEYBOARD, cmd::KB_GET_CONFIG, &[])?.first().copied().unwrap_or(0) & 0x1F)
    }

    pub fn set_game_mode_flags(&self, flags: u8) -> Result<()> {
        self.req(feature::KEYBOARD, cmd::KB_SET_CONFIG, &[flags]).map(drop)
    }

    pub fn polling_rate(&self) -> Result<u16> {
        let d = self.req(feature::USB_DEVICE, cmd::USB_GET_POLLING, &[])?;
        Ok(d.first().copied().unwrap_or(0) as u16 | (d.get(1).copied().unwrap_or(0) as u16) << 8)
    }

    // -- lighting -------------------------------------------------------------------
    pub fn lighting_mode(&self) -> Result<u8> {
        Ok(self.req(feature::LIGHTINGS, cmd::LIGHT_GET_MODE, &[])?.first().copied().unwrap_or(0))
    }

    pub fn set_lighting_mode(&self, mode: u8) -> Result<()> {
        self.req(feature::LIGHTINGS, cmd::LIGHT_SET_MODE, &[mode]).map(drop)
    }

    pub fn lighting(&self, zone: u8) -> Result<LightingConfig> {
        Ok(LightingConfig::parse(&self.req(feature::LIGHTINGS, cmd::LIGHT_GET_LAYER, &[zone])?))
    }

    pub fn set_lighting(&self, cfg: &LightingConfig, zone: u8) -> Result<()> {
        if self.lighting_mode()? != lighting_mode::GENERAL {
            self.set_lighting_mode(lighting_mode::GENERAL)?;
        }
        self.req(feature::LIGHTINGS, cmd::LIGHT_SET_LAYER, &cfg.encode(zone))?;
        thread::sleep(Duration::from_millis(100)); // the web app waits after each layer write
        Ok(())
    }

    // -- key bindings ---------------------------------------------------------------
    pub fn bindings_enabled(&self) -> Result<bool> {
        Ok(self.req(feature::BINDINGS, cmd::BIND_GET_CONFIG, &[])?.first().is_some_and(|b| *b != 0))
    }

    /// All bindings. List reads right after a write return stale data, so wait.
    pub fn bindings(&self) -> Result<Vec<Binding>> {
        if let Some(t) = self.last_binding_write {
            let left = (t + BINDING_SETTLE).saturating_duration_since(Instant::now());
            thread::sleep(left);
        }
        let r = self.dev.request(feature::BINDINGS, cmd::BIND_GET_ALL, &[0, 0],
                                 Duration::from_secs(3), None)?;
        Ok(bindings::parse_bindings(&r.data))
    }

    /// Write a record from one of the `bindings::encode_*` helpers (2 bytes = clear).
    pub fn set_binding(&mut self, record: &[u8]) -> Result<()> {
        let c = if record.len() == 2 { cmd::BIND_CLEAR } else { cmd::BIND_SET };
        self.req(feature::BINDINGS, c, record)?;
        self.last_binding_write = Some(Instant::now());
        Ok(())
    }

    pub fn clear_binding(&mut self, key: u8, layer: u8) -> Result<()> {
        self.set_binding(&bindings::encode_clear(key, layer))
    }

    // -- modules --------------------------------------------------------------------
    fn module_state(&self, f: u8) -> Result<(bool, String)> {
        let d = self.req(f, cmd::MODULE_GET_STATE, &[])?;
        let side = d.get(1).copied().unwrap_or(0);
        Ok((d.first().is_some_and(|b| *b != 0),
            ["None", "Left", "Right"].get(side as usize).map_or_else(|| side.to_string(),
                                                                     |s| s.to_string())))
    }

    pub fn numpad_state(&self) -> Result<(bool, String)> {
        self.module_state(feature::NUMPAD_MODULE)
    }

    pub fn media_dock_state(&self) -> Result<(bool, String)> {
        self.module_state(feature::MEDIA_DOCK_MODULE)
    }

    pub fn media_dock_config(&self) -> Result<MediaDockConfig> {
        Ok(MediaDockConfig::parse(&self.req(feature::MEDIA_DOCK_MODULE, cmd::DOCK_GET_CONFIG, &[])?))
    }

    pub fn set_media_dock_config(&self, cfg: &MediaDockConfig) -> Result<()> {
        self.req(feature::MEDIA_DOCK_MODULE, cmd::DOCK_SET_CONFIG, &cfg.encode()).map(drop)
    }

    pub fn media_dock_time(&self) -> Result<u32> {
        let mut d = self.req(feature::MEDIA_DOCK_MODULE, cmd::DOCK_GET_TIME, &[])?;
        d.resize(4, 0);
        Ok(u32::from_le_bytes([d[0], d[1], d[2], d[3]]))
    }

    /// Set the dock clock to local wall-clock time (as the official app does).
    pub fn sync_media_dock_time(&self) -> Result<u32> {
        let local = local_epoch();
        self.req(feature::MEDIA_DOCK_MODULE, cmd::DOCK_SET_TIME, &local.to_le_bytes())?;
        Ok(local)
    }

    pub fn close(&self) {
        self.dev.close();
    }
}

impl Drop for DarkMount {
    fn drop(&mut self) {
        self.dev.close();
    }
}

/// Seconds since the epoch in local time (UTC + the current offset).
pub fn local_epoch() -> u32 {
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs()) as i64;
    let mut tm: libc::tm = unsafe { std::mem::zeroed() };
    let t = now as libc::time_t;
    // SAFETY: localtime_r writes into our tm.
    unsafe { libc::localtime_r(&t, &mut tm) };
    (now + tm.tm_gmtoff) as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(v: &[u8]) -> String {
        v.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn lighting_encode_matches_python() {
        let c = LightingConfig {
            effect: effect::BREATHING, direction: 2, brightness: 80, speed: 30,
            color_mode: color_mode::DUAL, colors: vec![[1, 2, 3], [4, 5, 6]],
            ..Default::default()
        };
        assert_eq!(h(&c.encode(0)), "000302501e01010203040506");
        let g = LightingConfig {
            effect: effect::RIPPLE, direction: 6, brightness: 100, speed: 50,
            color_mode: color_mode::ORIENTED_GRADIENT, colors: vec![[255, 0, 0], [0, 0, 255]],
            stops: vec![0, 100], gradient_type: 1, gradient_angle: 300, ripple: Some([5, 7, 0]),
            ..Default::default()
        };
        let enc = g.encode(2);
        assert_eq!(h(&enc), "020706643203012c0102ff0000000000ff64050700");
        // the keyboard replies without the zone byte
        let back = LightingConfig::parse(&enc[1..]);
        assert_eq!((back.colors.clone(), back.stops.clone(), back.gradient_angle, back.ripple),
                   (g.colors, g.stops, 300, Some([5, 7, 0])));
    }

    #[test]
    fn dock_config_matches_python() {
        let c = MediaDockConfig { menu_color: [238, 127, 0], clock_mode: 1, screensaver_mode: 2,
                                  screensaver_timeout: 300, screen_off_timeout: 600 };
        assert_eq!(h(&c.encode()), "ee7f0001022c015802");
        assert_eq!(MediaDockConfig::parse(&c.encode()), c);
    }

    #[test]
    fn colors() {
        assert_eq!(parse_color("#ee7f00"), Some([0xee, 0x7f, 0]));
        assert_eq!(parse_color("ee7f0"), None);
        assert_eq!(fmt_color([255, 40, 0]), "#ff2800");
        assert_eq!(bcdish(0x19), 19);
        assert_eq!(bcdish(0x1A), 26);
    }
}
