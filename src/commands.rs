//! Tauri v2 IPC 命令层：将全部设备读写、小屏烧录、原生文件选择与剪贴板读图暴露给前端。

use std::fs;
use std::path::Path;

use base64::Engine;
use serde::Deserialize;
use serde_json::{json, Value};
use tauri::{AppHandle, Emitter, State};

use crate::devices::catalog::get_metadata_json;
use crate::devices::manager::{DeviceDescriptor, DeviceManager};
use crate::studio::server::{DEFAULT_HTTP_PORT, DEFAULT_UDP_PORT};
use crate::studio::StudioEngine;

pub struct AppState {
    pub manager: DeviceManager,
    pub studio: StudioEngine,
}

#[derive(Debug, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct ScreenUploadOpts {
    pub name: Option<String>,
    pub reuse_uuid: Option<String>,
}

#[tauri::command]
pub fn app_meta() -> Value {
    get_metadata_json(env!("CARGO_PKG_VERSION"))
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
    tokio::task::spawn_blocking(move || mgr.start_direct_rgb(&id, include_logo))
        .await
        .map_err(|e| format!("开启实时灯效推流异常: {e}"))?
}

#[tauri::command]
pub async fn studio_direct_stop(state: State<'_, AppState>, id: String) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let studio = state.studio.clone();
    tokio::task::spawn_blocking(move || {
        if studio.server.is_running() {
            studio.server.stop(&mgr);
        }
        mgr.stop_direct_rgb(&id)
    })
    .await
    .map_err(|e| format!("停止实时灯效推流异常: {e}"))?
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
    let srv = state.studio.server.clone();
    let up = udp_port.unwrap_or(DEFAULT_UDP_PORT);
    let hp = http_port.unwrap_or(DEFAULT_HTTP_PORT);
    let logo = include_logo.unwrap_or(true);
    tokio::task::spawn_blocking(move || {
        let st = srv.start(mgr, id, up, hp, logo)?;
        Ok::<Value, String>(serde_json::to_value(st).unwrap_or_else(|_| json!({})))
    })
    .await
    .map_err(|e| format!("启动外部控灯服务异常: {e}"))?
}

#[tauri::command]
pub async fn studio_server_stop(state: State<'_, AppState>) -> Result<Value, String> {
    let mgr = state.manager.clone();
    let srv = state.studio.server.clone();
    tokio::task::spawn_blocking(move || {
        let st = srv.stop(&mgr);
        Ok::<Value, String>(serde_json::to_value(st).unwrap_or_else(|_| json!({})))
    })
    .await
    .map_err(|e| format!("停止外部控灯服务异常: {e}"))?
}

#[tauri::command]
pub fn app_hide_to_tray(window: tauri::WebviewWindow) -> Result<Value, String> {
    window.hide().map_err(|e| format!("隐藏到系统托盘失败: {e}"))?;
    Ok(json!({ "ok": true }))
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
