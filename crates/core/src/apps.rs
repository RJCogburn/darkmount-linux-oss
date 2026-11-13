//! Installed programs, as the desktop's application menu sees them: the
//! `.desktop` entries in `~/.local/share/applications` and every
//! `$XDG_DATA_DIRS/applications` (system packages, Flatpaks, Snaps).

use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct App {
    /// desktop file id, e.g. "org.kde.dolphin.desktop"
    pub id: String,
    pub name: String,
    pub comment: String,
    pub keywords: String,
    /// icon name or absolute path from the entry
    pub icon: String,
    /// the Exec line without field codes (%f, %U, ...): a command that starts it
    pub command: String,
    pub terminal: bool,
    pub path: PathBuf,
}

impl App {
    /// Does the app match a search (name, description, keywords, command)?
    pub fn matches(&self, query: &str) -> bool {
        let q = query.trim().to_lowercase();
        q.is_empty() || q.split_whitespace().all(|w| {
            self.name.to_lowercase().contains(w) || self.comment.to_lowercase().contains(w)
                || self.keywords.to_lowercase().contains(w)
                || self.command.to_lowercase().contains(w)
        })
    }
}

fn data_dirs() -> Vec<PathBuf> {
    let home = env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
    let mut dirs = vec![env::var_os("XDG_DATA_HOME").map(PathBuf::from)
        .unwrap_or_else(|| home.join(".local/share"))];
    let sys = env::var("XDG_DATA_DIRS").ok().filter(|s| !s.is_empty())
        .unwrap_or_else(|| "/usr/local/share:/usr/share".into());
    dirs.extend(sys.split(':').filter(|s| !s.is_empty()).map(PathBuf::from));
    dirs
}

/// Remove desktop-entry field codes; "%%" becomes "%".
pub fn strip_field_codes(exec: &str) -> String {
    let mut out = String::new();
    let mut it = exec.chars().peekable();
    while let Some(c) = it.next() {
        if c == '%' {
            if let Some('%') = it.next() { out.push('%') }
        } else {
            out.push(c);
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Parse one .desktop file; None for entries the menu wouldn't show.
pub fn parse_entry(text: &str, id: &str, path: &Path) -> Option<App> {
    let lang = env::var("LANG").unwrap_or_default();
    let short = lang.split(['_', '.', '@']).next().unwrap_or("").to_string();
    let mut in_main = false;
    let mut keys: HashMap<String, String> = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            in_main = line == "[Desktop Entry]";
            continue;
        }
        if !in_main || line.starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once('=') {
            keys.entry(k.trim().to_string()).or_insert_with(|| v.trim().to_string());
        }
    }
    let get = |k: &str| -> String {
        if !short.is_empty() {
            if let Some(v) = keys.get(&format!("{k}[{short}]")) {
                return v.clone();
            }
        }
        keys.get(k).cloned().unwrap_or_default()
    };
    let truthy = |k: &str| get(k).eq_ignore_ascii_case("true");
    if get("Type") != "Application" || truthy("NoDisplay") || truthy("Hidden") {
        return None;
    }
    let name = get("Name");
    let exec = get("Exec");
    if name.is_empty() || exec.is_empty() {
        return None;
    }
    Some(App {
        id: id.to_string(),
        name,
        comment: get("Comment"),
        keywords: get("Keywords").replace(';', " "),
        icon: get("Icon"),
        command: strip_field_codes(&exec),
        terminal: truthy("Terminal"),
        path: path.to_path_buf(),
    })
}

/// All installed programs, sorted by name. An entry in an earlier directory (your
/// own) hides one with the same id further down (the system's).
pub fn installed() -> Vec<App> {
    let mut seen: HashMap<String, Option<App>> = HashMap::new();
    for dir in data_dirs() {
        let apps = dir.join("applications");
        let mut stack = vec![apps.clone()];
        while let Some(d) = stack.pop() {
            let Ok(rd) = fs::read_dir(&d) else { continue };
            for e in rd.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                    continue;
                }
                if p.extension().and_then(|x| x.to_str()) != Some("desktop") {
                    continue;
                }
                // ids of entries in subfolders use "-" for "/" (spec)
                let id = p.strip_prefix(&apps).map(|r| r.to_string_lossy().replace('/', "-"))
                    .unwrap_or_default();
                if seen.contains_key(&id) {
                    continue;
                }
                let app = fs::read_to_string(&p).ok().and_then(|t| parse_entry(&t, &id, &p));
                seen.insert(id, app); // None too: a hidden user entry hides the system one
            }
        }
    }
    let mut out: Vec<App> = seen.into_values().flatten().collect();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

/// Look up an icon file for an icon name (current theme, Breeze, hicolor, pixmaps).
pub fn icon_path(icon: &str) -> Option<PathBuf> {
    if icon.is_empty() {
        return None;
    }
    let p = Path::new(icon);
    if p.is_absolute() {
        if p.is_file() {
            return Some(p.to_path_buf());
        }
        return ["png", "svg", "xpm"].iter().map(|e| PathBuf::from(format!("{icon}.{e}")))
            .find(|f| f.is_file());
    }
    let themes = ["breeze-dark", "breeze", "hicolor", "Adwaita"];
    let cats = ["apps", "places", "devices", "categories", "mimetypes", "actions"];
    let mut bases: Vec<PathBuf> = data_dirs().into_iter().map(|d| d.join("icons")).collect();
    if let Some(h) = env::var_os("HOME") {
        bases.insert(0, PathBuf::from(h).join(".icons"));
    }
    for base in &bases {
        for theme in themes {
            let t = base.join(theme);
            if !t.is_dir() {
                continue;
            }
            let mut dirs = Vec::new();
            for cat in cats {
                // hicolor/Adwaita: <size>/<category>
                for size in ["scalable", "64x64", "48x48", "128x128", "256x256", "32x32"] {
                    dirs.push(t.join(size).join(cat));
                }
                // Breeze: <category>/<size>
                for size in ["64", "48", "32", "22", "16"] {
                    dirs.push(t.join(cat).join(size));
                }
            }
            for dir in dirs {
                for ext in ["svg", "png"] {
                    let f = dir.join(format!("{icon}.{ext}"));
                    if f.is_file() {
                        return Some(f);
                    }
                }
            }
        }
    }
    for ext in ["svg", "png", "xpm"] {
        let f = Path::new("/usr/share/pixmaps").join(format!("{icon}.{ext}"));
        if f.is_file() {
            return Some(f);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn field_codes() {
        assert_eq!(strip_field_codes("dolphin %u"), "dolphin");
        assert_eq!(strip_field_codes("/usr/bin/flatpak run --command=x org.App @@u %U @@"),
                   "/usr/bin/flatpak run --command=x org.App @@u @@");
        assert_eq!(strip_field_codes("printf 100%% %f"), "printf 100%");
    }

    #[test]
    fn entries() {
        let p = Path::new("/x.desktop");
        let e = parse_entry("[Desktop Entry]\nType=Application\nName=Dolphin\nComment=Files\n\
                             Exec=dolphin %u\nIcon=system-file-manager\nKeywords=files;folder;\n\
                             [Desktop Action new]\nName=New window\nExec=dolphin --new\n",
                            "org.kde.dolphin.desktop", p).unwrap();
        assert_eq!((e.name.as_str(), e.command.as_str()), ("Dolphin", "dolphin"));
        assert!(e.matches("folder") && e.matches("dolph") && !e.matches("browser"));
        assert!(parse_entry("[Desktop Entry]\nType=Application\nName=X\nExec=x\nNoDisplay=true\n",
                            "x", p).is_none());
        assert!(parse_entry("[Desktop Entry]\nType=Link\nName=X\nURL=y\n", "x", p).is_none());
    }

    #[test]
    fn finds_installed_programs() {
        let apps = installed();
        assert!(!apps.is_empty());
        assert!(apps.windows(2).all(|w| w[0].name.to_lowercase() <= w[1].name.to_lowercase()));
    }
}
