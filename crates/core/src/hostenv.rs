//! Programs we start should get the desktop's environment, not ours: inside an
//! AppImage the runtime's library path and similar variables would break them.

use std::process::Command;

const APPIMAGE_VARS: &[&str] = &["APPDIR", "APPIMAGE", "ARGV0", "OWD", "LD_LIBRARY_PATH",
                                 "LD_PRELOAD", "PYTHONPATH", "PYTHONHOME", "QT_PLUGIN_PATH",
                                 "GIO_MODULE_DIR", "GDK_PIXBUF_MODULE_FILE"];

/// Remove AppImage-runtime variables from a command's environment (only when this
/// process runs from an AppImage).
pub fn clean(cmd: &mut Command) {
    if std::env::var_os("APPIMAGE").is_some() || std::env::var_os("APPDIR").is_some() {
        for v in APPIMAGE_VARS {
            cmd.env_remove(v);
        }
    }
}
