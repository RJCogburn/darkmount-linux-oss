//! Single instance: the first copy listens on a socket in $XDG_RUNTIME_DIR; a second
//! copy connects, asks it to show its window, and exits.

use std::io::{self, Read, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;

use darkmount_core::paths;

fn socket_path() -> PathBuf {
    paths::runtime_dir().join("darkmount-linux.sock")
}

/// Ok(Some(listener)) when this is the first copy; Ok(None) when another copy was
/// asked to show itself.
pub fn claim() -> io::Result<Option<UnixListener>> {
    let path = socket_path();
    if let Ok(mut s) = UnixStream::connect(&path) {
        s.write_all(b"show\n")?;
        return Ok(None);
    }
    let _ = std::fs::remove_file(&path); // left over from a crash
    UnixListener::bind(&path).map(Some)
}

/// Call `on_show` whenever another copy starts.
pub fn serve(listener: UnixListener, on_show: impl Fn() + Send + 'static) {
    thread::Builder::new()
        .name("instance".into())
        .spawn(move || {
            for mut conn in listener.incoming().flatten() {
                let mut buf = [0u8; 16];
                if conn.read(&mut buf).is_ok_and(|n| buf[..n].starts_with(b"show")) {
                    on_show();
                }
            }
        })
        .expect("thread starts");
}
