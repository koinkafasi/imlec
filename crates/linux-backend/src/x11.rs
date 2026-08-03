use crate::keyboard::InputSignal;
use crate::pointer::Pointer;
use anyhow::{anyhow, Context, Result};
use pc_core::render::{DirtyRect, Renderer};
use pc_core::{Config, ParticleSystem};
use std::path::PathBuf;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime};
use x11rb::connection::Connection;
use x11rb::protocol::shape::{ConnectionExt as _, SK, SO};
use x11rb::protocol::xproto::{
    ClipOrdering, ColormapAlloc, ConnectionExt as _, CreateGCAux, CreateWindowAux, EventMask,
    ImageFormat, StackMode, WindowClass,
};

/// Keeps PutImage requests comfortably below the server's maximum request size.
const MAX_CHUNK_BYTES: usize = 256 * 1024;

pub fn run(config: Config, config_path: Option<PathBuf>) -> Result<()> {
    let (conn, screen_num) = x11rb::connect(None).context("connecting to the X server")?;
    let screen = &conn.setup().roots[screen_num];
    let root = screen.root;
    let width = screen.width_in_pixels;
    let height = screen.height_in_pixels;

    let depth32 = screen
        .allowed_depths
        .iter()
        .find(|d| d.depth == 32)
        .ok_or_else(|| {
            anyhow!("no 32-bit visual; a compositing manager such as picom is required")
        })?;
    let visual = depth32
        .visuals
        .first()
        .ok_or_else(|| anyhow!("32-bit depth has no visuals"))?;

    let colormap = conn.generate_id()?;
    conn.create_colormap(ColormapAlloc::NONE, colormap, root, visual.visual_id)?;

    let window = conn.generate_id()?;
    let aux = CreateWindowAux::new()
        .background_pixel(0)
        .border_pixel(0)
        .colormap(colormap)
        .override_redirect(1)
        .event_mask(EventMask::NO_EVENT);
    conn.create_window(
        32,
        window,
        root,
        0,
        0,
        width,
        height,
        0,
        WindowClass::INPUT_OUTPUT,
        visual.visual_id,
        &aux,
    )?;

    // Empty input shape: every click and hover falls through to the window below.
    conn.shape_rectangles(
        SO::SET,
        SK::INPUT,
        ClipOrdering::UNSORTED,
        window,
        0,
        0,
        &[],
    )?;

    let gc = conn.generate_id()?;
    conn.create_gc(gc, window, &CreateGCAux::new())?;
    conn.map_window(window)?;
    conn.flush()?;

    let mut renderer = Renderer::new(width as u32, height as u32)
        .ok_or_else(|| anyhow!("allocating a {width}x{height} pixmap failed"))?;
    let mut system = ParticleSystem::new(config);
    let mut pointer = Pointer::detect();
    pointer.set_bounds(width as f32, height as f32);

    let (tx, rx) = mpsc::channel::<InputSignal>();
    let needs_motion = matches!(pointer, Pointer::Relative(_));
    crate::keyboard::spawn(move |signal| {
        if !needs_motion && matches!(signal, InputSignal::Motion { .. }) {
            return;
        }
        let _ = tx.send(signal);
    })
    .context("starting evdev readers")?;

    let mut frame_interval = Duration::from_secs_f32(1.0 / system.config().general.fps as f32);
    let mut scratch: Vec<u8> = Vec::new();
    let mut last_tick = Instant::now();
    let mut last_raise = Instant::now();
    let mut last_config_check = Instant::now();
    let mut config_mtime = config_path.as_ref().and_then(mtime);

    loop {
        let idle = system.is_idle() && !renderer.has_previous();

        // Idle: block on input so the process uses no CPU at all.
        let signal = if idle {
            match rx.recv_timeout(Duration::from_millis(500)) {
                Ok(sig) => Some(sig),
                Err(mpsc::RecvTimeoutError::Timeout) => None,
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
        } else {
            rx.try_recv().ok()
        };

        if let Some(signal) = signal {
            handle_signal(signal, &mut system, &mut pointer);
        }
        while let Ok(signal) = rx.try_recv() {
            handle_signal(signal, &mut system, &mut pointer);
        }

        if last_config_check.elapsed() >= Duration::from_secs(2) {
            last_config_check = Instant::now();
            if let Some(path) = &config_path {
                let current = mtime(path);
                if current != config_mtime {
                    config_mtime = current;
                    match Config::load_from(path) {
                        Ok(cfg) => {
                            frame_interval = Duration::from_secs_f32(1.0 / cfg.general.fps as f32);
                            system.set_config(cfg);
                            log::info!("reloaded {}", path.display());
                        }
                        Err(err) => log::warn!("config reload failed, keeping previous: {err:#}"),
                    }
                }
            }
        }

        if system.is_idle() && !renderer.has_previous() {
            last_tick = Instant::now();
            continue;
        }

        let now = Instant::now();
        system.update(now.duration_since(last_tick).as_secs_f32());
        last_tick = now;

        if let Some(rect) = renderer.render(system.particles(), (0.0, 0.0), 1.0) {
            put_image(&conn, window, gc, &renderer, rect, &mut scratch)?;
        }

        // Override-redirect windows can still be covered by newly mapped windows.
        if last_raise.elapsed() >= Duration::from_secs(1) {
            last_raise = Instant::now();
            let _ = conn.configure_window(
                window,
                &x11rb::protocol::xproto::ConfigureWindowAux::new().stack_mode(StackMode::ABOVE),
            );
        }
        conn.flush()?;

        let elapsed = last_tick.elapsed();
        if elapsed < frame_interval {
            std::thread::sleep(frame_interval - elapsed);
        }
    }
    Ok(())
}

fn handle_signal(signal: InputSignal, system: &mut ParticleSystem, pointer: &mut Pointer) {
    match signal {
        InputSignal::Motion { dx, dy } => pointer.on_motion(dx, dy),
        InputSignal::Key(class) => {
            if let Some(kind) = class.emit_kind() {
                if let Some((x, y)) = pointer.position() {
                    system.emit(kind, x, y);
                }
            }
        }
    }
}

fn put_image<C: Connection>(
    conn: &C,
    window: u32,
    gc: u32,
    renderer: &Renderer,
    rect: DirtyRect,
    scratch: &mut Vec<u8>,
) -> Result<()> {
    let row_bytes = rect.w as usize * 4;
    let rows_per_chunk = (MAX_CHUNK_BYTES / row_bytes.max(1)).max(1);

    let mut y = 0;
    while y < rect.h {
        let h = rows_per_chunk.min((rect.h - y) as usize) as i32;
        let chunk = DirtyRect {
            x: rect.x,
            y: rect.y + y,
            w: rect.w,
            h,
        };
        renderer.blit_bgra_tight(scratch, chunk);
        conn.put_image(
            ImageFormat::Z_PIXMAP,
            window,
            gc,
            chunk.w as u16,
            chunk.h as u16,
            chunk.x as i16,
            chunk.y as i16,
            0,
            32,
            scratch,
        )?;
        y += h;
    }
    Ok(())
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
