//! Platform-independent particle engine, config model and colour/shape maths.
//! Backends supply input events and a way to draw; everything else lives here.

pub mod color;
pub mod config;
pub mod input;
pub mod particle;
pub mod render;
pub mod shape;

pub use color::{ColorMode, Rgba};
pub use config::{Config, Emitter, General};
pub use input::KeyClass;
pub use particle::{EmitKind, Particle, ParticleSystem};
pub use render::{DirtyRect, Renderer};
pub use shape::Shape;

/// Screen-space rectangle in global (virtual desktop) coordinates.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn contains(&self, x: f32, y: f32) -> bool {
        x >= self.x && y >= self.y && x < self.x + self.width && y < self.y + self.height
    }

    /// True if a particle of `size` could still touch this rect.
    pub fn intersects_point(&self, x: f32, y: f32, size: f32) -> bool {
        let r = size * 0.5;
        x + r >= self.x
            && y + r >= self.y
            && x - r < self.x + self.width
            && y - r < self.y + self.height
    }
}
