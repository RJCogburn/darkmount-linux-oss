//! System tray icon (KDE/freedesktop StatusNotifierItem): click to show or hide the
//! window. The menu has quick keyboard controls (backlight, effect, brightness, game
//! mode, dock clock), read back from the keyboard every minute.

use std::thread;
use std::time::Duration;

use darkmount_core::darkmount::device::{color_mode, effect, lighting_mode, DarkMount, LightingConfig};
use ksni::blocking::TrayMethods;
use ksni::menu::{CheckmarkItem, RadioGroup, RadioItem, StandardItem, SubMenu};
use slint::{ComponentHandle, Weak};

use crate::darkmount_page::KbWorker;
use crate::{MainWindow, APP_NAME};

const EFFECTS: [(&str, u8); 6] = [("Static", effect::STATIC), ("Color Wave", effect::COLOR_WAVE), ("Tornado", effect::TORNADO),
                                  ("Breathing", effect::BREATHING), ("Reactive", effect::REACTIVE), ("Matrix", effect::MATRIX)];
const BRIGHTNESS: [u8; 5] = [10, 30, 50, 70, 100];

#[derive(Clone, Default)]
struct DmState {
    ok: bool,
    light: bool,
    effect: Option<u8>,
    brightness: u8,
    game: bool,
}

struct Tray {
    ui: Weak<MainWindow>,
    kb: KbWorker,
    dm: DmState,
}

fn notify(title: &str, text: &str) {
    let mut c = std::process::Command::new("notify-send");
    c.args(["-a", APP_NAME, "-t", "4000", title, text]);
    darkmount_core::hostenv::clean(&mut c);
    if let Ok(mut child) = c.spawn() {
        thread::spawn(move || { let _ = child.wait(); });
    }
}

fn read_dm(kb: &KbWorker) -> DmState {
    if !darkmount_core::presence::keyboard_present() {
        return DmState::default();
    }
    kb.call(|k| {
        let mode = k.lighting_mode().map_err(|e| e.to_string())?;
        let cfg = k.lighting(0).map_err(|e| e.to_string())?;
        let game = k.game_mode().map_err(|e| e.to_string())?;
        Ok(DmState {
            ok: true,
            light: mode != lighting_mode::OFF && cfg.effect != effect::OFF,
            effect: (cfg.effect <= effect::MATRIX).then_some(cfg.effect),
            brightness: cfg.brightness,
            game,
        })
    }).unwrap_or_default()
}

impl Tray {
    fn toggle_window(&self) {
        let ui = self.ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = ui.upgrade() {
                if w.window().is_visible() {
                    let _ = w.hide();
                } else {
                    let _ = w.show();
                }
            }
        });
    }

    fn show(&self) {
        let ui = self.ui.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(w) = ui.upgrade() {
                let _ = w.show();
            }
        });
    }

    /// Run a keyboard change, then read the state back for the menu.
    fn dm(&mut self, f: impl FnOnce(&mut DarkMount) -> Result<(), String> + Send + 'static) {
        if self.kb.call(f).is_none() {
            notify("Dark Mount", "Keyboard not available");
        }
        self.dm = read_dm(&self.kb);
    }
}

fn set_effect(k: &mut DarkMount, e: u8) -> Result<(), String> {
    let mut cfg = k.lighting(0).map_err(|e| e.to_string())?;
    if cfg.effect == effect::OFF || cfg.colors.is_empty() {
        cfg.colors = vec![[255, 40, 0]];
        cfg.color_mode = color_mode::SINGLE;
    }
    cfg.effect = e;
    if e == effect::STATIC && cfg.color_mode != color_mode::SINGLE {
        cfg.colors.truncate(1);
        cfg.color_mode = color_mode::SINGLE;
    }
    if e == effect::MATRIX && cfg.color_mode == color_mode::SINGLE {
        cfg.color_mode = color_mode::DUAL;
        let first = cfg.colors[0];
        cfg.colors = vec![first, [255, 255, 255]];
    }
    // directions: Right for Color Wave, Clockwise for Tornado, Down for Matrix
    match e {
        effect::COLOR_WAVE => cfg.direction = 3,
        effect::TORNADO => cfg.direction = 4,
        effect::MATRIX => cfg.direction = 1,
        _ => {}
    }
    if cfg.speed == 0 {
        cfg.speed = 50;
    }
    k.set_lighting(&cfg, 0).map_err(|e| e.to_string())
}

impl ksni::Tray for Tray {
    fn id(&self) -> String {
        "darkmount-linux".into()
    }
    fn title(&self) -> String {
        APP_NAME.into()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        tray_icons()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        let d = if self.dm.ok { "be quiet! Dark Mount" } else { "Keyboard not connected" };
        ksni::ToolTip { title: APP_NAME.into(), description: d.into(), ..Default::default() }
    }
    fn activate(&mut self, _x: i32, _y: i32) {
        self.toggle_window();
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        let mut m: Vec<ksni::MenuItem<Self>> = vec![];
        m.push(StandardItem { label: format!("Open {APP_NAME}"), activate: Box::new(|t: &mut Self| t.show()), ..Default::default() }.into());
        m.push(ksni::MenuItem::Separator);
        let ok = self.dm.ok;
        m.push(CheckmarkItem {
            label: "Backlight".into(), enabled: ok, checked: self.dm.light,
            activate: Box::new(|t: &mut Self| {
                let on = !t.dm.light;
                t.dm(move |k| k.set_lighting_mode(if on { lighting_mode::GENERAL } else { lighting_mode::OFF }).map_err(|e| e.to_string()));
            }),
            ..Default::default()
        }.into());
        m.push(SubMenu {
            label: "Effect".into(), enabled: ok,
            submenu: vec![RadioGroup {
                selected: self.dm.effect.and_then(|e| EFFECTS.iter().position(|x| x.1 == e)).unwrap_or(usize::MAX),
                select: Box::new(|t: &mut Self, i| {
                    let e = EFFECTS[i].1;
                    t.dm(move |k| set_effect(k, e));
                }),
                options: EFFECTS.iter().map(|(l, _)| RadioItem { label: (*l).into(), ..Default::default() }).collect(),
            }.into()],
            ..Default::default()
        }.into());
        m.push(SubMenu {
            label: "Brightness".into(), enabled: ok,
            submenu: vec![RadioGroup {
                selected: BRIGHTNESS.iter().position(|&b| b == self.dm.brightness).unwrap_or(usize::MAX),
                select: Box::new(|t: &mut Self, i| {
                    let v = BRIGHTNESS[i];
                    t.dm(move |k| {
                        let mut cfg: LightingConfig = k.lighting(0).map_err(|e| e.to_string())?;
                        cfg.brightness = v;
                        k.set_lighting(&cfg, 0).map_err(|e| e.to_string())
                    });
                }),
                options: BRIGHTNESS.iter().map(|b| RadioItem { label: format!("{b} %"), ..Default::default() }).collect(),
            }.into()],
            ..Default::default()
        }.into());
        m.push(CheckmarkItem {
            label: "Game mode".into(), enabled: ok, checked: self.dm.game,
            activate: Box::new(|t: &mut Self| {
                let on = !t.dm.game;
                t.dm(move |k| k.set_game_mode(on).map_err(|e| e.to_string()));
            }),
            ..Default::default()
        }.into());
        m.push(ksni::MenuItem::Separator);
        m.push(StandardItem {
            label: "Sync dock clock".into(), enabled: ok,
            activate: Box::new(|t: &mut Self| {
                if t.kb.call(|k| k.sync_media_dock_time().map_err(|e| e.to_string())).is_some() {
                    notify("Dark Mount", "Dock clock synced");
                }
            }),
            ..Default::default()
        }.into());
        m.push(ksni::MenuItem::Separator);
        m.push(StandardItem {
            label: "Quit".into(), icon_name: "application-exit".into(),
            activate: Box::new(|_: &mut Self| {
                let _ = slint::invoke_from_event_loop(|| {
                    let _ = slint::quit_event_loop();
                });
            }),
            ..Default::default()
        }.into());
        m
    }
}

pub fn spawn(ui: Weak<MainWindow>, kb: KbWorker) -> Result<(), ksni::Error> {
    let handle = Tray { ui, kb: kb.clone(), dm: DmState::default() }.spawn()?;
    thread::Builder::new().name("tray".into()).spawn(move || {
        thread::sleep(Duration::from_millis(1500));
        loop {
            let dm = read_dm(&kb);
            if handle.update(move |t| t.dm = dm).is_none() {
                return;
            }
            thread::sleep(Duration::from_secs(60));
        }
    }).ok();
    Ok(())
}

/// The tray icon: an orange keyboard outline (the app's accent colour), as ARGB32 pixmaps.
fn tray_icons() -> Vec<ksni::Icon> {
    const SVG: &str = r##"<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 24 24" fill="none" stroke="#ee7f00" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect x="1.5" y="5.5" width="21" height="13" rx="2"/><path d="M5.5 9.5h1M9 9.5h1M12.5 9.5h1M16 9.5h1M5.5 12.5h1M9 12.5h1M12.5 12.5h1M16 12.5h2.5M7 15.5h10"/></svg>"##;
    let Ok(tree) = resvg::usvg::Tree::from_str(SVG, &resvg::usvg::Options::default()) else { return vec![] };
    [16u32, 22, 24, 32, 48, 64].iter().filter_map(|&n| {
        let mut pix = resvg::tiny_skia::Pixmap::new(n, n)?;
        let k = n as f32 / 24.0;
        resvg::render(&tree, resvg::tiny_skia::Transform::from_scale(k, k), &mut pix.as_mut());
        // premultiplied RGBA -> straight ARGB (network byte order)
        let data = pix.data().as_chunks::<4>().0.iter().flat_map(|p| {
            let a = p[3] as u32;
            let un = |c: u8| (c as u32 * 255 + a / 2).checked_div(a).map_or(0, |v| v.min(255) as u8);
            [p[3], un(p[0]), un(p[1]), un(p[2])]
        }).collect();
        Some(ksni::Icon { width: n as i32, height: n as i32, data })
    }).collect()
}
