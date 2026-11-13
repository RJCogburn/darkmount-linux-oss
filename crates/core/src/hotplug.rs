//! Device plug/unplug notifications from the kernel (netlink uevents).
//!
//! One socket for the whole app. The callback runs on the listener thread about
//! 0.8 s after a burst of relevant events (udev needs that long to give the user
//! access to new nodes), and once more 2.5 s later for permissions that arrive late.

use std::io;
use std::mem;
use std::os::fd::{AsRawFd, FromRawFd, OwnedFd};
use std::thread;
use std::time::{Duration, Instant};

const NETLINK_KOBJECT_UEVENT: libc::c_int = 15;
const SUBSYSTEMS: &[&[u8]] = &[b"usb", b"input", b"hidraw", b"video4linux", b"sound", b"hid"];
const QUICK: Duration = Duration::from_millis(800);
const LATE: Duration = Duration::from_millis(2500);

/// Is this uevent an add/remove/bind/unbind in a subsystem we care about?
pub fn relevant(msg: &[u8]) -> bool {
    let action = [&b"add@"[..], b"remove@", b"bind@", b"unbind@"];
    if !action.iter().any(|a| msg.starts_with(a)) {
        return false;
    }
    msg.split(|&b| b == 0).any(|field| {
        field.strip_prefix(b"SUBSYSTEM=").is_some_and(|s| SUBSYSTEMS.contains(&s))
    })
}

fn open() -> io::Result<OwnedFd> {
    // SAFETY: plain socket/bind calls with a zeroed, correctly sized sockaddr_nl.
    unsafe {
        let fd = libc::socket(libc::AF_NETLINK, libc::SOCK_DGRAM | libc::SOCK_CLOEXEC,
                              NETLINK_KOBJECT_UEVENT);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        let fd = OwnedFd::from_raw_fd(fd);
        let mut addr: libc::sockaddr_nl = mem::zeroed();
        addr.nl_family = libc::AF_NETLINK as u16;
        addr.nl_groups = 1; // kernel events
        let r = libc::bind(fd.as_raw_fd(), &addr as *const _ as *const libc::sockaddr,
                           mem::size_of::<libc::sockaddr_nl>() as u32);
        if r < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(fd)
    }
}

/// Wait up to `timeout` for one datagram; None on timeout.
fn recv(fd: &OwnedFd, buf: &mut [u8], timeout: Option<Duration>) -> io::Result<Option<usize>> {
    let ms = timeout.map_or(-1, |t| t.as_millis().min(i32::MAX as u128) as i32);
    let mut pfd = libc::pollfd { fd: fd.as_raw_fd(), events: libc::POLLIN, revents: 0 };
    // SAFETY: one valid pollfd; recv into a buffer of the given length.
    unsafe {
        let r = libc::poll(&mut pfd, 1, ms);
        if r < 0 {
            let e = io::Error::last_os_error();
            return if e.kind() == io::ErrorKind::Interrupted { Ok(None) } else { Err(e) };
        }
        if r == 0 {
            return Ok(None);
        }
        let n = libc::recv(fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len(), 0);
        if n < 0 { Err(io::Error::last_os_error()) } else { Ok(Some(n as usize)) }
    }
}

/// Start the listener thread. Err if netlink isn't available (then callers keep
/// polling as before).
pub fn watch(on_change: impl Fn() + Send + 'static) -> io::Result<()> {
    let fd = open()?;
    thread::Builder::new().name("hotplug".into()).spawn(move || {
        let mut buf = vec![0u8; 16384];
        let mut quick: Option<Instant> = None;
        let mut late: Option<Instant> = None;
        loop {
            let next = [quick, late].into_iter().flatten().min();
            let timeout = next.map(|t| t.saturating_duration_since(Instant::now()));
            match recv(&fd, &mut buf, timeout) {
                Ok(Some(n)) => {
                    if relevant(&buf[..n]) {
                        quick = Some(Instant::now() + QUICK);
                    }
                }
                Ok(None) => {}
                Err(_) => thread::sleep(Duration::from_secs(1)),
            }
            let now = Instant::now();
            if quick.is_some_and(|t| t <= now) {
                quick = None;
                late = Some(now + LATE);
                on_change();
            } else if late.is_some_and(|t| t <= now) {
                late = None;
                on_change();
            }
        }
    })?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::relevant;

    #[test]
    fn filters_events() {
        assert!(relevant(b"add@/devices/x\0ACTION=add\0SUBSYSTEM=hidraw\0"));
        assert!(relevant(b"remove@/devices/x\0SUBSYSTEM=usb\0"));
        assert!(!relevant(b"change@/devices/x\0SUBSYSTEM=usb\0"));
        assert!(!relevant(b"add@/devices/x\0SUBSYSTEM=net\0"));
        assert!(!relevant(b"add@/devices/x\0SUBSYSTEM=usbmisc\0"));
    }
}
