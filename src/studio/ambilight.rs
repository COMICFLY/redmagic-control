//! 纯 Rust Win32 屏幕取色流光溢彩（Ambilight）采样器：
//! 将主显示器实时降采样为 16×6 色彩网格 + 20 段顶部边缘灯条色彩，每帧耗时 < 1ms。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;

pub const AMBI_W: usize = 16;
pub const AMBI_H: usize = 6;
pub const AMBI_LOGO_N: usize = 20;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbilightSnapshot {
    pub active: bool,
    pub width: usize,
    pub height: usize,
    /// 16 × 6 × 3 字节 (R, G, B)
    pub grid: Vec<u8>,
    /// 20 × 3 字节 (R, G, B) 对应顶部 20 颗 Logo 灯条
    pub logo: Vec<u8>,
}

impl Default for AmbilightSnapshot {
    fn default() -> Self {
        Self {
            active: false,
            width: AMBI_W,
            height: AMBI_H,
            grid: vec![0u8; AMBI_W * AMBI_H * 3],
            logo: vec![0u8; AMBI_LOGO_N * 3],
        }
    }
}

#[derive(Clone)]
pub struct AmbilightEngine {
    running: Arc<AtomicBool>,
    state: Arc<Mutex<AmbilightSnapshot>>,
}

impl AmbilightEngine {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(AmbilightSnapshot::default())),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn snapshot(&self) -> AmbilightSnapshot {
        self.state.lock().unwrap().clone()
    }

    pub fn set_enabled(&self, enabled: bool) {
        if enabled {
            self.start();
        } else {
            self.stop();
        }
    }

    pub fn start(&self) {
        if self.running.swap(true, Ordering::SeqCst) {
            return;
        }
        {
            let mut st = self.state.lock().unwrap();
            st.active = true;
        }
        let running = self.running.clone();
        let state = self.state.clone();
        thread::spawn(move || {
            #[cfg(windows)]
            run_screen_sampler(running, state);
            #[cfg(not(windows))]
            {
                let _ = (running, state);
            }
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        let mut st = self.state.lock().unwrap();
        *st = AmbilightSnapshot::default();
    }
}

/// 轻微增强饱和度，让屏幕色在机械键盘 LED 灯珠上更鲜艳通透
fn boost_vibrance(r: f32, g: f32, b: f32, sat: f32) -> (f32, f32, f32) {
    let avg = (r + g + b) / 3.0;
    let nr = (avg + (r - avg) * sat).clamp(0.0, 255.0);
    let ng = (avg + (g - avg) * sat).clamp(0.0, 255.0);
    let nb = (avg + (b - avg) * sat).clamp(0.0, 255.0);
    (nr, ng, nb)
}

#[cfg(windows)]
fn run_screen_sampler(running: Arc<AtomicBool>, state: Arc<Mutex<AmbilightSnapshot>>) {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Graphics::Gdi::{
        CreateCompatibleDC, CreateDIBSection, DeleteDC, DeleteObject, GetDC, ReleaseDC,
        SelectObject, SetStretchBltMode, StretchBlt, BITMAPINFO, BITMAPINFOHEADER, BI_RGB,
        DIB_RGB_COLORS, HALFTONE, SRCCOPY,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

    const SAMPLE_W: i32 = 32;
    const SAMPLE_H: i32 = 12;

    let mut smooth_grid = vec![0.0f32; AMBI_W * AMBI_H * 3];
    let mut smooth_logo = vec![0.0f32; AMBI_LOGO_N * 3];

    unsafe {
        let hdc_screen = GetDC(null_mut());
        if hdc_screen.is_null() {
            return;
        }
        let hdc_mem = CreateCompatibleDC(hdc_screen);
        let mut bmi: BITMAPINFO = std::mem::zeroed();
        bmi.bmiHeader = BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: SAMPLE_W,
            biHeight: -SAMPLE_H, // top-down DIB
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB,
            biSizeImage: 0,
            biXPelsPerMeter: 0,
            biYPelsPerMeter: 0,
            biClrUsed: 0,
            biClrImportant: 0,
        };
        let mut bits: *mut std::ffi::c_void = null_mut();
        let hbmp = CreateDIBSection(hdc_mem, &bmi, DIB_RGB_COLORS, &mut bits, null_mut(), 0);
        if hbmp.is_null() || bits.is_null() {
            DeleteDC(hdc_mem);
            ReleaseDC(null_mut(), hdc_screen);
            return;
        }
        let old_obj = SelectObject(hdc_mem, hbmp);
        SetStretchBltMode(hdc_mem, HALFTONE);

        while running.load(Ordering::SeqCst) {
            let sw = GetSystemMetrics(SM_CXSCREEN).max(64);
            let sh = GetSystemMetrics(SM_CYSCREEN).max(64);

            let ok = StretchBlt(
                hdc_mem, 0, 0, SAMPLE_W, SAMPLE_H, hdc_screen, 0, 0, sw, sh, SRCCOPY,
            );
            if ok != 0 {
                let px = std::slice::from_raw_parts(
                    bits as *const u8,
                    (SAMPLE_W * SAMPLE_H * 4) as usize,
                );
                let mut out_grid = vec![0u8; AMBI_W * AMBI_H * 3];
                for gy in 0..AMBI_H {
                    for gx in 0..AMBI_W {
                        // 每个网格对 2×2 采样像素求均值，进一步抗噪
                        let mut sr = 0.0f32;
                        let mut sg = 0.0f32;
                        let mut sb = 0.0f32;
                        for dy in 0..2 {
                            for dx in 0..2 {
                                let idx = (((gy * 2 + dy) * (SAMPLE_W as usize)) + (gx * 2 + dx)) * 4;
                                sb += px[idx] as f32;
                                sg += px[idx + 1] as f32;
                                sr += px[idx + 2] as f32;
                            }
                        }
                        let (vr, vg, vb) = boost_vibrance(sr * 0.25, sg * 0.25, sb * 0.25, 1.38);
                        let o = (gy * AMBI_W + gx) * 3;
                        smooth_grid[o] = smooth_grid[o] * 0.55 + vr * 0.45;
                        smooth_grid[o + 1] = smooth_grid[o + 1] * 0.55 + vg * 0.45;
                        smooth_grid[o + 2] = smooth_grid[o + 2] * 0.55 + vb * 0.45;
                        out_grid[o] = smooth_grid[o].round() as u8;
                        out_grid[o + 1] = smooth_grid[o + 1].round() as u8;
                        out_grid[o + 2] = smooth_grid[o + 2].round() as u8;
                    }
                }

                // 顶部边缘映射给 20 颗 Logo 灯条
                let mut out_logo = vec![0u8; AMBI_LOGO_N * 3];
                for li in 0..AMBI_LOGO_N {
                    let sx = ((li * (SAMPLE_W as usize - 1)) / (AMBI_LOGO_N - 1))
                        .min(SAMPLE_W as usize - 1);
                    let idx0 = sx * 4;
                    let idx1 = ((SAMPLE_W as usize) + sx) * 4;
                    let r = (px[idx0 + 2] as f32 + px[idx1 + 2] as f32) * 0.5;
                    let g = (px[idx0 + 1] as f32 + px[idx1 + 1] as f32) * 0.5;
                    let b = (px[idx0] as f32 + px[idx1] as f32) * 0.5;
                    let (vr, vg, vb) = boost_vibrance(r, g, b, 1.42);
                    let o = li * 3;
                    smooth_logo[o] = smooth_logo[o] * 0.55 + vr * 0.45;
                    smooth_logo[o + 1] = smooth_logo[o + 1] * 0.55 + vg * 0.45;
                    smooth_logo[o + 2] = smooth_logo[o + 2] * 0.55 + vb * 0.45;
                    out_logo[o] = smooth_logo[o].round() as u8;
                    out_logo[o + 1] = smooth_logo[o + 1].round() as u8;
                    out_logo[o + 2] = smooth_logo[o + 2].round() as u8;
                }

                if let Ok(mut st) = state.lock() {
                    st.grid = out_grid;
                    st.logo = out_logo;
                }
            }
            thread::sleep(Duration::from_millis(40)); // ~25 FPS
        }

        SelectObject(hdc_mem, old_obj);
        DeleteObject(hbmp);
        DeleteDC(hdc_mem);
        ReleaseDC(null_mut(), hdc_screen);
        let _ = null::<u8>();
    }
}
