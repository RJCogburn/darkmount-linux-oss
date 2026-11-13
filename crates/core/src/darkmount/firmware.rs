//! Firmware update (DFU feature) for the Dark Mount, following the official app's
//! sequence exactly:
//!
//! ```text
//! normal mode (PID 0x0001)
//!   DFU/Sync(upgrade_id, mcus)        -> keyboard reboots into its bootloader
//! bootloader (PID 0x0009)
//!   DFU/Sync(upgrade_id, mcus)        -> resume point per MCU (resumable!)
//!   for each MCU whose resume point is Transfer:
//!     DFU/Transfer(mcu, crc16, size)
//!       <- DataRequest(mcu, chunk, offset, length)   (repeated)
//!       -> DFU/WriteData(mcu, bytes)                 (chunk by chunk)
//!       <- TransferComplete(mcu)
//!     DFU/Validate(mcu)
//!       <- ValidationComplete(mcu)
//!   <- ResumePointChanged(mcu, Ready) for all  -> DFU/Ready
//!   <- ResumePointChanged(mcu, Commit) for all -> DFU/Commit
//!   <- CommitComplete                          -> keyboard reboots with new firmware
//! ```
//!
//! Images come from be quiet!'s update server (cached under
//! `~/.cache/darkmount/firmware/<version>/`) and are checked against the manifest's
//! SHA-512 "integrity" values before anything is written. Same cache as the Python app.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use base64::Engine as _;
use serde_json::Value;
use sha2::{Digest, Sha512};

use super::device::{lighting_mode, DarkMount, MediaDockConfig};
use super::qlink::{self, crc16_modbus, feature, ClientType, HidrawNode, QLinkDevice};

pub const PID_NORMAL: u16 = 0x0001;
pub const PID_BOOTLOADER: u16 = 0x0009;
pub const BASE_URL: &str = "https://dfu-release.bequiet.com/fw/dark_mount";
/// DFUState.UpgradeID + 1 in the official app
const UPGRADE_ID: u32 = 5648 + 1;

mod cmd {
    pub const SYNC: u8 = 1;
    pub const TRANSFER: u8 = 2;
    pub const WRITE_DATA: u8 = 3;
    pub const VALIDATE: u8 = 4;
    pub const READY: u8 = 5;
    pub const COMMIT: u8 = 6;
}

mod notify {
    pub const ERROR_OCCURRED: u8 = 1;
    pub const DATA_REQUEST: u8 = 2;
    pub const TRANSFER_COMPLETE: u8 = 3;
    pub const VALIDATION_COMPLETE: u8 = 4;
    pub const READY_COMPLETE: u8 = 5;
    pub const COMMIT_COMPLETE: u8 = 6;
    pub const ABORTED: u8 = 7;
    pub const RESUME_POINT_CHANGED: u8 = 8;
}

mod rp {
    pub const TRANSFER: u8 = 0;
    pub const VALIDATE: u8 = 1;
    pub const READY: u8 = 2;
    pub const COMMIT: u8 = 3;
    pub const WRITE_DATA: u8 = 4;
    pub fn name(r: u8) -> &'static str {
        ["Transfer", "Validate", "Ready", "Commit", "WriteData"].get(r as usize).copied()
            .unwrap_or("?")
    }
}

pub fn mcu_label(mcu: u8) -> &'static str {
    match mcu {
        0 => "Keyboard",
        1 => "Media Dock",
        2 => "Numpad",
        _ => "MCU",
    }
}

fn mcu_index(name: &str) -> Option<u8> {
    match name {
        "Main" => Some(0),
        "MediaModule" => Some(1),
        "NumpadModule" => Some(2),
        _ => None,
    }
}

pub type Error = String;

pub fn cache_dir() -> PathBuf {
    let base = std::env::var_os("XDG_CACHE_HOME").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache"));
    base.join("darkmount/firmware")
}

/// "1.31.0" -> (1, 31, 0)
pub fn version_key(v: &str) -> (u32, u32, u32) {
    let mut it = v.split('.').map(|x| x.parse::<u32>().unwrap_or(0));
    (it.next().unwrap_or(0), it.next().unwrap_or(0), it.next().unwrap_or(0))
}

// ------------------------------------------------------------------------- packages
#[derive(Debug, Clone)]
pub struct Image {
    pub mcu: u8,
    pub name: String,
    pub version: String,
    pub source: String,
    pub integrity: String,
    pub data: Option<Vec<u8>>,
}

impl Image {
    pub fn verify(&self) -> bool {
        self.data.as_ref().is_some_and(|d| {
            base64::engine::general_purpose::STANDARD.encode(Sha512::digest(d)) == self.integrity
        })
    }
}

#[derive(Debug, Clone)]
pub struct Package {
    pub version: String,
    pub images: BTreeMap<u8, Image>,
    pub folder: PathBuf,
    pub release_date: String,
    pub notes: Vec<String>,
}

/// The newest firmware set in a manifest (devices + devices_v2, as the official app).
pub fn select_package(manifest: &Value) -> Result<Vec<Value>, Error> {
    let first = |d: &Value| d["firmwares"].as_array().cloned().unwrap_or_default();
    let mut best = manifest["devices"].as_array().and_then(|a| a.first()).map(first)
        .ok_or("manifest has no devices")?;
    let ver = |fws: &[Value]| version_key(fws.first().and_then(|f| f["version"].as_str())
        .unwrap_or("0"));
    for cand in manifest["devices_v2"].as_array().into_iter().flatten() {
        let fws = first(cand);
        if !fws.is_empty() && ver(&fws) > ver(&best) {
            best = fws;
        }
    }
    if best.is_empty() {
        return Err("manifest has no firmware".into());
    }
    Ok(best)
}

fn package_from(manifest: &Value, folder: &Path) -> Result<Package, Error> {
    let fws = select_package(manifest)?;
    let mut images = BTreeMap::new();
    for (i, fw) in fws.iter().enumerate() {
        let name = fw["mcu"].as_str().unwrap_or("").to_string();
        let mcu = mcu_index(&name).unwrap_or(i as u8);
        let source = fw["source"].as_str().ok_or("firmware without source")?.to_string();
        let data = fs::read(folder.join(&source)).ok();
        images.insert(mcu, Image {
            mcu, name, version: fw["version"].as_str().unwrap_or("").to_string(), source,
            integrity: fw["integrity"].as_str().unwrap_or("").to_string(), data,
        });
    }
    let notes = fs::read_to_string(folder.join("patch-notes.json")).ok()
        .and_then(|t| serde_json::from_str::<Value>(&t).ok())
        .map(|v| v["notes"].as_array().into_iter().flatten()
            .flat_map(|item| item["content"]["en"].as_array().cloned().unwrap_or_default())
            .filter_map(|n| n.as_str().map(str::to_string)).collect())
        .unwrap_or_default();
    Ok(Package {
        version: fws[0]["version"].as_str().unwrap_or("").to_string(),
        images,
        folder: folder.to_path_buf(),
        release_date: fws[0]["releaseDate"].as_str().unwrap_or("").to_string(),
        notes,
    })
}

/// A firmware set from a folder (index.json + .bin files), every image verified.
pub fn load_package(folder: &Path) -> Result<Package, Error> {
    let text = fs::read_to_string(folder.join("index.json"))
        .map_err(|e| format!("{}: no readable index.json ({e})", folder.display()))?;
    let manifest: Value = serde_json::from_str(&text)
        .map_err(|e| format!("{}: bad index.json ({e})", folder.display()))?;
    let pkg = package_from(&manifest, folder)?;
    for img in pkg.images.values() {
        if img.data.is_none() {
            return Err(format!("{} is missing", folder.join(&img.source).display()));
        }
        if !img.verify() {
            return Err(format!("{}: SHA-512 does not match the manifest", img.source));
        }
    }
    Ok(pkg)
}

/// Downloaded, verified packages, newest first.
pub fn cached_packages() -> Vec<Package> {
    let Ok(rd) = fs::read_dir(cache_dir()) else { return Vec::new() };
    let mut dirs: Vec<PathBuf> = rd.flatten().map(|e| e.path())
        .filter(|p| p.file_name().and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(|c: char| c.is_ascii_digit())))
        .collect();
    dirs.sort_by_key(|p| std::cmp::Reverse(version_key(&p.file_name().unwrap().to_string_lossy())));
    dirs.iter().filter_map(|d| load_package(d).ok()).collect()
}

fn http_get(url: &str, timeout_s: u32) -> Result<Vec<u8>, Error> {
    let out = Command::new("curl")
        .args(["-fsSL", "--max-time", &timeout_s.to_string(), "-A", "darkmount-linux", url])
        .output()
        .map_err(|e| format!("curl: {e}"))?;
    if !out.status.success() {
        return Err(format!("download failed: {}", String::from_utf8_lossy(&out.stderr).trim()));
    }
    Ok(out.stdout)
}

/// Fetch the newest firmware into the cache (needs internet once).
pub fn download_latest(progress: &dyn Fn(&str)) -> Result<Package, Error> {
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let manifest: Value = serde_json::from_slice(&http_get(&format!("{BASE_URL}/index.json?v={t}"), 30)?)
        .map_err(|e| format!("bad manifest: {e}"))?;
    let fws = select_package(&manifest)?;
    let version = fws[0]["version"].as_str().unwrap_or("0").to_string();
    let folder = cache_dir().join(&version);
    fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    fs::write(folder.join("index.json"), serde_json::to_string_pretty(&manifest).unwrap_or_default())
        .map_err(|e| e.to_string())?;
    if let Ok(n) = http_get(&format!("{BASE_URL}/patch-notes.json"), 30) {
        let _ = fs::write(folder.join("patch-notes.json"), n);
    }
    for fw in &fws {
        let src = fw["source"].as_str().ok_or("firmware without source")?;
        let path = folder.join(src);
        if !path.exists() {
            progress(&format!("downloading {src}"));
            fs::write(&path, http_get(&format!("{BASE_URL}/{src}"), 120)?).map_err(|e| e.to_string())?;
        }
    }
    let pkg = package_from(&manifest, &folder)?;
    for img in pkg.images.values() {
        if !img.verify() {
            let _ = fs::remove_file(folder.join(&img.source));
            return Err(format!("{}: checksum mismatch after download", img.source));
        }
    }
    Ok(pkg)
}

// -------------------------------------------------------------------------- backups
/// On-keyboard settings, saved before an update (it resets them) and restored after.
/// Same JSON as the Python app (`~/.local/share/darkmount/backups/*.json`).
pub fn backups_dir() -> PathBuf {
    crate::paths::data_dir().join("backups")
}

fn hex(b: &[u8]) -> String {
    b.iter().map(|x| format!("{x:02x}")).collect()
}

fn unhex(s: &str) -> Option<Vec<u8>> {
    (0..s.len()).step_by(2).map(|i| s.get(i..i + 2).and_then(|h| u8::from_str_radix(h, 16).ok()))
        .collect()
}

pub fn snapshot(kb: &DarkMount) -> Result<Value, Error> {
    let e = |x: qlink::Error| x.to_string();
    let info = kb.device_info().map_err(e)?;
    let mut snap = serde_json::json!({
        "time": crate::darkmount::firmware::now_text(),
        "firmware": info.firmware.first().map(|f| f.1.clone()).unwrap_or_else(|| "?".into()),
        "bindings": kb.bindings().map_err(e)?.iter().map(|b| hex(&b.data)).collect::<Vec<_>>(),
        "lighting_mode": kb.lighting_mode().map_err(e)?,
        "lighting": hex(&kb.lighting(0).map_err(e)?.encode(0)),
        "game_flags": kb.game_mode_flags().map_err(e)?,
    });
    if kb.media_dock_state().map_err(e)?.0 {
        snap["dock"] = Value::String(hex(&kb.media_dock_config().map_err(e)?.encode()));
    }
    Ok(snap)
}

pub fn now_text() -> String {
    let out = Command::new("date").arg("+%Y-%m-%d %H:%M:%S").output();
    out.ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

pub fn save_snapshot(snap: &Value) -> Result<PathBuf, Error> {
    let dir = backups_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let stamp = Command::new("date").arg("+%Y%m%d-%H%M%S").output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default();
    let fw = snap["firmware"].as_str().unwrap_or("?");
    let p = dir.join(format!("{stamp}-fw{fw}.json"));
    fs::write(&p, serde_json::to_string_pretty(snap).unwrap_or_default()).map_err(|e| e.to_string())?;
    Ok(p)
}

pub fn latest_snapshot() -> Option<Value> {
    let mut files: Vec<PathBuf> = fs::read_dir(backups_dir()).ok()?.flatten().map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "json")).collect();
    files.sort();
    serde_json::from_str(&fs::read_to_string(files.last()?).ok()?).ok()
}

pub fn restore_snapshot(kb: &mut DarkMount, snap: &Value, report: &dyn Fn(&str)) -> Result<(), Error> {
    let e = |x: qlink::Error| x.to_string();
    let mut n = 0;
    for rec in snap["bindings"].as_array().into_iter().flatten() {
        if let Some(r) = rec.as_str().and_then(unhex) {
            kb.set_binding(&r).map_err(e)?;
            n += 1;
        }
    }
    report(&format!("restored {n} key bindings"));
    if let Some(raw) = snap["lighting"].as_str().and_then(unhex) {
        let mode = snap["lighting_mode"].as_u64().unwrap_or(1) as u8;
        if mode == lighting_mode::OFF {
            kb.set_lighting_mode(lighting_mode::OFF).map_err(e)?;
        } else {
            kb.set_lighting_mode(lighting_mode::GENERAL).map_err(e)?;
            // SetLayerConfig with the saved record (it already starts with the zone)
            kb.dev.request(feature::LIGHTINGS, 6, &raw, Duration::from_secs(1), None).map_err(e)?;
        }
        report("restored lighting");
    }
    if let Some(f) = snap["game_flags"].as_u64() {
        kb.set_game_mode_flags(f as u8).map_err(e)?;
        report("restored game mode keys");
    }
    if let Some(d) = snap["dock"].as_str().and_then(unhex) {
        if kb.media_dock_state().map_err(e)?.0 {
            kb.set_media_dock_config(&MediaDockConfig::parse(&d)).map_err(e)?;
            report("restored media dock settings");
        }
    }
    Ok(())
}

// -------------------------------------------------------------------------- updater
pub fn find(pid: u16) -> Option<HidrawNode> {
    qlink::list_nodes(qlink::VID_BEQUIET).into_iter()
        .find(|n| n.pid == pid && n.usage_page == Some(qlink::VENDOR_USAGE_PAGE))
}

fn wait_for(pid: u16, timeout: Duration) -> Option<HidrawNode> {
    let end = Instant::now() + timeout;
    while Instant::now() < end {
        if let Some(n) = find(pid) {
            // udev may not have applied permissions yet
            if fs::OpenOptions::new().read(true).write(true).open(&n.path).is_ok() {
                return Some(n);
            }
        }
        thread::sleep(Duration::from_millis(300));
    }
    None
}

fn systemctl(args: &[&str]) -> String {
    Command::new("systemctl").arg("--user").args(args).output().ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string()).unwrap_or_default()
}

/// `report(stage, percent, message)` gets progress: stages prepare, transfer,
/// validate, install, restart, done.
pub type Report<'a> = &'a dyn Fn(&str, f32, &str);

/// Back up settings, update, restore them (updates reset the keyboard to factory).
/// `release` closes any session the caller holds (another session would keep the
/// keyboard from accepting update commands).
pub fn update_with_backup(pkg: &Package, report: Report, restore: bool) -> Result<(), Error> {
    let mut snap = None;
    if find(PID_NORMAL).is_some() {
        match DarkMount::open(ClientType::Web, false).and_then(|kb| {
            snapshot(&kb).map_err(qlink::Error::Other)
        }) {
            Ok(s) => {
                let p = save_snapshot(&s)?;
                report("prepare", 0.0, &format!("settings backed up to {}", p.display()));
                snap = Some(s);
            }
            Err(e) => report("prepare", 0.0, &format!("warning: couldn't back up settings ({e})")),
        }
    } else if restore {
        snap = latest_snapshot();
        if let Some(s) = &snap {
            report("prepare", 0.0, &format!("will restore settings saved {}",
                                             s["time"].as_str().unwrap_or("?")));
        }
    }
    Updater::new(pkg.clone(), vec![0, 1, 2], report).run()?;
    if let (true, Some(s)) = (restore, snap) {
        thread::sleep(Duration::from_secs(1));
        let done = |m: &str| report("done", 100.0, m);
        match DarkMount::open(ClientType::Web, false) {
            Ok(mut kb) => {
                if let Err(e) = restore_snapshot(&mut kb, &s, &done) {
                    report("done", 100.0, &format!("warning: couldn't restore settings ({e})"));
                }
            }
            Err(e) => report("done", 100.0, &format!("warning: couldn't restore settings ({e})")),
        }
    }
    Ok(())
}

pub struct Updater<'a> {
    pkg: Package,
    mcus: Vec<u8>,
    report: Report<'a>,
    dev: Option<QLinkDevice>,
    rp: Vec<u8>,
    written: BTreeMap<u8, usize>,
    active: Option<u8>,
    started: Vec<u8>,
}

impl<'a> Updater<'a> {
    pub fn new(pkg: Package, mut mcus: Vec<u8>, report: Report<'a>) -> Updater<'a> {
        mcus.sort_unstable();
        Updater { pkg, mcus, report, dev: None, rp: Vec::new(), written: BTreeMap::new(),
                  active: None, started: Vec::new() }
    }

    fn dev(&self) -> Result<&QLinkDevice, Error> {
        self.dev.as_ref().ok_or_else(|| "not connected".into())
    }

    fn req(&self, c: u8, payload: &[u8], timeout: Duration) -> Result<Vec<u8>, qlink::Error> {
        let d = self.dev.as_ref().ok_or_else(|| qlink::Error::Other("not connected".into()))?;
        Ok(d.request(feature::DFU, c, payload, timeout, None)?.data)
    }

    fn open(&mut self, node: HidrawNode, keepalive: bool) -> Result<(), Error> {
        let d = QLinkDevice::open(node, ClientType::Web, false).map_err(|e| e.to_string())?;
        d.open_session(keepalive).map_err(|e| e.to_string())?;
        self.dev = Some(d);
        Ok(())
    }

    fn close(&mut self) {
        if let Some(d) = self.dev.take() {
            d.close();
        }
    }

    fn image(&self, mcu: u8) -> &[u8] {
        self.pkg.images.get(&mcu).and_then(|i| i.data.as_deref()).unwrap_or(&[])
    }

    fn sync(&self) -> Result<Vec<u8>, qlink::Error> {
        let mut payload = UPGRADE_ID.to_le_bytes().to_vec();
        payload.push(self.mcus.len() as u8);
        payload.extend(&self.mcus);
        let d = self.req(cmd::SYNC, &payload, Duration::from_secs(5))?;
        let n = d.get(4).copied().unwrap_or(0) as usize;
        Ok(d.get(5..5 + n).unwrap_or(&[]).to_vec())
    }

    fn progress(&self, msg: &str) {
        let total: usize = self.mcus.iter().map(|m| self.image(*m).len()).sum();
        let done: usize = self.mcus.iter().map(|m| self.written.get(m).copied().unwrap_or(0)).sum();
        (self.report)("transfer", 100.0 * done as f32 / total.max(1) as f32, msg);
    }

    fn start_next(&mut self) -> Result<bool, Error> {
        let next = (0..self.rp.len()).find(|&i| self.rp[i] == rp::TRANSFER
                                           && !self.started.contains(&self.mcus[i]));
        match next {
            Some(i) => {
                self.transfer(i)?;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    fn transfer(&mut self, idx: usize) -> Result<(), Error> {
        let mcu = self.mcus[idx];
        self.active = Some(mcu);
        self.started.push(mcu);
        let img = self.image(mcu).to_vec();
        let crc = crc16_modbus(&img);
        (self.report)("transfer", 0.0, &format!("sending {} firmware ({} bytes)", mcu_label(mcu),
                                                img.len()));
        let mut p = vec![mcu, (crc & 0xFF) as u8, (crc >> 8) as u8];
        p.extend((img.len() as u32).to_le_bytes());
        self.req(cmd::TRANSFER, &p, Duration::from_secs(3)).map_err(|e| e.to_string())?;
        Ok(())
    }

    fn write(&mut self, d: &[u8]) -> Result<(), Error> {
        if d.len() < 11 {
            return Err("short data request".into());
        }
        let mcu = d[0];
        let chunk = d[1] as usize | (d[2] as usize) << 8;
        let offset = u32::from_le_bytes([d[3], d[4], d[5], d[6]]) as usize;
        let length = u32::from_le_bytes([d[7], d[8], d[9], d[10]]) as usize;
        let idx = self.mcus.iter().position(|&m| m == mcu)
            .ok_or_else(|| format!("keyboard requested data for unexpected MCU {mcu}"))?;
        self.rp[idx] = rp::WRITE_DATA;
        if chunk == 0 {
            return Ok(());
        }
        let img = self.image(mcu).to_vec();
        let end = (offset + length).min(img.len());
        let mut p = offset;
        while p < end {
            let mut payload = vec![mcu];
            payload.extend(&img[p..(p + chunk).min(end)]);
            self.req(cmd::WRITE_DATA, &payload, Duration::from_secs(5)).map_err(|e| e.to_string())?;
            p += chunk;
            self.written.insert(mcu, p.min(img.len()));
            self.progress(&format!("{}: {}/{}", mcu_label(mcu), self.written[&mcu], img.len()));
            thread::sleep(Duration::from_millis(2));
        }
        Ok(())
    }

    /// Run the whole update (verifies images; pauses the helper service meanwhile).
    pub fn run(&mut self) -> Result<(), Error> {
        for img in self.pkg.images.values() {
            if !img.verify() {
                return Err(format!("{}: checksum mismatch", img.source));
            }
        }
        let service = systemctl(&["is-active", "darkmount-clock.service"]) == "active";
        if service {
            (self.report)("prepare", 0.0, "pausing the darkmount background service");
            systemctl(&["stop", "darkmount-clock.service"]);
            thread::sleep(Duration::from_secs(1));
        }
        let r = self.run_inner();
        self.close();
        if service {
            systemctl(&["start", "darkmount-clock.service"]);
        }
        r
    }

    fn run_inner(&mut self) -> Result<(), Error> {
        let mut node = find(PID_NORMAL).or_else(|| find(PID_BOOTLOADER)).ok_or("keyboard not found")?;
        if let Some(m) = self.mcus.iter().find(|m| !self.pkg.images.contains_key(m)) {
            return Err(format!("package has no image for MCU {m}"));
        }
        if node.pid == PID_NORMAL {
            (self.report)("prepare", 0.0, "asking the keyboard to enter update mode");
            self.open(node, true)?;
            // the keyboard usually reboots into its bootloader right away, without
            // answering; only a real status error means it refused
            if let Err(e @ qlink::Error::Status { .. }) = self.sync() {
                return Err(format!("keyboard refused update mode: {e}"));
            }
            self.dev = None; // drop without CloseSession: the device is gone
            (self.report)("prepare", 0.0, "waiting for the bootloader");
            node = wait_for(PID_BOOTLOADER, Duration::from_secs(30)).ok_or(
                "keyboard did not come back in bootloader mode within 30 s (it is still running \
                 its old firmware)")?;
            thread::sleep(Duration::from_millis(500));
        } else {
            (self.report)("prepare", 0.0, "keyboard is already in bootloader mode — resuming");
        }

        self.open(node, false)?;
        self.rp = self.sync().map_err(|e| e.to_string())?;
        if self.rp.len() != self.mcus.len() {
            return Err(format!("bootloader reports {} MCUs, expected {}", self.rp.len(),
                               self.mcus.len()));
        }
        let points: Vec<String> = self.mcus.iter().zip(&self.rp)
            .map(|(m, r)| format!("{}={}", mcu_label(*m), rp::name(*r))).collect();
        (self.report)("transfer", 0.0, &format!("resume points: {}", points.join(", ")));
        // mirrors DFU_Run: act on the first MCU's resume point
        match self.rp[0] {
            rp::TRANSFER => self.transfer(0)?,
            rp::VALIDATE => {
                let m = self.mcus[0];
                self.req(cmd::VALIDATE, &[m], Duration::from_secs(3)).map_err(|e| e.to_string())?;
            }
            rp::READY => {
                self.req(cmd::READY, &[], Duration::from_secs(3)).map_err(|e| e.to_string())?;
            }
            rp::COMMIT => {
                let _ = self.req(cmd::COMMIT, &[], Duration::from_secs(3));
                self.rp.clear(); // nothing else to do; wait for the restart below
            }
            _ => {}
        }

        let mut last_event = Instant::now();
        while !self.rp.is_empty() {
            let f = match self.dev()?.next_notification(Duration::from_secs(1)) {
                Ok(f) => f,
                Err(_) => {
                    // device vanished: fine if it was installing (reboot)
                    if self.rp.iter().all(|r| *r == rp::READY || *r == rp::COMMIT) {
                        break;
                    }
                    return Err("keyboard disconnected during the update — run the update \
                                again to resume".into());
                }
            };
            let Some(f) = f else {
                let idle = last_event.elapsed();
                if idle > Duration::from_secs(5) && self.active.is_none() && self.start_next()? {
                    last_event = Instant::now();
                    continue;
                }
                if idle > Duration::from_secs(60) {
                    return Err("no response from the bootloader for 60 s".into());
                }
                continue;
            };
            last_event = Instant::now();
            if f.feature() != feature::DFU {
                continue;
            }
            let d = f.data().to_vec();
            let first = d.first().copied().unwrap_or(0);
            match f.command() {
                notify::DATA_REQUEST => self.write(&d)?,
                notify::TRANSFER_COMPLETE => {
                    self.active = None;
                    let last = self.mcus.last() == Some(&first);
                    (self.report)("validate", if last { 100.0 } else { 0.0 },
                                  &format!("validating {}", mcu_label(first)));
                    self.req(cmd::VALIDATE, &[first], Duration::from_secs(3))
                        .map_err(|e| e.to_string())?;
                }
                notify::VALIDATION_COMPLETE => {
                    // official app: only move on if this MCU isn't still marked WriteData
                    let idx = self.mcus.iter().position(|&m| m == first).unwrap_or(0);
                    if self.rp[idx] != rp::WRITE_DATA {
                        self.start_next()?;
                    }
                }
                notify::RESUME_POINT_CHANGED => {
                    let Some(idx) = self.mcus.iter().position(|&m| m == first) else { continue };
                    let new = d.get(1).copied().unwrap_or(0);
                    if self.rp[idx] != new {
                        self.rp[idx] = new;
                        if self.rp.iter().all(|r| *r == rp::READY) {
                            (self.report)("install", 100.0, "all images validated — installing");
                            self.req(cmd::READY, &[], Duration::from_secs(3))
                                .map_err(|e| e.to_string())?;
                        } else if self.rp.iter().all(|r| *r == rp::COMMIT) {
                            (self.report)("install", 100.0, "committing");
                            // the keyboard reboots immediately, often without replying
                            let _ = self.req(cmd::COMMIT, &[], Duration::from_secs(3));
                            break;
                        }
                    }
                }
                notify::COMMIT_COMPLETE => break,
                notify::READY_COMPLETE => {}
                notify::ABORTED => return Err("the bootloader aborted the update".into()),
                notify::ERROR_OCCURRED => {
                    let name = ["", "InvalidState", "InvalidLength", "WriteFailed", "LoaderError",
                                "ValidationFailed", "ForceSync"].get(first as usize).copied()
                        .unwrap_or("unknown");
                    return Err(format!("bootloader error: {name}"));
                }
                _ => {}
            }
        }
        self.close();
        (self.report)("restart", 100.0, "keyboard is restarting");
        wait_for(PID_NORMAL, Duration::from_secs(60))
            .ok_or("keyboard did not restart in normal mode within 60 s")?;
        thread::sleep(Duration::from_millis(1500));
        (self.report)("done", 100.0, "update complete");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions() {
        assert!(version_key("1.31.0") > version_key("1.19.0"));
        assert!(version_key("1.9.2") < version_key("1.12.0"));
    }

    #[test]
    fn picks_the_newest_set() {
        let m = serde_json::json!({
            "devices": [{"firmwares": [{"mcu": "Main", "version": "1.12.0", "source": "a.bin",
                                        "integrity": "x"}]}],
            "devices_v2": [{"firmwares": [{"mcu": "Main", "version": "1.31.0", "source": "b.bin",
                                           "integrity": "y"}]}],
        });
        assert_eq!(select_package(&m).unwrap()[0]["source"], "b.bin");
    }

    #[test]
    fn verifies_the_downloaded_package() {
        // the package the Python app downloaded on this machine, if present
        let dir = cache_dir().join("1.31.0");
        if !dir.join("index.json").exists() {
            return;
        }
        let pkg = load_package(&dir).expect("verifies");
        assert_eq!(pkg.version, "1.31.0");
        assert_eq!(pkg.images.keys().copied().collect::<Vec<_>>(), vec![0, 1, 2]);
        let mut broken = pkg.images[&0].clone();
        broken.data.as_mut().unwrap()[100] ^= 1;
        assert!(!broken.verify());
    }

    #[test]
    fn hex_round_trip() {
        assert_eq!(unhex(&hex(&[0, 1, 0xfe])).unwrap(), vec![0, 1, 0xfe]);
        assert!(unhex("zz").is_none());
    }
}
