use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rgba {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Rgba {
    pub const fn new(r: f32, g: f32, b: f32, a: f32) -> Self {
        Self { r, g, b, a }
    }

    pub fn from_hex(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('#');
        let v = u32::from_str_radix(s, 16).ok()?;
        match s.len() {
            6 => Some(Self::new(
                ((v >> 16) & 0xff) as f32 / 255.0,
                ((v >> 8) & 0xff) as f32 / 255.0,
                (v & 0xff) as f32 / 255.0,
                1.0,
            )),
            8 => Some(Self::new(
                ((v >> 24) & 0xff) as f32 / 255.0,
                ((v >> 16) & 0xff) as f32 / 255.0,
                ((v >> 8) & 0xff) as f32 / 255.0,
                (v & 0xff) as f32 / 255.0,
            )),
            _ => None,
        }
    }

    pub fn from_hsv(h: f32, s: f32, v: f32) -> Self {
        let h = h.rem_euclid(360.0);
        let c = v * s;
        let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
        let m = v - c;
        let (r, g, b) = match (h / 60.0) as u32 {
            0 => (c, x, 0.0),
            1 => (x, c, 0.0),
            2 => (0.0, c, x),
            3 => (0.0, x, c),
            4 => (x, 0.0, c),
            _ => (c, 0.0, x),
        };
        Self::new(r + m, g + m, b + m, 1.0)
    }

    pub fn lerp(self, other: Self, t: f32) -> Self {
        let t = t.clamp(0.0, 1.0);
        Self::new(
            self.r + (other.r - self.r) * t,
            self.g + (other.g - self.g) * t,
            self.b + (other.b - self.b) * t,
            self.a + (other.a - self.a) * t,
        )
    }

    pub fn to_hsv(self) -> (f32, f32, f32) {
        let max = self.r.max(self.g).max(self.b);
        let min = self.r.min(self.g).min(self.b);
        let delta = max - min;

        let hue = if delta <= f32::EPSILON {
            0.0
        } else if max == self.r {
            60.0 * (((self.g - self.b) / delta) % 6.0)
        } else if max == self.g {
            60.0 * ((self.b - self.r) / delta + 2.0)
        } else {
            60.0 * ((self.r - self.g) / delta + 4.0)
        };
        let saturation = if max <= f32::EPSILON {
            0.0
        } else {
            delta / max
        };
        (hue.rem_euclid(360.0), saturation, max)
    }

    pub fn to_hex(self) -> String {
        let [r, g, b, _] = self.to_rgba8();
        format!("#{r:02x}{g:02x}{b:02x}")
    }

    pub fn to_rgba8(self) -> [u8; 4] {
        [
            (self.r.clamp(0.0, 1.0) * 255.0) as u8,
            (self.g.clamp(0.0, 1.0) * 255.0) as u8,
            (self.b.clamp(0.0, 1.0) * 255.0) as u8,
            (self.a.clamp(0.0, 1.0) * 255.0) as u8,
        ]
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum ColorMode {
    Fixed {
        color: String,
    },
    Palette {
        colors: Vec<String>,
    },
    Gradient {
        from: String,
        to: String,
    },
    /// Hue cycles over time; saturation/value fixed.
    Rainbow {
        #[serde(default = "default_rainbow_speed")]
        speed: f32,
        #[serde(default = "default_unit")]
        saturation: f32,
        #[serde(default = "default_unit")]
        value: f32,
    },
}

fn default_rainbow_speed() -> f32 {
    120.0
}
fn default_unit() -> f32 {
    1.0
}

impl Default for ColorMode {
    fn default() -> Self {
        ColorMode::Palette {
            colors: vec![
                "#ff2d95".into(),
                "#ff9f1c".into(),
                "#2de2e6".into(),
                "#a06cff".into(),
                "#f9f871".into(),
            ],
        }
    }
}

/// Editing surface used by `imlec tune`. Every mode is presented as a list of
/// colour slots plus an HSV triple, so one set of controls drives all four.
impl ColorMode {
    pub fn mode_name(&self) -> &'static str {
        match self {
            ColorMode::Fixed { .. } => "fixed",
            ColorMode::Palette { .. } => "palette",
            ColorMode::Gradient { .. } => "gradient",
            ColorMode::Rainbow { .. } => "rainbow",
        }
    }

    /// Cycles between modes, carrying the first colour across so the change is
    /// visible rather than jarring.
    pub fn cycle(&mut self, forward: bool) {
        let carry = self.slot(0).unwrap_or(Rgba::new(1.0, 0.2, 0.6, 1.0));
        let hex = carry.to_hex();
        let order = ["fixed", "palette", "gradient", "rainbow"];
        let current = order
            .iter()
            .position(|m| *m == self.mode_name())
            .unwrap_or(0);
        let next = if forward {
            (current + 1) % order.len()
        } else {
            (current + order.len() - 1) % order.len()
        };
        *self = match order[next] {
            "fixed" => ColorMode::Fixed { color: hex },
            "palette" => ColorMode::Palette {
                colors: vec![hex.clone(), hex],
            },
            "gradient" => ColorMode::Gradient {
                from: hex,
                to: "#2de2e6".into(),
            },
            _ => ColorMode::Rainbow {
                speed: default_rainbow_speed(),
                saturation: 1.0,
                value: 1.0,
            },
        };
    }

    /// Number of editable colour slots. Rainbow has none; its HSV is generated.
    pub fn slot_count(&self) -> usize {
        match self {
            ColorMode::Fixed { .. } => 1,
            ColorMode::Palette { colors } => colors.len(),
            ColorMode::Gradient { .. } => 2,
            ColorMode::Rainbow { .. } => 0,
        }
    }

    pub fn slot(&self, index: usize) -> Option<Rgba> {
        let hex = match self {
            ColorMode::Fixed { color } if index == 0 => color,
            ColorMode::Palette { colors } => colors.get(index)?,
            ColorMode::Gradient { from, .. } if index == 0 => from,
            ColorMode::Gradient { to, .. } if index == 1 => to,
            _ => return None,
        };
        Rgba::from_hex(hex)
    }

    pub fn set_slot(&mut self, index: usize, color: Rgba) {
        let hex = color.to_hex();
        match self {
            ColorMode::Fixed { color } if index == 0 => *color = hex,
            ColorMode::Palette { colors } => {
                if let Some(slot) = colors.get_mut(index) {
                    *slot = hex;
                }
            }
            ColorMode::Gradient { from, .. } if index == 0 => *from = hex,
            ColorMode::Gradient { to, .. } if index == 1 => *to = hex,
            _ => {}
        }
    }

    /// Only palettes are variable length; other modes ignore this.
    pub fn add_slot(&mut self) {
        if let ColorMode::Palette { colors } = self {
            let seed = colors.last().cloned().unwrap_or_else(|| "#ff2d95".into());
            colors.push(seed);
        }
    }

    pub fn remove_slot(&mut self, index: usize) {
        if let ColorMode::Palette { colors } = self {
            if colors.len() > 1 && index < colors.len() {
                colors.remove(index);
            }
        }
    }

    /// Rainbow exposes its own knobs instead of slots.
    pub fn rainbow_mut(&mut self) -> Option<(&mut f32, &mut f32, &mut f32)> {
        match self {
            ColorMode::Rainbow {
                speed,
                saturation,
                value,
            } => Some((speed, saturation, value)),
            _ => None,
        }
    }

    /// `elapsed` drives time-based modes; jitter picks per-particle variation.
    pub fn resolve(&self, elapsed: f32) -> Rgba {
        match self {
            ColorMode::Fixed { color } => {
                Rgba::from_hex(color).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0))
            }
            ColorMode::Palette { colors } => {
                if colors.is_empty() {
                    return Rgba::new(1.0, 1.0, 1.0, 1.0);
                }
                let i = fastrand::usize(..colors.len());
                Rgba::from_hex(&colors[i]).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0))
            }
            ColorMode::Gradient { from, to } => {
                let a = Rgba::from_hex(from).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
                let b = Rgba::from_hex(to).unwrap_or(Rgba::new(1.0, 1.0, 1.0, 1.0));
                a.lerp(b, fastrand::f32())
            }
            ColorMode::Rainbow {
                speed,
                saturation,
                value,
            } => Rgba::from_hsv(
                elapsed * speed + fastrand::f32() * 20.0,
                *saturation,
                *value,
            ),
        }
    }
}
