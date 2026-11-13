//! darkmount-linux: desktop app for the be quiet! Dark Mount keyboard. One window
//! (the keyboard page), a tray icon, and a single running copy.

mod app_picker;
mod darkmount_page;
mod instance;
mod snapshot;
mod tray;

use std::time::Duration;

use darkmount_core::{hotplug, presence, udev};
use slint::ComponentHandle;

slint::include_modules!();

pub const APP_NAME: &str = "darkmount-linux";

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let start_hidden = std::env::args().any(|a| a == "--tray");
    // another copy running? ask it to show itself and stop here
    let listener = match instance::claim()? {
        Some(l) => l,
        None => return Ok(()),
    };

    let _ = slint::set_xdg_app_id("darkmount-linux");
    let ui = MainWindow::new()?;
    ui.set_version(concat!("v", env!("CARGO_PKG_VERSION")).into());
    let _picker = app_picker::AppPickerCtl::new(&ui);

    // the keyboard page: loads now if the keyboard is here, else when it's plugged in
    let dm = darkmount_page::DarkMountPage::new(&ui);
    {
        let dm = dm.clone();
        ui.on_find_device(move || dm.load());
    }
    {
        let weak = ui.as_weak();
        ui.on_grant_access(move || {
            let Some(ui) = weak.upgrade() else { return };
            ui.set_dm_status("Waiting for your password…".into());
            let weak = weak.clone();
            // pkexec blocks: run it off the UI thread, then reconnect
            std::thread::spawn(move || {
                let r = udev::install();
                let _ = slint::invoke_from_event_loop(move || {
                    let Some(ui) = weak.upgrade() else { return };
                    match r {
                        Ok(()) => {
                            ui.set_dm_status("Access granted — connecting…".into());
                            slint::Timer::single_shot(Duration::from_millis(1500), darkmount_page::reload);
                        }
                        Err(e) => ui.set_dm_status(format!("Couldn't install the udev rule: {e}").into()),
                    }
                });
            });
        });
    }
    if presence::keyboard_present() {
        dm.load();
    } else {
        ui.set_dm_status("Plug in the keyboard (it's found automatically).".into());
    }

    // plug and unplug: kernel events, plus a slow safety-net poll
    let refresh = move || {
        let here = presence::keyboard_present();
        let _ = slint::invoke_from_event_loop(move || darkmount_page::presence_changed(here));
    };
    let hotplug_ok = hotplug::watch(refresh).is_ok();
    let poll = slint::Timer::default();
    poll.start(slint::TimerMode::Repeated, Duration::from_secs(if hotplug_ok { 15 } else { 3 }), refresh);

    // other copies started later: show this window
    let weak = ui.as_weak();
    instance::serve(listener, move || {
        let weak = weak.clone();
        let _ = slint::invoke_from_event_loop(move || {
            if let Some(ui) = weak.upgrade() {
                let _ = ui.show();
            }
        });
    });

    // tray icon; closing the window only hides it while the tray is there
    let tray_ok = tray::spawn(ui.as_weak(), dm.worker()).is_ok();
    ui.window().on_close_requested(move || {
        if !tray_ok {
            let _ = slint::quit_event_loop();
        }
        slint::CloseRequestResponse::HideWindow
    });

    // development: DARKMOUNT_PANEL=2 opens a panel, DARKMOUNT_SNAPSHOT=file.png saves
    // the window as an image and quits
    if let Some(panel) = std::env::var("DARKMOUNT_PANEL").ok().and_then(|p| p.parse::<i32>().ok()) {
        let weak = ui.as_weak();
        slint::Timer::single_shot(Duration::from_secs(2), move || {
            if let Some(ui) = weak.upgrade() {
                ui.set_dm_panel(panel);
                ui.invoke_dm_panel_changed(panel);
            }
        });
    }
    if let Ok(path) = std::env::var("DARKMOUNT_SNAPSHOT") {
        let weak = ui.as_weak();
        let delay = std::env::var("DARKMOUNT_SNAPSHOT_DELAY").ok().and_then(|d| d.parse().ok()).unwrap_or(6);
        slint::Timer::single_shot(Duration::from_secs(delay), move || {
            if let Some(ui) = weak.upgrade() {
                match ui.window().take_snapshot() {
                    Ok(buf) => {
                        if let Err(e) = snapshot::save_png(&path, &buf) {
                            eprintln!("snapshot: {e}");
                        }
                    }
                    Err(e) => eprintln!("snapshot: {e}"),
                }
            }
            let _ = slint::quit_event_loop();
        });
    }
    if !(start_hidden && tray_ok) {
        ui.show()?;
    }
    slint::run_event_loop_until_quit()?;
    Ok(())
}
