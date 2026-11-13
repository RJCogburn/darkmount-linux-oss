//! darkmount-linux core: everything that isn't interface code. The `darkmount`
//! command-line tool and the desktop app are both built on it.
//!
//! Settings live in the same files the Python version used (`~/.config/darkmount`,
//! `~/.local/share/darkmount`), so upgrading keeps every setting.

pub mod apps;
pub mod autostart;
pub mod config;
pub mod darkmount;
pub mod hostenv;
pub mod hotplug;
pub mod paths;
pub mod presence;
pub mod service;
pub mod udev;
