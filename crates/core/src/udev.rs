//! The udev rule that gives the logged-in user access to the keyboard's hidraw nodes.

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

pub const RULE_PATH: &str = "/etc/udev/rules.d/70-bequiet-darkmount.rules";
pub const RULE: &str = include_str!("../../../70-bequiet-darkmount.rules");

/// Is the rule there (installed by the app, or by a distro package)?
pub fn installed() -> bool {
    [RULE_PATH, "/usr/lib/udev/rules.d/70-bequiet-darkmount.rules"].iter().any(|p| Path::new(p).exists())
}

/// Install the rule and re-apply it to plugged-in devices (asks for the password
/// through polkit). Err holds what went wrong, or "cancelled".
pub fn install() -> Result<(), String> {
    let script = format!("cat > {RULE_PATH} && udevadm control --reload-rules && udevadm trigger --subsystem-match=hidraw");
    let mut c = Command::new("pkexec");
    c.args(["sh", "-c", &script]).stdin(Stdio::piped()).stdout(Stdio::null()).stderr(Stdio::piped());
    crate::hostenv::clean(&mut c);
    let mut child = c.spawn().map_err(|e| format!("pkexec: {e}"))?;
    child.stdin.take().ok_or("no stdin")?.write_all(RULE.as_bytes()).map_err(|e| e.to_string())?;
    let out = child.wait_with_output().map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        let err = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(if err.is_empty() { "cancelled".into() } else { err })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn rule_covers_both_modes() {
        assert!(super::RULE.contains("\"0001\"") && super::RULE.contains("\"0009\""));
        assert!(super::RULE.contains("uaccess"));
    }
}
