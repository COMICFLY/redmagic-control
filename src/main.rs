#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod devices;
mod protocol;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use commands::AppState;
use devices::manager::DeviceManager;
use tauri::window::{Color, Effect, EffectsBuilder};
use tauri::{Emitter, WebviewUrl, WebviewWindowBuilder};

fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        let log_path = std::env::temp_dir().join("redmagic-control-panic.log");
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path) {
            let _ = writeln!(f, "[{}] PANIC: {info}", chrono::Local::now().format("%Y-%m-%d %H:%M:%S"));
        }
    }));
}

/// Windows 单实例互斥锁：重复双击 exe 时直接唤醒已有窗口，防止多实例争抢 WebView2 / HID 句柄
#[cfg(windows)]
struct SingleInstanceGuard(windows_sys::Win32::Foundation::HANDLE);

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.0.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    use std::ptr::null;
    use windows_sys::Win32::Foundation::{CloseHandle, GetLastError, ERROR_ALREADY_EXISTS};
    use windows_sys::Win32::System::Threading::CreateMutexW;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE,
    };

    let name: Vec<u16> = OsStr::new("Local\\REDMAGIC_Control_RS_SingleInstance")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let h = CreateMutexW(null(), 0, name.as_ptr());
        if h.is_null() {
            return Some(SingleInstanceGuard(std::ptr::null_mut()));
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            let title: Vec<u16> = OsStr::new("REDMAGIC Control")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let hwnd = FindWindowW(null(), title.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
            return None;
        }
        Some(SingleInstanceGuard(h))
    }
}

#[cfg(not(windows))]
struct SingleInstanceGuard;

#[cfg(not(windows))]
fn acquire_single_instance() -> Option<SingleInstanceGuard> {
    Some(SingleInstanceGuard)
}

#[cfg(windows)]
fn apply_win11_glass_chrome(hwnd_raw: *mut std::ffi::c_void) {
    use windows_sys::Win32::Graphics::Dwm::{
        DwmExtendFrameIntoClientArea, DwmSetWindowAttribute,
    };
    use windows_sys::Win32::UI::Controls::MARGINS;

    if hwnd_raw.is_null() {
        return;
    }
    const DWMWA_USE_IMMERSIVE_DARK_MODE: u32 = 20;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWA_BORDER_COLOR: u32 = 34;
    const DWMWA_SYSTEMBACKDROP_TYPE: u32 = 38;
    const DWMWCP_ROUND: i32 = 2;
    const DWMSBT_TRANSIENTWINDOW: i32 = 3; // Windows 11 原生 DWM Acrylic 毛玻璃

    unsafe {
        let dark: i32 = 1;
        let _ = DwmSetWindowAttribute(
            hwnd_raw,
            DWMWA_USE_IMMERSIVE_DARK_MODE,
            &dark as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
        let corner: i32 = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(
            hwnd_raw,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &corner as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
        // 细微冷灰玻璃外边框（COLORREF: 0x00BBGGRR）
        let border_bgr: u32 = 0x003a322e;
        let _ = DwmSetWindowAttribute(
            hwnd_raw,
            DWMWA_BORDER_COLOR,
            &border_bgr as *const u32 as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );
        let margins = MARGINS {
            cxLeftWidth: -1,
            cxRightWidth: -1,
            cyTopHeight: -1,
            cyBottomHeight: -1,
        };
        let _ = DwmExtendFrameIntoClientArea(hwnd_raw, &margins);
        let backdrop: i32 = DWMSBT_TRANSIENTWINDOW;
        let _ = DwmSetWindowAttribute(
            hwnd_raw,
            DWMWA_SYSTEMBACKDROP_TYPE,
            &backdrop as *const i32 as *const std::ffi::c_void,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

fn primary_webview_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("com.redmagic.control.rs")
        .join("wv2")
}

fn build_main_window(
    app: &tauri::App,
    data_dir: PathBuf,
) -> tauri::Result<tauri::WebviewWindow> {
    let effects = EffectsBuilder::new()
        .effect(Effect::Acrylic)
        .color(Color(10, 12, 18, 195))
        .build();

    let entry_url = match std::env::var("REDMAGIC_VIEW") {
        Ok(q) if !q.trim().is_empty() => format!("index.html?{}", q.trim().trim_start_matches('?')),
        _ => "index.html".to_string(),
    };

    let allow_devtools = std::env::var("REDMAGIC_DEVTOOLS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    WebviewWindowBuilder::new(app, "main", WebviewUrl::App(entry_url.into()))
        .title("REDMAGIC Control")
        .inner_size(1280.0, 820.0)
        .min_inner_size(1080.0, 680.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .resizable(true)
        .center()
        .devtools(allow_devtools)
        .zoom_hotkeys_enabled(false)
        .effects(effects)
        .data_directory(data_dir)
        .build()
}

fn main() {
    install_panic_logger();
    let args: Vec<String> = std::env::args().collect();
    let force_mock = std::env::var("MOCK_MODE")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // 命令行只读自检模式（不弹窗口，用于自动化真机/Mock 协议验证）
    if args.iter().any(|a| a == "--probe") {
        let mgr = DeviceManager::new(force_mock);
        let devs = mgr.list_devices(true);
        println!("PROBE_DEVICES={}", serde_json::to_string_pretty(&devs).unwrap());
        for d in &devs {
            match mgr.read_state(&d.id) {
                Ok(st) => {
                    println!(
                        "PROBE_STATE[{}] mode={} state={} mtu={} bat={} chg={} keys={} cap={}",
                        d.id,
                        st["connectionMode"].as_str().unwrap_or("-"),
                        st["linkState"].as_str().unwrap_or("-"),
                        st["chunkSize"],
                        st["battery"],
                        st["chargingState"],
                        st["hwKeyCount"],
                        st["capabilityHex"].as_str().unwrap_or("-")
                    );
                }
                Err(e) => println!("PROBE_STATE[{}] ERR={e}", d.id),
            }
        }
        mgr.close_all_sessions();
        return;
    }

    let _single_guard = if force_mock {
        None
    } else {
        match acquire_single_instance() {
            Some(g) => Some(g),
            None => return,
        }
    };

    let manager = DeviceManager::new(force_mock);
    let monitor_mgr = manager.clone();
    let cleanup_mgr = manager.clone();

    tauri::Builder::default()
        .manage(AppState { manager })
        .setup(move |app| {
            // 优先使用固定 WebView2 数据目录；Mock 预览或遭遇 0x800700AA（目录被占用）时自动使用独立临时目录避免闪退
            let win = if force_mock {
                let mock_dir = std::env::temp_dir()
                    .join(format!("redmagic-control-mock-{}", std::process::id()));
                build_main_window(app, mock_dir)?
            } else {
                match build_main_window(app, primary_webview_data_dir()) {
                    Ok(w) => w,
                    Err(_) => {
                        let fallback = std::env::temp_dir()
                            .join(format!("redmagic-control-wv2-{}", std::process::id()));
                        build_main_window(app, fallback)?
                    }
                }
            };
            #[cfg(windows)]
            if let Ok(hwnd) = win.hwnd() {
                apply_win11_glass_chrome(hwnd.0 as *mut std::ffi::c_void);
            }
            #[cfg(windows)]
            {
                let allow_devtools = std::env::var("REDMAGIC_DEVTOOLS")
                    .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                    .unwrap_or(false);
                let _ = win.with_webview(move |wv| unsafe {
                    if let Ok(core) = wv.controller().CoreWebView2() {
                        if let Ok(settings) = core.Settings() {
                            let _ = settings.SetAreDefaultContextMenusEnabled(allow_devtools);
                            let _ = settings.SetAreDevToolsEnabled(allow_devtools);
                            let _ = settings.SetIsStatusBarEnabled(false);
                            let _ = settings.SetIsZoomControlEnabled(false);
                        }
                    }
                });
            }
            let _ = win.set_focus();

            let handle = app.handle().clone();
            monitor_mgr.start_monitor(move |evt| {
                let _ = handle.emit("device:event", evt);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_meta,
            commands::device_list,
            commands::device_rescan,
            commands::device_wake,
            commands::device_switch_link,
            commands::device_state,
            commands::device_battery,
            commands::device_config_write,
            commands::device_key_write,
            commands::device_key_reset,
            commands::device_key_reset_all,
            commands::device_screen_upload,
            commands::device_screen_lib_save,
            commands::device_screen_lib_delete,
            commands::screen_pick_file,
            commands::screen_paste_clipboard,
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(move |_app_handle, event| {
            if let tauri::RunEvent::ExitRequested { .. } | tauri::RunEvent::Exit = event {
                cleanup_mgr.close_all_sessions();
            }
        });
}
