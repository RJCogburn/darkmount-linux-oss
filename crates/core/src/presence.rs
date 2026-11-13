//! Is a Dark Mount plugged in? (sysfs USB ids; no device access needed.)

use std::fs;
use std::path::Path;

pub const VENDOR: u16 = 0x373F;
/// Normal mode, and the firmware-update bootloader.
pub const PRODUCTS: [u16; 2] = [0x0001, 0x0009];

fn read_hex(p: &Path) -> Option<u16> {
    u16::from_str_radix(fs::read_to_string(p).ok()?.trim(), 16).ok()
}

/// Is a Dark Mount among the USB devices under `root` (normally /sys/bus/usb/devices)?
pub fn present_in(root: &Path) -> bool {
    fs::read_dir(root).into_iter().flatten().flatten().any(|e| {
        let d = e.path();
        read_hex(&d.join("idVendor")) == Some(VENDOR)
            && read_hex(&d.join("idProduct")).is_some_and(|p| PRODUCTS.contains(&p))
    })
}

pub fn keyboard_present() -> bool {
    present_in(Path::new("/sys/bus/usb/devices"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn finds_the_keyboard() {
        let dir = tempfile::tempdir().unwrap();
        let dev = dir.path().join("1-2");
        std::fs::create_dir(&dev).unwrap();
        std::fs::write(dev.join("idVendor"), "373f\n").unwrap();
        std::fs::write(dev.join("idProduct"), "0009\n").unwrap();
        assert!(super::present_in(dir.path()));
        std::fs::write(dev.join("idProduct"), "0042\n").unwrap();
        assert!(!super::present_in(dir.path()));
    }
}
