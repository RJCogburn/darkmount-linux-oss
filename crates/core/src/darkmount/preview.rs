//! Lighting preview: the color each key and glow mask shows for an effect at a
//! moment in time (an approximation of the keyboard's own animation).

use std::collections::HashMap;
use std::f32::consts::PI;

pub type Rgb = [u8; 3];

#[derive(Debug, Clone, PartialEq)]
pub struct PreviewSettings {
    pub on: bool,
    /// effect name: "Static", "ColorWave", "Tornado", "Breathing", "Reactive", "Matrix"
    pub effect: String,
    pub colors: Vec<Rgb>,
    pub brightness: u8,
    pub speed: u8,
    /// "Left", "Right", "Up", "Down", "Clockwise", "Counterclockwise"
    pub direction: String,
}

impl Default for PreviewSettings {
    fn default() -> Self {
        PreviewSettings { on: true, effect: "Static".into(), colors: vec![[255, 40, 0]],
                          brightness: 100, speed: 50, direction: "Right".into() }
    }
}

impl PreviewSettings {
    pub fn animated(&self) -> bool {
        self.on && self.effect != "Static"
    }
}

/// Keeps the per-key state animations need (reactive flashes, matrix drops).
pub struct Preview {
    pub settings: PreviewSettings,
    flashes: HashMap<String, f32>,
    drops: HashMap<i32, f32>,
    rng: u64,
    last_step: f32,
}

fn lerp(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let t = t.clamp(0.0, 1.0);
    [0, 1, 2].map(|i| (a[i] as f32 + (b[i] as f32 - a[i] as f32) * t) as u8)
}

fn scale(c: Rgb, k: f32) -> Rgb {
    c.map(|x| (x as f32 * k.clamp(0.0, 1.0)) as u8)
}

impl Preview {
    pub fn new(settings: PreviewSettings) -> Preview {
        Preview { settings, flashes: HashMap::new(), drops: HashMap::new(),
                  rng: 0x9E37_79B9_7F4A_7C15, last_step: 0.0 }
    }

    fn random(&mut self) -> f32 {
        // xorshift64*
        self.rng ^= self.rng >> 12;
        self.rng ^= self.rng << 25;
        self.rng ^= self.rng >> 27;
        ((self.rng.wrapping_mul(0x2545_F491_4F6C_DD1D) >> 40) as f32) / (1u64 << 24) as f32
    }

    fn palette(&self, u: f32) -> Rgb {
        let cs = &self.settings.colors;
        match cs.len() {
            0 => [255, 40, 0],
            1 => cs[0],
            n => {
                let u = u.rem_euclid(1.0) * n as f32;
                let i = u as usize;
                lerp(cs[i % n], cs[(i + 1) % n], u - i as f32)
            }
        }
    }

    /// Advance per-frame state (matrix drops). Call once per frame with the time in
    /// seconds since the preview started.
    pub fn step(&mut self, now: f32) {
        if self.settings.effect == "Matrix" && self.settings.on {
            let dt = (now - self.last_step).clamp(0.0, 0.2) * 25.0;
            let v = 0.01 * (0.3 + self.settings.speed as f32 / 60.0) * dt;
            for d in self.drops.values_mut() {
                *d += v;
            }
        }
        self.last_step = now;
    }

    /// The color at (nx, ny) in 0..1 across the board; None when the light is off.
    /// `key` is the key's name (reactive flashes), None for glow masks.
    pub fn color_at(&mut self, nx: f32, ny: f32, key: Option<&str>, now: f32) -> Option<Rgb> {
        let s = &self.settings;
        if !s.on {
            return None;
        }
        let first = s.colors.first().copied().unwrap_or([255, 40, 0]);
        let second = s.colors.get(1).copied();
        let t = now * (0.05 + s.speed as f32 / 100.0 * 0.6);
        let c = match s.effect.as_str() {
            "Breathing" => {
                let n = s.colors.len().max(1);
                let phase = (t * 0.6).rem_euclid(n as f32);
                let base = s.colors.get(phase as usize % n).copied().unwrap_or(first);
                scale(base, 0.5 - 0.5 * (phase.fract() * 2.0 * PI).cos())
            }
            "ColorWave" => {
                let pos = match s.direction.as_str() {
                    "Left" => nx,
                    "Down" => -ny,
                    "Up" => ny,
                    _ => -nx,
                };
                self.palette(pos + t)
            }
            "Tornado" => {
                let ang = (ny - 0.5).atan2((nx - 0.5) * 2.6) / (2.0 * PI);
                let sign = if s.direction == "Counterclockwise" { -1.0 } else { 1.0 };
                self.palette(ang * sign + t)
            }
            "Reactive" => {
                let speed = s.speed as f32;
                let idle = second.unwrap_or([0, 0, 0]);
                let key = key.unwrap_or("");
                if !key.is_empty() && self.random() < 0.0015 {
                    self.flashes.insert(key.to_string(), now);
                }
                let f = self.flashes.get(key).copied().unwrap_or(-10.0);
                let k = (1.0 - (now - f) / (0.4 + (100.0 - speed) / 100.0)).max(0.0);
                lerp(idle, first, k)
            }
            "Matrix" => {
                let col = (nx * 40.0) as i32;
                let needs_new = self.drops.get(&col).is_none_or(|d| *d > 1.4);
                if needs_new {
                    let start = -self.random() * 1.5;
                    self.drops.insert(col, start);
                }
                let dist = self.drops[&col] - ny;
                let head = second.unwrap_or([255, 255, 255]);
                if (0.0..0.06).contains(&dist) {
                    head
                } else if dist >= 0.06 {
                    lerp([0, 0, 0], first, (0.6 - dist * 2.5).max(0.0))
                } else {
                    [0, 0, 0]
                }
            }
            _ => first,
        };
        Some(scale(c, self.settings.brightness as f32 / 100.0))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn static_and_brightness() {
        let mut p = Preview::new(PreviewSettings { brightness: 50, ..Default::default() });
        assert_eq!(p.color_at(0.3, 0.3, Some("A"), 1.0), Some([127, 20, 0]));
        p.settings.on = false;
        assert_eq!(p.color_at(0.3, 0.3, None, 1.0), None);
    }

    #[test]
    fn wave_moves_through_the_colors() {
        let mut p = Preview::new(PreviewSettings {
            effect: "ColorWave".into(), colors: vec![[255, 0, 0], [0, 0, 255]],
            ..Default::default()
        });
        let a = p.color_at(0.0, 0.5, None, 0.0).unwrap();
        let b = p.color_at(0.5, 0.5, None, 0.0).unwrap();
        assert_ne!(a, b);
        assert_eq!(a, [255, 0, 0]);
    }

    #[test]
    fn breathing_goes_dark_and_bright() {
        let mut p = Preview::new(PreviewSettings { effect: "Breathing".into(),
                                                   ..Default::default() });
        let samples: Vec<u8> = (0..40).map(|i| p.color_at(0.5, 0.5, None, i as f32 * 0.1)
            .unwrap()[0]).collect();
        assert!(samples.iter().any(|&r| r < 30) && samples.iter().any(|&r| r > 220));
    }

    #[test]
    fn matrix_and_reactive_stay_in_range() {
        for eff in ["Matrix", "Reactive", "Tornado"] {
            let mut p = Preview::new(PreviewSettings { effect: eff.into(), ..Default::default() });
            for i in 0..200 {
                let now = i as f32 * 0.04;
                p.step(now);
                assert!(p.color_at((i % 10) as f32 / 10.0, 0.4, Some("K"), now).is_some());
            }
        }
    }
}
