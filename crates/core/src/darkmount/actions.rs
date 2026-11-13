//! Key actions run by the darkmount background service: the `[actions]` table of
//! `~/.config/darkmount/config.toml`, keyed by key label ("Image0", "Fn+F5").
//! Written in the same layout as the Python app so both read each other's files.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use crate::paths;

/// Marker the keyboard stores for "run a Linux command" (an OpenFile binding whose
/// target the service recognises).
pub const RUN_MARKER: &str = "darkmount:run";

const HEADER: &str = "\
# Dark Mount key actions, run by the darkmount background service.
# Key names: `darkmount keys names` (prefix with \"Fn+\" for the Fn layer).
# A command here runs whenever the keyboard reports that key's action.
# The file is reloaded automatically when saved.
";

pub fn config_path() -> PathBuf {
    paths::config_dir().join("config.toml")
}

/// Actions keyed by label as written (missing or broken file: none).
pub fn read_actions(path: &Path) -> BTreeMap<String, String> {
    let Ok(text) = fs::read_to_string(path) else { return BTreeMap::new() };
    let Ok(doc) = text.parse::<toml::Table>() else { return BTreeMap::new() };
    doc.get("actions").and_then(|a| a.as_table()).map(|t| {
        t.iter().map(|(k, v)| {
            let s = v.as_str().map(str::to_string).unwrap_or_else(|| v.to_string());
            (k.clone(), s)
        }).collect()
    }).unwrap_or_default()
}

fn toml_str(s: &str) -> String {
    if s.contains('\n') && !s.contains("'''") {
        return format!("'''\n{}\n'''", s.trim_end_matches('\n'));
    }
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n"))
}

fn toml_key(k: &str) -> String {
    if !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-') {
        k.to_string()
    } else {
        format!("\"{}\"", k.replace('\\', "\\\\").replace('"', "\\\""))
    }
}

pub fn write_actions(path: &Path, actions: &BTreeMap<String, String>) -> io::Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir)?;
    }
    let mut out = String::from(HEADER);
    out.push_str("\n[actions]\n");
    for (k, v) in actions {
        out.push_str(&format!("{} = {}\n", toml_key(k), toml_str(v)));
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, out)?;
    fs::rename(&tmp, path)
}

/// Set or remove (None/empty) the command for a label (case-insensitive match).
pub fn set_action(path: &Path, label: &str, command: Option<&str>) -> io::Result<()> {
    let mut acts = read_actions(path);
    acts.retain(|k, _| !k.eq_ignore_ascii_case(label));
    if let Some(c) = command.filter(|c| !c.trim().is_empty()) {
        acts.insert(label.to_string(), c.to_string());
    }
    write_actions(path, &acts)
}

/// "Fn+F5" for the Fn layer, "F5" otherwise.
pub fn label(key_name: &str, layer: u8) -> String {
    if layer != 0 { format!("Fn+{key_name}") } else { key_name.to_string() }
}

/// What the user sees for a key.
pub fn key_title(name: &str) -> String {
    const DISPLAY: [&str; 8] = ["⌨ keyboard", "🌐 internet", "📁 folder", "✉ email",
                                "✂ scissors", "〰 wave", "🔒 lock", "🌙 moon"];
    if let Some(i) = name.strip_prefix("Image").and_then(|d| d.parse::<usize>().ok()) {
        if let Some(icon) = DISPLAY.get(i) {
            return format!("Display key {i} ({icon})");
        }
    }
    match name {
        "DockMute" => "Media dock: top left",
        "DockPlayPause" => "Media dock: top right",
        "DockPrev" => "Media dock: bottom left",
        "DockNext" => "Media dock: bottom right",
        "Grave" => "` ~",
        "Minus" => "- _",
        "Equals" => "= +",
        "BracketLeft" => "[ {",
        "BracketRight" => "] }",
        "Backslash" => "\\ |",
        "Semicolon" => "; :",
        "Apostrophe" => "' \"",
        "Comma" => ", <",
        "Period" => ". >",
        "Slash" => "/ ?",
        "LeftGui" => "Left Win",
        "RightGui" => "Right Win",
        other => other,
    }
    .to_string()
}

// ------------------------------------------------------------------ running actions
/// An ActionFired notification: `[key, layer, handled_by_device, type, payload…]`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ActionFired {
    pub key: u8,
    pub layer: u8,
    /// the keyboard already did it (e.g. sent a shortcut)
    pub device_handled: bool,
    pub kind: u8,
    /// path or URL of open-file/folder/browser bindings
    pub text: String,
}

pub fn parse_action_fired(d: &[u8]) -> Option<ActionFired> {
    use super::bindings::bind_type::*;
    if d.len() < 4 {
        return None;
    }
    let mut a = ActionFired { key: d[0], layer: (d[1] >> 7) & 1, device_handled: d[2] != 0,
                              kind: d[3], text: String::new() };
    if matches!(a.kind, OPEN_FILE | OPEN_FOLDER | OPEN_BROWSER) && d.len() > 4 {
        let n = d[4] as usize;
        a.text = d.get(5..(5 + n).min(d.len())).unwrap_or(&[]).iter().map(|&c| c as char).collect();
    }
    Some(a)
}

/// What to do for a key action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// a command from config.toml, through `sh -c`
    Shell(String),
    /// xdg-open a URL, file or folder
    Open(String),
    /// run an executable file directly
    Exec(String),
    /// the binding points at something that doesn't exist here
    Missing(String),
    Nothing,
}

/// Decide what a key action does (as the Python service did). `actions` is keyed by
/// lower-case label.
pub fn decide(ev: &ActionFired, actions: &BTreeMap<String, String>) -> Launch {
    use super::bindings::bind_type::*;
    let label = label(&super::bindings::key_name(ev.key), ev.layer);
    if let Some(cmd) = actions.get(&label.to_lowercase()).filter(|c| !c.trim().is_empty()) {
        return Launch::Shell(cmd.clone());
    }
    if ev.device_handled || ev.text.is_empty() || ev.text == RUN_MARKER {
        return Launch::Nothing;
    }
    match ev.kind {
        OPEN_BROWSER => Launch::Open(ev.text.clone()),
        OPEN_FILE | OPEN_FOLDER => {
            let expanded = if let Some(rest) = ev.text.strip_prefix("~/") {
                format!("{}/{rest}", std::env::var("HOME").unwrap_or_default())
            } else {
                ev.text.clone()
            };
            let p = Path::new(&expanded);
            if !p.exists() {
                Launch::Missing(ev.text.clone())
            } else if ev.kind == OPEN_FILE && is_executable(p) {
                Launch::Exec(expanded)
            } else {
                Launch::Open(expanded)
            }
        }
        _ => Launch::Nothing,
    }
}

fn is_executable(p: &Path) -> bool {
    use std::os::unix::fs::PermissionsExt;
    p.is_file() && p.metadata().is_ok_and(|m| m.permissions().mode() & 0o111 != 0)
}

/// Start a program detached from this app (own session, no stdin/stdout).
pub fn spawn(launch: &Launch) -> io::Result<()> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command, Stdio};
    let mut cmd = match launch {
        Launch::Shell(c) => {
            let mut x = Command::new("sh");
            x.arg("-c").arg(c);
            x
        }
        Launch::Open(t) => {
            let mut x = Command::new("xdg-open");
            x.arg(t);
            x
        }
        Launch::Exec(p) => Command::new(p),
        Launch::Missing(_) | Launch::Nothing => return Ok(()),
    };
    crate::hostenv::clean(&mut cmd);
    cmd.stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).process_group(0);
    cmd.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_with_the_python_format() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path().join("config.toml");
        // as the Python app writes it
        fs::write(&p, format!("{HEADER}\n[actions]\nImage0 = '''\nP=\"/x y\"\nexec \"$P\"\n'''\n\
                                \"Fn+F5\" = \"konsole\"\n")).unwrap();
        let a = read_actions(&p);
        assert_eq!(a["Image0"], "P=\"/x y\"\nexec \"$P\"\n");
        assert_eq!(a["Fn+F5"], "konsole");
        set_action(&p, "fn+f5", Some("dolphin")).unwrap();
        set_action(&p, "Image3", Some("echo \"hi\"")).unwrap();
        let a = read_actions(&p);
        assert_eq!(a.len(), 3);
        assert_eq!(a["fn+f5"], "dolphin");
        assert_eq!(a["Image3"], "echo \"hi\"");
        assert_eq!(a["Image0"], "P=\"/x y\"\nexec \"$P\"\n");
        set_action(&p, "IMAGE3", None).unwrap();
        assert!(!read_actions(&p).contains_key("Image3"));
        assert!(fs::read_to_string(&p).unwrap().starts_with("# Dark Mount key actions"));
    }

    #[test]
    fn action_fired() {
        // Image1 (110), common layer, not handled, OpenFile "darkmount:run"
        let mut d = vec![110, 0, 0, 5, 13];
        d.extend(RUN_MARKER.bytes());
        let ev = parse_action_fired(&d).unwrap();
        assert_eq!((ev.key, ev.layer, ev.kind, ev.text.as_str()), (110, 0, 5, RUN_MARKER));
        let mut acts = BTreeMap::new();
        assert_eq!(decide(&ev, &acts), Launch::Nothing);
        acts.insert("image1".into(), "brave".into());
        assert_eq!(decide(&ev, &acts), Launch::Shell("brave".into()));
        let url = ActionFired { key: 111, layer: 1, device_handled: false, kind: 6,
                                text: "https://x.y".into() };
        assert_eq!(decide(&url, &acts), Launch::Open("https://x.y".into()));
        let gone = ActionFired { key: 111, layer: 0, device_handled: false, kind: 5,
                                 text: "/nope/x.exe".into() };
        assert_eq!(decide(&gone, &acts), Launch::Missing("/nope/x.exe".into()));
    }

    #[test]
    fn titles() {
        assert_eq!(key_title("Image7"), "Display key 7 (🌙 moon)");
        assert_eq!(key_title("DockNext"), "Media dock: bottom right");
        assert_eq!(key_title("F5"), "F5");
        assert_eq!(label("F5", 1), "Fn+F5");
    }
}
