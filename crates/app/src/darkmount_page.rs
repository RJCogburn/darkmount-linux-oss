//! Dark Mount page: keyboard connection (on a worker thread), the keyboard picture
//! with its lighting preview, and the Illumination panel.

use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::mpsc::{self, Sender};
use std::thread;
use std::time::{Duration, Instant};

use darkmount_core::darkmount::actions;
use darkmount_core::darkmount::bindings::{self as kb_bind, bind_type, Binding};
use darkmount_core::darkmount::device::{self, color_mode, effect, lighting_mode, LightingConfig};
use darkmount_core::darkmount::layout::{self, Layout, Mode, Side};
use darkmount_core::darkmount::preview::{Preview, PreviewSettings};
use darkmount_core::darkmount::qlink::ClientType;
use darkmount_core::darkmount::DarkMount;
use darkmount_core::paths;
use slint::{Color, ComponentHandle, Image, Model, ModelRc, SharedString, VecModel, Weak};

use crate::{FirmwarePart, Illumination, KeyBinding, KeyVisual, MainWindow, MaskVisual, MediaDock,
            PartVisual, ProductSettings};
use darkmount_core::darkmount::device::MediaDockConfig;
use darkmount_core::darkmount::firmware;

// ------------------------------------------------------------------------- worker
type Job = Box<dyn FnOnce(&mut Option<DarkMount>) + Send>;

/// Owns the keyboard session on its own thread; jobs run in order.
#[derive(Clone)]
pub struct KbWorker {
    tx: Sender<Job>,
}

impl KbWorker {
    pub fn new() -> KbWorker {
        let (tx, rx) = mpsc::channel::<Job>();
        thread::Builder::new()
            .name("darkmount".into())
            .spawn(move || {
                let mut kb: Option<DarkMount> = None;
                let mut idle = Idle::default();
                loop {
                    match rx.recv_timeout(Duration::from_millis(100)) {
                        Ok(job) => job(&mut kb),
                        Err(mpsc::RecvTimeoutError::Timeout) => {}
                        Err(mpsc::RecvTimeoutError::Disconnected) => break,
                    }
                    if let Some(k) = kb.as_mut() {
                        if idle.run(k).is_err() {
                            kb = None; // unplugged or gone wrong: reconnect on the next job
                        }
                    }
                }
            })
            .expect("thread starts");
        KbWorker { tx }
    }

    /// Run `f` with an open keyboard (connecting first if needed); `done` gets the
    /// result on the UI thread. A failed request drops the session so the next job
    /// reconnects.
    pub fn run<T: Send + 'static>(
        &self,
        f: impl FnOnce(&mut DarkMount) -> Result<T, String> + Send + 'static,
        done: impl FnOnce(Result<T, String>) + Send + 'static,
    ) {
        let job: Job = Box::new(move |kb: &mut Option<DarkMount>| {
            if kb.is_none() {
                // as IO Center: only that client type is told about key actions
                match DarkMount::open(ClientType::QControl, false) {
                    Ok(k) => *kb = Some(k),
                    Err(e) => {
                        let msg = e.to_string();
                        let _ = slint::invoke_from_event_loop(move || done(Err(msg)));
                        return;
                    }
                }
            }
            let r = f(kb.as_mut().expect("connected above"));
            if r.is_err() {
                *kb = None;
            }
            let _ = slint::invoke_from_event_loop(move || done(r));
        });
        let _ = self.tx.send(job);
    }

    /// Run `f` with the keyboard and wait for its result (for other threads, e.g. the
    /// tray menu). None if the keyboard isn't there or it took too long.
    pub fn call<T: Send + 'static>(&self, f: impl FnOnce(&mut DarkMount) -> Result<T, String>
                                   + Send + 'static) -> Option<T> {
        let (tx, rx) = mpsc::channel();
        let job: Job = Box::new(move |kb: &mut Option<DarkMount>| {
            if kb.is_none() {
                *kb = DarkMount::open(ClientType::QControl, false).ok();
            }
            let r = kb.as_mut().map(f);
            if matches!(r, Some(Err(_))) {
                *kb = None;
            }
            let _ = tx.send(r.and_then(Result::ok));
        });
        self.tx.send(job).ok()?;
        rx.recv_timeout(Duration::from_secs(8)).ok().flatten()
    }

    /// Close the session (e.g. the keyboard was unplugged).
    pub fn disconnect(&self) {
        let _ = self.tx.send(Box::new(|kb: &mut Option<DarkMount>| *kb = None));
    }
}

/// What the keyboard thread does between jobs (what the background service did while
/// no app was connected): run key actions and keep the dock clock right.
#[derive(Default)]
struct Idle {
    last_sync: Option<Instant>,
    last_wall: Option<std::time::SystemTime>,
}

const DOCK_SYNC_EVERY: Duration = Duration::from_secs(600);

impl Idle {
    fn run(&mut self, kb: &mut DarkMount) -> Result<(), darkmount_core::darkmount::qlink::Error> {
        use darkmount_core::darkmount::device::bindings_notify;
        use darkmount_core::darkmount::qlink::feature;
        // suspend/resume or a clock change: sync now
        let now = std::time::SystemTime::now();
        let jumped = self.last_wall.and_then(|w| now.duration_since(w).ok())
            .is_some_and(|d| d > Duration::from_secs(30));
        self.last_wall = Some(now);
        if jumped || self.last_sync.is_none_or(|t| t.elapsed() >= DOCK_SYNC_EVERY) {
            self.last_sync = Some(Instant::now());
            if kb.media_dock_state()?.0 {
                kb.sync_media_dock_time()?;
            }
        }
        while let Some(f) = kb.dev.next_notification(Duration::from_millis(20))? {
            if f.feature() != feature::BINDINGS || f.command() != bindings_notify::ACTION_FIRED {
                continue;
            }
            let Some(ev) = actions::parse_action_fired(f.data()) else { continue };
            let acts: BTreeMap<String, String> = actions::read_actions(&actions::config_path())
                .into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect();
            let launch = actions::decide(&ev, &acts);
            let label = actions::label(&kb_bind::key_name(ev.key), ev.layer);
            eprintln!("{label}: {launch:?}");
            if let actions::Launch::Missing(target) = &launch {
                let _ = std::process::Command::new("notify-send")
                    .args(["-a", "Dark Mount", "-i", "input-keyboard",
                           &format!("{label}: nothing to open"),
                           &format!("Bound to “{target}”, which doesn't exist here.")])
                    .spawn();
            } else if let Err(e) = actions::spawn(&launch) {
                eprintln!("{label}: couldn't start: {e}");
            }
        }
        Ok(())
    }
}

const SERVICE: &str = "darkmount-clock.service";

// --------------------------------------------------------------------- effect tables
const EFFECTS: [u8; 6] = [effect::STATIC, effect::COLOR_WAVE, effect::TORNADO, effect::BREATHING,
                          effect::REACTIVE, effect::MATRIX];
const PREVIEW_NAMES: [&str; 6] = ["Static", "ColorWave", "Tornado", "Breathing", "Reactive",
                                  "Matrix"];
/// Panel color modes: 0 single, 1 dual, 2 spectrum (gradient).
const MODE_TO_DEVICE: [u8; 3] = [color_mode::SINGLE, color_mode::DUAL, color_mode::GRADIENT];
const RAINBOW: [&str; 6] = ["#ff0000", "#ffc800", "#00ff00", "#00c8ff", "#0000ff", "#ff00ff"];

/// Color modes an effect offers, preferred first.
fn effect_modes(e: usize) -> &'static [usize] {
    match e {
        0 => &[0],
        1..=3 => &[2, 0, 1],
        4 => &[1, 0, 2],
        _ => &[2, 1], // Matrix
    }
}

/// (direction id, name) an effect offers.
fn effect_dirs(e: usize) -> &'static [(u8, &'static str)] {
    match e {
        1 => &[(2, "Left"), (3, "Right"), (0, "Up"), (1, "Down")],
        2 => &[(4, "Clockwise"), (5, "Counterclockwise")],
        5 => &[(1, "Down"), (0, "Up"), (2, "Left"), (3, "Right")],
        _ => &[],
    }
}

fn default_dir(e: usize) -> u8 {
    match e {
        1 => 3,
        2 => 4,
        5 => 1,
        _ => 1,
    }
}

/// A failed write, in words. NOT_ACTIVE: firmware 1.31 lets only the oldest open
/// session change settings, e.g. the previous app still running.
fn write_error(e: &str) -> String {
    if e.contains("NOT_ACTIVE") {
        "Another program (another copy of this app, or the background service started \
         from a different version?) is in charge of the keyboard — quit it to change settings here.".into()
    } else {
        format!("Couldn't change the setting: {e}")
    }
}

fn to_color(c: [u8; 3]) -> Color {
    Color::from_rgb_u8(c[0], c[1], c[2])
}

fn to_rgb(c: Color) -> [u8; 3] {
    [c.red(), c.green(), c.blue()]
}

fn hex_color(s: &str) -> Color {
    to_color(device::parse_color(s).unwrap_or([255, 40, 0]))
}

// ------------------------------------------------------------------------ the page
/// What the first read of the keyboard gives the page.
type Loaded = Result<(u8, LightingConfig, (bool, String), (bool, String), String, String), String>;

struct State {
    layout: Option<Layout>,
    numpad: Option<Side>,
    dock: Option<Side>,
    visual_layout: String,
    images: HashMap<PathBuf, Image>,
    preview: Preview,
    t0: Instant,
    loading: bool,
    bindings: Vec<Binding>,
    /// background-service commands, keyed by lower-case label
    commands: BTreeMap<String, String>,
    selected: Option<String>,
}

pub struct DarkMountPage {
    ui: Weak<MainWindow>,
    worker: KbWorker,
    state: RefCell<State>,
    keys: Rc<VecModel<KeyVisual>>,
    masks: Rc<VecModel<MaskVisual>>,
    anim: slint::Timer,
    apply: slint::Timer,
    /// (installed firmware, numpad and dock attached)
    installed: RefCell<(String, bool)>,
}

impl DarkMountPage {
    pub fn new(ui: &MainWindow) -> Rc<DarkMountPage> {
        let page = Rc::new(DarkMountPage {
            ui: ui.as_weak(),
            worker: KbWorker::new(),
            state: RefCell::new(State {
                layout: None,
                numpad: Some(Side::Right),
                dock: Some(Side::Left),
                visual_layout: "US".into(),
                images: HashMap::new(),
                preview: Preview::new(PreviewSettings::default()),
                t0: Instant::now(),
                loading: false,
                bindings: Vec::new(),
                commands: BTreeMap::new(),
                selected: None,
            }),
            keys: Rc::new(VecModel::default()),
            masks: Rc::new(VecModel::default()),
            anim: slint::Timer::default(),
            apply: slint::Timer::default(),
            installed: RefCell::new(("0.0.0".into(), false)),
        });
        ui.set_dm_keys(ModelRc::from(page.keys.clone()));
        ui.set_dm_masks(ModelRc::from(page.masks.clone()));
        page.rebuild();
        page.connect_callbacks(ui);
        PAGE.with(|c| *c.borrow_mut() = Some(page.clone()));
        page
    }

    fn connect_callbacks(self: &Rc<Self>, ui: &MainWindow) {
        let il = ui.global::<Illumination>();
        let p = Rc::downgrade(self);
        il.on_effect_clicked(move |i| {
            if let Some(p) = p.upgrade() {
                p.effect_clicked(i as usize);
            }
        });
        let p = Rc::downgrade(self);
        il.on_mode_clicked(move |i| {
            if let Some(p) = p.upgrade() {
                p.mode_clicked(i as usize);
            }
        });
        let p = Rc::downgrade(self);
        il.on_changed(move || {
            if let Some(p) = p.upgrade() {
                p.changed();
            }
        });
        il.on_hex_to_color(|s| hex_color(&s));
        let p = Rc::downgrade(self);
        il.on_stop_clicked(move |i| {
            if let Some(p) = p.upgrade() {
                if let Some(ui) = p.ui.upgrade() {
                    let il = ui.global::<Illumination>();
                    il.set_selected_stop(i);
                    if let Some(c) = il.get_stops().row_data(i as usize) {
                        il.set_stop_color(c);
                    }
                }
            }
        });
        let p = Rc::downgrade(self);
        il.on_stop_color_changed(move || {
            if let Some(p) = p.upgrade() {
                if let Some(ui) = p.ui.upgrade() {
                    let il = ui.global::<Illumination>();
                    let mut stops: Vec<Color> = il.get_stops().iter().collect();
                    let i = il.get_selected_stop() as usize;
                    if i < stops.len() {
                        stops[i] = il.get_stop_color();
                        il.set_stops(ModelRc::new(VecModel::from(stops)));
                    }
                }
                p.changed();
            }
        });
        let p = Rc::downgrade(self);
        il.on_stops_add(move |d| {
            if let Some(p) = p.upgrade() {
                if let Some(ui) = p.ui.upgrade() {
                    let il = ui.global::<Illumination>();
                    let mut stops: Vec<Color> = il.get_stops().iter().collect();
                    if d > 0 && stops.len() < 8 {
                        stops.push(hex_color(RAINBOW[stops.len() % RAINBOW.len()]));
                    } else if d < 0 && stops.len() > 2 {
                        stops.pop();
                    }
                    let sel = (il.get_selected_stop() as usize).min(stops.len() - 1);
                    il.set_selected_stop(sel as i32);
                    il.set_stop_color(stops[sel]);
                    il.set_stops(ModelRc::new(VecModel::from(stops)));
                }
                p.changed();
            }
        });
        {
            let g = ui.global::<MediaDock>();
            let dir = paths::assets_dir().join("dark_mount/images/media_dock");
            g.set_dock_img(self.image(&dir.join("media.webp")));
            g.set_menu_mask(self.image(&dir.join("masks/led-menu.svg")));
            let p = Rc::downgrade(self);
            g.on_key_clicked(move |name| {
                if let Some(p) = p.upgrade() {
                    p.select_key(&name);
                }
            });
        }
        let p = Rc::downgrade(self);
        ui.on_dm_panel_changed(move |panel| {
            if let Some(p) = p.upgrade() {
                // a key picked on one panel shouldn't show up on the other
                p.state.borrow_mut().selected = None;
                if let Some(ui) = p.ui.upgrade() {
                    ui.global::<KeyBinding>().set_has_key(false);
                    ui.global::<MediaDock>().set_sel("".into());
                }
                p.rebuild();
                match panel {
                    1 => p.load_bindings(),
                    2 => {
                        p.load_dock();
                        p.load_bindings();
                    }
                    3 => p.load_settings(),
                    _ => {}
                }
            }
        });
        let p = Rc::downgrade(self);
        ui.on_dm_key_clicked(move |name| {
            if let Some(p) = p.upgrade() {
                p.select_key(&name);
            }
        });
        let kbg = ui.global::<KeyBinding>();
        let p = Rc::downgrade(self);
        kbg.on_layer_clicked(move |layer| {
            if let Some(p) = p.upgrade() {
                if let Some(ui) = p.ui.upgrade() {
                    let g = ui.global::<KeyBinding>();
                    g.set_layer(layer);
                    g.set_has_key(false);
                    g.set_message("".into());
                }
                p.state.borrow_mut().selected = None;
                p.update_key_states();
            }
        });
        let p = Rc::downgrade(self);
        kbg.on_apply(move || {
            if let Some(p) = p.upgrade() {
                p.apply_binding();
            }
        });
        let p = Rc::downgrade(self);
        kbg.on_reset_all(move || {
            if let Some(p) = p.upgrade() {
                p.reset_all();
            }
        });
        // media dock
        let dock = ui.global::<MediaDock>();
        let p = Rc::downgrade(self);
        dock.on_changed(move || {
            if let Some(p) = p.upgrade() {
                let w = Rc::downgrade(&p);
                p.apply.start(slint::TimerMode::SingleShot, Duration::from_millis(300), move || {
                    if let Some(p) = w.upgrade() {
                        p.apply_dock();
                    }
                });
            }
        });
        let p = Rc::downgrade(self);
        dock.on_sync(move || {
            if let Some(p) = p.upgrade() {
                p.worker.run(|kb| kb.sync_media_dock_time().map_err(|e| e.to_string()),
                             |_| with_page(|p| p.load_dock()));
            }
        });
        let p = Rc::downgrade(self);
        dock.on_reset(move || {
            if let Some(p) = p.upgrade() {
                let cfg = MediaDockConfig { menu_color: [220, 77, 0], clock_mode: 0,
                                            screensaver_mode: 2, screensaver_timeout: 30,
                                            screen_off_timeout: 60 };
                p.worker.run(move |kb| kb.set_media_dock_config(&cfg).map_err(|e| e.to_string()),
                             |_| with_page(|p| p.load_dock()));
            }
        });
        // product settings
        let ps = ui.global::<ProductSettings>();
        let p = Rc::downgrade(self);
        ps.on_game_toggled(move |on| {
            if let Some(p) = p.upgrade() {
                p.write(move |kb| kb.set_game_mode(on));
            }
        });
        let p = Rc::downgrade(self);
        ps.on_flag_toggled(move |i, on| {
            if let Some(p) = p.upgrade() {
                let Some(ui) = p.ui.upgrade() else { return };
                let g = ui.global::<ProductSettings>();
                let mut flags: Vec<bool> = g.get_flags().iter().collect();
                if let Some(f) = flags.get_mut(i as usize) {
                    *f = on;
                }
                let bits = flags.iter().enumerate().fold(0u8, |a, (i, f)| a | ((*f as u8) << i));
                g.set_flags(ModelRc::new(VecModel::from(flags)));
                p.write(move |kb| kb.set_game_mode_flags(bits));
            }
        });
        let p = Rc::downgrade(self);
        ps.on_service_toggled(move |on| {
            // (re)write the unit so it starts this copy of the program (AppImage or not)
            if on {
                let _ = darkmount_core::service::install();
            }
            darkmount_core::service::systemctl(if on { &["enable", "--now", SERVICE] } else { &["disable", "--now", SERVICE] });
            if let Some(p) = p.upgrade() {
                p.service_status();
            }
        });
        ps.on_autostart_toggled(move |on| {
            if let Err(e) = darkmount_core::autostart::set(on, crate::APP_NAME) {
                eprintln!("autostart: {e}");
            }
        });
        let weak = self.ui.clone();
        ps.on_check_updates(move || {
            let Some(ui) = weak.upgrade() else { return };
            let g = ui.global::<ProductSettings>();
            g.set_checking(true);
            g.set_update_text("Checking…".into());
            let weak = weak.clone();
            thread::spawn(move || {
                let w2 = weak.clone();
                let r = firmware::download_latest(&|m: &str| {
                    let m = m.to_string();
                    let _ = w2.upgrade_in_event_loop(move |ui| {
                        ui.global::<ProductSettings>().set_update_text(m.into());
                    });
                });
                let _ = weak.upgrade_in_event_loop(move |ui| {
                    ui.global::<ProductSettings>().set_checking(false);
                    if let Err(e) = r {
                        ui.global::<ProductSettings>()
                            .set_update_text(format!("Couldn't download firmware: {e}").into());
                    }
                    with_page(|p| p.show_available());
                });
            });
        });
        let p = Rc::downgrade(self);
        ps.on_install(move || {
            if let Some(p) = p.upgrade() {
                p.install_update();
            }
        });
        let weak = self.ui.clone();
        kbg.on_choose_program(move || {
            let weak = weak.clone();
            crate::app_picker::open(move |cmd| {
                if let Some(ui) = weak.upgrade() {
                    ui.global::<KeyBinding>().set_command(cmd.into());
                }
            });
        });
        let weak = self.ui.clone();
        kbg.on_browse(move || {
            let Some(ui) = weak.upgrade() else { return };
            let folder = ui.global::<KeyBinding>().get_kind() == 9;
            let weak = weak.clone();
            thread::spawn(move || {
                if let Some(path) = pick_path(folder) {
                    let _ = weak.upgrade_in_event_loop(move |ui| {
                        ui.global::<KeyBinding>().set_path(path.into());
                    });
                }
            });
        });
    }

    /// The page became visible or the device appeared: (re)load from the keyboard.
    pub fn load(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.set_dm_status("Connecting…".into());
        self.worker.run(
            |kb| {
                let mode = kb.lighting_mode().map_err(|e| e.to_string())?;
                let light = kb.lighting(0).map_err(|e| e.to_string())?;
                let numpad = kb.numpad_state().map_err(|e| e.to_string())?;
                let dock = kb.media_dock_state().map_err(|e| e.to_string())?;
                let (_, vis) = kb.layout().map_err(|e| e.to_string())?;
                let fw = kb.device_info().map(|i| i.firmware.first().map(|f| f.1.clone())
                    .unwrap_or_default()).unwrap_or_default();
                Ok((mode, light, numpad, dock, vis, fw))
            },
            move |r| with_page(|p| p.loaded(r)),
        );
    }

    fn loaded(self: &Rc<Self>, r: Loaded) {
        let Some(ui) = self.ui.upgrade() else { return };
        match r {
            Err(e) => {
                ui.set_dm_connected(false);
                ui.set_need_access(e.contains("Permission denied") && !darkmount_core::udev::installed());
                ui.set_dm_status(e.into());
            }
            Ok((mode, light, numpad, dock, vis, fw)) => {
                ui.set_dm_connected(true);
                ui.set_need_access(false);
                ui.set_dm_status(format!("Connected · firmware {fw}").into());
                {
                    let mut s = self.state.borrow_mut();
                    s.numpad = if numpad.0 { Side::parse(&numpad.1) } else { None };
                    s.dock = if dock.0 { Side::parse(&dock.1) } else { None };
                    s.visual_layout = vis;
                    s.loading = true;
                }
                self.rebuild();
                self.show_lighting(mode, &light);
                self.state.borrow_mut().loading = false;
                self.update_preview();
            }
        }
    }

    /// Fill the Illumination panel from the keyboard's config (as the Python app did).
    fn show_lighting(&self, mode: u8, cfg: &LightingConfig) {
        let Some(ui) = self.ui.upgrade() else { return };
        let il = ui.global::<Illumination>();
        let on = mode != lighting_mode::OFF && cfg.effect != effect::OFF;
        let e = EFFECTS.iter().position(|&x| x == cfg.effect).unwrap_or(0);
        let modes = effect_modes(e);
        let device_mode = match cfg.color_mode {
            color_mode::DUAL => 1,
            color_mode::GRADIENT | color_mode::ORIENTED_GRADIENT => 2,
            _ => 0,
        };
        let m = if modes.contains(&device_mode) { device_mode } else { modes[0] };
        let mut colors: Vec<[u8; 3]> = cfg.colors.clone();
        if cfg.effect == effect::OFF || colors.is_empty() || colors.iter().all(|c| *c == [0, 0, 0]) {
            colors = vec![[255, 40, 0]];
        }
        il.set_on(on);
        il.set_effect(e as i32);
        self.set_modes(e, m);
        il.set_color1(to_color(colors[0]));
        il.set_color2(to_color(colors.get(1).copied().unwrap_or([0, 120, 255])));
        let stops: Vec<Color> = if device_mode == 2 && colors.len() >= 2 {
            colors.iter().map(|c| to_color(*c)).collect()
        } else {
            RAINBOW.iter().map(|h| hex_color(h)).collect()
        };
        il.set_selected_stop(0);
        il.set_stop_color(stops[0]);
        il.set_stops(ModelRc::new(VecModel::from(stops)));
        self.set_dirs(e, Some(cfg.direction));
        il.set_brightness(cfg.brightness as f32);
        il.set_speed(cfg.speed as f32);
    }

    fn set_modes(&self, e: usize, m: usize) {
        let Some(ui) = self.ui.upgrade() else { return };
        let il = ui.global::<Illumination>();
        let modes = effect_modes(e);
        il.set_modes(ModelRc::new(VecModel::from(
            (0..3).map(|i| modes.contains(&i)).collect::<Vec<bool>>())));
        il.set_mode(m as i32);
    }

    fn set_dirs(&self, e: usize, current: Option<u8>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let il = ui.global::<Illumination>();
        let dirs = effect_dirs(e);
        il.set_directions(ModelRc::new(VecModel::from(
            dirs.iter().map(|(_, n)| SharedString::from(*n)).collect::<Vec<_>>())));
        let idx = current.and_then(|d| dirs.iter().position(|(id, _)| *id == d)).unwrap_or(0);
        il.set_direction(idx as i32);
    }

    fn effect_clicked(self: &Rc<Self>, e: usize) {
        let Some(ui) = self.ui.upgrade() else { return };
        let il = ui.global::<Illumination>();
        il.set_effect(e as i32);
        let m = il.get_mode() as usize;
        let modes = effect_modes(e);
        self.set_modes(e, if modes.contains(&m) { m } else { modes[0] });
        self.set_dirs(e, Some(default_dir(e)));
        self.changed();
    }

    fn mode_clicked(self: &Rc<Self>, m: usize) {
        let Some(ui) = self.ui.upgrade() else { return };
        ui.global::<Illumination>().set_mode(m as i32);
        self.changed();
    }

    /// The panel's settings as a keyboard config.
    fn config(&self) -> Option<(bool, LightingConfig)> {
        let ui = self.ui.upgrade()?;
        let il = ui.global::<Illumination>();
        let e = (il.get_effect() as usize).min(EFFECTS.len() - 1);
        let m = (il.get_mode() as usize).min(2);
        let colors: Vec<[u8; 3]> = match m {
            0 => vec![to_rgb(il.get_color1())],
            1 => vec![to_rgb(il.get_color1()), to_rgb(il.get_color2())],
            _ => il.get_stops().iter().map(to_rgb).collect(),
        };
        let n = colors.len();
        let stops = if m == 2 {
            (0..n).map(|i| ((i * 100) as f32 / (n.max(2) - 1) as f32).round() as u8).collect()
        } else {
            Vec::new()
        };
        let dirs = effect_dirs(e);
        let direction = dirs.get(il.get_direction() as usize).map_or(0, |(id, _)| *id);
        Some((il.get_on(), LightingConfig {
            effect: EFFECTS[e],
            direction,
            brightness: il.get_brightness().round().clamp(0.0, 100.0) as u8,
            speed: il.get_speed().round().clamp(0.0, 100.0) as u8,
            color_mode: MODE_TO_DEVICE[m],
            colors,
            stops,
            ..Default::default()
        }))
    }

    fn changed(self: &Rc<Self>) {
        self.update_preview();
        if self.state.borrow().loading {
            return;
        }
        // send to the keyboard 250 ms after the last change
        let weak = Rc::downgrade(self);
        self.apply.start(slint::TimerMode::SingleShot, Duration::from_millis(250), move || {
            if let Some(p) = weak.upgrade() {
                p.apply_lighting();
            }
        });
    }

    fn apply_lighting(self: &Rc<Self>) {
        let Some((on, cfg)) = self.config() else { return };
        self.worker.run(
            move |kb| {
                if on {
                    kb.set_lighting(&cfg, 0).map_err(|e| e.to_string())
                } else {
                    kb.set_lighting_mode(lighting_mode::OFF).map_err(|e| e.to_string())
                }
            },
            move |r| with_page(|p| {
                if let (Err(e), Some(ui)) = (r, p.ui.upgrade()) {
                    ui.set_dm_status(write_error(&e).into());
                }
            }),
        );
    }

    // -- key binding ----------------------------------------------------------------
    fn layer(&self) -> u8 {
        self.ui.upgrade().map_or(0, |ui| ui.global::<KeyBinding>().get_layer() as u8)
    }

    fn load_bindings(self: &Rc<Self>) {
        self.state.borrow_mut().commands = actions::read_actions(&actions::config_path())
            .into_iter().map(|(k, v)| (k.to_lowercase(), v)).collect();
        self.worker.run(
            |kb| kb.bindings().map_err(|e| e.to_string()),
            move |r| with_page(|p| match r {
                Ok(b) => {
                    p.state.borrow_mut().bindings = b;
                    p.update_key_states();
                    let sel = p.state.borrow().selected.clone();
                    if let Some(k) = sel {
                        p.select_key(&k);
                    }
                }
                Err(e) => {
                    if let Some(ui) = p.ui.upgrade() {
                        ui.global::<KeyBinding>().set_message(format!("Couldn't read the key \
                            bindings: {e}").into());
                    }
                }
            }),
        );
    }

    fn binding_for(&self, name: &str) -> Option<Binding> {
        let layer = self.layer();
        self.state.borrow().bindings.iter()
            .find(|b| b.layer == layer && b.key_name() == name).cloned()
    }

    /// Selected / bound / not bindable, for each key on the picture.
    fn update_key_states(&self) {
        let layer = self.layer();
        let s = self.state.borrow();
        let bound: std::collections::HashSet<String> = s.bindings.iter()
            .filter(|b| b.layer == layer).map(|b| b.key_name()).collect();
        for i in 0..self.keys.row_count() {
            let Some(mut row) = self.keys.row_data(i) else { continue };
            let name = row.name.as_str();
            let dim = name == "Fn" || (layer == 1 && (name.starts_with("Image")
                || name.starts_with("Dock")));
            let state = if dim { 3 } else if s.selected.as_deref() == Some(name) { 1 }
                        else if bound.contains(name) { 2 } else { 0 };
            if row.state != state {
                row.state = state;
                self.keys.set_row_data(i, row);
            }
        }
    }

    /// Show a key's current function in the editor (as the Python app did).
    fn select_key(self: &Rc<Self>, name: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let g = ui.global::<KeyBinding>();
        let layer = self.layer();
        self.state.borrow_mut().selected = Some(name.to_string());
        self.update_key_states();
        g.set_has_key(true);
        g.set_message("".into());
        g.set_kicker(if layer == 1 { "FN LAYER" } else { "KEY" }.into());
        g.set_title(actions::key_title(name).into());
        let b = self.binding_for(name);
        let label = actions::label(name, layer).to_lowercase();
        let cmd = self.state.borrow().commands.get(&label).cloned().unwrap_or_default();
        let current = if !cmd.trim().is_empty() {
            let mut lines = cmd.trim().lines();
            let first = lines.next().unwrap_or("");
            format!("Current: run “{first}”{}", if lines.next().is_some() { " …" } else { "" })
        } else {
            format!("Current: {}", b.as_ref().map_or("default".into(), |b| b.describe()))
        };
        g.set_current(current.into());
        let mut kind = match b.as_ref().map(|b| b.kind) {
            Some(bind_type::STANDARD_KEY) => 1,
            Some(bind_type::MEDIA) => 2,
            Some(bind_type::MOUSE) if b.as_ref().is_some_and(|b| b.scroll) => 4,
            Some(bind_type::MOUSE) => 3,
            Some(bind_type::BACKLIGHT) => 5,
            Some(bind_type::WINDOWS_SHORTCUT) => 6,
            Some(bind_type::OPEN_FILE) if b.as_ref().is_some_and(|b| b.text == actions::RUN_MARKER) => 7,
            Some(bind_type::OPEN_FILE) => 8,
            Some(bind_type::OPEN_FOLDER) => 9,
            Some(bind_type::OPEN_BROWSER) => 10,
            Some(bind_type::DISABLE) => 11,
            _ => 0,
        };
        if !cmd.trim().is_empty() {
            kind = 7;
        }
        g.set_kind(kind);
        g.set_combo("".into());
        g.set_command(cmd.into());
        g.set_path("".into());
        g.set_url("".into());
        if let Some(b) = b {
            let idx = |v: u8, off: u8| v.saturating_sub(off) as i32;
            match kind {
                1 => {
                    let mut parts: Vec<String> = Vec::new();
                    for (n, bit) in kb_bind::MODIFIERS {
                        if b.modifiers & bit != 0 {
                            let n = n.to_lowercase();
                            let n = n.strip_prefix('l').map(str::to_string).unwrap_or(n);
                            parts.push(match n.as_str() {
                                "gui" => "super".into(),
                                "rgui" => "rsuper".into(),
                                _ => n,
                            });
                        }
                    }
                    parts.push(kb_bind::hid_name(b.usage));
                    g.set_combo(parts.join("+").into());
                }
                2 => g.set_media(idx(b.action, 1)),
                3 => g.set_mouse(idx(b.action, 1)),
                4 => g.set_scroll(idx(b.action, 1)),
                5 => g.set_light(idx(b.action, 2)),
                6 => g.set_host(idx(b.action, 1)),
                8 | 9 => g.set_path(b.text.clone().into()),
                10 => g.set_url(b.text.clone().into()),
                _ => {}
            }
        }
    }

    /// The record to write and the background-service command (Run Linux command).
    fn record(&self) -> Result<(Vec<u8>, Option<String>), String> {
        let ui = self.ui.upgrade().ok_or("window closed")?;
        let g = ui.global::<KeyBinding>();
        let name = self.state.borrow().selected.clone().ok_or("Select a key first")?;
        let key = kb_bind::key_id(&name).ok_or_else(|| format!("unknown key {name}"))?;
        let l = self.layer();
        let n = |v: i32, off: u8| (v.max(0) as u8).saturating_add(off);
        Ok(match g.get_kind() {
            0 => (kb_bind::encode_clear(key, l), None),
            1 => {
                let (mods, usage) = kb_bind::parse_combo(&g.get_combo())?;
                (kb_bind::encode_standard_key(key, l, usage, mods), None)
            }
            2 => (kb_bind::encode_media(key, l, n(g.get_media(), 1)), None),
            3 => (kb_bind::encode_mouse_button(key, l, n(g.get_mouse(), 1)), None),
            4 => (kb_bind::encode_mouse_scroll(key, l, n(g.get_scroll(), 1)), None),
            5 => (kb_bind::encode_backlight(key, l, n(g.get_light(), 2), 0), None),
            6 => (kb_bind::encode_host_action(key, l, n(g.get_host(), 1)), None),
            7 => {
                let cmd = g.get_command().trim().to_string();
                if cmd.is_empty() {
                    return Err("Enter a command".into());
                }
                // keep an existing on-keyboard open binding (e.g. for Windows)
                match self.binding_for(&name) {
                    Some(b) if matches!(b.kind, bind_type::OPEN_FILE | bind_type::OPEN_FOLDER
                                                | bind_type::OPEN_BROWSER) => (b.data, Some(cmd)),
                    _ => (kb_bind::encode_open(key, l, bind_type::OPEN_FILE, actions::RUN_MARKER)?,
                          Some(cmd)),
                }
            }
            8 | 9 => {
                let path = g.get_path().trim().to_string();
                if path.is_empty() {
                    return Err("Choose a path".into());
                }
                let k = if g.get_kind() == 8 { bind_type::OPEN_FILE } else { bind_type::OPEN_FOLDER };
                (kb_bind::encode_open(key, l, k, &path)?, None)
            }
            10 => {
                let url = g.get_url().trim().to_string();
                if url.is_empty() {
                    return Err("Enter a URL".into());
                }
                (kb_bind::encode_open(key, l, bind_type::OPEN_BROWSER, &url)?, None)
            }
            _ => (kb_bind::encode_disable(key, l), None),
        })
    }

    fn apply_binding(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let g = ui.global::<KeyBinding>();
        let (rec, cmd) = match self.record() {
            Ok(r) => r,
            Err(e) => {
                g.set_message(e.into());
                return;
            }
        };
        let name = self.state.borrow().selected.clone().unwrap_or_default();
        let label = actions::label(&name, self.layer());
        g.set_message("Saving…".into());
        self.worker.run(
            move |kb| kb.set_binding(&rec).map_err(|e| e.to_string()),
            move |r| with_page(|p| {
                let Some(ui) = p.ui.upgrade() else { return };
                let g = ui.global::<KeyBinding>();
                match r {
                    Ok(()) => {
                        let saved = actions::set_action(&actions::config_path(), &label,
                                                        cmd.as_deref());
                        g.set_message(match saved {
                            Ok(()) => "Saved.".into(),
                            Err(e) => format!("Saved on the keyboard, but the command \
                                               couldn't be stored: {e}").into(),
                        });
                        p.load_bindings();
                    }
                    Err(e) => g.set_message(write_error(&e).into()),
                }
            }),
        );
    }

    fn reset_all(self: &Rc<Self>) {
        self.worker.run(
            |kb| {
                let all = kb.bindings().map_err(|e| e.to_string())?;
                let mut labels = Vec::new();
                for b in &all {
                    kb.clear_binding(b.key, b.layer).map_err(|e| e.to_string())?;
                    labels.push(actions::label(&b.key_name(), b.layer));
                }
                Ok(labels)
            },
            move |r| with_page(|p| {
                let Some(ui) = p.ui.upgrade() else { return };
                let g = ui.global::<KeyBinding>();
                match r {
                    Ok(labels) => {
                        let path = actions::config_path();
                        for l in labels {
                            let _ = actions::set_action(&path, &l, None);
                        }
                        g.set_has_key(false);
                        g.set_message("".into());
                        p.state.borrow_mut().selected = None;
                        p.load_bindings();
                    }
                    Err(e) => g.set_message(write_error(&e).into()),
                }
            }),
        );
    }

    // -- writes, media dock, product settings -------------------------------------------
    /// A write; errors show in the status line.
    fn write(&self, f: impl FnOnce(&mut DarkMount) -> Result<(), darkmount_core::darkmount::qlink::Error>
             + Send + 'static) {
        self.worker.run(move |kb| f(kb).map_err(|e| e.to_string()), |r| with_page(|p| {
            if let (Err(e), Some(ui)) = (r, p.ui.upgrade()) {
                ui.set_dm_status(write_error(&e).into());
            }
        }));
    }

    fn load_dock(self: &Rc<Self>) {
        self.worker.run(
            |kb| {
                let e = |x: darkmount_core::darkmount::qlink::Error| x.to_string();
                let attached = kb.media_dock_state().map_err(e)?.0;
                if !attached {
                    return Ok(None);
                }
                Ok(Some((kb.media_dock_config().map_err(e)?, kb.media_dock_time().map_err(e)?)))
            },
            |r| with_page(|p| {
                let Some(ui) = p.ui.upgrade() else { return };
                let g = ui.global::<MediaDock>();
                match r {
                    Ok(Some((cfg, t))) => {
                        g.set_attached(true);
                        g.set_color(to_color(cfg.menu_color));
                        g.set_idle_on(cfg.screensaver_mode != 0);
                        g.set_idle_s(cfg.screensaver_timeout.max(5) as i32);
                        g.set_view(if cfg.screensaver_mode == 1 { 1 } else { 0 });
                        g.set_clock(if cfg.clock_mode == 0 { 0 } else { 1 });
                        g.set_off_on(cfg.screen_off_timeout > 0);
                        g.set_off_s(if cfg.screen_off_timeout > 0 { cfg.screen_off_timeout as i32 }
                                    else { 60 });
                        // the dock counts local time as seconds since the epoch
                        let secs = t as i64;
                        let (h, m) = ((secs / 3600) % 24, (secs / 60) % 60);
                        g.set_clock_text(format!("{h:02}:{m:02}").into());
                    }
                    Ok(None) => g.set_attached(false),
                    Err(e) => ui.set_dm_status(e.into()),
                }
            }),
        );
    }

    fn apply_dock(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let g = ui.global::<MediaDock>();
        let cfg = MediaDockConfig {
            menu_color: to_rgb(g.get_color()),
            clock_mode: if g.get_clock() == 0 { 0 } else { 1 },
            screensaver_mode: if !g.get_idle_on() { 0 } else if g.get_view() == 0 { 2 } else { 1 },
            screensaver_timeout: g.get_idle_s().clamp(5, 3600) as u16,
            screen_off_timeout: if g.get_off_on() { g.get_off_s().clamp(5, 36000) as u16 } else { 0 },
        };
        self.write(move |kb| kb.set_media_dock_config(&cfg));
    }

    fn load_settings(self: &Rc<Self>) {
        self.service_status();
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<ProductSettings>().set_autostart(darkmount_core::autostart::enabled());
        }
        self.worker.run(
            |kb| {
                let e = |x: darkmount_core::darkmount::qlink::Error| x.to_string();
                let info = kb.device_info().map_err(e)?;
                let modules = kb.numpad_state().map_err(e)?.0 && kb.media_dock_state().map_err(e)?.0;
                let (phys, vis) = kb.layout().map_err(e)?;
                Ok((info.firmware, modules, kb.game_mode().map_err(e)?,
                    kb.game_mode_flags().map_err(e)?, format!("{phys} {vis}"),
                    format!("Serial {} · polling {} Hz", kb.serial_number().map_err(e)?,
                            kb.polling_rate().map_err(e)?)))
            },
            |r| with_page(|p| {
                let Some(ui) = p.ui.upgrade() else { return };
                let g = ui.global::<ProductSettings>();
                match r {
                    Ok((fw, modules, game, flags, layout, info)) => {
                        let names = |n: &str| match n {
                            "Main" => "Keyboard", "MediaDock" => "Media Dock", "Numpad" => "Numpad",
                            other => other,
                        }.to_string();
                        *p.installed.borrow_mut() = (fw.first().map(|f| f.1.clone())
                            .unwrap_or_else(|| "0.0.0".into()), modules);
                        g.set_firmware(ModelRc::new(VecModel::from(fw.iter().map(|(n, v)| FirmwarePart {
                            name: names(n).into(), version: v.clone().into() }).collect::<Vec<_>>())));
                        g.set_game(game);
                        g.set_flags(ModelRc::new(VecModel::from(
                            (0..5).map(|i| flags & (1 << i) != 0).collect::<Vec<bool>>())));
                        g.set_layout_text(layout.into());
                        g.set_info_text(info.into());
                        p.show_available();
                    }
                    Err(e) => ui.set_dm_status(e.into()),
                }
            }),
        );
    }

    fn service_status(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let q = |a: &str| std::process::Command::new("systemctl").args(["--user", a, SERVICE])
            .output().ok().map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        let active = q("is-active") == "active";
        let enabled = q("is-enabled") == "enabled";
        let g = ui.global::<ProductSettings>();
        g.set_service(active);
        g.set_service_text(format!("{}{}", if active { "Running" } else { "Stopped" },
                                   if enabled { ", starts at login." } else { "." }).into());
    }

    fn show_available(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        let g = ui.global::<ProductSettings>();
        let (installed, modules) = self.installed.borrow().clone();
        let pkg = firmware::cached_packages().into_iter().next();
        match pkg {
            Some(pkg) if firmware::version_key(&pkg.version) > firmware::version_key(&installed) => {
                g.set_update_text(format!("Firmware {} is ready to install.{}", pkg.version,
                    if modules { "" } else { " Attach the numpad and media dock first." }).into());
                g.set_install_visible(true);
                g.set_install_enabled(modules);
            }
            Some(pkg) => {
                g.set_update_text(format!("Up to date (latest downloaded: {}).", pkg.version).into());
                g.set_install_visible(false);
            }
            None => {
                g.set_update_text("Press “Check for updates” to download the latest firmware \
                                   (needs internet once).".into());
                g.set_install_visible(false);
            }
        }
    }

    fn install_update(self: &Rc<Self>) {
        let Some(pkg) = firmware::cached_packages().into_iter().next() else { return };
        let Some(ui) = self.ui.upgrade() else { return };
        let g = ui.global::<ProductSettings>();
        g.set_installing(true);
        g.set_progress(0.0);
        // our own session would keep the keyboard from switching to its bootloader
        self.worker.disconnect();
        let weak = self.ui.clone();
        thread::spawn(move || {
            thread::sleep(Duration::from_millis(500));
            let w = weak.clone();
            let report = move |stage: &str, pct: f32, msg: &str| {
                let text = format!("{stage}: {msg}");
                let _ = w.upgrade_in_event_loop(move |ui| {
                    let g = ui.global::<ProductSettings>();
                    g.set_progress(pct);
                    g.set_update_text(text.into());
                });
            };
            let r = firmware::update_with_backup(&pkg, &report, true);
            let _ = weak.upgrade_in_event_loop(move |ui| {
                let g = ui.global::<ProductSettings>();
                g.set_installing(false);
                g.set_update_text(match r {
                    Ok(()) => "Update complete.".into(),
                    Err(e) => format!("Update failed: {e}. Run it again to resume.").into(),
                });
                with_page(|p| p.load());
            });
        });
    }

    // -- picture --------------------------------------------------------------------
    fn image(&self, path: &Path) -> Image {
        let mut s = self.state.borrow_mut();
        s.images.entry(path.to_path_buf())
            .or_insert_with(|| Image::load_from_path(path).unwrap_or_default())
            .clone()
    }

    /// Rebuild the picture for the current modules and panel.
    fn rebuild(self: &Rc<Self>) {
        let Some(ui) = self.ui.upgrade() else { return };
        let mode = if ui.get_dm_panel() == 1 { Mode::Keys } else { Mode::Lighting };
        let (numpad, dock, vis) = {
            let s = self.state.borrow();
            (s.numpad, s.dock, s.visual_layout.clone())
        };
        let dir = paths::assets_dir().join("dark_mount");
        let built = match layout::build(&dir, numpad, dock, mode, &vis) {
            Ok(l) => l,
            Err(e) => {
                ui.set_dm_status(format!("Keyboard picture: {e}").into());
                return;
            }
        };
        let parts: Vec<PartVisual> = built.parts.iter()
            .map(|p| PartVisual { image: self.image(&p.image), x: p.x }).collect();
        ui.set_dm_parts(ModelRc::new(VecModel::from(parts)));
        let masks: Vec<MaskVisual> = built.masks.iter().map(|m| MaskVisual {
            image: self.image(&m.image), x: m.rect.x, y: m.rect.y, w: m.rect.w, h: m.rect.h,
            color: Color::from_rgb_u8(0, 0, 0), lit: false,
        }).collect();
        self.masks.set_vec(masks);
        let keys: Vec<KeyVisual> = built.keys.iter().map(|k| KeyVisual {
            name: k.name.clone().into(), x: k.rect.x, y: k.rect.y, w: k.rect.w, h: k.rect.h,
            r: k.radius, color: Color::from_rgb_u8(0, 0, 0), lit: false, state: 0,
        }).collect();
        self.keys.set_vec(keys);
        self.update_key_states();
        let b = built.bounds;
        ui.set_dm_bounds(ModelRc::new(VecModel::from(vec![b.x, b.y, b.w, b.h])));
        self.state.borrow_mut().layout = Some(built);
        self.update_preview();
    }

    /// Preview settings from the panel; start or stop the animation.
    fn update_preview(self: &Rc<Self>) {
        let Some((on, cfg)) = self.config() else { return };
        let e = EFFECTS.iter().position(|&x| x == cfg.effect).unwrap_or(0);
        let dirs = effect_dirs(e);
        let direction = dirs.iter().find(|(id, _)| *id == cfg.direction).map_or("Right", |d| d.1);
        let settings = PreviewSettings {
            on,
            effect: PREVIEW_NAMES[e].into(),
            colors: cfg.colors.clone(),
            brightness: cfg.brightness,
            speed: cfg.speed,
            direction: direction.into(),
        };
        let animated = settings.animated();
        self.state.borrow_mut().preview.settings = settings;
        self.frame();
        if animated && !self.anim.running() {
            let weak = Rc::downgrade(self);
            self.anim.start(slint::TimerMode::Repeated, Duration::from_millis(40), move || {
                if let Some(p) = weak.upgrade() {
                    p.frame();
                }
            });
        } else if !animated {
            self.anim.stop();
        }
    }

    /// Color every key and glow mask for this moment.
    fn frame(&self) {
        let Some(ui) = self.ui.upgrade() else { return };
        // nothing to animate while hidden in the tray or the key panel is showing
        if !ui.window().is_visible() || ui.get_dm_panel() == 1 {
            return;
        }
        let mut s = self.state.borrow_mut();
        let now = s.t0.elapsed().as_secs_f32();
        let Some(l) = s.layout.take() else { return };
        let (w, h) = (l.bounds.x * 2.0 + l.bounds.w, l.bounds.y * 2.0 + l.bounds.h);
        s.preview.step(now);
        for (i, m) in l.masks.iter().enumerate() {
            let c = s.preview.color_at(m.rect.cx() / w, m.rect.cy() / h, None, now);
            if let Some(mut row) = self.masks.row_data(i) {
                row.lit = c.is_some();
                row.color = to_color(c.unwrap_or([0, 0, 0]));
                self.masks.set_row_data(i, row);
            }
        }
        for (i, k) in l.keys.iter().enumerate() {
            let c = s.preview.color_at(k.rect.cx() / w, k.rect.cy() / h, Some(&k.name), now);
            if let Some(mut row) = self.keys.row_data(i) {
                row.lit = c.is_some();
                row.color = to_color(c.unwrap_or([0, 0, 0]));
                self.keys.set_row_data(i, row);
            }
        }
        s.layout = Some(l);
    }

    pub fn worker(&self) -> KbWorker {
        self.worker.clone()
    }

    /// The keyboard was unplugged.
    pub fn gone(&self) {
        self.worker.disconnect();
        self.anim.stop();
        if let Some(ui) = self.ui.upgrade() {
            ui.set_dm_connected(false);
            ui.set_dm_status("Not connected".into());
        }
    }
}

thread_local! {
    /// The page lives on the UI thread; worker results find it through here, so
    /// nothing tied to that thread crosses into the worker.
    static PAGE: RefCell<Option<Rc<DarkMountPage>>> = const { RefCell::new(None) };
}

/// Run `f` with the page (UI thread only).
/// Connect again (UI thread), e.g. after access was granted.
pub fn reload() {
    with_page(|p| p.load());
}

fn with_page(f: impl FnOnce(&Rc<DarkMountPage>)) {
    if let Some(p) = PAGE.with(|c| c.borrow().clone()) {
        f(&p);
    }
}

thread_local! {
    static WAS_PRESENT: std::cell::Cell<Option<bool>> = const { std::cell::Cell::new(None) };
}

/// Presence update (UI thread): load when the keyboard appears, let go when it leaves.
pub fn presence_changed(present: bool) {
    let before = WAS_PRESENT.with(|w| w.replace(Some(present)));
    if before == Some(present) {
        return;
    }
    with_page(|p| if present { p.load() } else { p.gone() });
}

/// A file or folder from the desktop's own chooser (KDE kdialog, else zenity).
fn pick_path(folder: bool) -> Option<String> {
    use std::process::Command;
    let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
    let tries: Vec<Vec<&str>> = if folder {
        vec![vec!["kdialog", "--getexistingdirectory", &home],
             vec!["zenity", "--file-selection", "--directory"]]
    } else {
        vec![vec!["kdialog", "--getopenfilename", &home],
             vec!["zenity", "--file-selection"]]
    };
    for t in tries {
        if let Ok(out) = Command::new(t[0]).args(&t[1..]).output() {
            let p = String::from_utf8_lossy(&out.stdout).trim().to_string();
            return (out.status.success() && !p.is_empty()).then_some(p);
        }
    }
    None
}

