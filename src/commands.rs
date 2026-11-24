//! Tauri v2 IPC 命令层：将全部设备读写、小屏烧录、原生文件选择与剪贴板读图暴露给前端。

use std::fs;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::devices::catalog::get_metadata_json;
use crate::devices::manager::{DeviceDescriptor, DeviceManager};
use crate::macros::{MacroRecorder, MacroStore};
use crate::protocol::macro_proto::{
    compute_macro_table_size, parse_imported_macros, MacroDef, MacroLibrary,
};
use crate::settings::{open_directory_in_explorer, AppSettingsPatch, SettingsStore};
use crate::studio::runner::NativeStreamConfig;
use crate::studio::server::{DEFAULT_HTTP_PORT, DEFAULT_UDP_PORT};
use crate::studio::StudioEngine;
use crate::updater::{
    check_latest_release, download_and_stage_update, launch_staged_updater, open_external_url,
    UpdateProgress, AUTHOR_URL, RELEASES_URL, REPO_OWNER, REPO_URL,
};

pub struct AppState {
    pub manager: DeviceManager,
    pub studio: StudioEngine,
    pub settings: SettingsStore,
    pub macros: MacroStore,
    pub macro_recorder: MacroRecorder,
    pub pending_auto_resume: Arc<AtomicBool>,
    pub force_quit: Arc<AtomicBool>,
    pub update_in_progress: Arc<AtomicBool>,
    pub update_cancel: Arc<AtomicBool>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScreenUploadOpts {
    pub name: Option<String>,
    pub reuse_uuid: Option<String>,
}

#[tauri::command]
pub fn app_meta() -> Value {
    let mut meta = get_metadata_json(env!("CARGO_PKG_VERSION"));
    if let Some(obj) = meta.as_object_mut() {
        obj.insert("author".into(), json!(REPO_OWNER));
        obj.insert("authorUrl".into(), json!(AUTHOR_URL));
        obj.insert("repoUrl".into(), json!(REPO_URL));
        obj.insert("releasesUrl".into(), json!(RELEASES_URL));
    }
    meta
}

#[tauri::command]
pub async fn app_check_update() -> Result<Value, String> {
    tokio::task::spawn_blocking(|| {
        let res = check_latest_release(env!("CARGO_PKG_VERSION"))?;
        Ok::<Value, String>(serde_json::to_value(res).unwrap_or_else(|_| json!({})))
    })
    .await
    .map_err(|e| format!("检查更新任务异常: {e}"))?
}

#[tauri::command]
pub async fn app_install_update(
    app: AppHandle,
    state: State<'_, AppState>,
    download_url: String,
    expected_size: Option<u64>,
) -> Result<Value, String> {
    if state
        .update_in_progress
        .compare_exchange(false, true, Ordering::SeqCst, Ordering::SeqCst)
        .is_err()
    {
        return Err("更新正在下载或安装中，请稍候".to_string());
    }
    state.update_cancel.store(false, Ordering::SeqCst);

    let current_exe = match std::env::current_exe() {
        Ok(p) => p,
        Err(e) => {
            state.update_in_progress.store(false, Ordering::SeqCst);
            return Err(format!("无法定位当前程序路径: {e}"));
        }
    };

    let cancel = state.update_cancel.clone();
    let app_for_prog = app.clone();
    let exe_for_dl = current_exe.clone();
    let dl_res = tokio::task::spawn_blocking(move || {
        download_and_stage_update(
            &download_url,
            expected_size,
            &exe_for_dl,
            env!("CARGO_PKG_VERSION"),
            &cancel,
            |prog| {
                let _ = app_for_prog.emit("update:progress", &prog);
            },
        )
    })
    .await
    .map_err(|e| format!("下载更新任务异常: {e}"))?;

    let staged_path = match dl_res {
        Ok(p) => p,
        Err(e) => {
            let canceled = state.update_cancel.load(Ordering::Relaxed);
            state.update_in_progress.store(false, Ordering::SeqCst);
            let _ = app.emit(
                "update:progress",
                &UpdateProgress {
                    stage: if canceled { "canceled" } else { "error" }.to_string(),
                    downloaded: 0,
                    total: expected_size,
                    pct: 0,
                    speed_bps: 0,
                    message: e.clone(),
                },
            );
            return Err(e);
        }
    };

    let _ = app.emit(
        "update:progress",
        &UpdateProgress {
            stage: "installing".to_string(),
            downloaded: expected_size.unwrap_or(0),
            total: expected_size,
            pct: 100,
            speed_bps: 0,
            message: "校验通过，正在替换程序并自动重启…".to_string(),
        },
    );

    // 停机释放所有 HID 会话与推流线程，并置位强制退出标志防止被托盘常驻拦截
    state.force_quit.store(true, Ordering::SeqCst);
    state.studio.shutdown(&state.manager);
    state.manager.close_all_sessions();

    if let Err(e) = launch_staged_updater(&staged_path, &current_exe, std::process::id()) {
        state.force_quit.store(false, Ordering::SeqCst);
        state.update_in_progress.store(false, Ordering::SeqCst);
        let _ = fs::remove_file(&staged_path);
        return Err(e);
    }

    // 延迟 180ms 退出，确保前端收到成功响应并渲染重启提示，同时给 helper 进程留出交接窗口
    let app_for_exit = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(180));
        app_for_exit.exit(0);
    });

    Ok(json!({ "ok": true, "restarting": true }))
}

#[tauri::command]
pub fn app_cancel_update(state: State<'_, AppState>) -> Value {
    state.update_cancel.store(true, Ordering::SeqCst);
    json!({ "ok": true })
}

#[tauri::command]
pub fn app_open_url(url: String) -> Result<Value, String> {
    open_external_url(&url)?;
    Ok(json!({ "ok": true }))
}

#[tauri::command]
pub fn app_settings_get(state: State<'_, AppState>) -> Value {
    serde_json::to_value(state.settings.view()).unwrap_or_else(|_| json!({}))
}

#[tauri::command]
pub fn app_settings_set(
    state: State<'_, AppState>,
    patch: AppSettingsPatch,
) -> Result<Value, String> {
    let view = state.settings.apply_patch(patch)?;
    Ok(serde_json::to_value(view).unwrap_or_else(|_| json!({})))
}

#[tauri::command]
pub fn app_open_data_dir(state: State<'_, AppState>) -> Result<Value, String> {
    let dir = state.settings.data_dir();
    open_directory_in_explorer(&dir)?;
    Ok(json!({ "ok": true, "path": dir.to_string_lossy() }))
}

#[tauri::command]
pub async fn device_list(state: State<'_, AppState>) -> Result<Vec<DeviceDescriptor>, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.list_devices(false))
        .await
        .map_err(|e| format!("任务执行异常: {e}"))
}

#[tauri::command]
pub async fn device_rescan(state: State<'_, AppState>) -> Result<Vec<DeviceDescriptor>, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.rescan_devices())
        .await
        .map_err(|e| format!("扫描异常: {e}"))
}

#[tauri::command]
pub async fn device_wake(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.wake_device(&id))
        .await
        .map_err(|e| format!("唤醒异常: {e}"))?
}

#[tauri::command]
pub async fn device_switch_link(
    state: State<'_, AppState>,
    id: String,
    link_id: String,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.switch_link(&id, &link_id))
        .await
        .map_err(|e| format!("切换链路异常: {e}"))?
}

#[tauri::command]
pub async fn device_state(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.read_state(&id))
        .await
        .map_err(|e| format!("读取状态异常: {e}"))?
}

#[tauri::command]
pub async fn device_battery(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.read_battery(&id))
        .await
        .map_err(|e| format!("读取电量异常: {e}"))?
}

#[tauri::command]
pub async fn device_config_write(
    state: State<'_, AppState>,
    id: String,
    patch: Value,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.write_config(&id, &patch))
        .await
        .map_err(|e| format!("写入配置异常: {e}"))?
}

#[tauri::command]
pub async fn device_key_write(
    state: State<'_, AppState>,
    id: String,
    slot: usize,
    hex: String,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.write_key(&id, slot, &hex))
        .await
        .map_err(|e| format!("写入键位异常: {e}"))?
}

#[tauri::command]
pub async fn device_key_reset(
    state: State<'_, AppState>,
    id: String,
    slot: usize,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.reset_key(&id, slot))
        .await
        .map_err(|e| format!("恢复键位异常: {e}"))?
}

#[tauri::command]
pub async fn device_key_reset_all(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.reset_all_keys(&id))
        .await
        .map_err(|e| format!("重置键表异常: {e}"))?
}

#[tauri::command]
pub async fn device_screen_upload(
    app: AppHandle,
    state: State<'_, AppState>,
    id: String,
    base64_rgb565: String,
    opts: Option<ScreenUploadOpts>,
) -> Result<Value, String> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_rgb565.trim())
        .map_err(|e| format!("Base64 图像解码失败: {e}"))?;
    let opts = opts.unwrap_or_default();
    let name = opts.name.unwrap_or_default();
    let reuse_uuid = opts.reuse_uuid;
    let mgr = state.manager.clone();

    tokio::task::spawn_blocking(move || {
        mgr.upload_screen_image(&id, &raw, &name, reuse_uuid.as_deref(), |pct, sent, total| {
            let _ = app.emit(
                "screen:progress",
                json!({ "pct": pct, "sent": sent, "total": total, "slot": 0 }),
            );
        })
    })
    .await
    .map_err(|e| format!("小屏写入异常: {e}"))?
}

#[tauri::command]
pub async fn device_screen_lib_save(
    state: State<'_, AppState>,
    id: String,
    base64_rgb565: String,
    name: Option<String>,
) -> Result<Value, String> {
    let raw = base64::engine::general_purpose::STANDARD
        .decode(base64_rgb565.trim())
        .map_err(|e| format!("Base64 图像解码失败: {e}"))?;
    let name = name.unwrap_or_default();
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.save_screen_to_library(&id, &raw, &name))
        .await
        .map_err(|e| format!("保存图库异常: {e}"))?
}

#[tauri::command]
pub async fn device_screen_lib_delete(
    state: State<'_, AppState>,
    id: String,
    uuid: String,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.delete_screen_library_item(&id, &uuid))
        .await
        .map_err(|e| format!("删除图库异常: {e}"))?
}

#[tauri::command]
pub async fn screen_pick_file() -> Result<Value, String> {
    tokio::task::spawn_blocking(|| {
        let picked = rfd::FileDialog::new()
            .set_title("选择小屏图片")
            .add_filter("图片文件", &["png", "jpg", "jpeg", "webp", "bmp", "gif"])
            .add_filter("所有文件", &["*"])
            .pick_file();
        let Some(file_path) = picked else {
            return Ok(json!({ "canceled": true }));
        };
        let meta = fs::metadata(&file_path).map_err(|e| format!("读取文件信息失败: {e}"))?;
        if meta.len() > 30 * 1024 * 1024 {
            return Err("图片文件过大（上限 30 MB）".to_string());
        }
        let buf = fs::read(&file_path).map_err(|e| format!("读取图片失败: {e}"))?;
        let ext = Path::new(&file_path)
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        let mime = match ext.as_str() {
            "png" => "image/png",
            "webp" => "image/webp",
            "gif" => "image/gif",
            "bmp" => "image/bmp",
            _ => "image/jpeg",
        };
        let b64 = base64::engine::general_purpose::STANDARD.encode(&buf);
        let name = file_path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("image")
            .to_string();
        Ok(json!({
            "canceled": false,
            "filePath": file_path.to_string_lossy(),
            "name": name,
            "size": meta.len(),
            "dataUrl": format!("data:{mime};base64,{b64}"),
        }))
    })
    .await
    .map_err(|e| format!("打开文件对话框失败: {e}"))?
}

#[tauri::command]
pub async fn screen_paste_clipboard() -> Result<Value, String> {
    tokio::task::spawn_blocking(|| {
        let mut cb = arboard::Clipboard::new().map_err(|e| format!("无法访问系统剪贴板: {e}"))?;
        let Ok(img) = cb.get_image() else {
            return Ok(json!({ "empty": true }));
        };
        if img.width == 0 || img.height == 0 {
            return Ok(json!({ "empty": true }));
        }
        let mut png_bytes = Vec::new();
        {
            let mut enc = png::Encoder::new(&mut png_bytes, img.width as u32, img.height as u32);
            enc.set_color(png::ColorType::Rgba);
            enc.set_depth(png::BitDepth::Eight);
            let mut writer = enc
                .write_header()
                .map_err(|e| format!("PNG 编码头失败: {e}"))?;
            writer
                .write_image_data(&img.bytes)
                .map_err(|e| format!("PNG 编码失败: {e}"))?;
        }
        let b64 = base64::engine::general_purpose::STANDARD.encode(&png_bytes);
        Ok(json!({
            "empty": false,
            "width": img.width,
            "height": img.height,
            "name": format!("剪贴板截图 ({}×{})", img.width, img.height),
            "dataUrl": format!("data:image/png;base64,{b64}"),
        }))
    })
    .await
    .map_err(|e| format!("读取剪贴板失败: {e}"))?
}

/* ---- 灯效编程工作室：板载逐键 RGB / 实时 RAM 推流 / 传感器遥测 / 本地外部 API 桥 ---- */

#[tauri::command]
pub async fn device_custom_rgb_read(
    state: State<'_, AppState>,
    id: String,
    group: u8,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.read_custom_rgb(&id, group))
        .await
        .map_err(|e| format!("读取逐键配色异常: {e}"))?
}

#[tauri::command]
pub async fn device_custom_rgb_write(
    state: State<'_, AppState>,
    id: String,
    group: u8,
    rgb: Vec<u8>,
    activate: bool,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.write_custom_rgb(&id, group, &rgb, activate))
        .await
        .map_err(|e| format!("写入板载逐键配色异常: {e}"))?
}

#[tauri::command]
pub async fn studio_direct_start(
    state: State<'_, AppState>,
    id: String,
    include_logo: bool,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let studio = state.studio.clone();
    tokio::task::spawn_blocking(move || {
        if studio.runner.is_running() {
            studio.runner.stop(&mgr);
        }
        mgr.start_direct_rgb(&id, include_logo)
    })
    .await
    .map_err(|e| format!("开启实时灯效推流异常: {e}"))?
}

#[tauri::command]
pub async fn studio_direct_stop(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let studio = state.studio.clone();
    tokio::task::spawn_blocking(move || {
        if studio.runner.is_running() {
            studio.runner.stop(&mgr);
        }
        if studio.server.is_running() {
            studio.server.stop(&mgr);
        }
        mgr.stop_direct_rgb(&id)
    })
    .await
    .map_err(|e| format!("停止实时灯效推流异常: {e}"))?
}

#[tauri::command]
pub async fn studio_native_start(
    state: State<'_, AppState>,
    id: String,
    config: NativeStreamConfig,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let studio = state.studio.clone();
    let settings = state.settings.clone();
    state.pending_auto_resume.store(false, Ordering::SeqCst);
    tokio::task::spawn_blocking(move || {
        if studio.server.is_running() {
            studio.server.stop(&mgr);
        }
        let cfg_save = config.clone();
        let st = studio.runner.start(
            mgr,
            id,
            studio.audio.clone(),
            studio.ambilight.clone(),
            studio.keys.clone(),
            config,
        )?;
        settings.save_native_config(&cfg_save, true);
        settings.set_studio_running(Some(true), Some(false));
        Ok::<Value, String>(serde_json::to_value(st).unwrap_or_else(|_| json!({})))
    })
    .await
    .map_err(|e| format!("启动后台推流异常: {e}"))?
}

#[tauri::command]
pub async fn studio_native_update(
    state: State<'_, AppState>,
    config: NativeStreamConfig,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let studio = state.studio.clone();
    let settings = state.settings.clone();
    tokio::task::spawn_blocking(move || {
        let cfg_save = config.clone();
        let st = studio.runner.update_config(
            &mgr,
            &studio.audio,
            &studio.ambilight,
            &studio.keys,
            config,
        )?;
        settings.save_native_config(&cfg_save, true);
        Ok::<Value, String>(serde_json::to_value(st).unwrap_or_else(|_| json!({})))
    })
    .await
    .map_err(|e| format!("更新后台推流参数异常: {e}"))?
}

#[tauri::command]
pub async fn studio_native_stop(state: State<'_, AppState>) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let studio = state.studio.clone();
    let settings = state.settings.clone();
    state.pending_auto_resume.store(false, Ordering::SeqCst);
    tokio::task::spawn_blocking(move || {
        let st = studio.runner.stop(&mgr);
        if !settings.auto_resume_studio() {
            settings.set_studio_running(Some(false), None);
        }
        Ok::<Value, String>(serde_json::to_value(st).unwrap_or_else(|_| json!({})))
    })
    .await
    .map_err(|e| format!("停止后台推流异常: {e}"))?
}

#[tauri::command]
pub fn studio_status(state: State<'_, AppState>) -> Value {
    json!({
        "native": state.studio.runner.status(),
        "server": state.studio.server.status(),
        "directActive": state.manager.has_any_direct_rgb(),
    })
}

#[tauri::command]
pub async fn studio_stream_frame(
    state: State<'_, AppState>,
    id: String,
    keys_rgb: Vec<u8>,
    logo_rgb: Option<Vec<u8>>,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || {
        mgr.stream_rgb_frame(&id, &keys_rgb, logo_rgb.as_deref())?;
        Ok::<Value, String>(json!({ "ok": true }))
    })
    .await
    .map_err(|e| format!("实时帧推送异常: {e}"))?
}

#[tauri::command]
pub fn studio_sensors_set(
    state: State<'_, AppState>,
    audio: bool,
    ambilight: bool,
    keys: bool,
) -> Value {
    state.studio.set_sensors(audio, ambilight, keys);
    json!({
        "ok": true,
        "audio": state.studio.audio.is_running(),
        "ambilight": state.studio.ambilight.is_running(),
        "keys": state.studio.keys.is_running(),
    })
}

#[tauri::command]
pub fn studio_telemetry(state: State<'_, AppState>) -> Value {
    serde_json::to_value(state.studio.poll_telemetry()).unwrap_or_else(|_| json!({}))
}

#[tauri::command]
pub async fn studio_server_start(
    state: State<'_, AppState>,
    id: String,
    udp_port: Option<u16>,
    http_port: Option<u16>,
    include_logo: Option<bool>,
) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let studio = state.studio.clone();
    let settings = state.settings.clone();
    state.pending_auto_resume.store(false, Ordering::SeqCst);
    let up = udp_port.unwrap_or(DEFAULT_UDP_PORT);
    let hp = http_port.unwrap_or(DEFAULT_HTTP_PORT);
    let logo = include_logo.unwrap_or(true);
    tokio::task::spawn_blocking(move || {
        if studio.runner.is_running() {
            studio.runner.stop(&mgr);
        }
        let st = studio.server.start(mgr, id, up, hp, logo)?;
        settings.set_studio_running(Some(false), Some(true));
        Ok::<Value, String>(serde_json::to_value(st).unwrap_or_else(|_| json!({})))
    })
    .await
    .map_err(|e| format!("启动外部控灯服务异常: {e}"))?
}

#[tauri::command]
pub async fn studio_server_stop(state: State<'_, AppState>) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let srv = state.studio.server.clone();
    let settings = state.settings.clone();
    state.pending_auto_resume.store(false, Ordering::SeqCst);
    tokio::task::spawn_blocking(move || {
        let st = srv.stop(&mgr);
        if !settings.auto_resume_studio() {
            settings.set_studio_running(None, Some(false));
        }
        Ok::<Value, String>(serde_json::to_value(st).unwrap_or_else(|_| json!({})))
    })
    .await
    .map_err(|e| format!("停止外部控灯服务异常: {e}"))?
}

#[tauri::command]
pub fn app_hide_to_tray(
    state: State<'_, AppState>,
    window: tauri::WebviewWindow,
) -> Result<Value, String> {
    // 仅手写自定义 JS 脚本正在推流时需保留隐藏 WebView2（且关闭 GPU 合成）；
    // 其余情况（空闲挂起 / 纯 Rust 原生推流 / 外部 API）一律直接销毁 WebView2 释放全部 ~300MB 内存与 6 个浏览器进程！
    let custom_js_streaming =
        state.manager.has_any_direct_rgb() && !state.studio.is_rust_owned_streaming();
    if !custom_js_streaming {
        window
            .destroy()
            .map_err(|e| format!("释放前端窗口失败: {e}"))?;
        return Ok(json!({ "ok": true, "mode": "headless-rust" }));
    }
    let _ = window.emit("studio:tray-hidden", json!({ "ok": true }));
    #[cfg(windows)]
    let _ = window.with_webview(|wv| unsafe {
        let _ = wv.controller().SetIsVisible(false);
    });
    window.hide().map_err(|e| format!("隐藏到系统托盘失败: {e}"))?;
    Ok(json!({ "ok": true, "mode": "hidden-webview" }))
}

#[tauri::command]
pub async fn studio_export_preset(name: String, content: String) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let default_name = if name.trim().is_empty() {
            "redmagic-rgb-preset.json".to_string()
        } else if name.ends_with(".json") {
            name
        } else {
            format!("{name}.json")
        };
        let picked = rfd::FileDialog::new()
            .set_title("导出灯效编程方案")
            .set_file_name(&default_name)
            .add_filter("灯效方案 (JSON)", &["json"])
            .save_file();
        let Some(path) = picked else {
            return Ok(json!({ "canceled": true }));
        };
        fs::write(&path, content.as_bytes()).map_err(|e| format!("保存方案失败: {e}"))?;
        Ok(json!({ "canceled": false, "filePath": path.to_string_lossy() }))
    })
    .await
    .map_err(|e| format!("导出对话框异常: {e}"))?
}

#[tauri::command]
pub async fn studio_import_preset() -> Result<Value, String> {
    tokio::task::spawn_blocking(|| {
        let picked = rfd::FileDialog::new()
            .set_title("导入灯效编程方案")
            .add_filter("灯效方案 (JSON / JS)", &["json", "js"])
            .pick_file();
        let Some(path) = picked else {
            return Ok(json!({ "canceled": true }));
        };
        let content = fs::read_to_string(&path).map_err(|e| format!("读取方案失败: {e}"))?;
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("preset.json")
            .to_string();
        Ok(json!({ "canceled": false, "name": name, "content": content }))
    })
    .await
    .map_err(|e| format!("导入对话框异常: {e}"))?
}

/* ---------------- 板载宏管理与实时录制 IPC ---------------- */

#[tauri::command]
pub fn macro_list_get(state: State<'_, AppState>) -> Value {
    let macros = state.macros.list();
    let total_bytes = compute_macro_table_size(&macros);
    json!({
        "macros": macros,
        "totalBytes": total_bytes,
        "path": state.macros.path_string(),
        "recording": state.macro_recorder.is_recording(),
    })
}

#[tauri::command]
pub fn macro_list_save(state: State<'_, AppState>, macros: Vec<MacroDef>) -> Result<Value, String> {
    let saved = state.macros.save_all(macros)?;
    let total_bytes = compute_macro_table_size(&saved);
    Ok(json!({
        "macros": saved,
        "totalBytes": total_bytes,
        "path": state.macros.path_string(),
    }))
}

#[tauri::command]
pub async fn macro_sync_device(
    state: State<'_, AppState>,
    id: String,
    macros: Option<Vec<MacroDef>>,
) -> Result<Value, String> {
    let list = match macros {
        Some(m) => state.macros.save_all(m)?,
        None => state.macros.list(),
    };
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || mgr.write_macros(&id, &list))
        .await
        .map_err(|e| format!("同步宏表异常: {e}"))?
}

#[tauri::command]
pub async fn macro_bind_key(
    state: State<'_, AppState>,
    id: String,
    slot: usize,
    macro_index: usize,
    trigger_mode: u8,
    repeat_count: Option<u8>,
    macros: Option<Vec<MacroDef>>,
) -> Result<Value, String> {
    let list = match macros {
        Some(m) => state.macros.save_all(m)?,
        None => state.macros.list(),
    };
    let rep = repeat_count.unwrap_or(1).max(1);
    let mgr = state.manager.clone();
    tokio::task::spawn_blocking(move || {
        mgr.bind_macro_key(&id, slot, &list, macro_index, trigger_mode, rep)
    })
    .await
    .map_err(|e| format!("绑定宏键位异常: {e}"))?
}

#[tauri::command]
pub fn macro_record_start(
    app: AppHandle,
    state: State<'_, AppState>,
    record_mouse: Option<bool>,
    delay_mode: Option<String>,
    fixed_delay_ms: Option<u16>,
) -> Result<Value, String> {
    state.macro_recorder.start(
        app,
        record_mouse.unwrap_or(false),
        delay_mode.unwrap_or_else(|| "recorded".to_string()),
        fixed_delay_ms.unwrap_or(20),
    )?;
    Ok(json!({ "ok": true, "recording": true }))
}

#[tauri::command]
pub fn macro_record_stop(state: State<'_, AppState>) -> Value {
    let events = state.macro_recorder.stop();
    json!({
        "ok": true,
        "recording": false,
        "events": events,
    })
}

#[tauri::command]
pub async fn macro_export_file(name: String, macros: Vec<MacroDef>) -> Result<Value, String> {
    tokio::task::spawn_blocking(move || {
        let safe_base: String = name
            .trim()
            .chars()
            .map(|c| match c {
                '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|' => '_',
                _ => c,
            })
            .collect();
        let default_name = if safe_base.is_empty() {
            "redmagic-macros.json".to_string()
        } else if safe_base.ends_with(".json") {
            safe_base
        } else {
            format!("{safe_base}.json")
        };
        let picked = rfd::FileDialog::new()
            .set_title("导出宏配置")
            .set_file_name(&default_name)
            .add_filter("宏配置文件 (JSON)", &["json"])
            .save_file();
        let Some(path) = picked else {
            return Ok(json!({ "canceled": true }));
        };
        let payload = MacroLibrary { version: 1, macros };
        let content = serde_json::to_string_pretty(&payload)
            .map_err(|e| format!("序列化宏配置失败: {e}"))?;
        fs::write(&path, content.as_bytes()).map_err(|e| format!("保存宏文件失败: {e}"))?;
        Ok(json!({ "canceled": false, "filePath": path.to_string_lossy() }))
    })
    .await
    .map_err(|e| format!("导出宏对话框异常: {e}"))?
}

#[tauri::command]
pub async fn macro_import_file() -> Result<Value, String> {
    tokio::task::spawn_blocking(|| {
        let picked = rfd::FileDialog::new()
            .set_title("导入宏配置（支持 JSON 与官方 Goper .mac）")
            .add_filter("宏文件 (JSON / MAC)", &["json", "mac"])
            .add_filter("所有文件", &["*"])
            .pick_file();
        let Some(path) = picked else {
            return Ok(json!({ "canceled": true }));
        };
        let content = fs::read_to_string(&path).map_err(|e| format!("读取宏文件失败: {e}"))?;
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("导入的宏");
        let imported = parse_imported_macros(&content, stem)?;
        Ok(json!({
            "canceled": false,
            "filePath": path.to_string_lossy(),
            "macros": imported,
        }))
    })
    .await
    .map_err(|e| format!("导入宏对话框异常: {e}"))?
}
