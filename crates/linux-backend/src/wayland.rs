use crate::keyboard::InputSignal;
use crate::pointer::Pointer;
use anyhow::{anyhow, Context, Result};
use pc_core::render::{DirtyRect, Renderer};
use pc_core::{Config, ParticleSystem};
use smithay_client_toolkit::{
    compositor::{CompositorHandler, CompositorState},
    delegate_registry,
    dispatch2::Dispatch2,
    output::{OutputHandler, OutputState},
    reexports::{
        calloop::{
            channel,
            timer::{TimeoutAction, Timer},
            EventLoop, LoopHandle,
        },
        calloop_wayland_source::WaylandSource,
        client::{
            globals::registry_queue_init,
            protocol::{wl_output, wl_region, wl_shm, wl_surface},
            Connection, Proxy, QueueHandle,
        },
    },
    registry::{ProvidesRegistryState, RegistryState},
    registry_handlers,
    shell::{
        wlr_layer::{
            Anchor, KeyboardInteractivity, Layer, LayerShell, LayerShellHandler, LayerSurface,
            LayerSurfaceConfigure,
        },
        WaylandSurface,
    },
    shm::{
        slot::{Buffer, SlotPool},
        Shm, ShmHandler,
    },
};
use std::path::PathBuf;
use std::time::{Duration, Instant, SystemTime};

/// wl_region carries no events, but the object still needs a dispatch target.
struct RegionData;

impl Dispatch2<wl_region::WlRegion, Overlay> for RegionData {
    fn event(
        &self,
        _: &mut Overlay,
        _: &wl_region::WlRegion,
        _: <wl_region::WlRegion as Proxy>::Event,
        _: &Connection,
        _: &QueueHandle<Overlay>,
    ) {
    }
}

/// One of the two shm buffers behind a surface, plus the region in which it
/// still differs from the pixmap. Tracking this is what lets us commit partial
/// damage while double buffering.
struct SlotEntry {
    buffer: Option<Buffer>,
    stale: Option<DirtyRect>,
}

struct OutputSurface {
    output: wl_output::WlOutput,
    layer: LayerSurface,
    pool: Option<SlotPool>,
    slots: [SlotEntry; 2],
    next_slot: usize,
    width: u32,
    height: u32,
    scale: i32,
    origin: (f32, f32),
    renderer: Option<Renderer>,
    configured: bool,
}

impl OutputSurface {
    fn buffer_size(&self) -> (u32, u32) {
        (
            self.width * self.scale as u32,
            self.height * self.scale as u32,
        )
    }

    fn rebuild(&mut self, shm: &Shm) {
        let (bw, bh) = self.buffer_size();
        if bw == 0 || bh == 0 {
            return;
        }
        let len = bw as usize * bh as usize * 4 * 2;
        match SlotPool::new(len, shm) {
            Ok(pool) => self.pool = Some(pool),
            Err(err) => {
                log::error!("shm pool allocation failed: {err}");
                self.pool = None;
                return;
            }
        }
        self.slots = [
            SlotEntry {
                buffer: None,
                stale: None,
            },
            SlotEntry {
                buffer: None,
                stale: None,
            },
        ];
        self.renderer = Renderer::new(bw, bh);
        self.layer.wl_surface().set_buffer_scale(self.scale);
    }

    fn has_pending_paint(&self) -> bool {
        self.renderer.as_ref().is_some_and(|r| r.has_previous())
    }

    fn draw(&mut self, particles: &[pc_core::Particle]) {
        if !self.configured {
            return;
        }
        let (Some(pool), Some(renderer)) = (self.pool.as_mut(), self.renderer.as_mut()) else {
            return;
        };
        let (bw, bh) = (renderer.width(), renderer.height());
        let stride = bw as i32 * 4;

        let damage = renderer.render(particles, self.origin, self.scale as f32);

        // Pick a buffer the compositor is not currently reading from.
        let mut chosen = None;
        for offset in 0..2 {
            let idx = (self.next_slot + offset) % 2;
            let available = match &self.slots[idx].buffer {
                Some(buf) => pool.canvas(buf).is_some(),
                None => true,
            };
            if available {
                chosen = Some(idx);
                break;
            }
        }
        let Some(idx) = chosen else {
            // Both buffers in flight; the next tick will catch up.
            if let Some(d) = damage {
                for slot in &mut self.slots {
                    slot.stale = DirtyRect::union_opt(slot.stale, Some(d));
                }
            }
            return;
        };
        self.next_slot = (idx + 1) % 2;

        if self.slots[idx].buffer.is_none() {
            match pool.create_buffer(bw as i32, bh as i32, stride, wl_shm::Format::Argb8888) {
                Ok((buffer, _)) => {
                    self.slots[idx].buffer = Some(buffer);
                    // A fresh buffer is blank, so everything already drawn is stale in it.
                    self.slots[idx].stale = Some(DirtyRect {
                        x: 0,
                        y: 0,
                        w: bw as i32,
                        h: bh as i32,
                    });
                }
                Err(err) => {
                    log::error!("buffer allocation failed: {err}");
                    return;
                }
            }
        }

        let region = DirtyRect::union_opt(damage, self.slots[idx].stale);
        let Some(region) = region else { return };

        let buffer = self.slots[idx].buffer.as_ref().unwrap();
        let Some(canvas) = pool.canvas(buffer) else {
            return;
        };
        renderer.blit_bgra(canvas, stride as usize, region);

        self.slots[idx].stale = None;
        let other = (idx + 1) % 2;
        self.slots[other].stale = DirtyRect::union_opt(self.slots[other].stale, damage);

        let surface = self.layer.wl_surface();
        surface.damage_buffer(region.x, region.y, region.w, region.h);
        if buffer.attach_to(surface).is_err() {
            log::error!("failed to attach buffer");
            return;
        }
        self.layer.commit();
    }
}

pub struct Overlay {
    registry_state: RegistryState,
    output_state: OutputState,
    shm: Shm,
    compositor: CompositorState,
    layer_shell: LayerShell,
    qh: QueueHandle<Overlay>,
    loop_handle: LoopHandle<'static, Overlay>,

    surfaces: Vec<OutputSurface>,
    system: ParticleSystem,
    pointer: Pointer,
    frame_interval: Duration,
    last_tick: Instant,
    timer_armed: bool,
    config_path: Option<PathBuf>,
    config_mtime: Option<SystemTime>,
    exit: bool,
}

pub fn run(config: Config, config_path: Option<PathBuf>) -> Result<()> {
    let conn = Connection::connect_to_env().context("connecting to the Wayland compositor")?;
    let (globals, event_queue) = registry_queue_init(&conn).context("initialising registry")?;
    let qh: QueueHandle<Overlay> = event_queue.handle();

    let compositor = CompositorState::bind(&globals, &qh)
        .map_err(|e| anyhow!("wl_compositor unavailable: {e}"))?;
    let layer_shell = LayerShell::bind(&globals, &qh).map_err(|e| {
        anyhow!("wlr-layer-shell unavailable ({e}). Hyprland, Sway, river, niri and Wayfire support it; GNOME does not.")
    })?;
    let shm = Shm::bind(&globals, &qh).map_err(|e| anyhow!("wl_shm unavailable: {e}"))?;

    let mut event_loop: EventLoop<'static, Overlay> =
        EventLoop::try_new().context("creating event loop")?;
    let loop_handle = event_loop.handle();

    let frame_interval = Duration::from_secs_f32(1.0 / config.general.fps as f32);
    let pointer = Pointer::detect();
    let needs_motion = matches!(pointer, Pointer::Relative(_) | Pointer::Hyprland { .. });

    let mut overlay = Overlay {
        registry_state: RegistryState::new(&globals),
        output_state: OutputState::new(&globals, &qh),
        shm,
        compositor,
        layer_shell,
        qh: qh.clone(),
        loop_handle: loop_handle.clone(),
        surfaces: Vec::new(),
        system: ParticleSystem::new(config),
        pointer,
        frame_interval,
        last_tick: Instant::now(),
        timer_armed: false,
        config_mtime: config_path.as_ref().and_then(mtime),
        config_path,
        exit: false,
    };

    WaylandSource::new(conn.clone(), event_queue)
        .insert(loop_handle.clone())
        .map_err(|e| anyhow!("inserting Wayland source: {e}"))?;

    let (tx, rx) = channel::channel::<InputSignal>();
    loop_handle
        .insert_source(rx, |event, _, state| {
            if let channel::Event::Msg(signal) = event {
                state.on_input(signal);
            }
        })
        .map_err(|e| anyhow!("inserting input source: {e}"))?;

    crate::keyboard::spawn(move |signal| {
        if !needs_motion && matches!(signal, InputSignal::Motion { .. }) {
            return;
        }
        let _ = tx.send(signal);
    })
    .context("starting evdev readers")?;

    // Config reload poll. Cheap enough at 2s and avoids an inotify dependency.
    loop_handle
        .insert_source(Timer::from_duration(Duration::from_secs(2)), |_, _, state| {
            state.reload_config_if_changed();
            TimeoutAction::ToDuration(Duration::from_secs(2))
        })
        .map_err(|e| anyhow!("inserting config watcher: {e}"))?;

    loop {
        event_loop
            .dispatch(Duration::from_millis(500), &mut overlay)
            .context("event loop dispatch")?;
        overlay.sync_outputs();
        if overlay.exit {
            break;
        }
    }
    Ok(())
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}

impl Overlay {
    fn on_input(&mut self, signal: InputSignal) {
        match signal {
            InputSignal::Motion { dx, dy } => self.pointer.on_motion(dx, dy),
            InputSignal::Key(class) => {
                let Some(kind) = class.emit_kind() else { return };
                let Some((x, y)) = self.pointer.position() else {
                    return;
                };
                if self.system.emit(kind, x, y) {
                    self.arm_timer();
                }
            }
        }
    }

    fn arm_timer(&mut self) {
        if self.timer_armed {
            return;
        }
        self.timer_armed = true;
        self.last_tick = Instant::now();
        let handle = self.loop_handle.clone();
        let result = handle.insert_source(Timer::immediate(), |_, _, state| {
            state.tick();
            if state.system.is_idle() && !state.surfaces.iter().any(|s| s.has_pending_paint()) {
                state.timer_armed = false;
                TimeoutAction::Drop
            } else {
                TimeoutAction::ToDuration(state.frame_interval)
            }
        });
        if let Err(err) = result {
            log::error!("failed to arm animation timer: {err}");
            self.timer_armed = false;
        }
    }

    fn tick(&mut self) {
        let now = Instant::now();
        let dt = now.duration_since(self.last_tick).as_secs_f32();
        self.last_tick = now;
        self.system.update(dt);

        let particles = self.system.particles();
        for surface in &mut self.surfaces {
            surface.draw(particles);
        }
    }

    fn reload_config_if_changed(&mut self) {
        let Some(path) = self.config_path.clone() else {
            return;
        };
        let current = mtime(&path);
        if current == self.config_mtime {
            return;
        }
        self.config_mtime = current;
        match Config::load_from(&path) {
            Ok(config) => {
                log::info!("reloaded {}", path.display());
                self.frame_interval = Duration::from_secs_f32(1.0 / config.general.fps as f32);
                self.system.set_config(config);
                self.arm_timer();
            }
            Err(err) => log::warn!("config reload failed, keeping previous: {err:#}"),
        }
    }

    /// Creates surfaces for new outputs and drops them for removed ones.
    fn sync_outputs(&mut self) {
        let outputs: Vec<_> = self.output_state.outputs().collect();
        self.surfaces.retain(|s| outputs.contains(&s.output));

        for output in outputs {
            if self.surfaces.iter().any(|s| s.output == output) {
                self.update_geometry(&output);
                continue;
            }
            let Some(info) = self.output_state.info(&output) else {
                continue;
            };
            let (width, height) = info
                .logical_size
                .map(|(w, h)| (w.max(0) as u32, h.max(0) as u32))
                .unwrap_or((0, 0));
            let origin = info
                .logical_position
                .map(|(x, y)| (x as f32, y as f32))
                .unwrap_or((0.0, 0.0));

            let surface = self.compositor.create_surface(&self.qh);
            let layer = self.layer_shell.create_layer_surface(
                &self.qh,
                surface,
                Layer::Overlay,
                Some("imlec"),
                Some(&output),
            );
            layer.set_anchor(Anchor::TOP | Anchor::BOTTOM | Anchor::LEFT | Anchor::RIGHT);
            layer.set_exclusive_zone(-1);
            layer.set_keyboard_interactivity(KeyboardInteractivity::None);
            layer.set_size(0, 0);

            // Empty input region: pointer and touch events pass straight through.
            let region = self
                .compositor
                .wl_compositor()
                .create_region(&self.qh, RegionData);
            layer.wl_surface().set_input_region(Some(&region));
            region.destroy();
            layer.commit();

            self.surfaces.push(OutputSurface {
                output,
                layer,
                pool: None,
                slots: [
                    SlotEntry {
                        buffer: None,
                        stale: None,
                    },
                    SlotEntry {
                        buffer: None,
                        stale: None,
                    },
                ],
                next_slot: 0,
                width,
                height,
                scale: info.scale_factor.max(1),
                origin,
                renderer: None,
                configured: false,
            });
        }
        self.update_bounds();
    }

    fn update_geometry(&mut self, output: &wl_output::WlOutput) {
        let Some(info) = self.output_state.info(output) else {
            return;
        };
        let shm = self.shm.clone();
        let Some(surface) = self.surfaces.iter_mut().find(|s| &s.output == output) else {
            return;
        };
        if let Some((x, y)) = info.logical_position {
            surface.origin = (x as f32, y as f32);
        }
        let scale = info.scale_factor.max(1);
        if scale != surface.scale {
            surface.scale = scale;
            surface.rebuild(&shm);
        }
    }

    fn update_bounds(&mut self) {
        let mut max_x: f32 = 0.0;
        let mut max_y: f32 = 0.0;
        for s in &self.surfaces {
            max_x = max_x.max(s.origin.0 + s.width as f32);
            max_y = max_y.max(s.origin.1 + s.height as f32);
        }
        if max_x > 0.0 && max_y > 0.0 {
            self.pointer.set_bounds(max_x, max_y);
        }
    }
}

impl CompositorHandler for Overlay {
    fn scale_factor_changed(
        &mut self,
        _conn: &Connection,
        _qh: &QueueHandle<Self>,
        surface: &wl_surface::WlSurface,
        new_factor: i32,
    ) {
        let shm = self.shm.clone();
        if let Some(target) = self
            .surfaces
            .iter_mut()
            .find(|s| s.layer.wl_surface() == surface)
        {
            target.scale = new_factor.max(1);
            target.rebuild(&shm);
        }
    }

    fn transform_changed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: wl_output::Transform,
    ) {
    }

    fn frame(&mut self, _: &Connection, _: &QueueHandle<Self>, _: &wl_surface::WlSurface, _: u32) {}

    fn surface_enter(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }

    fn surface_leave(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        _: &wl_surface::WlSurface,
        _: &wl_output::WlOutput,
    ) {
    }
}

impl OutputHandler for Overlay {
    fn output_state(&mut self) -> &mut OutputState {
        &mut self.output_state
    }

    fn new_output(&mut self, _: &Connection, _: &QueueHandle<Self>, _: wl_output::WlOutput) {
        self.sync_outputs();
    }

    fn update_output(&mut self, _: &Connection, _: &QueueHandle<Self>, output: wl_output::WlOutput) {
        self.update_geometry(&output);
        self.update_bounds();
    }

    fn output_destroyed(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        output: wl_output::WlOutput,
    ) {
        self.surfaces.retain(|s| s.output != output);
        self.update_bounds();
    }
}

impl LayerShellHandler for Overlay {
    fn closed(&mut self, _: &Connection, _: &QueueHandle<Self>, layer: &LayerSurface) {
        self.surfaces.retain(|s| &s.layer != layer);
    }

    fn configure(
        &mut self,
        _: &Connection,
        _: &QueueHandle<Self>,
        layer: &LayerSurface,
        configure: LayerSurfaceConfigure,
        _serial: u32,
    ) {
        let shm = self.shm.clone();
        let Some(surface) = self.surfaces.iter_mut().find(|s| &s.layer == layer) else {
            return;
        };
        let (w, h) = configure.new_size;
        if w > 0 {
            surface.width = w;
        }
        if h > 0 {
            surface.height = h;
        }
        if surface.width == 0 || surface.height == 0 {
            return;
        }

        let needs_rebuild = surface
            .renderer
            .as_ref()
            .map(|r| (r.width(), r.height()) != surface.buffer_size())
            .unwrap_or(true);
        if needs_rebuild {
            surface.rebuild(&shm);
        }
        surface.configured = true;
        self.update_bounds();
    }
}

impl ShmHandler for Overlay {
    fn shm_state(&mut self) -> &mut Shm {
        &mut self.shm
    }
}

delegate_registry!(Overlay);

impl ProvidesRegistryState for Overlay {
    fn registry(&mut self) -> &mut RegistryState {
        &mut self.registry_state
    }
    registry_handlers![OutputState];
}

smithay_client_toolkit::delegate_dispatch2!(Overlay);
