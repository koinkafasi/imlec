use crate::color::ColorMode;
use crate::shape::Shape;
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub general: General,
    pub typing: Emitter,
    pub deleting: Emitter,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub enabled: bool,
    pub fps: u32,
    /// Hard cap on simultaneously live particles. Oldest are recycled past this.
    pub max_particles: usize,
    /// Reference cursor height in px. Particle size is a ratio of this.
    pub cursor_height_px: f32,
    /// Nudge the emission point relative to the pointer hotspot.
    pub offset_x: f32,
    pub offset_y: f32,
    /// Consecutive keystrokes ramp up particle count, power-mode style.
    pub combo_enabled: bool,
    pub combo_window_ms: u64,
    pub combo_max_multiplier: f32,
    /// Keystrokes closer together than this emit nothing (protects against key repeat floods).
    pub min_emit_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Emitter {
    pub enabled: bool,
    pub count: u32,
    pub shape: Shape,
    pub color: ColorMode,
    /// Particle size = general.cursor_height_px * size_ratio.
    pub size_ratio: f32,
    /// 0.0 = uniform size, 1.0 = size varies across the full range.
    pub size_jitter: f32,
    pub lifetime_ms: u64,
    pub lifetime_jitter: f32,
    /// Initial speed in px/s.
    pub speed: f32,
    pub speed_jitter: f32,
    /// Emission cone centre, degrees. 0 = right, -90 = up.
    pub direction_deg: f32,
    /// Cone width, degrees. 360 = omnidirectional.
    pub spread_deg: f32,
    /// Downward acceleration, px/s^2. Negative floats particles upward.
    pub gravity: f32,
    /// Velocity damping per second.
    pub drag: f32,
    /// Radians per second.
    pub rotation_speed: f32,
    /// Particles shrink to nothing over their lifetime.
    pub shrink: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            enabled: true,
            fps: 60,
            max_particles: 600,
            cursor_height_px: 20.0,
            offset_x: 0.0,
            offset_y: 0.0,
            combo_enabled: true,
            combo_window_ms: 600,
            combo_max_multiplier: 2.5,
            min_emit_interval_ms: 16,
        }
    }
}

impl Default for Emitter {
    fn default() -> Self {
        Self {
            enabled: true,
            count: 6,
            shape: Shape::Circle,
            color: ColorMode::default(),
            size_ratio: 0.5,
            size_jitter: 0.4,
            lifetime_ms: 500,
            lifetime_jitter: 0.3,
            speed: 130.0,
            speed_jitter: 0.6,
            direction_deg: -90.0,
            spread_deg: 140.0,
            gravity: 320.0,
            drag: 1.8,
            rotation_speed: 3.0,
            shrink: true,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            general: General::default(),
            typing: Emitter::default(),
            deleting: Emitter {
                count: 8,
                shape: Shape::Spark,
                color: ColorMode::Fixed {
                    color: "#ff3b30".into(),
                },
                speed: 90.0,
                direction_deg: 90.0,
                spread_deg: 360.0,
                gravity: -60.0,
                lifetime_ms: 340,
                ..Emitter::default()
            },
        }
    }
}

impl Config {
    pub fn config_path() -> Result<PathBuf> {
        let dirs = directories::ProjectDirs::from("", "", "particle-cursor")
            .context("no valid home directory")?;
        Ok(dirs.config_dir().join("config.toml"))
    }

    /// Loads the config, writing out a commented default if none exists.
    pub fn load_or_init() -> Result<Self> {
        let path = Self::config_path()?;
        if !path.exists() {
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&path, DEFAULT_TOML)?;
            return Ok(Self::default());
        }
        Self::load_from(&path)
    }

    pub fn load_from(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config =
            toml::from_str(&text).with_context(|| format!("parsing {}", path.display()))?;
        Ok(cfg.validated())
    }

    /// Clamps out-of-range values rather than failing, so a bad hand-edit degrades gracefully.
    pub fn validated(mut self) -> Self {
        self.general.fps = self.general.fps.clamp(15, 240);
        self.general.max_particles = self.general.max_particles.clamp(1, 20_000);
        self.general.cursor_height_px = self.general.cursor_height_px.clamp(1.0, 500.0);
        self.general.combo_max_multiplier = self.general.combo_max_multiplier.clamp(1.0, 20.0);
        for e in [&mut self.typing, &mut self.deleting] {
            e.count = e.count.clamp(0, 500);
            e.size_ratio = e.size_ratio.clamp(0.01, 20.0);
            e.size_jitter = e.size_jitter.clamp(0.0, 1.0);
            e.lifetime_ms = e.lifetime_ms.clamp(16, 10_000);
            e.lifetime_jitter = e.lifetime_jitter.clamp(0.0, 1.0);
            e.speed_jitter = e.speed_jitter.clamp(0.0, 1.0);
            e.spread_deg = e.spread_deg.clamp(0.0, 360.0);
            e.drag = e.drag.clamp(0.0, 20.0);
        }
        self
    }
}

pub const DEFAULT_TOML: &str = include_str!("../../../config/default.toml");
