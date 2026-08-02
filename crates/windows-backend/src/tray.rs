use crate::{ENABLED, QUIT, RELOAD};
use anyhow::{anyhow, Result};
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::sync::Mutex;
use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Shell::{
    Shell_NotifyIconW, ShellExecuteW, NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE,
    NOTIFYICONDATAW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DefWindowProcW, DestroyMenu, GetCursorPos, LoadIconW,
    PostMessageW, PostQuitMessage, SetForegroundWindow, TrackPopupMenu, IDI_APPLICATION,
    MF_CHECKED, MF_SEPARATOR, MF_STRING, MF_UNCHECKED, SW_SHOWNORMAL, TPM_RIGHTBUTTON, WM_APP,
    WM_COMMAND, WM_DESTROY, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
};

const WM_TRAY: u32 = WM_APP + 1;
const ID_TOGGLE: usize = 1;
const ID_CONFIG: usize = 2;
const ID_RELOAD: usize = 3;
const ID_EXIT: usize = 4;

static CONFIG_PATH: Mutex<Option<PathBuf>> = Mutex::new(None);

pub struct Tray {
    data: NOTIFYICONDATAW,
}

impl Tray {
    pub fn new(hwnd: HWND, config_path: Option<PathBuf>) -> Result<Self> {
        *CONFIG_PATH.lock().unwrap() = config_path;

        let icon = unsafe { LoadIconW(None, IDI_APPLICATION) }.unwrap_or_default();
        let mut data = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: hwnd,
            uID: 1,
            uFlags: NIF_ICON | NIF_MESSAGE | NIF_TIP,
            uCallbackMessage: WM_TRAY,
            hIcon: icon,
            ..Default::default()
        };
        for (slot, ch) in data.szTip.iter_mut().zip("imlec".encode_utf16()) {
            *slot = ch;
        }

        if !unsafe { Shell_NotifyIconW(NIM_ADD, &data) }.as_bool() {
            return Err(anyhow!("Shell_NotifyIcon failed to add the tray icon"));
        }
        Ok(Self { data })
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        unsafe {
            Shell_NotifyIconW(NIM_DELETE, &self.data);
        }
    }
}

pub unsafe extern "system" fn wnd_proc(
    hwnd: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match message {
        WM_TRAY => {
            let event = lparam.0 as u32;
            if event == WM_RBUTTONUP || event == WM_LBUTTONUP {
                show_menu(hwnd);
            }
            LRESULT(0)
        }
        WM_COMMAND => {
            match (wparam.0 & 0xffff) as usize {
                ID_TOGGLE => {
                    let now = !ENABLED.load(Ordering::Relaxed);
                    ENABLED.store(now, Ordering::Relaxed);
                }
                ID_CONFIG => open_config(),
                ID_RELOAD => RELOAD.store(true, Ordering::Relaxed),
                ID_EXIT => QUIT.store(true, Ordering::Relaxed),
                _ => {}
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

fn show_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let checked = if ENABLED.load(Ordering::Relaxed) {
            MF_CHECKED
        } else {
            MF_UNCHECKED
        };
        let _ = AppendMenuW(menu, MF_STRING | checked, ID_TOGGLE, w!("Effects enabled"));
        let _ = AppendMenuW(menu, MF_STRING, ID_CONFIG, w!("Open config file"));
        let _ = AppendMenuW(menu, MF_STRING, ID_RELOAD, w!("Reload config"));
        let _ = AppendMenuW(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let _ = AppendMenuW(menu, MF_STRING, ID_EXIT, w!("Exit"));

        let mut point = POINT::default();
        if GetCursorPos(&mut point).is_ok() {
            // Required so the menu closes when the user clicks elsewhere.
            let _ = SetForegroundWindow(hwnd);
            let _ = TrackPopupMenu(
                menu,
                TPM_RIGHTBUTTON,
                point.x,
                point.y,
                Some(0),
                hwnd,
                None,
            );
            let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        }
        let _ = DestroyMenu(menu);
    }
}

fn open_config() {
    let Some(path) = CONFIG_PATH.lock().unwrap().clone() else {
        return;
    };
    let file = HSTRING::from(path.as_os_str());
    unsafe {
        ShellExecuteW(None, w!("open"), &file, None, None, SW_SHOWNORMAL);
    }
}
