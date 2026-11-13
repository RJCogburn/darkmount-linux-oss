//! Where settings and data live (XDG; the same folders as the Python version).

use std::env;
use std::path::PathBuf;

fn home() -> PathBuf {
    env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| PathBuf::from("/"))
}

fn xdg(var: &str, fallback: &str) -> PathBuf {
    match env::var_os(var) {
        Some(v) if !v.is_empty() && PathBuf::from(&v).is_absolute() => PathBuf::from(v),
        _ => home().join(fallback),
    }
}

/// `~/.config/darkmount`
pub fn config_dir() -> PathBuf {
    xdg("XDG_CONFIG_HOME", ".config").join("darkmount")
}

/// `~/.local/share/darkmount`
pub fn data_dir() -> PathBuf {
    xdg("XDG_DATA_HOME", ".local/share").join("darkmount")
}

/// `~/.local/state/darkmount`
pub fn state_dir() -> PathBuf {
    xdg("XDG_STATE_HOME", ".local/state").join("darkmount")
}

/// `$XDG_RUNTIME_DIR` (per-login, tmpfs), else /tmp.
pub fn runtime_dir() -> PathBuf {
    env::var_os("XDG_RUNTIME_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("/tmp"))
}

/// Keyboard assets (layout data and images). Looked up in order: $DARKMOUNT_ASSETS,
/// next to the program (`assets/`), the AppImage/system location
/// (`../share/darkmount-linux/assets`), then the source tree (development).
pub fn assets_dir() -> PathBuf {
    if let Some(p) = env::var_os("DARKMOUNT_ASSETS") {
        return PathBuf::from(p);
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            for cand in [dir.join("assets"), dir.join("../share/darkmount-linux/assets")] {
                if cand.join("dark_mount").is_dir() {
                    return cand;
                }
            }
        }
    }
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../assets")
}

