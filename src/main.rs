#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod devices;
mod protocol;
mod studio;

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use commands::AppState;
use devices::manager::DeviceManager;
use studio::StudioEngine;
use tauri::menu::{MenuBuilder, MenuItemBuilder};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::window::{Color, Effect, EffectsBuilder};
use tauri::{AppHandle, Emitter, Manager, WebviewUrl, WebviewWindowBuilder, WindowEvent};

fn install_panic_logger() {
    std::panic::set_hook(Box::new(|info| {
        let log_path = std::env::temp_dir().join("redmagic-control-panic.log");
        if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(log_path) {
            let _ = writeln!(
                f,
                "[{}] PANIC: {info}",
                chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
            );
        }
    }));
}

/// Windows 单实例互斥锁 + 命名唤醒事件：
/// 即使主窗口在托盘挂起期间已彻底销毁（零 WebView2 进程），重复运行 exe 也能瞬间重建并唤起主界面。
#[cfg(windows)]
struct SingleInstanceGuard {
    mutex: windows_sys::Win32::Foundation::HANDLE,
    wake_event: windows_sys::Win32::Foundation::HANDLE,
}

#[cfg(windows)]
impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        unsafe {
            if !self.wake_event.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.wake_event);
            }
            if !self.mutex.is_null() {
                windows_sys::Win32::Foundation::CloseHandle(self.mutex);
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
    use windows_sys::Win32::System::Threading::{
        CreateEventW, CreateMutexW, OpenEventW, SetEvent, EVENT_MODIFY_STATE,
    };
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        FindWindowW, SetForegroundWindow, ShowWindow, SW_RESTORE, SW_SHOW,
    };

    let mutex_name: Vec<u16> = OsStr::new("Local\\REDMAGIC_Control_RS_SingleInstance")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let event_name: Vec<u16> = OsStr::new("Local\\REDMAGIC_Control_RS_WakeEvent")
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    unsafe {
        let h = CreateMutexW(null(), 0, mutex_name.as_ptr());
        if h.is_null() {
            return Some(SingleInstanceGuard {
                mutex: std::ptr::null_mut(),
                wake_event: std::ptr::null_mut(),
            });
        }
        if GetLastError() == ERROR_ALREADY_EXISTS {
            CloseHandle(h);
            // 优先通过命名事件唤醒常驻托盘进程（支持 WebView2 已销毁的纯 Rust 挂起态）
            let ev = OpenEventW(EVENT_MODIFY_STATE, 0, event_name.as_ptr());
            if !ev.is_null() {
                SetEvent(ev);
                CloseHandle(ev);
            }
            let title: Vec<u16> = OsStr::new("REDMAGIC Control")
                .encode_wide()
                .chain(std::iter::once(0))
                .collect();
            let hwnd = FindWindowW(null(), title.as_ptr());
            if !hwnd.is_null() {
                ShowWindow(hwnd, SW_SHOW);
                ShowWindow(hwnd, SW_RESTORE);
                SetForegroundWindow(hwnd);
            }
            return None;
        }
        let wake_event = CreateEventW(null(), 0, 0, event_name.as_ptr());
        Some(SingleInstanceGuard {
            mutex: h,
            wake_event,
        })
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

fn build_window_with_dir(
    app: &AppHandle,
    data_dir: PathBuf,
    use_env_view: bool,
) -> tauri::Result<tauri::WebviewWindow> {
    let effects = EffectsBuilder::new()
        .effect(Effect::Acrylic)
        .color(Color(10, 12, 18, 195))
        .build();

    let entry_url = if use_env_view {
        match std::env::var("REDMAGIC_VIEW") {
            Ok(q) if !q.trim().is_empty() => {
                format!("index.html?{}", q.trim().trim_start_matches('?'))
            }
            _ => "index.html".to_string(),
        }
    } else {
        "index.html".to_string()
    };

    let allow_devtools = std::env::var("REDMAGIC_DEVTOOLS")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false);

    // 仅禁用定时器节流以支持手写自定义 JS 后台推流；绝不加 --disable-backgrounding-occluded-windows，
    // 防止 Chromium GPU 进程对隐藏/遮挡窗口空跑 60fps 毛玻璃合成！
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
        .additional_browser_args("--disable-background-timer-throttling")
        .effects(effects)
        .data_directory(data_dir)
        .build()
}

/// 唤起已有主窗口，或在纯 Rust 托盘挂起（WebView2 已彻底销毁释放 ~300MB 内存）后按需重建主窗口
fn show_or_recreate_main_window(
    app: &AppHandle,
    force_mock: bool,
    use_env_view: bool,
    mgr: DeviceManager,
    studio: StudioEngine,
    force_quit: Arc<AtomicBool>,
) -> tauri::Result<()> {
    if let Some(w) = app.get_webview_window("main") {
        #[cfg(windows)]
        let _ = w.with_webview(|wv| unsafe {
            let _ = wv.controller().SetIsVisible(true);
        });
        let _ = w.show();
        let _ = w.unminimize();
        let _ = w.set_focus();
        let _ = w.emit("studio:tray-shown", serde_json::json!({ "ok": true }));
        return Ok(());
    }

    let win = if force_mock {
        let mock_dir =
            std::env::temp_dir().join(format!("redmagic-control-mock-{}", std::process::id()));
        build_window_with_dir(app, mock_dir, use_env_view)?
    } else {
        match build_window_with_dir(app, primary_webview_data_dir(), use_env_view) {
            Ok(w) => w,
            Err(_) => {
                let fallback = std::env::temp_dir()
                    .join(format!("redmagic-control-wv2-{}", std::process::id()));
                build_window_with_dir(app, fallback, use_env_view)?
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

    // 推流关闭策略：
    // 1. 纯 Rust 引擎或外部 API 运行时：直接允许关闭并销毁 WebView2（释放全部 ~300MB RAM 与 GPU 进程），
    //    进程与托盘继续在后台以 <0.3% CPU / ~15MB 内存稳帧推流；
    // 2. 手写自定义 JS 推流时：隐藏窗口并 SetIsVisible(false) 关闭 GPU 合成，使 GPU 占用降至 0%。
    let win_for_close = win.clone();
    win.on_window_event(move |event| {
        if let WindowEvent::CloseRequested { api, .. } = event {
            if force_quit.load(Ordering::SeqCst) {
                return;
            }
            if studio.is_rust_owned_streaming() {
                // 让 WebView2 窗口正常销毁；RunEvent::ExitRequested 会拦截退出保持托盘常驻
                return;
            }
            if mgr.has_any_direct_rgb() {
                api.prevent_close();
                let _ = win_for_close
                    .emit("studio:tray-hidden", serde_json::json!({ "ok": true }));
                #[cfg(windows)]
                let _ = win_for_close.with_webview(|wv| unsafe {
                    let _ = wv.controller().SetIsVisible(false);
                });
                let _ = win_for_close.hide();
            }
        }
    });

    let _ = win.set_focus();
    Ok(())
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
        println!(
            "PROBE_DEVICES={}",
            serde_json::to_string_pretty(&devs).unwrap()
        );
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

    let single_guard = if force_mock {
        None
    } else {
        match acquire_single_instance() {
            Some(g) => Some(g),
            None => return,
        }
    };

    #[cfg(windows)]
    let wake_event_raw: usize = single_guard
        .as_ref()
        .map(|g| g.wake_event as usize)
        .unwrap_or(0);

    let manager = DeviceManager::new(force_mock);
    let studio = StudioEngine::new();
    let monitor_mgr = manager.clone();
    let setup_mgr = manager.clone();
    let setup_studio = studio.clone();
    let exit_mgr = manager.clone();
    let exit_studio = studio.clone();
    let cleanup_mgr = manager.clone();
    let cleanup_studio = studio.clone();
    let force_quit = Arc::new(AtomicBool::new(false));
    let setup_force_quit = force_quit.clone();
    let exit_force_quit = force_quit.clone();
    let headless_shot = std::env::var("REDMAGIC_VIEW")
        .map(|v| !v.trim().is_empty())
        .unwrap_or(false);

    tauri::Builder::default()
        .manage(AppState { manager, studio })
        .setup(move |app| {
            let app_handle = app.handle().clone();
            show_or_recreate_main_window(
                &app_handle,
                force_mock,
                true,
                setup_mgr.clone(),
                setup_studio.clone(),
                setup_force_quit.clone(),
            )?;

            // 监听单实例唤醒事件（0% CPU 内核阻塞等待），重复双击 exe 时立即重建/唤起窗口
            #[cfg(windows)]
            if wake_event_raw != 0 {
                let wake_handle = app_handle.clone();
                let wake_mgr = setup_mgr.clone();
                let wake_studio = setup_studio.clone();
                let wake_fq = setup_force_quit.clone();
                std::thread::spawn(move || unsafe {
                    use windows_sys::Win32::System::Threading::{WaitForSingleObject, INFINITE};
                    let ev = wake_event_raw as windows_sys::Win32::Foundation::HANDLE;
                    while WaitForSingleObject(ev, INFINITE) == 0 {
                        let h = wake_handle.clone();
                        let m = wake_mgr.clone();
                        let s = wake_studio.clone();
                        let fq = wake_fq.clone();
                        let _ = wake_handle.run_on_main_thread(move || {
                            let _ = show_or_recreate_main_window(&h, force_mock, false, m, s, fq);
                        });
                    }
                });
            }

            // Windows 系统托盘图标与右键菜单（支持纯 Rust 零 WebView2 后台挂起、一键唤起、停止推流与退出）
            if !headless_shot {
                let show_item =
                    MenuItemBuilder::with_id("show", "打开 REDMAGIC Control").build(app)?;
                let stop_item =
                    MenuItemBuilder::with_id("stop_stream", "停止灯效推流（恢复板载）")
                        .build(app)?;
                let quit_item = MenuItemBuilder::with_id("quit", "退出程序").build(app)?;
                let tray_menu = MenuBuilder::new(app)
                    .items(&[&show_item, &stop_item, &quit_item])
                    .build()?;

                let click_mgr = setup_mgr.clone();
                let click_studio = setup_studio.clone();
                let click_fq = setup_force_quit.clone();
                let menu_mgr = setup_mgr.clone();
                let menu_studio = setup_studio.clone();
                let menu_fq = setup_force_quit.clone();

                let mut tray_builder = TrayIconBuilder::with_id("main-tray")
                    .tooltip("REDMAGIC Control — 轻量纯 Rust 后台灯效引擎")
                    .menu(&tray_menu)
                    .show_menu_on_left_click(false)
                    .on_tray_icon_event(move |tray, event| {
                        let restore = matches!(
                            event,
                            TrayIconEvent::Click {
                                button: MouseButton::Left,
                                button_state: MouseButtonState::Up,
                                ..
                            } | TrayIconEvent::DoubleClick {
                                button: MouseButton::Left,
                                ..
                            }
                        );
                        if restore {
                            let h = tray.app_handle().clone();
                            let _ = show_or_recreate_main_window(
                                &h,
                                force_mock,
                                false,
                                click_mgr.clone(),
                                click_studio.clone(),
                                click_fq.clone(),
                            );
                        }
                    })
                    .on_menu_event(move |app_handle, event| match event.id().as_ref() {
                        "show" => {
                            let _ = show_or_recreate_main_window(
                                app_handle,
                                force_mock,
                                false,
                                menu_mgr.clone(),
                                menu_studio.clone(),
                                menu_fq.clone(),
                            );
                        }
                        "stop_stream" => {
                            menu_studio.runner.stop(&menu_mgr);
                            menu_studio.server.stop(&menu_mgr);
                            menu_studio.set_sensors(false, false, false);
                            menu_mgr.stop_all_direct_rgb();
                            let _ = app_handle.emit(
                                "studio:stopped",
                                serde_json::json!({ "source": "tray" }),
                            );
                        }
                        "quit" => {
                            menu_fq.store(true, Ordering::SeqCst);
                            menu_studio.shutdown(&menu_mgr);
                            menu_mgr.close_all_sessions();
                            app_handle.exit(0);
                        }
                        _ => {}
                    });

                if let Some(ico) = app.default_window_icon() {
                    tray_builder = tray_builder.icon(ico.clone());
                }
                let _tray = tray_builder.build(app)?;
            }

            let handle = app.handle().clone();
            monitor_mgr.start_monitor(move |evt| {
                let _ = handle.emit("device:event", evt);
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::app_meta,
            commands::app_hide_to_tray,
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
            commands::device_custom_rgb_read,
            commands::device_custom_rgb_write,
            commands::studio_direct_start,
            commands::studio_direct_stop,
            commands::studio_stream_frame,
            commands::studio_native_start,
            commands::studio_native_update,
            commands::studio_native_stop,
            commands::studio_status,
            commands::studio_sensors_set,
            commands::studio_telemetry,
            commands::studio_server_start,
            commands::studio_server_stop,
            commands::studio_export_preset,
            commands::studio_import_preset,
        ])
        .build(tauri::generate_context!())
        .expect("构建 Tauri 应用失败")
        .run(move |_app_handle, event| {
            let _keep = &single_guard;
            match event {
                tauri::RunEvent::ExitRequested { api, .. } => {
                    let keep_alive = !exit_force_quit.load(Ordering::SeqCst)
                        && (exit_studio.is_rust_owned_streaming()
                            || exit_mgr.has_any_direct_rgb());
                    if keep_alive {
                        // 纯 Rust 托盘常驻模式：WebView2 已被销毁，保持极简 Rust 进程在托盘继续推流
                        api.prevent_exit();
                        return;
                    }
                    cleanup_studio.shutdown(&cleanup_mgr);
                    cleanup_mgr.close_all_sessions();
                }
                tauri::RunEvent::Exit => {
                    cleanup_studio.shutdown(&cleanup_mgr);
                    cleanup_mgr.close_all_sessions();
                }
                _ => {}
            }
        });
}
