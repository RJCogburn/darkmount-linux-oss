//! The Dark Mount as IO Center draws it: base images for the main board, numpad
//! and media dock, key rectangles and LED glow masks, from the layout files
//! shipped with IO Center (`assets/dark_mount/*.json`).

use std::collections::{HashMap, HashSet};
use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use super::bindings;

/// What the picture is for: the lighting preview, or clicking keys to bind them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Lighting,
    Keys,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Side {
    Left,
    Right,
}

impl Side {
    pub fn parse(s: &str) -> Option<Side> {
        match s {
            "Left" => Some(Side::Left),
            "Right" => Some(Side::Right),
            _ => None,
        }
    }
    fn key(self) -> &'static str {
        match self {
            Side::Left => "Left",
            Side::Right => "Right",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

impl Rect {
    pub fn cx(&self) -> f32 { self.x + self.w / 2.0 }
    pub fn cy(&self) -> f32 { self.y + self.h / 2.0 }
    pub fn contains(&self, x: f32, y: f32, margin: f32) -> bool {
        x >= self.x - margin && x <= self.x + self.w + margin
            && y >= self.y - margin && y <= self.y + self.h + margin
    }
}

/// A base image placed at an x offset.
#[derive(Debug, Clone, PartialEq)]
pub struct Part {
    pub image: PathBuf,
    pub x: f32,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Key {
    /// key name as in `bindings::KEY_IDS` ("Escape", "Image3", "DockMute", ...)
    pub name: String,
    pub rect: Rect,
    pub radius: f32,
}

/// An underglow LED mask image (tinted with the LED's color).
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub image: PathBuf,
    pub rect: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Layout {
    /// drawn in order: main board first, modules on top (as IO Center does)
    pub parts: Vec<Part>,
    pub keys: Vec<Key>,
    pub masks: Vec<Mask>,
    /// the area worth showing (all coordinates are already shifted so parts start at 0)
    pub bounds: Rect,
}

fn load(dir: &Path, name: &str) -> Result<Value, String> {
    let p = dir.join(name);
    let t = fs::read_to_string(&p).map_err(|e| format!("{}: {e}", p.display()))?;
    serde_json::from_str(&t).map_err(|e| format!("{}: {e}", p.display()))
}

fn image_path(dir: &Path, src: &str) -> PathBuf {
    dir.join(src.trim_start_matches("qrc:/manifests/dark_mount/"))
}

fn fnum(v: &Value) -> f32 {
    v.as_f64().unwrap_or(0.0) as f32
}

/// Layout key id ("Key_Escape") -> key name ("Escape").
fn key_names(dir: &Path) -> Result<HashMap<String, String>, String> {
    let m = load(dir, "dark_mount_keys_mapping.json")?;
    let mut ids: HashMap<String, u8> = HashMap::new();
    let mut add = |obj: &Value| {
        if let Some(o) = obj.as_object() {
            for (k, v) in o {
                if let Some(id) = v.as_u64() {
                    ids.insert(k.clone(), id as u8);
                }
            }
        }
    };
    add(&m["base"]);
    if let Some(d) = m["diff"].as_object() {
        for v in d.values() {
            add(v);
        }
    }
    Ok(ids.into_iter().map(|(k, id)| (k, bindings::key_name(id))).collect())
}

/// Build the picture for the given modules and mode.
pub fn build(dir: &Path, numpad: Option<Side>, dock: Option<Side>, mode: Mode,
             visual_layout: &str) -> Result<Layout, String> {
    let names = key_names(dir)?;
    let main_all = load(dir, "dark_mount_main_ansi.json")?;
    let num = load(dir, "dark_mount_numpad.json")?;
    let med = load(dir, "dark_mount_media.json")?;
    let mut parts = Vec::new();
    let mut keys = Vec::new();
    let mut masks = Vec::new();

    let add_keys = |keys: &mut Vec<Key>, list: &Value, ox: f32| {
        for k in list.as_array().into_iter().flatten() {
            let Some(name) = k["id"].as_str().and_then(|id| names.get(id)) else { continue };
            if name.starts_with("key") {
                continue; // not a key we know
            }
            let g = &k["geometry"];
            keys.push(Key {
                name: name.clone(),
                rect: Rect { x: fnum(&g["x"]) + ox, y: fnum(&g["y"]), w: fnum(&g["width"]),
                             h: fnum(&g["height"]) },
                radius: g.get("rx").map_or(3.0, fnum),
            });
        }
    };
    let add_masks = |masks: &mut Vec<Mask>, list: &Value, ox: f32, skip: &HashSet<String>| {
        for m in list.as_array().into_iter().flatten() {
            if m["id"].as_str().is_some_and(|id| skip.contains(id)) {
                continue;
            }
            let r = &m["mask"]["rect"];
            let Some(src) = m["mask"]["source"].as_str() else { continue };
            masks.push(Mask {
                image: image_path(dir, src),
                rect: Rect { x: fnum(&r["x"]) + ox, y: fnum(&r["y"]), w: fnum(&r["width"]),
                             h: fnum(&r["height"]) },
            });
        }
    };

    let main = &main_all["Default"];
    let imgs = &main["layoutImages"];
    let src = imgs[visual_layout].as_str().or_else(|| imgs["US"].as_str())
        .ok_or("layout has no main image")?;
    parts.push(Part { image: image_path(dir, src), x: 0.0 });
    add_keys(&mut keys, &main["keys"], 0.0);

    let mut np_disabled = HashSet::new();
    if let Some(side) = numpad {
        let section = if mode == Mode::Keys && num.get("KeyBinding").is_some_and(|s| s.get("images").is_some()) {
            &num["KeyBinding"]
        } else {
            &num["Default"]
        };
        let ox = num_pos(section, side).or_else(|| num_pos(&num["Default"], side)).unwrap_or(0.0);
        let src = section["images"][side.key()]["Default"].as_str()
            .or_else(|| num["Default"]["images"][side.key()]["Default"].as_str())
            .ok_or("numpad image missing")?;
        parts.push(Part { image: image_path(dir, src), x: ox });
        add_keys(&mut keys, &num["KeyBinding"]["keys"], ox);
        np_disabled = num["Default"]["disabledKeys"][side.key()].as_array().into_iter().flatten()
            .filter_map(|v| v.as_str().map(str::to_string)).collect();
        add_masks(&mut masks, &num["Default"]["ledMasks"], ox, &HashSet::new());
    }
    add_masks(&mut masks, &main["ledMasks"], 0.0, &np_disabled);
    if let Some(side) = dock {
        let section = if mode == Mode::Keys { &med["KeyBinding"] } else { &med["Default"] };
        let ox = num_pos(section, side).unwrap_or(0.0);
        let src = section["images"][side.key()]["Default"].as_str().ok_or("dock image missing")?;
        parts.push(Part { image: image_path(dir, src), x: ox });
        if mode == Mode::Keys {
            add_keys(&mut keys, &section["keys"], ox);
        }
    }

    // shift everything so the leftmost part starts at x = 0
    let x0 = parts.iter().map(|p| p.x).fold(f32::INFINITY, f32::min);
    for p in &mut parts {
        p.x -= x0;
    }
    for k in &mut keys {
        k.rect.x -= x0;
    }
    for m in &mut masks {
        m.rect.x -= x0;
    }
    let bounds = content_bounds(&keys, &masks);
    Ok(Layout { parts, keys, masks, bounds })
}

fn num_pos(section: &Value, side: Side) -> Option<f32> {
    section["positions"][side.key()]["physicalGeometryOffset"]["x"].as_f64().map(|x| x as f32)
}

/// Keys and glow masks, plus room for the case around them.
fn content_bounds(keys: &[Key], masks: &[Mask]) -> Rect {
    let rects = keys.iter().map(|k| k.rect).chain(masks.iter().map(|m| m.rect));
    let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
    for r in rects {
        x0 = x0.min(r.x);
        y0 = y0.min(r.y);
        x1 = x1.max(r.x + r.w);
        y1 = y1.max(r.y + r.h);
    }
    if x0 > x1 {
        return Rect::default();
    }
    let pad = 40.0;
    Rect { x: (x0 - pad).max(0.0), y: (y0 - pad).max(0.0), w: x1 - x0 + 2.0 * pad,
           h: y1 - y0 + 2.0 * pad }
}

impl Layout {
    /// The key under a point in layout coordinates.
    pub fn key_at(&self, x: f32, y: f32) -> Option<&Key> {
        self.keys.iter().find(|k| k.rect.contains(x, y, 2.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assets() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../../assets/dark_mount")
    }

    #[test]
    fn builds_the_desk_layout() {
        let l = build(&assets(), Some(Side::Right), Some(Side::Left), Mode::Lighting, "US")
            .unwrap();
        assert_eq!(l.parts.len(), 3);
        assert!(l.parts.iter().all(|p| p.image.exists()), "{:?}", l.parts);
        assert_eq!(l.parts.iter().map(|p| p.x).fold(f32::MAX, f32::min), 0.0);
        let names: HashSet<&str> = l.keys.iter().map(|k| k.name.as_str()).collect();
        for k in ["Escape", "Space", "Num5", "NumEnter", "F12"] {
            assert!(names.contains(k), "{k}");
        }
        assert!(!names.contains("DockMute")); // dock keys only in key mode
        assert!(l.masks.iter().all(|m| m.image.exists()));
        // numpad on the right hides the main board's right-edge glow
        assert!(l.masks.len() < 64 + 32);
        let esc = l.keys.iter().find(|k| k.name == "Escape").unwrap();
        assert_eq!(l.key_at(esc.rect.cx(), esc.rect.cy()).unwrap().name, "Escape");
    }

    #[test]
    fn key_mode_adds_dock_keys() {
        let l = build(&assets(), None, Some(Side::Left), Mode::Keys, "US").unwrap();
        let names: HashSet<&str> = l.keys.iter().map(|k| k.name.as_str()).collect();
        assert!(names.contains("DockMute") && names.contains("DockPlayPause"));
        assert!(!names.contains("Num5"));
    }
}
