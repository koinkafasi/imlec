//! Windows backend: a low-level keyboard hook feeds a click-through layered
//! window that is resized and moved to hug the live particles, so each frame
//! only ever uploads a small bitmap.

#![cfg(target_os = "windows")]

mod layer;
mod tray;

use anyhow::{anyhow, Context, Result};
use layer::LayeredSurface;
use pc_core::render::{particle_bounds, DirtyRect};
use pc_core::{Config, KeyClass, ParticleSystem, Renderer};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant, SystemTime};
use windows::core::w;
use windows::Win32::Foundation::{LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{
    SetProcessDpiAwarenessContext, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    VK_BACK, VK_CAPITAL, VK_CONTROL, VK_DELETE, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN, VK_MENU,
    VK_NUMLOCK, VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN, VK_SCROLL, VK_SHIFT,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CallNextHookEx, CreateWindowExW, DispatchMessageW, GetCursorPos, GetSystemMetrics, LoadCursorW,
    PeekMessageW, RegisterClassW, SetWindowsHookExW, TranslateMessage, UnhookWindowsHookEx,
    HC_ACTION, HHOOK, IDC_ARROW, KBDLLHOOKSTRUCT, MSG, PM_REMOVE, SM_CXVIRTUALSCREEN,
    SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WH_KEYBOARD_LL, WM_KEYDOWN, WM_QUIT,
    WM_SYSKEYDOWN, WNDCLASSW, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST,
    WS_EX_TRANSPARENT, WS_POPUP,
};

/// Bounded so a stalled render loop cannot grow the queue without limit.
const MAX_QUEUED_KEYS: usize = 64;

/// How often the config file's mtime is checked, so `imlec tune` feels live.
const CONFIG_POLL: Duration = Duration::from_millis(400);

static KEY_QUEUE: Mutex<Vec<KeyClass>> = Mutex::new(Vec::new());
pub(crate) static ENABLED: AtomicBool = AtomicBool::new(true);
pub(crate) static QUIT: AtomicBool = AtomicBool::new(false);
pub(crate) static RELOAD: AtomicBool = AtomicBool::new(false);

fn classify_vk(vk: u32) -> KeyClass {
    let vk = vk as u16;
    if vk == VK_BACK.0 || vk == VK_DELETE.0 {
        return KeyClass::Delete;
    }
    const MODIFIERS: [u16; 13] = [
        VK_SHIFT.0,
        VK_LSHIFT.0,
        VK_RSHIFT.0,
        VK_CONTROL.0,
        VK_LCONTROL.0,
        VK_RCONTROL.0,
        VK_MENU.0,
        VK_LMENU.0,
        VK_RMENU.0,
        VK_LWIN.0,
        VK_RWIN.0,
        VK_CAPITAL.0,
        VK_NUMLOCK.0,
    ];
    if MODIFIERS.contains(&vk) || vk == VK_SCROLL.0 {
        return KeyClass::Ignore;
    }
    KeyClass::Text
}

unsafe extern "system" fn keyboard_hook(code: i32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if code == HC_ACTION as i32 {
        let message = wparam.0 as u32;
        if message == WM_KEYDOWN || message == WM_SYSKEYDOWN {
            let info = unsafe { &*(lparam.0 as *const KBDLLHOOKSTRUCT) };
            let class = classify_vk(info.vkCode);
            if class != KeyClass::Ignore {
                if let Ok(mut queue) = KEY_QUEUE.lock() {
                    if queue.len() < MAX_QUEUED_KEYS {
                        queue.push(class);
                    }
                }
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wparam, lparam) }
}

struct HookGuard(HHOOK);

impl Drop for HookGuard {
    fn drop(&mut self) {
        unsafe {
            let _ = UnhookWindowsHookEx(self.0);
        }
    }
}

pub fn run(config: Config, config_path: Option<PathBuf>) -> Result<()> {
    unsafe {
        // Without this, GetCursorPos and the virtual screen metrics are reported
        // in scaled coordinates and particles land in the wrong place.
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
    }

    let hinstance = unsafe { GetModuleHandleW(None) }.context("GetModuleHandleW")?;
    let class_name = w!("ImlecOverlayClass");

    let wc = WNDCLASSW {
        lpfnWndProc: Some(tray::wnd_proc),
        hInstance: hinstance.into(),
        lpszClassName: class_name,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        ..Default::default()
    };
    if unsafe { RegisterClassW(&wc) } == 0 {
        return Err(anyhow!("RegisterClassW failed"));
    }

    let overlay = unsafe {
        CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            class_name,
            w!("imlec"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .context("creating the overlay window")?;

    // The overlay is WS_EX_NOACTIVATE, which breaks popup menus, so the tray
    // icon lives on its own plain hidden window.
    let tray_host = unsafe {
        CreateWindowExW(
            Default::default(),
            class_name,
            w!("imlec-tray"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinstance.into()),
            None,
        )
    }
    .context("creating the tray host window")?;

    let _tray = tray::Tray::new(tray_host, config_path.clone())?;

    let hook = unsafe { SetWindowsHookExW(WH_KEYBOARD_LL, Some(keyboard_hook), None, 0) }
        .context("installing the keyboard hook")?;
    let _hook_guard = HookGuard(hook);

    let mut surface = LayeredSurface::new(overlay)?;
    let mut renderer =
        Renderer::new(512, 512).ok_or_else(|| anyhow!("pixmap allocation failed"))?;
    let mut system = ParticleSystem::new(config);

    let virtual_screen = virtual_screen_rect();
    let mut frame_interval = Duration::from_secs_f32(1.0 / system.config().general.fps as f32);
    let mut last_tick = Instant::now();
    let mut last_config_check = Instant::now();
    let mut config_mtime = config_path.as_ref().and_then(mtime);

    loop {
        pump_messages();
        if QUIT.load(Ordering::Relaxed) {
            break;
        }

        drain_keys(&mut system);

        if last_config_check.elapsed() >= CONFIG_POLL {
            last_config_check = Instant::now();
            if let Some(path) = &config_path {
                let current = mtime(path);
                if current != config_mtime || RELOAD.swap(false, Ordering::Relaxed) {
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

        if system.is_idle() {
            surface.hide();
            last_tick = Instant::now();
            // Idle: nothing to draw, so poll the queue instead of burning frames.
            std::thread::sleep(Duration::from_millis(8));
            continue;
        }

        let now = Instant::now();
        system.update(now.duration_since(last_tick).as_secs_f32());
        last_tick = now;

        match particle_bounds(system.particles(), 1.0).and_then(|b| clamp_to(b, virtual_screen)) {
            Some(bounds) => {
                if !renderer.ensure_size(bounds.w as u32, bounds.h as u32) {
                    log::error!("failed to grow the pixmap to {}x{}", bounds.w, bounds.h);
                    continue;
                }
                let area = DirtyRect {
                    x: 0,
                    y: 0,
                    w: bounds.w,
                    h: bounds.h,
                };
                renderer.clear_rect(area);
                renderer.draw_particles(
                    system.particles(),
                    (bounds.x as f32, bounds.y as f32),
                    1.0,
                );
                surface.present(&renderer, bounds)?;
            }
            None => surface.hide(),
        }

        let elapsed = last_tick.elapsed();
        if elapsed < frame_interval {
            std::thread::sleep(frame_interval - elapsed);
        }
    }
    Ok(())
}

fn drain_keys(system: &mut ParticleSystem) {
    let keys: Vec<KeyClass> = {
        let Ok(mut queue) = KEY_QUEUE.lock() else {
            return;
        };
        if queue.is_empty() {
            return;
        }
        std::mem::take(&mut *queue)
    };
    if !ENABLED.load(Ordering::Relaxed) {
        return;
    }
    let Some((x, y)) = cursor_position() else {
        return;
    };
    for class in keys {
        if let Some(kind) = class.emit_kind() {
            system.emit(kind, x, y);
        }
    }
}

fn pump_messages() {
    let mut msg = MSG::default();
    while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
        if msg.message == WM_QUIT {
            QUIT.store(true, Ordering::Relaxed);
            return;
        }
        unsafe {
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

fn cursor_position() -> Option<(f32, f32)> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok()?;
    Some((point.x as f32, point.y as f32))
}

fn virtual_screen_rect() -> DirtyRect {
    unsafe {
        DirtyRect {
            x: GetSystemMetrics(SM_XVIRTUALSCREEN),
            y: GetSystemMetrics(SM_YVIRTUALSCREEN),
            w: GetSystemMetrics(SM_CXVIRTUALSCREEN),
            h: GetSystemMetrics(SM_CYVIRTUALSCREEN),
        }
    }
}

fn clamp_to(rect: DirtyRect, bounds: DirtyRect) -> Option<DirtyRect> {
    let x0 = rect.x.max(bounds.x);
    let y0 = rect.y.max(bounds.y);
    let x1 = (rect.x + rect.w).min(bounds.x + bounds.w);
    let y1 = (rect.y + rect.h).min(bounds.y + bounds.h);
    if x1 <= x0 || y1 <= y0 {
        None
    } else {
        Some(DirtyRect {
            x: x0,
            y: y0,
            w: x1 - x0,
            h: y1 - y0,
        })
    }
}

fn mtime(path: &PathBuf) -> Option<SystemTime> {
    std::fs::metadata(path).ok()?.modified().ok()
}
