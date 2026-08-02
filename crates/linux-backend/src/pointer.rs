use std::io::{Read, Write};
use std::net::Shutdown;
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use x11rb::connection::Connection;
use x11rb::protocol::xproto::ConnectionExt;

/// Where the particles spawn. Wayland deliberately offers no way to query the
/// global pointer position, so each compositor family needs its own route.
pub enum Pointer {
    /// Hyprland exposes `cursorpos` over its control socket: exact, cheap.
    Hyprland { socket: PathBuf, fallback: Relative },
    /// X11 answers QueryPointer against the root window: exact.
    X11 {
        conn: x11rb::rust_connection::RustConnection,
        root: x11rb::protocol::xproto::Window,
    },
    /// Anything else: integrate raw evdev motion. Approximate, since the
    /// compositor's acceleration curve is not reproduced.
    Relative(Relative),
}

pub struct Relative {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    seeded: bool,
}

impl Relative {
    fn new() -> Self {
        Self {
            x: 0.0,
            y: 0.0,
            width: 1920.0,
            height: 1080.0,
            seeded: false,
        }
    }

    fn set_bounds(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);
        if !self.seeded {
            self.x = self.width * 0.5;
            self.y = self.height * 0.5;
            self.seeded = true;
        }
        self.clamp();
    }

    fn motion(&mut self, dx: f32, dy: f32) {
        self.x += dx;
        self.y += dy;
        self.clamp();
    }

    fn clamp(&mut self) {
        self.x = self.x.clamp(0.0, self.width - 1.0);
        self.y = self.y.clamp(0.0, self.height - 1.0);
    }
}

impl Pointer {
    pub fn detect() -> Self {
        if let Some(socket) = hyprland_socket() {
            log::info!("pointer source: Hyprland IPC ({})", socket.display());
            return Pointer::Hyprland {
                socket,
                fallback: Relative::new(),
            };
        }
        if std::env::var_os("WAYLAND_DISPLAY").is_none() {
            if let Ok((conn, screen_num)) = x11rb::connect(None) {
                let root = conn.setup().roots[screen_num].root;
                log::info!("pointer source: X11 QueryPointer");
                return Pointer::X11 { conn, root };
            }
        }
        log::warn!(
            "no exact pointer source available; falling back to relative motion tracking. \
             Positions may drift from the real cursor."
        );
        Pointer::Relative(Relative::new())
    }

    pub fn set_bounds(&mut self, width: f32, height: f32) {
        match self {
            Pointer::Hyprland { fallback, .. } => fallback.set_bounds(width, height),
            Pointer::Relative(rel) => rel.set_bounds(width, height),
            Pointer::X11 { .. } => {}
        }
    }

    pub fn on_motion(&mut self, dx: f32, dy: f32) {
        match self {
            Pointer::Hyprland { fallback, .. } => fallback.motion(dx, dy),
            Pointer::Relative(rel) => rel.motion(dx, dy),
            Pointer::X11 { .. } => {}
        }
    }

    pub fn position(&mut self) -> Option<(f32, f32)> {
        match self {
            Pointer::Hyprland { socket, fallback } => match query_hyprland(socket) {
                Some(pos) => Some(pos),
                None => Some((fallback.x, fallback.y)),
            },
            Pointer::X11 { conn, root } => {
                let reply = conn.query_pointer(*root).ok()?.reply().ok()?;
                Some((reply.root_x as f32, reply.root_y as f32))
            }
            Pointer::Relative(rel) => Some((rel.x, rel.y)),
        }
    }
}

fn hyprland_socket() -> Option<PathBuf> {
    let sig = std::env::var("HYPRLAND_INSTANCE_SIGNATURE").ok()?;
    let candidates = [
        std::env::var("XDG_RUNTIME_DIR").ok().map(|r| {
            PathBuf::from(r)
                .join("hypr")
                .join(&sig)
                .join(".socket.sock")
        }),
        Some(PathBuf::from("/tmp/hypr").join(&sig).join(".socket.sock")),
    ];
    candidates.into_iter().flatten().find(|p| p.exists())
}

/// Hyprland dispatches this socket synchronously: the connection must be shut
/// down for writing straight away or the compositor stalls until it times out.
fn query_hyprland(socket: &PathBuf) -> Option<(f32, f32)> {
    let mut stream = UnixStream::connect(socket).ok()?;
    stream.write_all(b"cursorpos").ok()?;
    stream.flush().ok()?;
    stream.shutdown(Shutdown::Write).ok()?;

    let mut buf = String::new();
    stream.read_to_string(&mut buf).ok()?;

    let (x, y) = buf.trim().split_once(',')?;
    Some((x.trim().parse().ok()?, y.trim().parse().ok()?))
}
