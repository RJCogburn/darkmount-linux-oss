//! QLink transport for be quiet! devices over Linux hidraw.
//!
//! Reconstructed from the IO Center web app's protocol code. 64-byte frames:
//!
//! ```text
//! [0]  payload length (data length + 6)
//! [1]  sequence id (bit 7 = more frames follow)
//! [2]  session id
//! [3]  status
//! [4]  request id
//! [5]  feature id
//! [6]  command id
//! [7:] data
//! [62:64] CRC-16/Modbus over bytes 0..61, little-endian
//! ```
//!
//! Continuation frames (multi-frame transfers) only carry bytes 0..2 and put data
//! from byte 3 onwards.

use std::collections::VecDeque;
use std::fmt;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::os::fd::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError, Sender};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

pub const VID_BEQUIET: u16 = 0x373F;
pub const PACKET_SIZE: usize = 64;
pub const VENDOR_USAGE_PAGE: u32 = 0xFF00;

/// Feature ids.
pub mod feature {
    pub const ROOT: u8 = 1;
    pub const DFU: u8 = 2;
    pub const DEVICE_INFO: u8 = 3;
    pub const STORAGE: u8 = 4;
    pub const HUB: u8 = 5;
    pub const USB_DEVICE: u8 = 6;
    pub const KEYBOARD: u8 = 7;
    pub const MOUSE: u8 = 8;
    pub const BATTERY: u8 = 9;
    pub const PAIRING: u8 = 10;
    pub const LIGHTINGS: u8 = 16;
    pub const BINDINGS: u8 = 17;
    pub const MACROS: u8 = 18;
    pub const NUMPAD_MODULE: u8 = 32;
    pub const MEDIA_DOCK_MODULE: u8 = 33;

    pub fn name(id: u8) -> String {
        match id {
            ROOT => "ROOT", DFU => "DFU", DEVICE_INFO => "DEVICE_INFO", STORAGE => "STORAGE",
            HUB => "HUB", USB_DEVICE => "USB_DEVICE", KEYBOARD => "KEYBOARD", MOUSE => "MOUSE",
            BATTERY => "BATTERY", PAIRING => "PAIRING", LIGHTINGS => "LIGHTINGS",
            BINDINGS => "BINDINGS", MACROS => "MACROS", NUMPAD_MODULE => "NUMPAD_MODULE",
            MEDIA_DOCK_MODULE => "MEDIA_DOCK_MODULE",
            _ => return format!("0x{id:02x}"),
        }
        .to_string()
    }
}

/// Root feature commands.
pub mod root {
    pub const OPEN_SESSION: u8 = 1;
    pub const CLOSE_SESSION: u8 = 2;
    pub const KEEP_ALIVE: u8 = 3;
    pub const GET_SUPPORTED_FEATURES: u8 = 4;
    pub const GET_QLINK_VERSION: u8 = 5;
}

/// Reply status codes.
pub mod status {
    pub const SUCCESS: u8 = 0;
    pub const NOT_ACTIVE: u8 = 9;

    pub fn name(s: u8) -> String {
        const NAMES: [&str; 14] = ["SUCCESS", "INVALID_SESSION_ID", "INVALID_FEATURE_ID",
            "INVALID_COMMAND_ID", "INVALID_REQUEST_ID", "INVALID_PARAMETER", "TIMEOUT", "BUSY",
            "NOT_AUTHORIZED", "NOT_ACTIVE", "INVALID_STATE", "INVALID_SIZE",
            "INSUFFICIENT_RESOURCES", "NOT_SUPPORTED"];
        NAMES.get(s as usize).map_or_else(|| s.to_string(), |n| n.to_string())
    }
}

/// Client types for OpenSession.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ClientType {
    Unspecified = 0,
    /// The Windows IO Center app.
    QControl = 1,
    Web = 2,
}

#[derive(Debug)]
pub enum Error {
    Io(io::Error),
    Timeout(String),
    /// The device answered with a status other than SUCCESS.
    Status { feature: u8, command: u8, status: u8 },
    Other(String),
}

impl Error {
    pub fn is_not_active(&self) -> bool {
        matches!(self, Error::Status { status: status::NOT_ACTIVE, .. })
    }
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::Io(e) => write!(f, "{e}"),
            Error::Timeout(what) => write!(f, "Timeout waiting for {what}"),
            Error::Status { feature: fe, command, status: st } => {
                write!(f, "{}:{command} failed: {}", feature::name(*fe), status::name(*st))
            }
            Error::Other(s) => f.write_str(s),
        }
    }
}

impl std::error::Error for Error {}

impl From<io::Error> for Error {
    fn from(e: io::Error) -> Self {
        Error::Io(e)
    }
}

pub type Result<T> = std::result::Result<T, Error>;

pub fn crc16_modbus(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= b as u16;
        for _ in 0..8 {
            crc = if crc & 1 != 0 { (crc >> 1) ^ 0xA001 } else { crc >> 1 };
        }
    }
    crc
}

/// One 64-byte frame as received.
#[derive(Clone, PartialEq, Eq)]
pub struct Frame(pub [u8; PACKET_SIZE]);

impl fmt::Debug for Frame {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Frame({})", hex(&self.0))
    }
}

impl Frame {
    pub fn from_bytes(b: &[u8]) -> Frame {
        let mut raw = [0u8; PACKET_SIZE];
        let n = b.len().min(PACKET_SIZE);
        raw[..n].copy_from_slice(&b[..n]);
        Frame(raw)
    }
    pub fn length(&self) -> u8 { self.0[0] }
    pub fn sequence_id(&self) -> u8 { self.0[1] & 0x7F }
    pub fn more(&self) -> bool { self.0[1] >> 7 != 0 }
    pub fn session_id(&self) -> u8 { self.0[2] }
    pub fn status(&self) -> u8 { self.0[3] }
    pub fn request_id(&self) -> u8 { self.0[4] }
    pub fn feature(&self) -> u8 { self.0[5] }
    pub fn command(&self) -> u8 { self.0[6] }
    pub fn is_notification(&self) -> bool {
        self.request_id() == 0 && self.sequence_id() == 0
    }
    /// Mirrors the web app: first-frame data starts at 7, continuation data at 3.
    pub fn data(&self) -> &[u8] {
        let start = if self.sequence_id() > 0 { 3 } else { 7 };
        let end = (self.length() as usize + 1).min(PACKET_SIZE);
        if end <= start { &[] } else { &self.0[start..end] }
    }
    pub fn crc_ok(&self) -> bool {
        let got = u16::from_le_bytes([self.0[PACKET_SIZE - 2], self.0[PACKET_SIZE - 1]]);
        got == crc16_modbus(&self.0[..PACKET_SIZE - 2])
    }
}

/// A (reassembled) reply.
#[derive(Debug, Clone)]
pub struct Response {
    pub status: u8,
    pub feature: u8,
    pub command: u8,
    pub data: Vec<u8>,
}

/// Build the 64-byte frames for a request.
///
/// Mirrors the official app byte for byte, including its "more frames" flag: the
/// expected frame count is ceil(len / 55) even though continuation frames carry 59
/// bytes, so for some large payloads the last frame still has bit 7 set. The
/// firmware is tested against that behaviour.
pub fn build_frames(session: u8, request_id: u8, feature: u8, command: u8,
                    payload: &[u8]) -> Vec<[u8; PACKET_SIZE]> {
    let first_max = PACKET_SIZE - 9;
    let (chunks, total): (Vec<&[u8]>, usize) = if payload.len() <= first_max {
        (vec![payload], 1)
    } else {
        let mut chunks = vec![&payload[..first_max]];
        chunks.extend(payload[first_max..].chunks(first_max + 4));
        (chunks, payload.len().div_ceil(first_max))
    };
    chunks.iter().enumerate().map(|(idx, chunk)| {
        let mut p = [0u8; PACKET_SIZE];
        p[0] = ((chunk.len() + 6) & 0xFF) as u8;
        p[1] = if total > 1 {
            (idx as u8) | if total - 1 == idx { 0 } else { 0x80 }
        } else {
            0
        };
        p[2] = session;
        if idx == 0 {
            p[3] = status::SUCCESS;
            p[4] = request_id;
            p[5] = feature;
            p[6] = command;
            p[7..7 + chunk.len()].copy_from_slice(chunk);
        } else {
            p[3..3 + chunk.len()].copy_from_slice(chunk);
        }
        let crc = crc16_modbus(&p[..PACKET_SIZE - 2]);
        p[PACKET_SIZE - 2..].copy_from_slice(&crc.to_le_bytes());
        p
    }).collect()
}

pub fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" ")
}

// ------------------------------------------------------------------------------ nodes
/// A hidraw node of a be quiet! device.
#[derive(Debug, Clone)]
pub struct HidrawNode {
    pub path: PathBuf,
    pub vid: u16,
    pub pid: u16,
    pub name: String,
    pub usage_page: Option<u32>,
    pub uses_report_ids: bool,
}

/// (first usage page, uses report ids) from a HID report descriptor.
pub fn parse_descriptor(desc: &[u8]) -> (Option<u32>, bool) {
    let mut i = 0;
    let mut usage_page = None;
    let mut report_ids = false;
    while i < desc.len() {
        let prefix = desc[i];
        if prefix == 0xFE {
            // long item
            let n = desc.get(i + 1).copied().unwrap_or(0) as usize;
            i += 3 + n;
            continue;
        }
        let size = [0usize, 1, 2, 4][(prefix & 3) as usize];
        let tag = prefix & 0xFC;
        let mut val = 0u32;
        for (k, b) in desc.iter().skip(i + 1).take(size).enumerate() {
            val |= (*b as u32) << (8 * k);
        }
        if tag == 0x04 && usage_page.is_none() {
            usage_page = Some(val);
        } else if tag == 0x84 {
            report_ids = true;
        }
        i += 1 + size;
    }
    (usage_page, report_ids)
}

/// hidraw nodes of a vendor (default: be quiet!).
pub fn list_nodes(vid: u16) -> Vec<HidrawNode> {
    let mut out = Vec::new();
    let Ok(dir) = fs::read_dir("/sys/class/hidraw") else { return out };
    let mut entries: Vec<_> = dir.flatten().map(|e| e.path()).collect();
    entries.sort();
    for sys in entries {
        let Ok(uevent) = fs::read_to_string(sys.join("device/uevent")) else { continue };
        let prop = |k: &str| uevent.lines().find_map(|l| l.strip_prefix(k)).unwrap_or("");
        let parts: Vec<&str> = prop("HID_ID=").split(':').collect();
        let (Some(v), Some(p)) = (parts.get(1), parts.get(2)) else { continue };
        let (Ok(v), Ok(p)) = (u32::from_str_radix(v, 16), u32::from_str_radix(p, 16)) else {
            continue;
        };
        if v as u16 != vid {
            continue;
        }
        let (page, rids) = fs::read(sys.join("device/report_descriptor"))
            .map(|d| parse_descriptor(&d))
            .unwrap_or((None, false));
        let name = sys.file_name().unwrap_or_default();
        out.push(HidrawNode {
            path: Path::new("/dev").join(name),
            vid: v as u16,
            pid: p as u16,
            name: prop("HID_NAME=").to_string(),
            usage_page: page,
            uses_report_ids: rids,
        });
    }
    out
}

/// The QLink control node (vendor usage page 0xFF00).
pub fn find_control_node(pid: Option<u16>) -> Result<HidrawNode> {
    list_nodes(VID_BEQUIET)
        .into_iter()
        .find(|n| n.usage_page == Some(VENDOR_USAGE_PAGE) && pid.is_none_or(|p| n.pid == p))
        .ok_or_else(|| Error::Other(
            "No be quiet! QLink device found (vendor 0x373F, usage page 0xFF00). Is the \
             keyboard plugged in?".into()))
}

// ----------------------------------------------------------------------------- device
#[derive(Debug, Clone, Copy, Default)]
pub struct Session {
    pub session_id: u8,
    pub connection_id: u32,
    /// 1 = this is the active session (may write on firmware >= 1.31)
    pub state: u8,
    pub timeout: u8,
}

struct Io {
    file: File,
    req_id: u8,
    notifications: VecDeque<Frame>,
}

struct Inner {
    node: HidrawNode,
    client_type: ClientType,
    debug: bool,
    io: Mutex<Io>,
    session: Mutex<Session>,
    keepalive: Mutex<Option<(Sender<()>, JoinHandle<()>)>>,
    foreign_seen: Mutex<Option<Instant>>,
    closed: AtomicBool,
}

/// A QLink connection over a hidraw node. Cheap to clone (shared connection).
#[derive(Clone)]
pub struct QLinkDevice {
    inner: Arc<Inner>,
}

fn poll_read(fd: i32, timeout: Duration) -> io::Result<bool> {
    let mut pfd = libc::pollfd { fd, events: libc::POLLIN, revents: 0 };
    let ms = timeout.as_millis().min(i32::MAX as u128) as i32;
    // SAFETY: one valid pollfd for the duration of the call.
    let r = unsafe { libc::poll(&mut pfd, 1, ms) };
    if r < 0 {
        let e = io::Error::last_os_error();
        return if e.kind() == io::ErrorKind::Interrupted { Ok(false) } else { Err(e) };
    }
    Ok(r > 0)
}

impl QLinkDevice {
    pub fn open(node: HidrawNode, client_type: ClientType, debug: bool) -> Result<Self> {
        let file = OpenOptions::new().read(true).write(true).open(&node.path).map_err(|e| {
            if e.kind() == io::ErrorKind::PermissionDenied {
                Error::Other(format!("Permission denied opening {}. Install the udev rule \
                                      and replug the keyboard.", node.path.display()))
            } else {
                Error::Io(e)
            }
        })?;
        Ok(QLinkDevice {
            inner: Arc::new(Inner {
                node,
                client_type,
                debug,
                io: Mutex::new(Io { file, req_id: 1, notifications: VecDeque::new() }),
                session: Mutex::new(Session::default()),
                keepalive: Mutex::new(None),
                foreign_seen: Mutex::new(None),
                closed: AtomicBool::new(false),
            }),
        })
    }

    pub fn node(&self) -> &HidrawNode {
        &self.inner.node
    }

    pub fn session(&self) -> Session {
        *self.inner.session.lock().unwrap()
    }

    /// When traffic for another client's session was last seen.
    pub fn foreign_seen(&self) -> Option<Instant> {
        *self.inner.foreign_seen.lock().unwrap()
    }

    fn write(&self, io: &mut Io, frame: &[u8; PACKET_SIZE]) -> Result<()> {
        if self.inner.debug {
            eprintln!(">> {}", hex(frame));
        }
        let mut buf = Vec::with_capacity(PACKET_SIZE + 1);
        if !self.inner.node.uses_report_ids {
            buf.push(0);
        }
        buf.extend_from_slice(frame);
        io.file.write_all(&buf)?;
        Ok(())
    }

    fn read(&self, io: &mut Io, timeout: Duration) -> Result<Option<Frame>> {
        if !poll_read(io.file.as_raw_fd(), timeout)? {
            return Ok(None);
        }
        let mut buf = [0u8; 256];
        let n = io.file.read(&mut buf)?;
        let raw = if self.inner.node.uses_report_ids && n > 0 { &buf[1..n] } else { &buf[..n] };
        let f = Frame::from_bytes(raw);
        if self.inner.debug {
            eprintln!("<< {}", hex(&f.0));
        }
        if !f.is_notification() && f.session_id() != self.session().session_id {
            *self.inner.foreign_seen.lock().unwrap() = Some(Instant::now());
        }
        Ok(Some(f))
    }

    /// Send a request and return the reassembled reply.
    pub fn request(&self, feature: u8, command: u8, payload: &[u8], timeout: Duration,
                   session: Option<u8>) -> Result<Response> {
        let mut io = self.inner.io.lock().unwrap();
        let sess = session.unwrap_or_else(|| self.session().session_id);
        io.req_id = io.req_id.wrapping_add(1).max(1);
        let rid = io.req_id;
        for f in build_frames(sess, rid, feature, command, payload) {
            self.write(&mut io, &f)?;
        }
        let deadline = Instant::now() + timeout;
        let what = || format!("{}:{command}", feature::name(feature));
        let first = loop {
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(Error::Timeout(what()));
            }
            let Some(f) = self.read(&mut io, left)? else { continue };
            if f.is_notification() {
                // never a reply (notifications can share feature + command ids)
                io.notifications.push_back(f);
                continue;
            }
            if f.feature() == feature && f.command() == command {
                break f;
            }
        };
        let mut data = first.data().to_vec();
        let mut last = first.clone();
        while last.sequence_id() > 0 || last.more() {
            let left = deadline.saturating_duration_since(Instant::now())
                .max(Duration::from_millis(50));
            let Some(next) = self.read(&mut io, left)? else {
                return Err(Error::Timeout("continuation frame".into()));
            };
            data.extend_from_slice(next.data());
            let more = next.more();
            last = next;
            if !more {
                break;
            }
        }
        if first.status() != status::SUCCESS {
            return Err(Error::Status { feature, command, status: first.status() });
        }
        Ok(Response { status: first.status(), feature: first.feature(),
                      command: first.command(), data })
    }

    /// Wait up to `timeout` for an unsolicited frame. Reads in short slices so
    /// keep-alives and other requests can interleave.
    pub fn next_notification(&self, timeout: Duration) -> Result<Option<Frame>> {
        let deadline = Instant::now() + timeout;
        loop {
            let mut io = self.inner.io.lock().unwrap();
            if let Some(f) = io.notifications.pop_front() {
                return Ok(Some(f));
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Ok(None);
            }
            if let Some(f) = self.read(&mut io, left.min(Duration::from_millis(200)))? {
                if f.is_notification() {
                    return Ok(Some(f));
                }
            }
        }
    }

    // -- session ------------------------------------------------------------------
    pub fn open_session(&self, keepalive: bool) -> Result<Session> {
        let nonce = 10000 + SystemTime::now().duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.subsec_nanos()) % 90000;
        let mut payload = nonce.to_le_bytes().to_vec();
        payload.push(self.inner.client_type as u8);
        let r = self.request(feature::ROOT, root::OPEN_SESSION, &payload,
                             Duration::from_secs(1), Some(0))?;
        let d = &r.data;
        if d.len() < 5 {
            return Err(Error::Other("short OpenSession reply".into()));
        }
        let s = Session {
            connection_id: u32::from_le_bytes([d[0], d[1], d[2], d[3]]),
            session_id: d[4],
            state: d.get(5).copied().unwrap_or(0),
            timeout: d.get(6).copied().unwrap_or(0),
        };
        if !(1..255).contains(&s.session_id) {
            return Err(Error::Other(format!("Device refused session (id={})", s.session_id)));
        }
        *self.inner.session.lock().unwrap() = s;
        if s.timeout > 0 && keepalive {
            self.start_keepalive(s.timeout);
        }
        Ok(s)
    }

    pub fn close_session(&self) {
        self.stop_keepalive();
        let id = self.session().session_id;
        if id != 0 {
            let _ = self.request(feature::ROOT, root::CLOSE_SESSION, &[id],
                                 Duration::from_secs(1), None);
        }
        *self.inner.session.lock().unwrap() = Session::default();
    }

    /// Become the keyboard's active session (needed for writes on firmware >= 1.31):
    /// only the oldest open session may write, so re-open until ours is active.
    pub fn ensure_active(&self, timeout: Duration) -> bool {
        if self.session().state == 1 {
            return true;
        }
        let end = Instant::now() + timeout;
        while Instant::now() < end {
            self.close_session();
            thread::sleep(Duration::from_millis(400));
            if self.open_session(true).is_ok_and(|s| s.state == 1) {
                return true;
            }
        }
        false
    }

    fn start_keepalive(&self, timeout_s: u8) {
        self.stop_keepalive();
        let (tx, rx) = mpsc::channel::<()>();
        let dev = self.clone();
        let every = Duration::from_millis(timeout_s as u64 * 500);
        let handle = thread::spawn(move || {
            while let Err(RecvTimeoutError::Timeout) = rx.recv_timeout(every) {
                let _ = dev.request(feature::ROOT, root::KEEP_ALIVE, &[],
                                    Duration::from_secs(1), None);
            }
        });
        *self.inner.keepalive.lock().unwrap() = Some((tx, handle));
    }

    fn stop_keepalive(&self) {
        let taken = self.inner.keepalive.lock().unwrap().take();
        if let Some((tx, handle)) = taken {
            let _ = tx.send(());
            let _ = handle.join();
        }
    }

    /// Close the session (the file closes when the last clone is dropped).
    pub fn close(&self) {
        if !self.inner.closed.swap(true, Ordering::SeqCst) {
            self.close_session();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unhex(s: &str) -> Vec<u8> {
        (0..s.len()).step_by(2).map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap()).collect()
    }

    #[test]
    fn crc_check_value() {
        assert_eq!(crc16_modbus(b"123456789"), 0x4B37);
    }

    #[test]
    fn single_frame_matches_python() {
        let f = build_frames(5, 7, 16, 6, &(0..10).collect::<Vec<u8>>());
        assert_eq!(f.len(), 1);
        assert_eq!(f[0].to_vec(), unhex("10000500071006000102030405060708090000000000000000000000000000000000000000000000000000000000000000000000000000000000000000005065"));
        assert!(Frame(f[0]).crc_ok());
    }

    #[test]
    fn multi_frame_matches_python() {
        let big: Vec<u8> = (0..130).map(|i| i as u8).collect();
        let f = build_frames(9, 200, 17, 2, &big);
        let want = [
            "3d800900c81102000102030405060708090a0b0c0d0e0f101112131415161718191a1b1c1d1e1f202122232425262728292a2b2c2d2e2f30313233343536a586",
            "4181093738393a3b3c3d3e3f404142434445464748494a4b4c4d4e4f505152535455565758595a5b5c5d5e5f606162636465666768696a6b6c6d6e6f7071ad5b",
            "16020972737475767778797a7b7c7d7e7f8081000000000000000000000000000000000000000000000000000000000000000000000000000000000000009ee9",
        ];
        assert_eq!(f.len(), 3);
        for (got, want) in f.iter().zip(want) {
            assert_eq!(got.to_vec(), unhex(want));
        }
        assert!(f.iter().all(|x| Frame(*x).crc_ok()));
    }

    #[test]
    fn descriptor_parsing() {
        // Usage Page (Vendor 0xFF00), Usage, Collection, Report ID 5 ...
        let desc = [0x06, 0x00, 0xFF, 0x09, 0x01, 0xA1, 0x01, 0x85, 0x05, 0xC0];
        assert_eq!(parse_descriptor(&desc), (Some(0xFF00), true));
        assert_eq!(parse_descriptor(&[0x05, 0x01, 0x09, 0x06]), (Some(1), false));
    }
}
