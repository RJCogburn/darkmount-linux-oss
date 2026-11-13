//! Key bindings for the Dark Mount (BINDINGS feature, 0x11).

use std::fmt::Write as _;

/// Physical key ids used by the keyboard (from be quiet!'s Dark Mount key map).
pub const KEY_IDS: &[(&str, u8)] = &[
    ("Grave", 1), ("1", 2), ("2", 3), ("3", 4), ("4", 5), ("5", 6), ("6", 7), ("7", 8),
    ("8", 9), ("9", 10), ("0", 11), ("Minus", 12), ("Equals", 13), ("Backspace", 14),
    ("Tab", 15), ("Q", 16), ("W", 17), ("E", 18), ("R", 19), ("T", 20), ("Y", 21), ("U", 22),
    ("I", 23), ("O", 24), ("P", 25), ("BracketLeft", 26), ("BracketRight", 27),
    ("Backslash", 28), ("CapsLock", 29), ("A", 30), ("S", 31), ("D", 32), ("F", 33),
    ("G", 34), ("H", 35), ("J", 36), ("K", 37), ("L", 38), ("Semicolon", 39),
    ("Apostrophe", 40), ("Enter", 41), ("LeftShift", 42), ("Z", 43), ("X", 44), ("C", 45),
    ("V", 46), ("B", 47), ("N", 48), ("M", 49), ("Comma", 50), ("Period", 51), ("Slash", 52),
    ("RightShift", 53), ("LeftCtrl", 54), ("Fn", 55), ("LeftAlt", 56), ("Space", 57),
    ("RightAlt", 58), ("RightCtrl", 59), ("Insert", 60), ("Delete", 61), ("Left", 62),
    ("Home", 63), ("End", 64), ("Up", 65), ("Down", 66), ("PageUp", 67), ("PageDown", 68),
    ("Right", 69), ("NumLock", 70), ("Num7", 71), ("Num4", 72), ("Num1", 73),
    ("NumSlash", 74), ("Num8", 75), ("Num5", 76), ("Num2", 77), ("Num0", 78),
    ("NumAsterisk", 79), ("Num9", 80), ("Num6", 81), ("Num3", 82), ("NumPeriod", 83),
    ("NumMinus", 84), ("NumPlus", 85), ("NumEnter", 86), ("Escape", 87), ("F1", 88),
    ("F2", 89), ("F3", 90), ("F4", 91), ("F5", 92), ("F6", 93), ("F7", 94), ("F8", 95),
    ("F9", 96), ("F10", 97), ("F11", 98), ("F12", 99), ("PrintScreen", 100),
    ("ScrollLock", 101), ("Pause", 102), ("LeftGui", 103), ("RightGui", 104),
    ("NonUsBackslash", 105), ("Image0", 109), ("Image1", 110), ("Image2", 111),
    ("Image3", 112), ("Image4", 113), ("Image5", 114), ("Image6", 115), ("Image7", 116),
    ("DockMute", 117), ("DockPlayPause", 118), ("DockPrev", 119), ("DockNext", 120),
    ("DockLeft", 121), ("DockRight", 128),
];

pub fn key_id(name: &str) -> Option<u8> {
    KEY_IDS.iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|&(_, id)| id)
}

pub fn key_name(id: u8) -> String {
    KEY_IDS.iter().find(|&&(_, i)| i == id).map_or_else(|| format!("key{id}"), |(n, _)| n.to_string())
}

/// USB HID keyboard usage ids, for "send this key" bindings.
pub fn hid_usage(name: &str) -> Option<u8> {
    hid_table().into_iter().find(|(n, _)| n.eq_ignore_ascii_case(name)).map(|(_, u)| u)
}

pub fn hid_name(usage: u8) -> String {
    hid_table().into_iter().find(|&(_, u)| u == usage)
        .map_or_else(|| format!("0x{usage:02x}"), |(n, _)| n)
}

pub fn hid_table() -> Vec<(String, u8)> {
    let mut t: Vec<(String, u8)> = Vec::with_capacity(120);
    for i in 0..26u8 {
        t.push((((b'A' + i) as char).to_string(), 0x04 + i));
    }
    for i in 0..10u8 {
        t.push((((i + 1) % 10).to_string(), 0x1E + i));
    }
    let named: &[(&str, u8)] = &[
        ("Enter", 0x28), ("Escape", 0x29), ("Backspace", 0x2A), ("Tab", 0x2B), ("Space", 0x2C),
        ("Minus", 0x2D), ("Equals", 0x2E), ("BracketLeft", 0x2F), ("BracketRight", 0x30),
        ("Backslash", 0x31), ("Semicolon", 0x33), ("Apostrophe", 0x34), ("Grave", 0x35),
        ("Comma", 0x36), ("Period", 0x37), ("Slash", 0x38), ("CapsLock", 0x39),
    ];
    t.extend(named.iter().map(|&(n, u)| (n.to_string(), u)));
    for i in 1..=12u8 {
        t.push((format!("F{i}"), 0x3A + i - 1));
    }
    let named: &[(&str, u8)] = &[
        ("PrintScreen", 0x46), ("ScrollLock", 0x47), ("Pause", 0x48), ("Insert", 0x49),
        ("Home", 0x4A), ("PageUp", 0x4B), ("Delete", 0x4C), ("End", 0x4D), ("PageDown", 0x4E),
        ("Right", 0x4F), ("Left", 0x50), ("Down", 0x51), ("Up", 0x52), ("NumLock", 0x53),
        ("NumSlash", 0x54), ("NumAsterisk", 0x55), ("NumMinus", 0x56), ("NumPlus", 0x57),
        ("NumEnter", 0x58),
    ];
    t.extend(named.iter().map(|&(n, u)| (n.to_string(), u)));
    for i in 1..=9u8 {
        t.push((format!("Num{i}"), 0x59 + i - 1));
    }
    let named: &[(&str, u8)] = &[("Num0", 0x62), ("NumPeriod", 0x63), ("NonUsBackslash", 0x64),
                                 ("Menu", 0x65)];
    t.extend(named.iter().map(|&(n, u)| (n.to_string(), u)));
    for i in 13..=24u8 {
        t.push((format!("F{i}"), 0x68 + i - 13));
    }
    let named: &[(&str, u8)] = &[
        ("LeftCtrl", 0xE0), ("LeftShift", 0xE1), ("LeftAlt", 0xE2), ("LeftGui", 0xE3),
        ("RightCtrl", 0xE4), ("RightShift", 0xE5), ("RightAlt", 0xE6), ("RightGui", 0xE7),
    ];
    t.extend(named.iter().map(|&(n, u)| (n.to_string(), u)));
    t
}

pub const LAYER_COMMON: u8 = 0;
pub const LAYER_FN: u8 = 1;

/// Binding record types.
pub mod bind_type {
    pub const DEFAULT: u8 = 0;
    pub const STANDARD_KEY: u8 = 1;
    pub const MEDIA: u8 = 2;
    pub const MOUSE: u8 = 3;
    pub const OPEN_FOLDER: u8 = 4;
    pub const OPEN_FILE: u8 = 5;
    pub const OPEN_BROWSER: u8 = 6;
    pub const WINDOWS_SHORTCUT: u8 = 7;
    pub const PROFILE: u8 = 8;
    pub const BACKLIGHT: u8 = 9;
    pub const MACRO: u8 = 10;
    pub const ALT_CODE: u8 = 11;
    pub const DISABLE: u8 = 12;

    pub fn name(t: u8) -> &'static str {
        ["Default", "StandardKey", "Media", "Mouse", "OpenFolder", "OpenFile", "OpenBrowser",
         "WindowsShortcut", "Profile", "Backlight", "Macro", "AltCode", "Disable"]
            .get(t as usize).copied().unwrap_or("Unknown")
    }

    /// Types whose action the host app performs when the key is pressed.
    pub fn is_host(t: u8) -> bool {
        matches!(t, OPEN_FOLDER | OPEN_FILE | OPEN_BROWSER | WINDOWS_SHORTCUT | PROFILE)
    }
}

/// Modifier bits for StandardKey bindings.
pub const MODIFIERS: &[(&str, u8)] = &[("LCtrl", 1), ("LShift", 2), ("LAlt", 4), ("LGui", 8),
    ("RCtrl", 16), ("RShift", 32), ("RAlt", 64), ("RGui", 128)];

const MOD_ALIASES: &[(&str, u8)] = &[("ctrl", 1), ("shift", 2), ("alt", 4), ("super", 8),
    ("win", 8), ("meta", 8), ("rctrl", 16), ("rshift", 32), ("ralt", 64), ("altgr", 64),
    ("rsuper", 128)];

pub const MEDIA_ACTIONS: &[&str] = &["VolumeUp", "VolumeDown", "Mute", "MicMute", "NextTrack",
                             "PrevTrack", "PlayPause", "Stop"];
pub const MOUSE_BUTTONS: &[&str] = &["Left", "Right", "Middle", "Forward", "Back"];
pub const MOUSE_SCROLL: &[&str] = &["ScrollUp", "ScrollDown", "ScrollLeft", "ScrollRight"];
/// 'WindowsShortcut' actions: performed by the host app, not the keyboard.
pub const HOST_ACTIONS: &[&str] = &["FileExplorer", "Calculator", "TaskManager", "LockPC",
    "ShutDownPC", "SleepPC", "HibernatePC", "SnippingTool", "Notepad", "Paint", "AirplaneMode",
    "XboxGameBar", "SystemSettings", "Refresh", "TabbingApp", "CloseApp", "Copy", "Paste", "Cut",
    "InternetBrowser", "EmailReader"];
pub const BACKLIGHT_ACTIONS: &[&str] = &["SelectEffect", "NextEffect", "PrevEffect",
                                         "BrightnessUp", "BrightnessDown"];

/// Name of a 1-based enum value from a table.
pub fn name_of(table: &[&str], v: u8) -> String {
    (v as usize).checked_sub(1).and_then(|i| table.get(i))
        .map_or_else(|| v.to_string(), |s| s.to_string())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Binding {
    pub key: u8,
    pub layer: u8,
    pub kind: u8,
    /// the whole record as the keyboard sent it
    pub data: Vec<u8>,
    pub modifiers: u8,
    pub usage: u8,
    pub action: u8,
    pub text: String,
    /// Mouse: wheel instead of button
    pub scroll: bool,
    /// Mouse button mode: "", "double-click", "while-pressed", "auto-fire N"
    pub mode: String,
    pub effect: u8,
    pub macro_id: u8,
    pub alt_code: u16,
}

impl Binding {
    pub fn key_name(&self) -> String {
        key_name(self.key)
    }

    pub fn describe(&self) -> String {
        use bind_type::*;
        match self.kind {
            STANDARD_KEY => {
                let mut parts: Vec<String> = MODIFIERS.iter()
                    .filter(|(_, b)| self.modifiers & b != 0).map(|(n, _)| n.to_string()).collect();
                parts.push(hid_name(self.usage));
                format!("key {}", parts.join("+"))
            }
            MEDIA => format!("media {}", name_of(MEDIA_ACTIONS, self.action)),
            MOUSE if self.scroll => format!("mouse {}", name_of(MOUSE_SCROLL, self.action)),
            MOUSE => {
                let mut s = format!("mouse {}", name_of(MOUSE_BUTTONS, self.action));
                if !self.mode.is_empty() {
                    let _ = write!(s, " ({})", self.mode);
                }
                s
            }
            WINDOWS_SHORTCUT => format!("system shortcut: {}", name_of(HOST_ACTIONS, self.action)),
            OPEN_FILE | OPEN_FOLDER | OPEN_BROWSER => {
                format!("{} {:?}", bind_type::name(self.kind), self.text)
            }
            BACKLIGHT => {
                let mut s = format!("backlight {}", name_of(BACKLIGHT_ACTIONS, self.action));
                if self.action == 1 {
                    let _ = write!(s, " {}", self.effect);
                }
                s
            }
            MACRO => format!("macro {}", self.macro_id),
            ALT_CODE => format!("alt-code {}", self.alt_code),
            DISABLE | DEFAULT => "disabled".into(),
            _ => format!("{} {}", bind_type::name(self.kind), super::qlink::hex(&self.data)),
        }
    }
}

fn latin1(b: &[u8]) -> String {
    b.iter().map(|&c| c as char).collect()
}

/// Parse a GetBindings reply (mirrors the official app's parser).
pub fn parse_bindings(t: &[u8]) -> Vec<Binding> {
    use bind_type::*;
    let mut out = Vec::new();
    if t.len() < 2 {
        return out;
    }
    let total = t[0] as usize + t[1] as usize;
    let i = &t[2..];
    let at = |k: usize| i.get(k).copied().unwrap_or(0);
    let mut a = 0usize;
    for _ in 0..total {
        if a + 3 > i.len() {
            break;
        }
        let mut kind = i[a + 2];
        if kind == DEFAULT {
            kind = DISABLE;
        }
        if kind > DISABLE {
            break; // unknown record type: its size is unknown, stop
        }
        let p = a + 3;
        let mut b = Binding { key: i[a], layer: (i[a + 1] >> 7) & 1, kind, ..Default::default() };
        let size = match kind {
            STANDARD_KEY => {
                b.modifiers = at(p);
                b.usage = at(p + 1);
                5
            }
            ALT_CODE => {
                b.alt_code = at(p + 1) as u16 | (at(p + 2) as u16) << 8;
                6
            }
            MEDIA => {
                b.action = at(p);
                if b.action == 10 {
                    // specific sound device: length-prefixed id
                    let n = at(p + 1) as usize;
                    b.text = latin1(i.get(p + 2..(p + 2 + n).min(i.len())).unwrap_or(&[]));
                    5 + n
                } else {
                    4
                }
            }
            MOUSE => {
                if at(p) == 2 {
                    b.scroll = true;
                    b.action = at(p + 1) + 1;
                    5
                } else {
                    let v = at(p + 1);
                    b.action = (v & 31) + 1;
                    b.mode = if v >> 7 & 1 != 0 {
                        "double-click".into()
                    } else if v >> 6 & 1 != 0 {
                        "while-pressed".into()
                    } else if at(p + 2) != 0 {
                        format!("auto-fire {}", at(p + 2))
                    } else {
                        String::new()
                    };
                    6
                }
            }
            OPEN_FILE | OPEN_FOLDER | OPEN_BROWSER => {
                let n = at(p) as usize;
                b.text = latin1(i.get(p + 1..(p + 1 + n).min(i.len())).unwrap_or(&[]));
                4 + n
            }
            WINDOWS_SHORTCUT => {
                b.action = at(p);
                4
            }
            PROFILE => {
                b.action = at(p);
                if b.action == 1 { 20 } else { 4 }
            }
            BACKLIGHT => {
                b.action = at(p);
                if b.action == 1 {
                    b.effect = at(p + 1);
                    5
                } else {
                    4
                }
            }
            MACRO => {
                b.action = at(p);
                b.macro_id = at(p + 1);
                5
            }
            _ => 3, // Disable
        };
        b.data = i[a..(a + size).min(i.len())].to_vec();
        out.push(b);
        a += size;
        if a >= i.len() {
            break;
        }
    }
    out
}

/// The keyboard expects the Fn layer as bit 7 of the second byte (verified on
/// firmware 1.19.0: 0x01 is rejected by GetBinding; 0x80 round-trips).
fn keyref(key: u8, layer: u8) -> Vec<u8> {
    vec![key, if layer != 0 { 0x80 } else { 0 }]
}

pub fn encode_standard_key(key: u8, layer: u8, usage: u8, modifiers: u8) -> Vec<u8> {
    let mut v = keyref(key, layer);
    v.extend([bind_type::STANDARD_KEY, modifiers, usage]);
    v
}

pub fn encode_media(key: u8, layer: u8, action: u8) -> Vec<u8> {
    let mut v = keyref(key, layer);
    v.extend([bind_type::MEDIA, action]);
    v
}

/// `button` is 1-based (Left = 1).
pub fn encode_mouse_button(key: u8, layer: u8, button: u8) -> Vec<u8> {
    let mut v = keyref(key, layer);
    v.extend([bind_type::MOUSE, 1, button.saturating_sub(1) & 31, 0]);
    v
}

/// `direction` is 1-based (ScrollUp = 1).
pub fn encode_mouse_scroll(key: u8, layer: u8, direction: u8) -> Vec<u8> {
    let mut v = keyref(key, layer);
    v.extend([bind_type::MOUSE, 2, direction.saturating_sub(1)]);
    v
}

pub fn encode_host_action(key: u8, layer: u8, action: u8) -> Vec<u8> {
    let mut v = keyref(key, layer);
    v.extend([bind_type::WINDOWS_SHORTCUT, action]);
    v
}

pub fn encode_backlight(key: u8, layer: u8, action: u8, effect: u8) -> Vec<u8> {
    let mut v = keyref(key, layer);
    v.extend([bind_type::BACKLIGHT, action]);
    if action == 1 {
        v.push(effect);
    }
    v
}

/// OpenFile / OpenFolder / OpenBrowser: performed by the host when pressed.
pub fn encode_open(key: u8, layer: u8, kind: u8, target: &str) -> Result<Vec<u8>, String> {
    use bind_type::*;
    if !matches!(kind, OPEN_FILE | OPEN_FOLDER | OPEN_BROWSER) {
        return Err(format!("not an open binding: {kind}"));
    }
    let raw: Vec<u8> = target.chars().map(|c| {
        u8::try_from(c as u32).map_err(|_| format!("{c:?} can't be stored on the keyboard"))
    }).collect::<Result<_, _>>()?;
    if raw.len() > 200 {
        return Err("target too long (max 200 characters)".into());
    }
    let mut v = keyref(key, layer);
    v.extend([kind, raw.len() as u8]);
    v.extend(raw);
    Ok(v)
}

/// The official app sends type 0 for "disable".
pub fn encode_disable(key: u8, layer: u8) -> Vec<u8> {
    let mut v = keyref(key, layer);
    v.push(0);
    v
}

pub fn encode_clear(key: u8, layer: u8) -> Vec<u8> {
    keyref(key, layer)
}

/// "ctrl+shift+Escape" -> (modifiers, usage)
pub fn parse_combo(combo: &str) -> Result<(u8, u8), String> {
    let mut mods = 0u8;
    let mut usage = None;
    for p in combo.split('+').map(str::trim).filter(|p| !p.is_empty()) {
        if let Some(&(_, b)) = MOD_ALIASES.iter().find(|(n, _)| n.eq_ignore_ascii_case(p)) {
            mods |= b;
            continue;
        }
        usage = Some(hid_usage(p).ok_or_else(|| format!("unknown key {p:?}"))?);
    }
    usage.map(|u| (mods, u)).ok_or_else(|| "combo needs a non-modifier key".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn h(v: &[u8]) -> String {
        v.iter().map(|b| format!("{b:02x}")).collect()
    }

    #[test]
    fn encoders_match_python() {
        let (m, u) = parse_combo("ctrl+shift+Escape").unwrap();
        assert_eq!((m, u), (3, 0x29));
        assert_eq!(h(&encode_standard_key(key_id("F1").unwrap(), 1, u, m)), "5880010329");
        assert_eq!(h(&encode_open(109, 0, bind_type::OPEN_BROWSER, "https://x.y").unwrap()),
                   "6d00060b68747470733a2f2f782e79");
        assert_eq!(h(&encode_disable(5, 0)), "050000");
        assert_eq!(h(&encode_clear(5, 1)), "0580");
        assert_eq!(h(&encode_mouse_button(3, 0, 2)), "030003010100");
        assert_eq!(h(&encode_mouse_scroll(3, 0, 2)), "0300030201");
        assert_eq!(h(&encode_backlight(3, 0, 1, 4)), "0300090104");
        assert!(parse_combo("ctrl+Esc").is_err());
        assert!(parse_combo("ctrl+shift").is_err());
    }

    #[test]
    fn parser_matches_python() {
        let mut data = vec![2, 1, 2, 0x80, 1, 1, 0x29, 109, 0, 6, 3];
        data.extend(b"a.b");
        data.extend([30, 0, 7, 6]);
        let b = parse_bindings(&data);
        assert_eq!(b.len(), 3);
        assert_eq!((b[0].key, b[0].layer, b[0].kind), (2, 1, 1));
        assert_eq!(b[0].describe(), "key LCtrl+Escape");
        assert_eq!((b[1].key, b[1].text.as_str()), (109, "a.b"));
        assert_eq!(b[1].describe(), "OpenBrowser \"a.b\"");
        assert_eq!(b[2].describe(), "system shortcut: SleepPC");
        assert_eq!(b[2].key_name(), "A");
    }

    #[test]
    fn hid_table_spot_checks() {
        assert_eq!(hid_usage("a"), Some(0x04));
        assert_eq!(hid_usage("0"), Some(0x27));
        assert_eq!(hid_usage("F12"), Some(0x45));
        assert_eq!(hid_usage("F13"), Some(0x68));
        assert_eq!(hid_usage("Num0"), Some(0x62));
        assert_eq!(hid_name(0xE3), "LeftGui");
    }
}
