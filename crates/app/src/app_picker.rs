//! "Choose a program" for any page: installed programs with icons and search, or
//! any file. The opener gets the command that starts the choice.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use darkmount_core::apps::{self, App};
use slint::{ComponentHandle, Image, ModelRc, VecModel, Weak};

use crate::{AppItem, AppPicker, MainWindow};

type OnPick = Box<dyn Fn(String)>;

pub struct AppPickerCtl {
    ui: Weak<MainWindow>,
    apps: RefCell<Vec<App>>,
    shown: RefCell<Vec<usize>>,
    icons: RefCell<HashMap<String, Image>>,
    on_pick: RefCell<Option<OnPick>>,
}

/// Quote for a POSIX shell (the command runs through `sh -c`).
pub fn shell_quote(s: &str) -> String {
    if !s.is_empty() && s.chars().all(|c| c.is_ascii_alphanumeric() || "/._-+:@%".contains(c)) {
        s.to_string()
    } else {
        format!("'{}'", s.replace('\'', "'\\''"))
    }
}

impl AppPickerCtl {
    pub fn new(ui: &MainWindow) -> Rc<AppPickerCtl> {
        let ctl = Rc::new(AppPickerCtl {
            ui: ui.as_weak(),
            apps: RefCell::new(Vec::new()),
            shown: RefCell::new(Vec::new()),
            icons: RefCell::new(HashMap::new()),
            on_pick: RefCell::new(None),
        });
        let g = ui.global::<AppPicker>();
        let c = Rc::downgrade(&ctl);
        g.on_search(move |q| {
            if let Some(c) = c.upgrade() {
                c.filter(&q);
            }
        });
        let c = Rc::downgrade(&ctl);
        g.on_chosen(move |i| {
            if let Some(c) = c.upgrade() {
                let idx = c.shown.borrow().get(i as usize).copied();
                if let Some(idx) = idx {
                    let app = c.apps.borrow()[idx].clone();
                    let cmd = if app.terminal {
                        format!("konsole -e {}", app.command)
                    } else {
                        app.command
                    };
                    c.pick(cmd);
                }
            }
        });
        let c = Rc::downgrade(&ctl);
        g.on_cancel(move || {
            if let Some(c) = c.upgrade() {
                c.close();
            }
        });
        let weak = ui.as_weak();
        g.on_browse(move || {
            let weak = weak.clone();
            std::thread::spawn(move || {
                let home = std::env::var("HOME").unwrap_or_else(|_| "/".into());
                let out = std::process::Command::new("kdialog")
                    .args(["--getopenfilename", &home])
                    .output()
                    .or_else(|_| std::process::Command::new("zenity")
                        .arg("--file-selection").output());
                let Ok(out) = out else { return };
                let path = String::from_utf8_lossy(&out.stdout).trim().to_string();
                if out.status.success() && !path.is_empty() {
                    let _ = weak.upgrade_in_event_loop(move |_| {
                        PICKER.with(|p| {
                            if let Some(c) = p.borrow().as_ref() {
                                c.pick(shell_quote(&path));
                            }
                        });
                    });
                }
            });
        });
        PICKER.with(|p| *p.borrow_mut() = Some(ctl.clone()));
        ctl
    }

    /// Show the picker; `on_pick` gets the command for the program chosen.
    pub fn open(&self, on_pick: impl Fn(String) + 'static) {
        *self.on_pick.borrow_mut() = Some(Box::new(on_pick));
        *self.apps.borrow_mut() = apps::installed();
        let Some(ui) = self.ui.upgrade() else { return };
        let g = ui.global::<AppPicker>();
        g.set_total(self.apps.borrow().len() as i32);
        g.set_query("".into());
        self.filter("");
        g.set_open(true);
    }

    fn icon(&self, name: &str) -> Image {
        if let Some(i) = self.icons.borrow().get(name) {
            return i.clone();
        }
        let img = apps::icon_path(name).and_then(|p| Image::load_from_path(&p).ok())
            .unwrap_or_default();
        self.icons.borrow_mut().insert(name.to_string(), img.clone());
        img
    }

    fn filter(&self, query: &str) {
        let Some(ui) = self.ui.upgrade() else { return };
        let apps = self.apps.borrow();
        let shown: Vec<usize> = (0..apps.len()).filter(|&i| apps[i].matches(query)).collect();
        let items: Vec<AppItem> = shown.iter().map(|&i| {
            let a = &apps[i];
            AppItem { name: a.name.clone().into(), comment: a.comment.clone().into(),
                      icon: self.icon(&a.icon), command: a.command.clone().into() }
        }).collect();
        drop(apps);
        *self.shown.borrow_mut() = shown;
        ui.global::<AppPicker>().set_items(ModelRc::new(VecModel::from(items)));
    }

    fn pick(&self, command: String) {
        if let Some(f) = self.on_pick.borrow().as_ref() {
            f(command);
        }
        self.close();
    }

    fn close(&self) {
        *self.on_pick.borrow_mut() = None;
        if let Some(ui) = self.ui.upgrade() {
            ui.global::<AppPicker>().set_open(false);
        }
    }
}

thread_local! {
    static PICKER: RefCell<Option<Rc<AppPickerCtl>>> = const { RefCell::new(None) };
}

/// Open the picker from anywhere on the UI thread.
pub fn open(on_pick: impl Fn(String) + 'static) {
    PICKER.with(|p| {
        if let Some(c) = p.borrow().as_ref() {
            c.open(on_pick);
        }
    });
}

#[cfg(test)]
mod tests {
    use super::shell_quote;

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("/usr/bin/konsole"), "/usr/bin/konsole");
        assert_eq!(shell_quote("/run/media/x/My App.AppImage"), "'/run/media/x/My App.AppImage'");
        assert_eq!(shell_quote("it's"), "'it'\\''s'");
    }
}
