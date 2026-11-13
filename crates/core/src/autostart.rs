//! Start at login: an XDG autostart entry (~/.config/autostart/<id>.desktop), the
//! same file the Python app wrote, so whichever app is installed owns it.

use std::fs;
use std::io;
use std::path::PathBuf;

pub const DESKTOP_ID: &str = "darkmount-linux";

fn path() -> PathBuf {
    crate::paths::config_dir().parent().map(|p| p.to_path_buf()).unwrap_or_default()
        .join("autostart").join(format!("{DESKTOP_ID}.desktop"))
}

pub fn enabled() -> bool {
    path().exists()
}

/// The command that starts this program: the AppImage if running from one.
pub fn launch_command(arg: &str) -> String {
    let exe = std::env::var("APPIMAGE").ok().or_else(|| {
        std::env::current_exe().ok().map(|p| p.to_string_lossy().into_owned())
    }).unwrap_or_else(|| DESKTOP_ID.into());
    let q = if exe.contains(' ') { format!("\"{exe}\"") } else { exe };
    format!("{q} {arg}")
}

pub fn set(on: bool, name: &str) -> io::Result<()> {
    let p = path();
    if !on {
        return match fs::remove_file(&p) {
            Err(e) if e.kind() != io::ErrorKind::NotFound => Err(e),
            _ => Ok(()),
        };
    }
    if let Some(d) = p.parent() {
        fs::create_dir_all(d)?;
    }
    fs::write(&p, format!("[Desktop Entry]\nType=Application\nName={name}\n\
        Comment=be quiet! Dark Mount keyboard (system tray)\n\
        Exec={}\nIcon={DESKTOP_ID}\nTerminal=false\nX-GNOME-Autostart-enabled=true\n\
        X-KDE-autostart-after=panel\n", launch_command("--tray")))
}
