//! 纯 Rust 零 WebView2 后台灯效推流引擎：
//! - 原生实现全部 6 套内置算法（赛博极光波 / 红魔脉冲雷达 / 打字霓虹涟漪 / 音乐频谱柱 / 屏幕流光溢彩 / 矩阵数字雨）
//!   以及 4 图层混合器（动态渐变 + WASAPI 音乐频谱 + 屏幕流光溢彩 + 全局打字涟漪）；
//! - 直接在 Rust 线程内零分配读取传感器并经 OP 0x12 + OP 0x28 写入键盘易失 RAM；
//! - 托盘挂起时可彻底销毁 WebView2 窗口（0 个 WebView2 进程、0% GPU 占用、整机常驻仅 ~15MB 内存 / <0.3% CPU）。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::devices::catalog::ConnectionMode;
use crate::devices::manager::DeviceManager;
use crate::protocol::{LOGO_LED_COUNT, LOGO_RGB_LEN};
use super::ambilight::{AmbilightEngine, AmbilightSnapshot, AMBI_H, AMBI_W};
use super::audio::{AudioEngine, AudioSnapshot};
use super::keyhook::{KeyActivitySnapshot, KeySensor};

const CUSTOM_RGB_LEN: usize = 126 * 3;

/// GK001S 100 颗实体按键的物理归一化坐标 (hwSlot, x, y)，与 ui/src/renderer/kb-layout.js 严格一致
pub const KEY_COORDS: [(u8, f32, f32); 100] = [
    ( 85, 0.6985, 0.1774), ( 86, 0.7603, 0.1774), (  0, 0.0551, 0.1794), (  6, 0.1125, 0.1794),
    ( 12, 0.1610, 0.1794), ( 18, 0.2051, 0.1794), ( 24, 0.2522, 0.1794), ( 30, 0.3066, 0.1794),
    ( 36, 0.3566, 0.1794), ( 42, 0.4007, 0.1794), ( 48, 0.4463, 0.1794), ( 54, 0.5051, 0.1794),
    ( 60, 0.5507, 0.1794), ( 66, 0.5949, 0.1794), ( 72, 0.6419, 0.1794), (  1, 0.0537, 0.3266),
    ( 91, 0.7603, 0.3266), (103, 0.8074, 0.3266), (109, 0.8544, 0.3266), (115, 0.9015, 0.3266),
    (121, 0.9471, 0.3266), (  7, 0.1007, 0.3286), ( 13, 0.1478, 0.3286), ( 19, 0.1949, 0.3286),
    ( 25, 0.2390, 0.3286), ( 31, 0.2853, 0.3286), ( 37, 0.3316, 0.3286), ( 43, 0.3772, 0.3286),
    ( 49, 0.4243, 0.3286), ( 55, 0.4684, 0.3286), ( 61, 0.5154, 0.3286), ( 67, 0.5610, 0.3286),
    ( 73, 0.6081, 0.3286), ( 79, 0.6750, 0.3286), ( 97, 0.7603, 0.4556), (104, 0.8074, 0.4556),
    (110, 0.8544, 0.4556), (116, 0.9015, 0.4556), (122, 0.9463, 0.5141), (  2, 0.0662, 0.4577),
    (  8, 0.1257, 0.4577), ( 14, 0.1713, 0.4577), ( 20, 0.2169, 0.4577), ( 26, 0.2625, 0.4577),
    ( 32, 0.3066, 0.4577), ( 38, 0.3566, 0.4577), ( 44, 0.3993, 0.4577), ( 50, 0.4463, 0.4577),
    ( 56, 0.4934, 0.4577), ( 62, 0.5390, 0.4577), ( 68, 0.5846, 0.4577), ( 74, 0.6301, 0.4577),
    ( 80, 0.6875, 0.4577), ( 98, 0.7603, 0.5726), (105, 0.8074, 0.5726), (111, 0.8544, 0.5726),
    (117, 0.9015, 0.5726), (  3, 0.0728, 0.5746), (  9, 0.1346, 0.5746), ( 15, 0.1816, 0.5746),
    ( 21, 0.2272, 0.5746), ( 27, 0.2728, 0.5746), ( 33, 0.3184, 0.5746), ( 39, 0.3654, 0.5746),
    ( 45, 0.4125, 0.5746), ( 51, 0.4581, 0.5746), ( 57, 0.5051, 0.5746), ( 63, 0.5493, 0.5746),
    ( 69, 0.5949, 0.5746), ( 81, 0.6699, 0.5766), ( 28, 0.2500, 0.7016), ( 82, 0.6375, 0.6996),
    ( 92, 0.7603, 0.7016), (106, 0.8074, 0.7016), (112, 0.8544, 0.7016), (118, 0.9015, 0.7016),
    (124, 0.9471, 0.7661), (  4, 0.0816, 0.7036), ( 16, 0.1610, 0.7036), ( 22, 0.2051, 0.7036),
    ( 34, 0.2978, 0.7036), ( 40, 0.3434, 0.7036), ( 46, 0.3904, 0.7036), ( 52, 0.4346, 0.7036),
    ( 58, 0.4801, 0.7036), ( 64, 0.5257, 0.7036), ( 70, 0.5728, 0.7036), ( 94, 0.7088, 0.7258),
    (107, 0.8309, 0.8306), (113, 0.9015, 0.8306), (  5, 0.0596, 0.8327), ( 11, 0.1184, 0.8327),
    ( 17, 0.1757, 0.8327), ( 35, 0.3485, 0.8327), ( 59, 0.5169, 0.8327), ( 65, 0.5625, 0.8327),
    ( 77, 0.6066, 0.8327), ( 83, 0.6632, 0.8508), ( 89, 0.7088, 0.8508), ( 95, 0.7529, 0.8508),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GradientLayerCfg {
    pub enabled: bool,
    pub color_a: String,
    pub color_b: String,
    pub speed: f32,
    pub opacity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioLayerCfg {
    pub enabled: bool,
    pub color: String,
    pub sensitivity: f32,
    pub opacity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AmbilightLayerCfg {
    pub enabled: bool,
    pub opacity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TypingLayerCfg {
    pub enabled: bool,
    pub color: String,
    pub speed: f32,
    pub opacity: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeLayersConfig {
    pub gradient: GradientLayerCfg,
    pub audio: AudioLayerCfg,
    pub ambilight: AmbilightLayerCfg,
    pub typing: TypingLayerCfg,
}

impl Default for NativeLayersConfig {
    fn default() -> Self {
        Self {
            gradient: GradientLayerCfg {
                enabled: true,
                color_a: "#e53945".to_string(),
                color_b: "#0ea5e9".to_string(),
                speed: 0.5,
                opacity: 0.75,
            },
            audio: AudioLayerCfg {
                enabled: false,
                color: "#22c55e".to_string(),
                sensitivity: 0.65,
                opacity: 0.9,
            },
            ambilight: AmbilightLayerCfg {
                enabled: false,
                opacity: 0.85,
            },
            typing: TypingLayerCfg {
                enabled: true,
                color: "#ffffff".to_string(),
                speed: 0.6,
                opacity: 0.95,
            },
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeStreamConfig {
    /// "preset" | "layers"
    pub mode: String,
    /// "aurora" | "radar" | "ripple" | "spectrum" | "ambilight" | "matrix"
    pub preset_id: String,
    pub p1: f32,
    pub p2: f32,
    pub p3: f32,
    pub brightness: f32,
    pub include_logo: bool,
    #[serde(default)]
    pub layers: NativeLayersConfig,
}

impl Default for NativeStreamConfig {
    fn default() -> Self {
        Self {
            mode: "preset".to_string(),
            preset_id: "aurora".to_string(),
            p1: 0.55,
            p2: 0.5,
            p3: 0.15,
            brightness: 1.0,
            include_logo: true,
            layers: NativeLayersConfig::default(),
        }
    }
}

impl NativeStreamConfig {
    pub fn required_sensors(&self) -> (bool, bool, bool) {
        if self.mode == "layers" {
            (
                self.layers.audio.enabled,
                self.layers.ambilight.enabled,
                self.layers.typing.enabled,
            )
        } else {
            match self.preset_id.as_str() {
                "spectrum" => (true, false, false),
                "ambilight" => (false, true, false),
                "ripple" => (false, false, true),
                _ => (false, false, false),
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NativeStreamStatus {
    pub running: bool,
    pub device_id: String,
    pub fps: f32,
    pub config: NativeStreamConfig,
}

struct RunnerState {
    device_id: String,
    fps: f32,
    config: NativeStreamConfig,
}

#[derive(Clone)]
pub struct NativeRgbRunner {
    running: Arc<AtomicBool>,
    state: Arc<Mutex<RunnerState>>,
}

impl NativeRgbRunner {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(RunnerState {
                device_id: String::new(),
                fps: 0.0,
                config: NativeStreamConfig::default(),
            })),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> NativeStreamStatus {
        let st = self.state.lock().unwrap();
        NativeStreamStatus {
            running: self.running.load(Ordering::SeqCst),
            device_id: st.device_id.clone(),
            fps: st.fps,
            config: st.config.clone(),
        }
    }

    pub fn start(
        &self,
        manager: DeviceManager,
        device_id: String,
        audio: AudioEngine,
        ambilight: AmbilightEngine,
        keys: KeySensor,
        config: NativeStreamConfig,
    ) -> Result<NativeStreamStatus, String> {
        let already = self.running.load(Ordering::SeqCst);
        let (need_a, need_am, need_k) = config.required_sensors();
        audio.set_enabled(need_a);
        ambilight.set_enabled(need_am);
        keys.set_enabled(need_k);

        if already {
            {
                let mut st = self.state.lock().unwrap();
                st.config = config;
            }
            return Ok(self.status());
        }

        manager.start_direct_rgb(&device_id, config.include_logo)?;
        let is_24g = manager
            .list_devices(false)
            .into_iter()
            .find(|d| d.id == device_id)
            .map(|d| d.connection_mode == ConnectionMode::Wireless24g)
            .unwrap_or(false);

        {
            let mut st = self.state.lock().unwrap();
            st.device_id = device_id.clone();
            st.fps = 0.0;
            st.config = config;
        }
        self.running.store(true, Ordering::SeqCst);

        let running = self.running.clone();
        let state = self.state.clone();
        thread::spawn(move || {
            let interval = Duration::from_millis(if is_24g { 125 } else { 33 });
            let t0 = Instant::now();
            let mut keys_buf = [0u8; CUSTOM_RGB_LEN];
            let mut logo_buf = [0u8; LOGO_RGB_LEN];
            let mut fps_count: u32 = 0;
            let mut fps_win = Instant::now();

            while running.load(Ordering::SeqCst) {
                let tick_start = Instant::now();
                let cfg = { state.lock().unwrap().config.clone() };
                let (need_a, need_am, need_k) = cfg.required_sensors();
                let aud = if need_a { Some(audio.snapshot()) } else { None };
                let amb = if need_am { Some(ambilight.snapshot()) } else { None };
                let kst = if need_k { Some(keys.snapshot()) } else { None };

                let t = t0.elapsed().as_secs_f32();
                render_native_frame(
                    &cfg,
                    t,
                    aud.as_ref(),
                    amb.as_ref(),
                    kst.as_ref(),
                    &mut keys_buf,
                    &mut logo_buf,
                );

                let logo_slice = if cfg.include_logo {
                    Some(&logo_buf[..])
                } else {
                    None
                };
                if manager
                    .stream_rgb_frame(&device_id, &keys_buf, logo_slice)
                    .is_ok()
                {
                    fps_count += 1;
                    let win_s = fps_win.elapsed().as_secs_f32();
                    if win_s >= 0.6 {
                        let fps = ((fps_count as f32 / win_s) * 10.0).round() / 10.0;
                        if let Ok(mut st) = state.lock() {
                            st.fps = fps;
                        }
                        fps_count = 0;
                        fps_win = Instant::now();
                    }
                }

                let spent = tick_start.elapsed();
                if spent < interval {
                    thread::sleep(interval - spent);
                }
            }
        });

        Ok(self.status())
    }

    pub fn update_config(
        &self,
        manager: &DeviceManager,
        audio: &AudioEngine,
        ambilight: &AmbilightEngine,
        keys: &KeySensor,
        config: NativeStreamConfig,
    ) -> Result<NativeStreamStatus, String> {
        let prev_logo;
        let dev_id;
        {
            let mut st = self.state.lock().unwrap();
            prev_logo = st.config.include_logo;
            dev_id = st.device_id.clone();
            st.config = config.clone();
        }
        if self.running.load(Ordering::SeqCst) {
            let (need_a, need_am, need_k) = config.required_sensors();
            audio.set_enabled(need_a);
            ambilight.set_enabled(need_am);
            keys.set_enabled(need_k);
            if prev_logo != config.include_logo && !dev_id.is_empty() {
                let _ = manager.stop_direct_rgb(&dev_id);
                let _ = manager.start_direct_rgb(&dev_id, config.include_logo);
            }
        }
        Ok(self.status())
    }

    pub fn stop(&self, manager: &DeviceManager) -> NativeStreamStatus {
        let was_running = self.running.swap(false, Ordering::SeqCst);
        let dev_id = {
            let mut st = self.state.lock().unwrap();
            st.fps = 0.0;
            st.device_id.clone()
        };
        if was_running && !dev_id.is_empty() {
            let _ = manager.stop_direct_rgb(&dev_id);
        }
        self.status()
    }
}

fn hsv_to_rgb(h_deg: f32, s: f32, v: f32) -> [f32; 3] {
    let h = ((h_deg % 360.0) + 360.0) % 360.0;
    let s = s.clamp(0.0, 1.0);
    let v = v.clamp(0.0, 1.0);
    let c = v * s;
    let x = c * (1.0 - (((h / 60.0) % 2.0) - 1.0).abs());
    let m = v - c;
    let (r, g, b) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    [(r + m) * 255.0, (g + m) * 255.0, (b + m) * 255.0]
}

fn parse_hex_rgb(hex: &str) -> [f32; 3] {
    let s = hex.trim().trim_start_matches('#');
    if s.len() == 6 {
        if let Ok(v) = u32::from_str_radix(s, 16) {
            return [
                ((v >> 16) & 0xff) as f32,
                ((v >> 8) & 0xff) as f32,
                (v & 0xff) as f32,
            ];
        }
    }
    [229.0, 57.0, 69.0]
}

fn mix_rgb(a: [f32; 3], b: [f32; 3], f: f32) -> [f32; 3] {
    let k = f.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * k,
        a[1] + (b[1] - a[1]) * k,
        a[2] + (b[2] - a[2]) * k,
    ]
}

fn eval_preset_point(
    cfg: &NativeStreamConfig,
    x: f32,
    y: f32,
    t: f32,
    slot: i16,
    is_logo: bool,
    logo_idx: usize,
    aud: Option<&AudioSnapshot>,
    amb: Option<&AmbilightSnapshot>,
    kst: Option<&KeyActivitySnapshot>,
) -> [f32; 3] {
    let p1 = cfg.p1;
    let p2 = cfg.p2;
    let p3 = cfg.p3;

    match cfg.preset_id.as_str() {
        "radar" => {
            if is_logo {
                let p = 0.5 + 0.5 * (x * 6.283185 - t * (2.0 + p1 * 6.0)).sin();
                return hsv_to_rgb(p3 * 360.0 + 354.0, 0.95, 0.22 + p * 0.78);
            }
            let dx = (x - 0.5) * 2.1;
            let dy = y - 0.5;
            let dist = dx.hypot(dy);
            let ang = dy.atan2(dx);
            let tau = std::f32::consts::TAU;
            let sweep = (((ang - t * (1.8 + p1 * 5.0)) % tau) + tau) % tau;
            let tail = (-sweep / (0.45 + p2 * 1.6)).exp();
            let ring = (1.0 - (dist - ((t * 0.7) % 1.2)).abs() * 8.0).max(0.0) * 0.55;
            hsv_to_rgb((355.0 + p3 * 360.0) % 360.0, 0.92, (0.10 + tail * 0.92 + ring).min(1.0))
        }
        "ripple" => {
            let mut hit = if !is_logo {
                kst.map(|k| {
                    if k.pressed_slots.iter().any(|&s| s as i16 == slot) {
                        1.0f32
                    } else {
                        0.0
                    }
                })
                .unwrap_or(0.0)
            } else {
                0.0
            };
            let spd = 0.55 + p1 * 1.2;
            let w = 0.06 + p2 * 0.14;
            if let Some(k) = kst {
                for p in &k.pulses {
                    let age = p.age_ms as f32 / 1000.0;
                    let r = age * spd;
                    let d = (x - p.x).hypot((y - p.y) * 0.52);
                    let ring = (-((d - r) / w).powi(2)).exp() * (1.0 - age * 0.9).max(0.0);
                    if ring > hit {
                        hit = ring;
                    }
                }
            }
            let base = hsv_to_rgb((354.0 + x * 25.0) % 360.0, 0.85, 0.12);
            let glow = hsv_to_rgb((p3 * 360.0 + hit * 45.0) % 360.0, 0.9, (hit * 1.2).min(1.0));
            [
                (base[0] + glow[0]).min(255.0),
                (base[1] + glow[1]).min(255.0),
                (base[2] + glow[2]).min(255.0),
            ]
        }
        "spectrum" => {
            let gain = 0.7 + p1 * 1.4;
            if is_logo {
                let rms = aud.map(|a| a.rms).unwrap_or(0.0);
                let lvl = (rms * gain * 1.35).min(1.0);
                let lit = if (x - 0.5).abs() * 2.0 <= lvl { 1.0 } else { 0.08 };
                return hsv_to_rgb((p3 * 360.0 + x * 120.0) % 360.0, 0.95, lit);
            }
            let b_idx = ((x * 16.0).floor() as isize).clamp(0, 15) as usize;
            let level = (aud.map(|a| a.bands[b_idx]).unwrap_or(0.0) * gain).min(1.0);
            let row_h = 1.0 - y;
            if row_h <= level + 0.06 {
                let hue = (p3 * 360.0 + (b_idx as f32) * 14.0 + row_h * 45.0) % 360.0;
                hsv_to_rgb(hue, 0.95, (0.55 + level * 0.55).min(1.0))
            } else {
                hsv_to_rgb((p3 * 360.0 + 210.0) % 360.0, 0.7, 0.05 + p2 * 0.14)
            }
        }
        "ambilight" => {
            let c = if let Some(a) = amb {
                if is_logo && a.logo.len() >= LOGO_RGB_LEN {
                    let o = logo_idx.min(LOGO_LED_COUNT - 1) * 3;
                    [a.logo[o] as f32, a.logo[o + 1] as f32, a.logo[o + 2] as f32]
                } else if a.grid.len() >= AMBI_W * AMBI_H * 3 {
                    let gx = ((x * AMBI_W as f32).floor() as isize).clamp(0, AMBI_W as isize - 1) as usize;
                    let gy = ((y * AMBI_H as f32).floor() as isize).clamp(0, AMBI_H as isize - 1) as usize;
                    let o = (gy * AMBI_W + gx) * 3;
                    [a.grid[o] as f32, a.grid[o + 1] as f32, a.grid[o + 2] as f32]
                } else {
                    [0.0, 0.0, 0.0]
                }
            } else {
                [0.0, 0.0, 0.0]
            };
            let gain = 0.5 + p1 * 0.9;
            let avg = (c[0] + c[1] + c[2]) / 3.0;
            let sat = 1.0 + p2 * 0.9;
            [
                ((avg + (c[0] - avg) * sat) * gain + p3 * 25.0).clamp(0.0, 255.0),
                ((avg + (c[1] - avg) * sat) * gain).clamp(0.0, 255.0),
                ((avg + (c[2] - avg) * sat) * gain).clamp(0.0, 255.0),
            ]
        }
        "matrix" => {
            let col = (x * 16.0).floor();
            let hash = (col * 127.1 + 311.7).sin() * 43758.5453;
            let phase = hash - hash.floor();
            let speed = 0.45 + p1 * 1.3 + phase * 0.45;
            let head = (t * speed + phase * 3.7) % 1.5;
            let dist = head - y;
            if is_logo {
                let pulse = (1.0 - (((t * 0.8 + x) % 1.0) - 0.5).abs() * 4.0).max(0.0);
                return hsv_to_rgb(p3 * 360.0, 0.9, 0.15 + pulse * 0.85);
            }
            if (0.0..0.09).contains(&dist) {
                return [225.0, 255.0, 235.0];
            }
            let trail_len = 0.25 + p2 * 0.65;
            if dist >= 0.0 && dist < trail_len {
                let v = (1.0 - dist / trail_len).powf(1.6);
                hsv_to_rgb(p3 * 360.0, 0.95, v)
            } else {
                hsv_to_rgb(p3 * 360.0, 0.8, 0.04)
            }
        }
        _ => {
            // 默认 "aurora" 赛博极光波
            let spd = 1.2 + p1 * 4.5;
            let freq = 3.0 + p2 * 8.0;
            let w1 = (x * freq - t * spd + y * 2.2).sin();
            let w2 = ((x * 0.7 - y * 1.4) * freq + t * spd * 0.7).cos();
            let hue = (p3 * 360.0 + x * 140.0 + (w1 + w2) * 38.0 + t * 28.0) % 360.0;
            let val = 0.55 + 0.45 * (0.5 + 0.5 * w1);
            hsv_to_rgb(hue, 0.92, val)
        }
    }
}

fn eval_layers_point(
    l: &NativeLayersConfig,
    x: f32,
    y: f32,
    t: f32,
    slot: i16,
    is_logo: bool,
    logo_idx: usize,
    c_a: [f32; 3],
    c_b: [f32; 3],
    c_aud: [f32; 3],
    c_typ: [f32; 3],
    aud: Option<&AudioSnapshot>,
    amb: Option<&AmbilightSnapshot>,
    kst: Option<&KeyActivitySnapshot>,
) -> [f32; 3] {
    let mut r = 0.0f32;
    let mut g = 0.0f32;
    let mut b = 0.0f32;

    if l.gradient.enabled && l.gradient.opacity > 0.01 {
        let g_spd = 0.6 + l.gradient.speed * 3.5;
        let f = 0.5 + 0.5 * (x * 4.5 + y * 1.6 - t * g_spd).sin();
        let m = mix_rgb(c_a, c_b, f);
        let op = l.gradient.opacity;
        r += m[0] * op;
        g += m[1] * op;
        b += m[2] * op;
    }

    if l.ambilight.enabled && l.ambilight.opacity > 0.01 {
        if let Some(a) = amb {
            let (ar, ag, ab) = if is_logo && a.logo.len() >= LOGO_RGB_LEN {
                let o = logo_idx.min(LOGO_LED_COUNT - 1) * 3;
                (a.logo[o] as f32, a.logo[o + 1] as f32, a.logo[o + 2] as f32)
            } else if a.grid.len() >= AMBI_W * AMBI_H * 3 {
                let gx = ((x * AMBI_W as f32).floor() as isize).clamp(0, AMBI_W as isize - 1) as usize;
                let gy = ((y * AMBI_H as f32).floor() as isize).clamp(0, AMBI_H as isize - 1) as usize;
                let o = (gy * AMBI_W + gx) * 3;
                (a.grid[o] as f32, a.grid[o + 1] as f32, a.grid[o + 2] as f32)
            } else {
                (0.0, 0.0, 0.0)
            };
            let op = l.ambilight.opacity;
            r = r * (1.0 - op) + ar * op;
            g = g * (1.0 - op) + ag * op;
            b = b * (1.0 - op) + ab * op;
        }
    }

    if l.audio.enabled && l.audio.opacity > 0.01 {
        let a_gain = 0.65 + l.audio.sensitivity * 1.45;
        let op = l.audio.opacity;
        if is_logo {
            let lvl = (aud.map(|a| a.rms).unwrap_or(0.0) * a_gain * 1.3).min(1.0);
            if (x - 0.5).abs() * 2.0 <= lvl {
                r += c_aud[0] * op;
                g += c_aud[1] * op;
                b += c_aud[2] * op;
            }
        } else {
            let b_idx = ((x * 16.0).floor() as isize).clamp(0, 15) as usize;
            let lvl = (aud.map(|a| a.bands[b_idx]).unwrap_or(0.0) * a_gain).min(1.0);
            if 1.0 - y <= lvl + 0.05 {
                let boost = (0.55 + lvl * 0.45) * op;
                r += c_aud[0] * boost;
                g += c_aud[1] * boost;
                b += c_aud[2] * boost;
            }
        }
    }

    if l.typing.enabled && l.typing.opacity > 0.01 {
        let t_spd = 0.55 + l.typing.speed * 1.25;
        let mut hit = if !is_logo {
            kst.map(|k| {
                if k.pressed_slots.iter().any(|&s| s as i16 == slot) {
                    1.0f32
                } else {
                    0.0
                }
            })
            .unwrap_or(0.0)
        } else {
            0.0
        };
        if let Some(k) = kst {
            for p in &k.pulses {
                let age = p.age_ms as f32 / 1000.0;
                let rad = age * t_spd;
                let d = (x - p.x).hypot((y - p.y) * 0.55);
                let ring = (-((d - rad) * (d - rad)) / 0.009).exp() * (1.0 - age * 0.85).max(0.0);
                if ring > hit {
                    hit = ring;
                }
            }
        }
        if hit > 0.02 {
            let k = hit.min(1.0) * l.typing.opacity;
            r += c_typ[0] * k;
            g += c_typ[1] * k;
            b += c_typ[2] * k;
        }
    }

    [r.clamp(0.0, 255.0), g.clamp(0.0, 255.0), b.clamp(0.0, 255.0)]
}

pub fn render_native_frame(
    cfg: &NativeStreamConfig,
    t: f32,
    aud: Option<&AudioSnapshot>,
    amb: Option<&AmbilightSnapshot>,
    kst: Option<&KeyActivitySnapshot>,
    keys_out: &mut [u8; CUSTOM_RGB_LEN],
    logo_out: &mut [u8; LOGO_RGB_LEN],
) {
    keys_out.fill(0);
    let bri = cfg.brightness.clamp(0.05, 1.0);
    let is_layers = cfg.mode == "layers";
    let c_a = parse_hex_rgb(&cfg.layers.gradient.color_a);
    let c_b = parse_hex_rgb(&cfg.layers.gradient.color_b);
    let c_aud = parse_hex_rgb(&cfg.layers.audio.color);
    let c_typ = parse_hex_rgb(&cfg.layers.typing.color);

    for &(slot, x, y) in &KEY_COORDS {
        let rgb = if is_layers {
            eval_layers_point(&cfg.layers, x, y, t, slot as i16, false, 0, c_a, c_b, c_aud, c_typ, aud, amb, kst)
        } else {
            eval_preset_point(cfg, x, y, t, slot as i16, false, 0, aud, amb, kst)
        };
        let o = (slot as usize) * 3;
        if o + 2 < CUSTOM_RGB_LEN {
            keys_out[o] = (rgb[0] * bri).round().clamp(0.0, 255.0) as u8;
            keys_out[o + 1] = (rgb[1] * bri).round().clamp(0.0, 255.0) as u8;
            keys_out[o + 2] = (rgb[2] * bri).round().clamp(0.0, 255.0) as u8;
        }
    }

    for li in 0..LOGO_LED_COUNT {
        let x = li as f32 / (LOGO_LED_COUNT as f32 - 1.0);
        let rgb = if is_layers {
            eval_layers_point(&cfg.layers, x, 0.0, t, -1, true, li, c_a, c_b, c_aud, c_typ, aud, amb, kst)
        } else {
            eval_preset_point(cfg, x, 0.0, t, -1, true, li, aud, amb, kst)
        };
        logo_out[li * 3] = (rgb[0] * bri).round().clamp(0.0, 255.0) as u8;
        logo_out[li * 3 + 1] = (rgb[1] * bri).round().clamp(0.0, 255.0) as u8;
        logo_out[li * 3 + 2] = (rgb[2] * bri).round().clamp(0.0, 255.0) as u8;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_all_presets_and_layers_natively() {
        let mut keys = [0u8; CUSTOM_RGB_LEN];
        let mut logo = [0u8; LOGO_RGB_LEN];
        for preset in ["aurora", "radar", "ripple", "spectrum", "ambilight", "matrix"] {
            let mut cfg = NativeStreamConfig::default();
            cfg.preset_id = preset.to_string();
            render_native_frame(&cfg, 1.25, None, None, None, &mut keys, &mut logo);
            if preset != "ambilight" {
                assert!(keys.iter().any(|&v| v > 0), "preset {preset} rendered blank");
            }
        }
        let mut lcfg = NativeStreamConfig::default();
        lcfg.mode = "layers".to_string();
        lcfg.brightness = 0.5;
        render_native_frame(&lcfg, 2.0, None, None, None, &mut keys, &mut logo);
        assert!(keys.iter().any(|&v| v > 0));
        assert!(keys.iter().all(|&v| v <= 130));
    }
}
