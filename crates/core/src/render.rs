use crate::particle::Particle;
use tiny_skia::{BlendMode, Color, FillRule, Paint, PathBuilder, Pixmap, Rect, Shader, Transform};

/// Integer pixel rectangle used for damage tracking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DirtyRect {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
}

impl DirtyRect {
    pub fn union(self, other: Self) -> Self {
        let x0 = self.x.min(other.x);
        let y0 = self.y.min(other.y);
        let x1 = (self.x + self.w).max(other.x + other.w);
        let y1 = (self.y + self.h).max(other.y + other.h);
        Self {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        }
    }

    pub fn union_opt(a: Option<Self>, b: Option<Self>) -> Option<Self> {
        match (a, b) {
            (Some(a), Some(b)) => Some(a.union(b)),
            (Some(a), None) => Some(a),
            (None, b) => b,
        }
    }

    pub fn clamp(self, w: i32, h: i32) -> Option<Self> {
        let x0 = self.x.max(0);
        let y0 = self.y.max(0);
        let x1 = (self.x + self.w).min(w);
        let y1 = (self.y + self.h).min(h);
        if x1 <= x0 || y1 <= y0 {
            None
        } else {
            Some(Self {
                x: x0,
                y: y0,
                w: x1 - x0,
                h: y1 - y0,
            })
        }
    }
}

/// Bounding box of every particle, in global coordinates scaled by `scale`.
/// `None` when there is nothing to draw.
pub fn particle_bounds(particles: &[Particle], scale: f32) -> Option<DirtyRect> {
    let mut bounds: Option<DirtyRect> = None;
    for p in particles {
        // Two extra pixels of slack for anti-aliased edges.
        let half = p.size * scale * 0.5 + 2.0;
        let x = p.x * scale;
        let y = p.y * scale;
        let r = DirtyRect {
            x: (x - half).floor() as i32,
            y: (y - half).floor() as i32,
            w: (half * 2.0).ceil() as i32 + 1,
            h: (half * 2.0).ceil() as i32 + 1,
        };
        bounds = Some(match bounds {
            Some(b) => b.union(r),
            None => r,
        });
    }
    bounds
}

/// Rasterises particles into a premultiplied RGBA pixmap.
///
/// Full-surface clears would burn gigabytes of memory bandwidth per second at 4K,
/// so `render` confines every frame to the union of the previous and current
/// particle bounds.
pub struct Renderer {
    pixmap: Pixmap,
    prev: Option<DirtyRect>,
}

impl Renderer {
    pub fn new(width: u32, height: u32) -> Option<Self> {
        Some(Self {
            pixmap: Pixmap::new(width.max(1), height.max(1))?,
            prev: None,
        })
    }

    pub fn width(&self) -> u32 {
        self.pixmap.width()
    }

    pub fn height(&self) -> u32 {
        self.pixmap.height()
    }

    pub fn data(&self) -> &[u8] {
        self.pixmap.data()
    }

    /// True while the last frame still has pixels on screen that need clearing.
    pub fn has_previous(&self) -> bool {
        self.prev.is_some()
    }

    /// Grows the backing pixmap if needed. Returns false if allocation failed.
    pub fn ensure_size(&mut self, width: u32, height: u32) -> bool {
        if self.pixmap.width() >= width && self.pixmap.height() >= height {
            return true;
        }
        match Pixmap::new(width.max(1), height.max(1)) {
            Some(pixmap) => {
                self.pixmap = pixmap;
                self.prev = None;
                true
            }
            None => false,
        }
    }

    pub fn clear_rect(&mut self, rect: DirtyRect) {
        let Some(rect) = rect.clamp(self.pixmap.width() as i32, self.pixmap.height() as i32) else {
            return;
        };
        let clear = Paint {
            shader: Shader::SolidColor(Color::TRANSPARENT),
            blend_mode: BlendMode::Clear,
            anti_alias: false,
            ..Default::default()
        };
        if let Some(r) = Rect::from_xywh(rect.x as f32, rect.y as f32, rect.w as f32, rect.h as f32)
        {
            self.pixmap
                .fill_rect(r, &clear, Transform::identity(), None);
        }
    }

    /// `origin` is the pixmap's top-left in global coordinates.
    pub fn draw_particles(&mut self, particles: &[Particle], origin: (f32, f32), scale: f32) {
        let mut paint = Paint {
            anti_alias: true,
            blend_mode: BlendMode::SourceOver,
            ..Default::default()
        };

        for p in particles {
            let lx = (p.x - origin.0) * scale;
            let ly = (p.y - origin.1) * scale;
            let size = p.size * scale;
            if size <= 0.2 || p.color.a <= 0.004 {
                continue;
            }

            let path = if p.shape.is_circle() {
                PathBuilder::from_circle(lx, ly, size * 0.5)
            } else {
                let verts = p.shape.vertices(lx, ly, size, p.rotation);
                let pts = verts.as_slice();
                if pts.len() < 3 {
                    continue;
                }
                let mut pb = PathBuilder::new();
                pb.move_to(pts[0].0, pts[0].1);
                for v in &pts[1..] {
                    pb.line_to(v.0, v.1);
                }
                pb.close();
                pb.finish()
            };

            let Some(path) = path else { continue };
            let Some(color) = Color::from_rgba(p.color.r, p.color.g, p.color.b, p.color.a) else {
                continue;
            };
            paint.set_color(color);
            self.pixmap.fill_path(
                &path,
                &paint,
                FillRule::Winding,
                Transform::identity(),
                None,
            );
        }
    }

    /// Damage-tracked frame for fixed surfaces (Wayland layer surfaces, X11 windows).
    /// Returns the region of the pixmap that changed, if any.
    pub fn render(
        &mut self,
        particles: &[Particle],
        origin: (f32, f32),
        scale: f32,
    ) -> Option<DirtyRect> {
        let pw = self.pixmap.width() as i32;
        let ph = self.pixmap.height() as i32;

        let current = particle_bounds(particles, scale)
            .map(|b| DirtyRect {
                x: b.x - (origin.0 * scale) as i32,
                y: b.y - (origin.1 * scale) as i32,
                w: b.w,
                h: b.h,
            })
            .and_then(|b| b.clamp(pw, ph));

        let damage = DirtyRect::union_opt(self.prev, current).and_then(|d| d.clamp(pw, ph));
        self.prev = current;

        let damage = damage?;
        self.clear_rect(damage);
        self.draw_particles(particles, origin, scale);
        Some(damage)
    }

    /// Copies a sub-rectangle into a BGRA destination that shares the pixmap's
    /// coordinate origin. This is the byte order both wl_shm ARGB8888 and a
    /// 32-bit Windows DIB expect on little-endian hosts.
    pub fn blit_bgra(&self, dst: &mut [u8], dst_stride: usize, rect: DirtyRect) {
        let src = self.pixmap.data();
        let src_stride = self.pixmap.width() as usize * 4;
        let row_len = rect.w as usize * 4;
        for row in 0..rect.h as usize {
            let y = rect.y as usize + row;
            let so = y * src_stride + rect.x as usize * 4;
            let dof = y * dst_stride + rect.x as usize * 4;
            if so + row_len > src.len() || dof + row_len > dst.len() {
                break;
            }
            let (s, d) = (&src[so..so + row_len], &mut dst[dof..dof + row_len]);
            for (sp, dp) in s.chunks_exact(4).zip(d.chunks_exact_mut(4)) {
                dp[0] = sp[2];
                dp[1] = sp[1];
                dp[2] = sp[0];
                dp[3] = sp[3];
            }
        }
    }

    /// Copies a sub-rectangle into a tightly packed BGRA buffer whose stride is
    /// `rect.w * 4`, starting at offset zero. Used by X11 PutImage and by the
    /// Windows layered window, which both want the region without its offset.
    pub fn blit_bgra_tight(&self, dst: &mut Vec<u8>, rect: DirtyRect) {
        let src = self.pixmap.data();
        let src_stride = self.pixmap.width() as usize * 4;
        let row_len = rect.w as usize * 4;
        dst.clear();
        dst.reserve(row_len * rect.h as usize);
        for row in 0..rect.h as usize {
            let y = rect.y as usize + row;
            let so = y * src_stride + rect.x as usize * 4;
            if so + row_len > src.len() {
                break;
            }
            for sp in src[so..so + row_len].chunks_exact(4) {
                dst.extend_from_slice(&[sp[2], sp[1], sp[0], sp[3]]);
            }
        }
    }
}
