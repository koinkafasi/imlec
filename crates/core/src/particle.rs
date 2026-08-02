use crate::color::Rgba;
use crate::config::{Config, Emitter};
use crate::shape::Shape;
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EmitKind {
    Typing,
    Deleting,
}

#[derive(Debug, Clone, Copy)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub size: f32,
    pub color: Rgba,
    pub shape: Shape,
    pub rotation: f32,
    vx: f32,
    vy: f32,
    base_size: f32,
    base_alpha: f32,
    rot_speed: f32,
    age: f32,
    life: f32,
    gravity: f32,
    drag: f32,
    shrink: bool,
}

impl Particle {
    fn integrate(&mut self, dt: f32) -> bool {
        self.age += dt;
        if self.age >= self.life {
            return false;
        }
        let damping = (1.0 - self.drag * dt).max(0.0);
        self.vx *= damping;
        self.vy = self.vy * damping + self.gravity * dt;
        self.x += self.vx * dt;
        self.y += self.vy * dt;
        self.rotation += self.rot_speed * dt;

        let t = self.age / self.life;
        // Quadratic falloff reads as a snappier fade than linear at short lifetimes.
        self.color.a = self.base_alpha * (1.0 - t) * (1.0 - t);
        self.size = if self.shrink {
            self.base_size * (1.0 - t * 0.85)
        } else {
            self.base_size
        };
        true
    }
}

pub struct ParticleSystem {
    config: Config,
    particles: Vec<Particle>,
    combo: u32,
    last_key: Option<Instant>,
    last_emit: Option<Instant>,
    start: Instant,
}

impl ParticleSystem {
    pub fn new(config: Config) -> Self {
        let cap = config.general.max_particles;
        Self {
            config,
            particles: Vec::with_capacity(cap),
            combo: 0,
            last_key: None,
            last_emit: None,
            start: Instant::now(),
        }
    }

    pub fn config(&self) -> &Config {
        &self.config
    }

    pub fn set_config(&mut self, config: Config) {
        self.particles.clear();
        self.particles.reserve(config.general.max_particles);
        self.config = config;
    }

    pub fn particles(&self) -> &[Particle] {
        &self.particles
    }

    pub fn is_idle(&self) -> bool {
        self.particles.is_empty()
    }

    pub fn combo(&self) -> u32 {
        self.combo
    }

    /// Spawns a burst at the given screen position. Returns false if the burst was
    /// suppressed (disabled, or key repeat firing faster than min_emit_interval_ms).
    pub fn emit(&mut self, kind: EmitKind, x: f32, y: f32) -> bool {
        let g = &self.config.general;
        if !g.enabled {
            return false;
        }
        let now = Instant::now();
        if let Some(last) = self.last_emit {
            if now.duration_since(last) < Duration::from_millis(g.min_emit_interval_ms) {
                return false;
            }
        }

        let combo_window = Duration::from_millis(g.combo_window_ms);
        self.combo = match self.last_key {
            Some(last) if now.duration_since(last) <= combo_window => self.combo.saturating_add(1),
            _ => 1,
        };
        self.last_key = Some(now);
        self.last_emit = Some(now);

        let emitter = match kind {
            EmitKind::Typing => &self.config.typing,
            EmitKind::Deleting => &self.config.deleting,
        };
        if !emitter.enabled || emitter.count == 0 {
            return false;
        }

        let multiplier = if g.combo_enabled {
            (1.0 + self.combo as f32 * 0.05).min(g.combo_max_multiplier)
        } else {
            1.0
        };
        let count = ((emitter.count as f32 * multiplier).round() as usize).max(1);

        let ox = x + g.offset_x;
        let oy = y + g.offset_y;
        let elapsed = self.start.elapsed().as_secs_f32();
        let base_size = g.cursor_height_px * emitter.size_ratio;
        let max = g.max_particles;

        for _ in 0..count {
            let p = spawn(emitter, ox, oy, base_size, elapsed);
            if self.particles.len() >= max {
                // Budget reached: overwrite the oldest particle rather than growing.
                self.particles.remove(0);
            }
            self.particles.push(p);
        }
        true
    }

    pub fn update(&mut self, dt: f32) {
        let dt = dt.clamp(0.0, 0.1);
        let mut i = 0;
        while i < self.particles.len() {
            if self.particles[i].integrate(dt) {
                i += 1;
            } else {
                self.particles.swap_remove(i);
            }
        }
        if let Some(last) = self.last_key {
            if last.elapsed() > Duration::from_millis(self.config.general.combo_window_ms) {
                self.combo = 0;
            }
        }
    }

    pub fn clear(&mut self) {
        self.particles.clear();
        self.combo = 0;
    }
}

fn spawn(e: &Emitter, x: f32, y: f32, base_size: f32, elapsed: f32) -> Particle {
    let dir = e.direction_deg.to_radians();
    let spread = e.spread_deg.to_radians();
    let angle = dir + (fastrand::f32() - 0.5) * spread;
    let speed = e.speed * (1.0 - e.speed_jitter + fastrand::f32() * e.speed_jitter * 2.0);
    let size = base_size * (1.0 - e.size_jitter * 0.5 + fastrand::f32() * e.size_jitter);
    let life = e.lifetime_ms as f32 / 1000.0
        * (1.0 - e.lifetime_jitter * 0.5 + fastrand::f32() * e.lifetime_jitter);
    let color = e.color.resolve(elapsed);

    Particle {
        x,
        y,
        vx: angle.cos() * speed,
        vy: angle.sin() * speed,
        size,
        base_size: size,
        base_alpha: color.a,
        color,
        shape: e.shape,
        rotation: fastrand::f32() * std::f32::consts::TAU,
        rot_speed: (fastrand::f32() - 0.5) * 2.0 * e.rotation_speed,
        age: 0.0,
        life: life.max(0.016),
        gravity: e.gravity,
        drag: e.drag,
        shrink: e.shrink,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn particles_expire() {
        let mut sys = ParticleSystem::new(Config::default());
        assert!(sys.emit(EmitKind::Typing, 100.0, 100.0));
        assert!(!sys.is_idle());
        for _ in 0..200 {
            sys.update(0.05);
        }
        assert!(sys.is_idle());
    }

    #[test]
    fn respects_max_particles() {
        let mut cfg = Config::default();
        cfg.general.max_particles = 10;
        cfg.general.min_emit_interval_ms = 0;
        cfg.typing.count = 8;
        let mut sys = ParticleSystem::new(cfg);
        for _ in 0..20 {
            sys.emit(EmitKind::Typing, 0.0, 0.0);
        }
        assert!(sys.particles().len() <= 10);
    }

    #[test]
    fn disabled_emits_nothing() {
        let mut cfg = Config::default();
        cfg.general.enabled = false;
        let mut sys = ParticleSystem::new(cfg);
        assert!(!sys.emit(EmitKind::Typing, 0.0, 0.0));
        assert!(sys.is_idle());
    }
}
