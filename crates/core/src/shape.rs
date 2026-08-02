use serde::{Deserialize, Serialize};

pub const MAX_VERTS: usize = 12;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Shape {
    #[default]
    Circle,
    Square,
    Triangle,
    Diamond,
    Star,
    Spark,
    Hexagon,
}

/// Fixed-capacity vertex buffer so emission never allocates.
pub struct Verts {
    pub points: [(f32, f32); MAX_VERTS],
    pub len: usize,
}

impl Verts {
    pub fn as_slice(&self) -> &[(f32, f32)] {
        &self.points[..self.len]
    }
}

impl Shape {
    pub fn is_circle(self) -> bool {
        matches!(self, Shape::Circle)
    }

    /// Polygon outline centred on (cx, cy). `size` is the full width/height.
    /// Circle returns an inscribed 12-gon so backends without arc primitives still work.
    pub fn vertices(self, cx: f32, cy: f32, size: f32, rotation: f32) -> Verts {
        let r = size * 0.5;
        let mut points = [(0.0f32, 0.0f32); MAX_VERTS];
        let len = match self {
            Shape::Circle => regular(
                &mut points,
                cx,
                cy,
                r,
                12,
                rotation,
                -std::f32::consts::FRAC_PI_2,
            ),
            Shape::Square => regular(
                &mut points,
                cx,
                cy,
                r * std::f32::consts::SQRT_2,
                4,
                rotation,
                std::f32::consts::FRAC_PI_4,
            ),
            Shape::Triangle => regular(
                &mut points,
                cx,
                cy,
                r,
                3,
                rotation,
                -std::f32::consts::FRAC_PI_2,
            ),
            Shape::Diamond => regular(
                &mut points,
                cx,
                cy,
                r,
                4,
                rotation,
                -std::f32::consts::FRAC_PI_2,
            ),
            Shape::Hexagon => regular(&mut points, cx, cy, r, 6, rotation, 0.0),
            Shape::Star => star(&mut points, cx, cy, r, r * 0.42, 5, rotation),
            Shape::Spark => star(&mut points, cx, cy, r, r * 0.18, 4, rotation),
        };
        Verts { points, len }
    }
}

fn regular(
    out: &mut [(f32, f32); MAX_VERTS],
    cx: f32,
    cy: f32,
    r: f32,
    n: usize,
    rotation: f32,
    offset: f32,
) -> usize {
    let step = std::f32::consts::TAU / n as f32;
    for (i, point) in out.iter_mut().take(n).enumerate() {
        let a = offset + rotation + step * i as f32;
        *point = (cx + r * a.cos(), cy + r * a.sin());
    }
    n
}

fn star(
    out: &mut [(f32, f32); MAX_VERTS],
    cx: f32,
    cy: f32,
    outer: f32,
    inner: f32,
    spikes: usize,
    rotation: f32,
) -> usize {
    let n = spikes * 2;
    let step = std::f32::consts::TAU / n as f32;
    for (i, point) in out.iter_mut().take(n).enumerate() {
        let r = if i % 2 == 0 { outer } else { inner };
        let a = -std::f32::consts::FRAC_PI_2 + rotation + step * i as f32;
        *point = (cx + r * a.cos(), cy + r * a.sin());
    }
    n
}
