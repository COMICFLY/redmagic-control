//! 纯 Rust 低延迟全局按键活动感知器（Win32 GetAsyncKeyState 60Hz 轮询）：
//! - 零全局钩子注入，无杀软误报与卡顿风险；
//! - 将按下的物理按键精准映射到 GK001S hwSlot (0..125) 与归一化坐标 (nx, ny)，
//!   供打字涟漪、热力衰减与自定义 JS Shader 实时响应。

use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyPulse {
    pub slot: u8,
    pub x: f32,
    pub y: f32,
    pub age_ms: u32,
}

#[derive(Debug, Clone, Default, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct KeyActivitySnapshot {
    pub active: bool,
    pub pressed_slots: Vec<u8>,
    pub pulses: Vec<KeyPulse>,
}

struct PulseItem {
    slot: u8,
    x: f32,
    y: f32,
    at: Instant,
}

#[derive(Clone)]
pub struct KeySensor {
    running: Arc<AtomicBool>,
    pressed: Arc<Mutex<Vec<u8>>>,
    pulses: Arc<Mutex<VecDeque<PulseItem>>>,
}

impl KeySensor {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            pressed: Arc::new(Mutex::new(Vec::new())),
            pulses: Arc::new(Mutex::new(VecDeque::new())),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
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
        let running = self.running.clone();
        let pressed = self.pressed.clone();
        let pulses = self.pulses.clone();
        thread::spawn(move || {
            #[cfg(windows)]
            run_key_poll(running, pressed, pulses);
            #[cfg(not(windows))]
            {
                let _ = (running, pressed, pulses);
            }
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        self.pressed.lock().unwrap().clear();
        self.pulses.lock().unwrap().clear();
    }

    pub fn snapshot(&self) -> KeyActivitySnapshot {
        let active = self.running.load(Ordering::SeqCst);
        if !active {
            return KeyActivitySnapshot::default();
        }
        let now = Instant::now();
        let pressed_slots = self.pressed.lock().unwrap().clone();
        let mut q = self.pulses.lock().unwrap();
        while let Some(front) = q.front() {
            if now.duration_since(front.at).as_millis() > 1400 {
                q.pop_front();
            } else {
                break;
            }
        }
        let pulses = q
            .iter()
            .map(|p| KeyPulse {
                slot: p.slot,
                x: p.x,
                y: p.y,
                age_ms: now.duration_since(p.at).as_millis().min(1400) as u32,
            })
            .collect();
        KeyActivitySnapshot {
            active,
            pressed_slots,
            pulses,
        }
    }
}

/// (Win32 VK, GK001S hwSlot, 归一化 x, 归一化 y) — 对齐 kb-layout.js 物理坐标
#[cfg(windows)]
const VK_MAP: &[(i32, u8, f32, f32)] = &[
    // Row 0: Esc + F1..F12 + Ins
    (0x1B, 0, 0.03, 0.14),   // Esc
    (0x70, 6, 0.09, 0.14),   // F1
    (0x71, 12, 0.14, 0.14),  // F2
    (0x72, 18, 0.19, 0.14),  // F3
    (0x73, 24, 0.24, 0.14),  // F4
    (0x74, 30, 0.30, 0.14),  // F5
    (0x75, 36, 0.35, 0.14),  // F6
    (0x76, 42, 0.40, 0.14),  // F7
    (0x77, 48, 0.45, 0.14),  // F8
    (0x78, 54, 0.51, 0.14),  // F9
    (0x79, 60, 0.56, 0.14),  // F10
    (0x7A, 66, 0.61, 0.14),  // F11
    (0x7B, 72, 0.66, 0.14),  // F12
    (0x2D, 78, 0.72, 0.14),  // Insert
    // Row 1: ` 1..0 - = Backspace Home NumLock / * -
    (0xC0, 1, 0.03, 0.30),   // `
    (0x31, 7, 0.08, 0.30),   // 1
    (0x32, 13, 0.13, 0.30),  // 2
    (0x33, 19, 0.18, 0.30),  // 3
    (0x34, 25, 0.23, 0.30),  // 4
    (0x35, 31, 0.28, 0.30),  // 5
    (0x36, 37, 0.33, 0.30),  // 6
    (0x37, 43, 0.38, 0.30),  // 7
    (0x38, 49, 0.43, 0.30),  // 8
    (0x39, 55, 0.48, 0.30),  // 9
    (0x30, 61, 0.53, 0.30),  // 0
    (0xBD, 67, 0.58, 0.30),  // -
    (0xBB, 73, 0.63, 0.30),  // =
    (0x08, 79, 0.70, 0.30),  // Backspace
    (0x24, 97, 0.78, 0.30),  // Home
    (0x90, 103, 0.83, 0.30), // NumLock
    (0x6F, 109, 0.88, 0.30), // Num /
    (0x6A, 115, 0.93, 0.30), // Num *
    (0x6D, 121, 0.97, 0.30), // Num -
    // Row 2: Tab Q..P [ ] \ Del Num7..9 +
    (0x09, 2, 0.04, 0.46),   // Tab
    (0x51, 8, 0.10, 0.46),   // Q
    (0x57, 14, 0.15, 0.46),  // W
    (0x45, 20, 0.20, 0.46),  // E
    (0x52, 26, 0.25, 0.46),  // R
    (0x54, 32, 0.30, 0.46),  // T
    (0x59, 38, 0.35, 0.46),  // Y
    (0x55, 44, 0.40, 0.46),  // U
    (0x49, 50, 0.45, 0.46),  // I
    (0x4F, 56, 0.50, 0.46),  // O
    (0x50, 62, 0.55, 0.46),  // P
    (0xDB, 68, 0.60, 0.46),  // [
    (0xDD, 74, 0.65, 0.46),  // ]
    (0xDC, 80, 0.71, 0.46),  // \
    (0x2E, 98, 0.78, 0.46),  // Delete
    (0x67, 104, 0.83, 0.46), // Num 7
    (0x68, 110, 0.88, 0.46), // Num 8
    (0x69, 116, 0.93, 0.46), // Num 9
    (0x6B, 122, 0.97, 0.54), // Num +
    // Row 3: Caps A..L ; ' Enter PgUp Num4..6
    (0x14, 3, 0.05, 0.62),   // Caps
    (0x41, 9, 0.11, 0.62),   // A
    (0x53, 15, 0.16, 0.62),  // S
    (0x44, 21, 0.21, 0.62),  // D
    (0x46, 27, 0.26, 0.62),  // F
    (0x47, 33, 0.31, 0.62),  // G
    (0x48, 39, 0.36, 0.62),  // H
    (0x4A, 45, 0.41, 0.62),  // J
    (0x4B, 51, 0.46, 0.62),  // K
    (0x4C, 57, 0.51, 0.62),  // L
    (0xBA, 63, 0.56, 0.62),  // ;
    (0xDE, 69, 0.61, 0.62),  // '
    (0x0D, 81, 0.69, 0.62),  // Enter
    (0x21, 99, 0.78, 0.62),  // PgUp
    (0x64, 105, 0.83, 0.62), // Num 4
    (0x65, 111, 0.88, 0.62), // Num 5
    (0x66, 117, 0.93, 0.62), // Num 6
    // Row 4: LShift Z..M , . / RShift Up PgDn Num1..3
    (0xA0, 4, 0.06, 0.77),   // LShift
    (0x5A, 16, 0.14, 0.77),  // Z
    (0x58, 22, 0.19, 0.77),  // X
    (0x43, 28, 0.24, 0.77),  // C
    (0x56, 34, 0.29, 0.77),  // V
    (0x42, 40, 0.34, 0.77),  // B
    (0x4E, 46, 0.39, 0.77),  // N
    (0x4D, 52, 0.44, 0.77),  // M
    (0xBC, 58, 0.49, 0.77),  // ,
    (0xBE, 64, 0.54, 0.77),  // .
    (0xBF, 70, 0.59, 0.77),  // /
    (0xA1, 82, 0.67, 0.77),  // RShift
    (0x26, 94, 0.73, 0.77),  // Up
    (0x22, 100, 0.78, 0.77), // PgDn
    (0x61, 106, 0.83, 0.77), // Num 1
    (0x62, 112, 0.88, 0.77), // Num 2
    (0x63, 118, 0.93, 0.77), // Num 3
    // Row 5: LCtrl LWin LAlt Space RAlt RCtrl Left Down Right Num0 Num.
    (0xA2, 5, 0.04, 0.92),   // LCtrl
    (0x5B, 11, 0.09, 0.92),  // LWin
    (0xA4, 17, 0.15, 0.92),  // LAlt
    (0x20, 35, 0.36, 0.92),  // Space
    (0xA5, 59, 0.57, 0.92),  // RAlt
    (0xA3, 71, 0.66, 0.92),  // RCtrl
    (0x25, 89, 0.69, 0.92),  // Left
    (0x28, 95, 0.74, 0.92),  // Down
    (0x27, 101, 0.79, 0.92), // Right
    (0x60, 107, 0.86, 0.92), // Num 0
    (0x6E, 119, 0.93, 0.92), // Num .
];

#[cfg(windows)]
fn run_key_poll(
    running: Arc<AtomicBool>,
    pressed: Arc<Mutex<Vec<u8>>>,
    pulses: Arc<Mutex<VecDeque<PulseItem>>>,
) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    let mut prev = vec![false; VK_MAP.len()];
    while running.load(Ordering::SeqCst) {
        let mut cur_pressed = Vec::new();
        let mut new_pulses = Vec::new();
        let now = Instant::now();

        for (i, &(vk, slot, x, y)) in VK_MAP.iter().enumerate() {
            let down = unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
            if down {
                cur_pressed.push(slot);
                if !prev[i] {
                    new_pulses.push(PulseItem { slot, x, y, at: now });
                }
            }
            prev[i] = down;
        }

        if let Ok(mut p) = pressed.lock() {
            *p = cur_pressed;
        }
        if !new_pulses.is_empty() {
            if let Ok(mut q) = pulses.lock() {
                for item in new_pulses {
                    q.push_back(item);
                    while q.len() > 24 {
                        q.pop_front();
                    }
                }
            }
        }
        thread::sleep(Duration::from_millis(16));
    }
}
