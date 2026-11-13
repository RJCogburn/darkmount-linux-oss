//! The background helper as a systemd user service (`darkmount-clock.service`): it
//! syncs the dock clock and runs key actions while the app isn't running.

use std::fs;
use std::io;
use std::path::PathBuf;
use std::process::Command;

pub const NAME: &str = "darkmount-clock.service";

pub fn unit_path() -> PathBuf {
    crate::paths::config_dir().parent().map(|p| p.to_path_buf()).unwrap_or_default()
        .join("systemd/user").join(NAME)
}

/// How to start `darkmount daemon`: through the AppImage when running from one, else
/// the `darkmount` program installed next to this one, else from $PATH.
pub fn helper_command() -> String {
    let quote = |p: String| if p.contains(' ') { format!("\"{p}\"") } else { p };
    if let Ok(img) = std::env::var("APPIMAGE") {
        return format!("{} darkmount daemon --interval 600", quote(img));
    }
    let beside = std::env::current_exe().ok().and_then(|e| e.parent().map(|d| d.join("darkmount")))
        .filter(|p| p.is_file());
    match beside {
        Some(p) => format!("{} daemon --interval 600", quote(p.to_string_lossy().into_owned())),
        None => "darkmount daemon --interval 600".into(),
    }
}

pub fn unit_text(exec: &str) -> String {
    format!("[Unit]\nDescription=be quiet! Dark Mount helper (dock clock sync, key actions)\n\
             After=graphical-session.target\n\n[Service]\nExecStart={exec}\nRestart=on-failure\n\
             RestartSec=10\n\n[Install]\nWantedBy=default.target\n")
}

/// Write the unit (pointing at this copy of the program) and reload systemd.
pub fn install() -> io::Result<()> {
    let p = unit_path();
    if let Some(d) = p.parent() {
        fs::create_dir_all(d)?;
    }
    fs::write(&p, unit_text(&helper_command()))?;
    systemctl(&["daemon-reload"]);
    Ok(())
}

pub fn systemctl(args: &[&str]) -> String {
    let mut c = Command::new("systemctl");
    c.arg("--user").args(args);
    crate::hostenv::clean(&mut c);
    c.output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

#[cfg(test)]
mod tests {
    #[test]
    fn unit() {
        let t = super::unit_text("\"/a b/darkmount-linux.AppImage\" darkmount daemon --interval 600");
        assert!(t.contains("ExecStart=\"/a b/darkmount-linux.AppImage\" darkmount daemon"));
        assert!(t.contains("WantedBy=default.target"));
    }
}
