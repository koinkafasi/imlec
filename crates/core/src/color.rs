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

impl ColorMode {
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
