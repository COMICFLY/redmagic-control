//! 纯 Rust Windows WASAPI 系统回环音频采集 + 512 点 Radix-2 FFT 16 段实时频谱引擎。
//! 无需任何外部音频库，直接通过官方 windows 0.62 COM 绑定抓取系统扬声器输出。

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use serde::Serialize;

pub const BAND_COUNT: usize = 16;
const FFT_N: usize = 512;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AudioSnapshot {
    pub active: bool,
    pub bands: [f32; BAND_COUNT],
    pub rms: f32,
    pub bass: f32,
    pub mid: f32,
    pub treble: f32,
    pub error: Option<String>,
}

impl Default for AudioSnapshot {
    fn default() -> Self {
        Self {
            active: false,
            bands: [0.0; BAND_COUNT],
            rms: 0.0,
            bass: 0.0,
            mid: 0.0,
            treble: 0.0,
            error: None,
        }
    }
}

#[derive(Clone)]
pub struct AudioEngine {
    running: Arc<AtomicBool>,
    state: Arc<Mutex<AudioSnapshot>>,
}

impl AudioEngine {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            state: Arc::new(Mutex::new(AudioSnapshot::default())),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn snapshot(&self) -> AudioSnapshot {
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
            st.error = None;
        }
        let running = self.running.clone();
        let state = self.state.clone();
        thread::spawn(move || {
            #[cfg(windows)]
            run_wasapi_loop(running, state);
            #[cfg(not(windows))]
            {
                let _ = (running, state);
            }
        });
    }

    pub fn stop(&self) {
        self.running.store(false, Ordering::SeqCst);
        let mut st = self.state.lock().unwrap();
        *st = AudioSnapshot::default();
    }
}

/// 纯 Rust 512 点原地 Radix-2 Cooley-Tukey FFT，返回前半段幅值谱
fn fft_magnitudes(samples: &[f32; FFT_N]) -> [f32; FFT_N / 2] {
    let mut re = [0.0f32; FFT_N];
    let mut im = [0.0f32; FFT_N];

    // Hann 窗减少频谱泄漏
    let inv = std::f32::consts::TAU / (FFT_N as f32 - 1.0);
    for i in 0..FFT_N {
        let w = 0.5 * (1.0 - (inv * i as f32).cos());
        re[i] = samples[i] * w;
    }

    // 位反转重排
    let mut j = 0usize;
    for i in 1..FFT_N {
        let mut bit = FFT_N >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j ^= bit;
        if i < j {
            re.swap(i, j);
        }
    }

    // 蝶形运算
    let mut len = 2usize;
    while len <= FFT_N {
        let ang = -std::f32::consts::TAU / (len as f32);
        let wlen_re = ang.cos();
        let wlen_im = ang.sin();
        let half = len >> 1;
        let mut i = 0;
        while i < FFT_N {
            let mut w_re = 1.0f32;
            let mut w_im = 0.0f32;
            for k in 0..half {
                let u_re = re[i + k];
                let u_im = im[i + k];
                let v_re = re[i + k + half] * w_re - im[i + k + half] * w_im;
                let v_im = re[i + k + half] * w_im + im[i + k + half] * w_re;
                re[i + k] = u_re + v_re;
                im[i + k] = u_im + v_im;
                re[i + k + half] = u_re - v_re;
                im[i + k + half] = u_im - v_im;
                let nxt_re = w_re * wlen_re - w_im * wlen_im;
                let nxt_im = w_re * wlen_im + w_im * wlen_re;
                w_re = nxt_re;
                w_im = nxt_im;
            }
            i += len;
        }
        len <<= 1;
    }

    let mut mag = [0.0f32; FFT_N / 2];
    let scale = 2.0 / (FFT_N as f32);
    for k in 0..(FFT_N / 2) {
        mag[k] = (re[k] * re[k] + im[k] * im[k]).sqrt() * scale;
    }
    mag
}

fn update_spectrum(
    ring: &[f32; FFT_N],
    smoothed: &mut [f32; BAND_COUNT],
    agc_peak: &mut f32,
    sample_rate: u32,
) -> (f32, f32, f32, f32) {
    let mut sq_sum = 0.0f32;
    for &s in ring {
        sq_sum += s * s;
    }
    let rms = (sq_sum / FFT_N as f32).sqrt().min(1.0);
    let mags = fft_magnitudes(ring);

    // 16 段对数频带：45Hz ~ 14kHz
    let sr = sample_rate.max(8000) as f32;
    let f_min = 45.0f32;
    let f_max = (sr * 0.44).min(14000.0);
    let ratio = (f_max / f_min).powf(1.0 / BAND_COUNT as f32);

    let mut raw = [0.0f32; BAND_COUNT];
    let mut frame_max = 0.015f32;
    for b in 0..BAND_COUNT {
        let f0 = f_min * ratio.powi(b as i32);
        let f1 = f0 * ratio;
        let k0 = ((f0 * FFT_N as f32) / sr).floor().max(1.0) as usize;
        let k1 = ((f1 * FFT_N as f32) / sr).ceil().max((k0 + 1) as f32) as usize;
        let k1 = k1.min(FFT_N / 2);
        let mut peak = 0.0f32;
        for &m in &mags[k0..k1] {
            if m > peak {
                peak = m;
            }
        }
        // 高频轻微补偿，视觉平衡低频与人声/高频
        let tilt = 1.0 + (b as f32) * 0.14;
        let v = (peak * tilt).sqrt();
        raw[b] = v;
        if v > frame_max {
            frame_max = v;
        }
    }

    // 自适应增益（慢衰减，快跟随），保证轻音乐与高动态游戏都有饱满跳动
    if frame_max > *agc_peak {
        *agc_peak = *agc_peak * 0.65 + frame_max * 0.35;
    } else {
        *agc_peak = (*agc_peak * 0.988).max(0.08);
    }

    let denom = (*agc_peak).max(0.08);
    for b in 0..BAND_COUNT {
        let norm = (raw[b] / denom).clamp(0.0, 1.0);
        // 快上升、柔和回落
        if norm > smoothed[b] {
            smoothed[b] = smoothed[b] * 0.35 + norm * 0.65;
        } else {
            smoothed[b] = smoothed[b] * 0.78 + norm * 0.22;
        }
    }

    let bass = (smoothed[0] + smoothed[1] + smoothed[2] + smoothed[3]) * 0.25;
    let mid = (smoothed[4] + smoothed[5] + smoothed[6] + smoothed[7] + smoothed[8] + smoothed[9]) / 6.0;
    let treble = (smoothed[10] + smoothed[11] + smoothed[12] + smoothed[13] + smoothed[14] + smoothed[15]) / 6.0;
    ((rms * 4.5).clamp(0.0, 1.0), bass, mid, treble)
}

#[cfg(windows)]
fn run_wasapi_loop(running: Arc<AtomicBool>, state: Arc<Mutex<AudioSnapshot>>) {
    use windows::Win32::Media::Audio::{
        eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
        MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
        AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CoUninitialize, CLSCTX_ALL,
        COINIT_MULTITHREADED,
    };

    unsafe {
        let com_ok = CoInitializeEx(None, COINIT_MULTITHREADED).is_ok();
        while running.load(Ordering::SeqCst) {
            if let Err(err) = capture_session(&running, &state) {
                if let Ok(mut st) = state.lock() {
                    st.error = Some(err);
                }
                for _ in 0..10 {
                    if !running.load(Ordering::SeqCst) {
                        break;
                    }
                    thread::sleep(Duration::from_millis(120));
                }
            }
        }
        if com_ok {
            CoUninitialize();
        }
    }

    unsafe fn capture_session(
        running: &Arc<AtomicBool>,
        state: &Arc<Mutex<AudioSnapshot>>,
    ) -> Result<(), String> {
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| format!("初始化音频枚举器失败: {e}"))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eRender, eConsole)
            .map_err(|e| format!("未找到默认播放设备: {e}"))?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| format!("激活 IAudioClient 失败: {e}"))?;

        let pwfx = client
            .GetMixFormat()
            .map_err(|e| format!("读取混音格式失败: {e}"))?;
        let wfx: WAVEFORMATEX = *pwfx;
        let channels = (wfx.nChannels as usize).max(1);
        let sample_rate = wfx.nSamplesPerSec;
        let bits = wfx.wBitsPerSample;
        let format_tag = wfx.wFormatTag;

        let init_res = client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            AUDCLNT_STREAMFLAGS_LOOPBACK,
            300_000, // 30ms buffer
            0,
            pwfx,
            None,
        );
        CoTaskMemFree(Some(pwfx as *const _ as *const std::ffi::c_void));
        init_res.map_err(|e| format!("WASAPI 回环初始化失败: {e}"))?;

        let capture: IAudioCaptureClient = client
            .GetService()
            .map_err(|e| format!("获取捕获客户端失败: {e}"))?;
        client
            .Start()
            .map_err(|e| format!("启动音频回环失败: {e}"))?;

        if let Ok(mut st) = state.lock() {
            st.error = None;
            st.active = true;
        }

        let mut ring = [0.0f32; FFT_N];
        let mut ring_pos = 0usize;
        let mut smoothed = [0.0f32; BAND_COUNT];
        let mut agc_peak = 0.15f32;
        let mut silent_ticks = 0u32;

        while running.load(Ordering::SeqCst) {
            let mut got_samples = false;
            loop {
                let pkt_len = match capture.GetNextPacketSize() {
                    Ok(n) => n,
                    Err(_) => {
                        let _ = client.Stop();
                        return Err("音频输出设备已切换，正在重新挂载…".to_string());
                    }
                };
                if pkt_len == 0 {
                    break;
                }
                let mut data_ptr: *mut u8 = std::ptr::null_mut();
                let mut frames: u32 = 0;
                let mut flags: u32 = 0;
                if capture
                    .GetBuffer(&mut data_ptr, &mut frames, &mut flags, None, None)
                    .is_err()
                {
                    let _ = client.Stop();
                    return Err("读取音频缓冲区失败".to_string());
                }
                let is_silent = (flags & (AUDCLNT_BUFFERFLAGS_SILENT.0 as u32)) != 0 || data_ptr.is_null();
                let n_frames = frames as usize;
                if is_silent {
                    for _ in 0..n_frames {
                        ring[ring_pos] = 0.0;
                        ring_pos = (ring_pos + 1) % FFT_N;
                    }
                } else if bits == 32 && (format_tag == 3 || format_tag == 0xFFFE) {
                    let f32_ptr = data_ptr as *const f32;
                    for f in 0..n_frames {
                        let mut mono = 0.0f32;
                        for ch in 0..channels {
                            mono += *f32_ptr.add(f * channels + ch);
                        }
                        ring[ring_pos] = mono / channels as f32;
                        ring_pos = (ring_pos + 1) % FFT_N;
                    }
                    got_samples = true;
                } else if bits == 16 {
                    let i16_ptr = data_ptr as *const i16;
                    for f in 0..n_frames {
                        let mut mono = 0.0f32;
                        for ch in 0..channels {
                            mono += (*i16_ptr.add(f * channels + ch) as f32) / 32768.0;
                        }
                        ring[ring_pos] = mono / channels as f32;
                        ring_pos = (ring_pos + 1) % FFT_N;
                    }
                    got_samples = true;
                }
                let _ = capture.ReleaseBuffer(frames);
            }

            if !got_samples {
                silent_ticks = silent_ticks.saturating_add(1);
                if silent_ticks >= 3 {
                    // 无音频流时平滑归零，保持呼吸回落感
                    for b in &mut smoothed {
                        *b *= 0.82;
                        if *b < 0.005 {
                            *b = 0.0;
                        }
                    }
                    if let Ok(mut st) = state.lock() {
                        st.bands = smoothed;
                        st.rms *= 0.8;
                        st.bass = (smoothed[0] + smoothed[1] + smoothed[2] + smoothed[3]) * 0.25;
                        st.mid = (smoothed[4] + smoothed[5] + smoothed[6] + smoothed[7]) * 0.25;
                        st.treble = (smoothed[12] + smoothed[13] + smoothed[14] + smoothed[15]) * 0.25;
                    }
                }
            } else {
                silent_ticks = 0;
                // 按时间顺序重排环形缓冲
                let mut ordered = [0.0f32; FFT_N];
                let tail = FFT_N - ring_pos;
                ordered[..tail].copy_from_slice(&ring[ring_pos..]);
                ordered[tail..].copy_from_slice(&ring[..ring_pos]);

                let (rms, bass, mid, treble) =
                    update_spectrum(&ordered, &mut smoothed, &mut agc_peak, sample_rate);
                if let Ok(mut st) = state.lock() {
                    st.bands = smoothed;
                    st.rms = rms;
                    st.bass = bass;
                    st.mid = mid;
                    st.treble = treble;
                }
            }
            thread::sleep(Duration::from_millis(16));
        }
        let _ = client.Stop();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_detects_sine_wave_band() {
        let sr = 48000u32;
        let mut buf = [0.0f32; FFT_N];
        for i in 0..FFT_N {
            buf[i] = (std::f32::consts::TAU * 440.0 * (i as f32) / (sr as f32)).sin();
        }
        let mut sm = [0.0f32; BAND_COUNT];
        let mut agc = 0.1f32;
        let (rms, _bass, mid, _treble) = update_spectrum(&buf, &mut sm, &mut agc, sr);
        assert!(rms > 0.5);
        assert!(mid > 0.15);
    }
}
