use anyhow::{anyhow, Context, Result};
use pc_core::render::DirtyRect;
use pc_core::Renderer;
use std::ffi::c_void;
use std::time::Instant;
use windows::Win32::Foundation::{HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC, SelectObject,
    AC_SRC_ALPHA, AC_SRC_OVER, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, BLENDFUNCTION, DIB_RGB_COLORS,
    HBITMAP, HDC, HGDIOBJ,
};
use windows::Win32::UI::WindowsAndMessaging::{
    SetWindowPos, ShowWindow, UpdateLayeredWindow, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE,
    SWP_NOSIZE, SW_HIDE, SW_SHOWNA, ULW_ALPHA,
};

/// A per-pixel-alpha layered window. It is resized and moved to the bounding box
/// of the live particles every frame, so the bitmap uploaded to the compositor
/// stays small no matter how large the desktop is.
pub struct LayeredSurface {
    hwnd: HWND,
    screen_dc: HDC,
    mem_dc: HDC,
    bitmap: Option<HBITMAP>,
    previous_bitmap: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
    visible: bool,
    last_topmost: Instant,
}

impl LayeredSurface {
    pub fn new(hwnd: HWND) -> Result<Self> {
        let screen_dc = unsafe { GetDC(None) };
        if screen_dc.is_invalid() {
            return Err(anyhow!("GetDC failed"));
        }
        let mem_dc = unsafe { CreateCompatibleDC(Some(screen_dc)) };
        if mem_dc.is_invalid() {
            unsafe { ReleaseDC(None, screen_dc) };
            return Err(anyhow!("CreateCompatibleDC failed"));
        }
        Ok(Self {
            hwnd,
            screen_dc,
            mem_dc,
            bitmap: None,
            previous_bitmap: HGDIOBJ::default(),
            bits: std::ptr::null_mut(),
            width: 0,
            height: 0,
            visible: false,
            last_topmost: Instant::now(),
        })
    }

    fn ensure(&mut self, width: i32, height: i32) -> Result<()> {
        if self.bitmap.is_some() && self.width >= width && self.height >= height {
            return Ok(());
        }
        let width = width.max(self.width).max(256);
        let height = height.max(self.height).max(256);

        let mut info = BITMAPINFO::default();
        info.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height selects a top-down DIB, matching the pixmap layout.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        };

        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = unsafe {
            CreateDIBSection(Some(self.mem_dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        }
        .context("CreateDIBSection")?;

        let previous = unsafe { SelectObject(self.mem_dc, bitmap.into()) };
        if let Some(old) = self.bitmap.replace(bitmap) {
            unsafe {
                let _ = DeleteObject(old.into());
            }
        } else {
            self.previous_bitmap = previous;
        }
        self.bits = bits as *mut u8;
        self.width = width;
        self.height = height;
        Ok(())
    }

    pub fn present(&mut self, renderer: &Renderer, bounds: DirtyRect) -> Result<()> {
        self.ensure(bounds.w, bounds.h)?;
        if self.bits.is_null() {
            return Err(anyhow!("DIB has no backing memory"));
        }

        let stride = self.width as usize * 4;
        let len = stride * self.height as usize;
        let dst = unsafe { std::slice::from_raw_parts_mut(self.bits, len) };
        renderer.blit_bgra(
            dst,
            stride,
            DirtyRect {
                x: 0,
                y: 0,
                w: bounds.w,
                h: bounds.h,
            },
        );

        if !self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_SHOWNA);
            }
            self.visible = true;
        }

        let position = POINT {
            x: bounds.x,
            y: bounds.y,
        };
        let size = SIZE {
            cx: bounds.w,
            cy: bounds.h,
        };
        let source = POINT { x: 0, y: 0 };
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };

        unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                Some(self.screen_dc),
                Some(&position),
                Some(&size),
                Some(self.mem_dc),
                Some(&source),
                Default::default(),
                Some(&blend),
                ULW_ALPHA,
            )
        }
        .context("UpdateLayeredWindow")?;

        // Newly created windows can steal the top of the z-order.
        if self.last_topmost.elapsed().as_secs() >= 1 {
            self.last_topmost = Instant::now();
            unsafe {
                let _ = SetWindowPos(
                    self.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                );
            }
        }
        Ok(())
    }

    pub fn hide(&mut self) {
        if self.visible {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
            self.visible = false;
        }
    }
}

impl Drop for LayeredSurface {
    fn drop(&mut self) {
        unsafe {
            if self.bitmap.is_some() && !self.previous_bitmap.is_invalid() {
                SelectObject(self.mem_dc, self.previous_bitmap);
            }
            if let Some(bitmap) = self.bitmap.take() {
                let _ = DeleteObject(bitmap.into());
            }
            let _ = DeleteDC(self.mem_dc);
            ReleaseDC(None, self.screen_dc);
        }
    }
}
