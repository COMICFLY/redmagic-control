//! 本地宏库持久化（%LOCALAPPDATA%\com.redmagic.control.rs\macros.json）与 500Hz 低延迟按键/鼠标录制器：
//! - 原子临时文件落盘（macros.json.tmp -> macros.json），断电不损坏；
//! - 录制器基于 Win32 GetAsyncKeyState 500Hz 轮询（零全局钩子注入，无杀软误报），
//!   精确区分左/右修饰键（Type 0x09）、标准键盘键（Type 0x0A）及 5 键鼠标（Type 0x01）。

use std::fs;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use serde::Serialize;
use tauri::{AppHandle, Emitter};

use crate::protocol::macro_proto::{
    event_label, normalize_macro_def, MacroDef, MacroEvent, MacroLibrary,
};
use crate::settings::app_data_dir;

pub fn macros_file_path() -> PathBuf {
    app_data_dir().join("macros.json")
}

fn now_epoch_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn default_mock_macros() -> Vec<MacroDef> {
    let ts = now_epoch_ms();
    vec![
        MacroDef {
            id: "macro-copy-paste".to_string(),
            name: "快速复制粘贴".to_string(),
            delay_mode: "fixed".to_string(),
            fixed_delay_ms: 20,
            events: vec![
                MacroEvent { kind: "modifier".into(), code: 0x01, down: true, delay_ms: 15, label: "左 Ctrl".into() },
                MacroEvent { kind: "key".into(), code: 0x06, down: true, delay_ms: 15, label: "C".into() },
                MacroEvent { kind: "key".into(), code: 0x06, down: false, delay_ms: 15, label: "C".into() },
                MacroEvent { kind: "modifier".into(), code: 0x01, down: false, delay_ms: 35, label: "左 Ctrl".into() },
                MacroEvent { kind: "modifier".into(), code: 0x01, down: true, delay_ms: 15, label: "左 Ctrl".into() },
                MacroEvent { kind: "key".into(), code: 0x19, down: true, delay_ms: 15, label: "V".into() },
                MacroEvent { kind: "key".into(), code: 0x19, down: false, delay_ms: 15, label: "V".into() },
                MacroEvent { kind: "modifier".into(), code: 0x01, down: false, delay_ms: 0, label: "左 Ctrl".into() },
            ],
            updated_at: ts,
        },
        MacroDef {
            id: "macro-rapid-click".to_string(),
            name: "鼠标左键连点".to_string(),
            delay_mode: "fixed".to_string(),
            fixed_delay_ms: 25,
            events: vec![
                MacroEvent { kind: "mouse".into(), code: 0x01, down: true, delay_ms: 25, label: "鼠标左键".into() },
                MacroEvent { kind: "mouse".into(), code: 0x01, down: false, delay_ms: 25, label: "鼠标左键".into() },
            ],
            updated_at: ts,
        },
    ]
}

#[derive(Clone)]
pub struct MacroStore {
    inner: Arc<Mutex<MacroLibrary>>,
    path: PathBuf,
    mock: bool,
}

impl MacroStore {
    pub fn load(mock: bool) -> Self {
        Self::load_at(macros_file_path(), mock)
    }

    pub fn load_at(path: PathBuf, mock: bool) -> Self {
        let initial = if mock {
            MacroLibrary {
                version: 1,
                macros: default_mock_macros(),
            }
        } else {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<MacroLibrary>(&s).ok())
                .map(|mut lib| {
                    lib.macros = lib
                        .macros
                        .iter()
                        .enumerate()
                        .map(|(i, m)| normalize_macro_def(m, i))
                        .collect();
                    lib
                })
                .unwrap_or_default()
        };
        Self {
            inner: Arc::new(Mutex::new(initial)),
            path,
            mock,
        }
    }

    pub fn path_string(&self) -> String {
        self.path.to_string_lossy().to_string()
    }

    pub fn list(&self) -> Vec<MacroDef> {
        self.inner.lock().unwrap().macros.clone()
    }

    pub fn save_all(&self, macros: Vec<MacroDef>) -> Result<Vec<MacroDef>, String> {
        if macros.len() > 255 {
            return Err("最多支持保存 255 条宏".to_string());
        }
        let ts = now_epoch_ms();
        let normalized: Vec<MacroDef> = macros
            .iter()
            .enumerate()
            .map(|(i, m)| {
                let mut n = normalize_macro_def(m, i);
                if n.updated_at == 0 {
                    n.updated_at = ts;
                }
                n
            })
            .collect();

        let mut guard = self.inner.lock().unwrap();
        guard.macros = normalized.clone();
        self.persist_locked(&guard)?;
        Ok(normalized)
    }

    fn persist_locked(&self, lib: &MacroLibrary) -> Result<(), String> {
        if self.mock {
            return Ok(());
        }
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("创建配置目录失败: {e}"))?;
        }
        let json =
            serde_json::to_string_pretty(lib).map_err(|e| format!("序列化宏库失败: {e}"))?;
        let tmp = self.path.with_extension("json.tmp");
        if fs::write(&tmp, &json).is_ok() {
            if fs::rename(&tmp, &self.path).is_err() {
                fs::write(&self.path, &json).map_err(|e| format!("写入宏库文件失败: {e}"))?;
                let _ = fs::remove_file(&tmp);
            }
        } else {
            fs::write(&self.path, &json).map_err(|e| format!("写入宏库文件失败: {e}"))?;
        }
        Ok(())
    }
}

/* ---------------- 500Hz Win32 低延迟按键 / 鼠标录制器 ---------------- */

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RecordedEventPayload {
    event: MacroEvent,
    prev_delay_ms: Option<u16>,
    count: usize,
}

#[derive(Clone)]
pub struct MacroRecorder {
    running: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<MacroEvent>>>,
    last_event_at: Arc<Mutex<Option<Instant>>>,
    record_mouse: Arc<AtomicBool>,
}

impl MacroRecorder {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            events: Arc::new(Mutex::new(Vec::new())),
            last_event_at: Arc::new(Mutex::new(None)),
            record_mouse: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn is_recording(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn start(
        &self,
        app: AppHandle,
        record_mouse: bool,
        delay_mode: String,
        fixed_delay_ms: u16,
    ) -> Result<(), String> {
        if self.running.swap(true, Ordering::SeqCst) {
            return Ok(());
        }
        self.record_mouse.store(record_mouse, Ordering::SeqCst);
        self.events.lock().unwrap().clear();
        *self.last_event_at.lock().unwrap() = None;

        let running = self.running.clone();
        let events = self.events.clone();
        let last_event_at = self.last_event_at.clone();
        let delay_mode = match delay_mode.as_str() {
            "fixed" | "none" => delay_mode,
            _ => "recorded".to_string(),
        };
        let fixed_ms = fixed_delay_ms.clamp(1, 10_000);

        thread::spawn(move || {
            #[cfg(windows)]
            run_recorder_loop(
                app,
                running,
                events,
                last_event_at,
                record_mouse,
                delay_mode,
                fixed_ms,
            );
            #[cfg(not(windows))]
            {
                let _ = (app, running, events, last_event_at, record_mouse, delay_mode, fixed_ms);
            }
        });
        Ok(())
    }

    pub fn stop(&self) -> Vec<MacroEvent> {
        self.running.store(false, Ordering::SeqCst);
        let rec_mouse = self.record_mouse.load(Ordering::SeqCst);
        let mut guard = self.events.lock().unwrap();
        // 点击“停止录制”按钮时若开启了鼠标录制，自动剔除最后 220ms 内悬空的鼠标左键按下事件
        if rec_mouse {
            if let Some(last_at) = *self.last_event_at.lock().unwrap() {
                if last_at.elapsed() <= Duration::from_millis(220) {
                    if let Some(last) = guard.last() {
                        if last.kind == "mouse" && last.code == 0x01 && last.down {
                            guard.pop();
                            if let Some(prev) = guard.last_mut() {
                                prev.delay_ms = 0;
                            }
                        }
                    }
                }
            }
        }
        let out = guard.clone();
        guard.clear();
        out
    }
}

/// (Win32 VK, kind, hardware code) — 对齐官方 Goper.exe `0x82d618` / `0x82d970`
#[cfg(windows)]
const RECORDER_KEY_TABLE: &[(i32, &str, u8)] = &[
    // 8 颗独立修饰键（Type = 0x09, Button = modifier bitmask）
    (0xA2, "modifier", 0x01), // LCtrl
    (0xA0, "modifier", 0x02), // LShift
    (0xA4, "modifier", 0x04), // LAlt
    (0x5B, "modifier", 0x08), // LWin
    (0xA3, "modifier", 0x10), // RCtrl
    (0xA1, "modifier", 0x20), // RShift
    (0xA5, "modifier", 0x40), // RAlt
    (0x5C, "modifier", 0x80), // RWin
    // 字母 A..Z (0x04..0x1D)
    (0x41, "key", 0x04), (0x42, "key", 0x05), (0x43, "key", 0x06), (0x44, "key", 0x07),
    (0x45, "key", 0x08), (0x46, "key", 0x09), (0x47, "key", 0x0A), (0x48, "key", 0x0B),
    (0x49, "key", 0x0C), (0x4A, "key", 0x0D), (0x4B, "key", 0x0E), (0x4C, "key", 0x0F),
    (0x4D, "key", 0x10), (0x4E, "key", 0x11), (0x4F, "key", 0x12), (0x50, "key", 0x13),
    (0x51, "key", 0x14), (0x52, "key", 0x15), (0x53, "key", 0x16), (0x54, "key", 0x17),
    (0x55, "key", 0x18), (0x56, "key", 0x19), (0x57, "key", 0x1A), (0x58, "key", 0x1B),
    (0x59, "key", 0x1C), (0x5A, "key", 0x1D),
    // 主键盘数字 1..0 (0x1E..0x27)
    (0x31, "key", 0x1E), (0x32, "key", 0x1F), (0x33, "key", 0x20), (0x34, "key", 0x21),
    (0x35, "key", 0x22), (0x36, "key", 0x23), (0x37, "key", 0x24), (0x38, "key", 0x25),
    (0x39, "key", 0x26), (0x30, "key", 0x27),
    // 常用控制与符号键
    (0x0D, "key", 0x28), // Enter
    (0x1B, "key", 0x29), // Esc
    (0x08, "key", 0x2A), // Backspace
    (0x09, "key", 0x2B), // Tab
    (0x20, "key", 0x2C), // Space
    (0xBD, "key", 0x2D), // -
    (0xBB, "key", 0x2E), // =
    (0xDB, "key", 0x2F), // [
    (0xDD, "key", 0x30), // ]
    (0xDC, "key", 0x31), // \
    (0xBA, "key", 0x33), // ;
    (0xDE, "key", 0x34), // '
    (0xC0, "key", 0x35), // `
    (0xBC, "key", 0x36), // ,
    (0xBE, "key", 0x37), // .
    (0xBF, "key", 0x38), // /
    (0x14, "key", 0x39), // Caps Lock
    // F1..F12
    (0x70, "key", 0x3A), (0x71, "key", 0x3B), (0x72, "key", 0x3C), (0x73, "key", 0x3D),
    (0x74, "key", 0x3E), (0x75, "key", 0x3F), (0x76, "key", 0x40), (0x77, "key", 0x41),
    (0x78, "key", 0x42), (0x79, "key", 0x43), (0x7A, "key", 0x44), (0x7B, "key", 0x45),
    // 功能导航区
    (0x2C, "key", 0x46), // Print Screen
    (0x91, "key", 0x47), // Scroll Lock
    (0x13, "key", 0x48), // Pause
    (0x2D, "key", 0x49), // Insert
    (0x24, "key", 0x4A), // Home
    (0x21, "key", 0x4B), // Page Up
    (0x2E, "key", 0x4C), // Delete
    (0x23, "key", 0x4D), // End
    (0x22, "key", 0x4E), // Page Down
    (0x27, "key", 0x4F), // Right
    (0x25, "key", 0x50), // Left
    (0x28, "key", 0x51), // Down
    (0x26, "key", 0x52), // Up
    // 小键盘
    (0x90, "key", 0x53), // Num Lock
    (0x6F, "key", 0x54), // Numpad /
    (0x6A, "key", 0x55), // Numpad *
    (0x6D, "key", 0x56), // Numpad -
    (0x6B, "key", 0x57), // Numpad +
    (0x61, "key", 0x59), (0x62, "key", 0x5A), (0x63, "key", 0x5B),
    (0x64, "key", 0x5C), (0x65, "key", 0x5D), (0x66, "key", 0x5E),
    (0x67, "key", 0x5F), (0x68, "key", 0x60), (0x69, "key", 0x61),
    (0x60, "key", 0x62), (0x6E, "key", 0x63),
    (0x5D, "key", 0x65), // Menu / Apps
];

#[cfg(windows)]
const RECORDER_MOUSE_TABLE: &[(i32, &str, u8)] = &[
    (0x01, "mouse", 0x01), // 左键
    (0x02, "mouse", 0x02), // 右键
    (0x04, "mouse", 0x04), // 中键
    (0x05, "mouse", 0x08), // 后退 (XBUTTON1)
    (0x06, "mouse", 0x10), // 前进 (XBUTTON2)
];

#[cfg(windows)]
fn run_recorder_loop(
    app: AppHandle,
    running: Arc<AtomicBool>,
    events: Arc<Mutex<Vec<MacroEvent>>>,
    last_event_at: Arc<Mutex<Option<Instant>>>,
    record_mouse: bool,
    delay_mode: String,
    fixed_delay_ms: u16,
) {
    use windows_sys::Win32::UI::Input::KeyboardAndMouse::GetAsyncKeyState;

    let mut table: Vec<(i32, &str, u8)> = RECORDER_KEY_TABLE.to_vec();
    if record_mouse {
        table.extend_from_slice(RECORDER_MOUSE_TABLE);
    }

    // 等待用户松开点击“开始录制”时的鼠标左键（最多 350ms），避免录进多余的鼠标弹起
    let start_wait = Instant::now();
    while running.load(Ordering::SeqCst) && start_wait.elapsed() < Duration::from_millis(350) {
        let lbtn = unsafe { (GetAsyncKeyState(0x01) as u16 & 0x8000) != 0 };
        if !lbtn && start_wait.elapsed() >= Duration::from_millis(80) {
            break;
        }
        thread::sleep(Duration::from_millis(10));
    }

    // 采样基线状态，忽略录制开始前就已经按住的键
    let mut prev: Vec<bool> = table
        .iter()
        .map(|&(vk, _, _)| unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 })
        .collect();

    let mut last_ts: Option<Instant> = None;

    while running.load(Ordering::SeqCst) {
        let now = Instant::now();
        for (i, &(vk, kind, code)) in table.iter().enumerate() {
            let down = unsafe { (GetAsyncKeyState(vk) as u16 & 0x8000) != 0 };
            if down == prev[i] {
                continue;
            }
            prev[i] = down;

            let prev_delay = if let Some(prev_at) = last_ts {
                let d = match delay_mode.as_str() {
                    "fixed" => fixed_delay_ms,
                    "none" => 1,
                    _ => now
                        .duration_since(prev_at)
                        .as_millis()
                        .clamp(1, u16::MAX as u128) as u16,
                };
                Some(d)
            } else {
                None
            };
            last_ts = Some(now);
            *last_event_at.lock().unwrap() = Some(now);

            let ev = MacroEvent {
                kind: kind.to_string(),
                code,
                down,
                delay_ms: 0,
                label: event_label(kind, code, down),
            };

            let count = {
                let mut list = events.lock().unwrap();
                if let Some(d) = prev_delay {
                    if let Some(last) = list.last_mut() {
                        last.delay_ms = d;
                    }
                }
                if list.len() < 750 {
                    list.push(ev.clone());
                }
                list.len()
            };

            let _ = app.emit(
                "macro:record-event",
                &RecordedEventPayload {
                    event: ev,
                    prev_delay_ms: prev_delay,
                    count,
                },
            );
        }

        thread::sleep(Duration::from_millis(2));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn macro_store_persists_and_reloads_atomically() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "redmagic-macro-test-{}",
            std::process::id()
        ));
        let path = tmp_dir.join("macros.json");
        let _ = fs::remove_dir_all(&tmp_dir);

        let store = MacroStore::load_at(path.clone(), false);
        assert!(store.list().is_empty());

        let m = MacroDef {
            id: "test-1".into(),
            name: "测试宏".into(),
            delay_mode: "fixed".into(),
            fixed_delay_ms: 30,
            events: vec![
                MacroEvent { kind: "key".into(), code: 0x04, down: true, delay_ms: 30, label: String::new() },
                MacroEvent { kind: "key".into(), code: 0x04, down: false, delay_ms: 0, label: String::new() },
            ],
            updated_at: 0,
        };
        let saved = store.save_all(vec![m]).unwrap();
        assert_eq!(saved.len(), 1);
        assert_eq!(saved[0].events[0].label, "A");
        assert!(path.exists());

        let reloaded = MacroStore::load_at(path.clone(), false);
        assert_eq!(reloaded.list().len(), 1);
        assert_eq!(reloaded.list()[0].name, "测试宏");
        assert_eq!(reloaded.list()[0].events[0].code, 0x04);

        let _ = fs::remove_dir_all(&tmp_dir);
    }
}
