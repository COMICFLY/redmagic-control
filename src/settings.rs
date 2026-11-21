//! 应用偏好与灯效编程状态持久化 + Win32 注册表开机自启管理（HKCU\...\Run）：
//! - 默认开启「关闭窗口时挂起至系统托盘」、「开机自启静默挂起」与「启动时自动恢复上次推流」；
//! - 采用临时文件原子落盘至 %LOCALAPPDATA%\com.redmagic.control.rs\settings.json，防止断电损坏；
//! - 直接操作当前用户 HKCU 启动项，无需管理员权限。

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

use crate::studio::runner::{NativeLayersConfig, NativeStreamConfig};
use crate::studio::server::{DEFAULT_HTTP_PORT, DEFAULT_UDP_PORT};

const AUTOSTART_VALUE_NAME: &str = "REDMAGICControl";
#[cfg(windows)]
const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn default_true() -> bool {
    true
}
fn default_sub_mode() -> String {
    "script".to_string()
}
fn default_script_id() -> String {
    "aurora".to_string()
}
fn default_stream_source() -> String {
    "script".to_string()
}
fn default_paint_color() -> String {
    "#e53945".to_string()
}
fn default_p1() -> f32 {
    0.55
}
fn default_p2() -> f32 {
    0.5
}
fn default_p3() -> f32 {
    0.15
}
fn default_brightness() -> f32 {
    1.0
}
fn default_udp_port() -> u16 {
    DEFAULT_UDP_PORT
}
fn default_http_port() -> u16 {
    DEFAULT_HTTP_PORT
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedStudioState {
    #[serde(default = "default_sub_mode")]
    pub sub_mode: String,
    #[serde(default = "default_script_id")]
    pub script_id: String,
    #[serde(default)]
    pub script_code: Option<String>,
    #[serde(default = "default_p1")]
    pub p1: f32,
    #[serde(default = "default_p2")]
    pub p2: f32,
    #[serde(default = "default_p3")]
    pub p3: f32,
    #[serde(default)]
    pub layers: NativeLayersConfig,
    #[serde(default = "default_stream_source")]
    pub stream_source: String,
    #[serde(default = "default_true")]
    pub include_logo: bool,
    #[serde(default = "default_brightness")]
    pub brightness: f32,
    #[serde(default)]
    pub paint_group: u8,
    #[serde(default = "default_paint_color")]
    pub paint_color: String,
    #[serde(default = "default_udp_port")]
    pub udp_port: u16,
    #[serde(default = "default_http_port")]
    pub http_port: u16,
    #[serde(default = "default_true")]
    pub native_eligible: bool,
    #[serde(default)]
    pub was_streaming: bool,
    #[serde(default)]
    pub was_server_running: bool,
}

impl Default for PersistedStudioState {
    fn default() -> Self {
        Self {
            sub_mode: default_sub_mode(),
            script_id: default_script_id(),
            script_code: None,
            p1: default_p1(),
            p2: default_p2(),
            p3: default_p3(),
            layers: NativeLayersConfig::default(),
            stream_source: default_stream_source(),
            include_logo: true,
            brightness: default_brightness(),
            paint_group: 0,
            paint_color: default_paint_color(),
            udp_port: DEFAULT_UDP_PORT,
            http_port: DEFAULT_HTTP_PORT,
            native_eligible: true,
            was_streaming: false,
            was_server_running: false,
        }
    }
}

impl PersistedStudioState {
    pub fn to_native_config(&self) -> Option<NativeStreamConfig> {
        let is_layers = self.stream_source == "layers";
        if !is_layers && !self.native_eligible {
            return None;
        }
        Some(NativeStreamConfig {
            mode: if is_layers {
                "layers".to_string()
            } else {
                "preset".to_string()
            },
            preset_id: if self.script_id.is_empty() {
                "aurora".to_string()
            } else {
                self.script_id.clone()
            },
            p1: self.p1.clamp(0.0, 1.0),
            p2: self.p2.clamp(0.0, 1.0),
            p3: self.p3.clamp(0.0, 1.0),
            brightness: self.brightness.clamp(0.05, 1.0),
            include_logo: self.include_logo,
            layers: self.layers.clone(),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettings {
    #[serde(default = "default_true")]
    pub close_to_tray: bool,
    #[serde(default = "default_true")]
    pub start_minimized: bool,
    #[serde(default = "default_true")]
    pub auto_check_update: bool,
    #[serde(default = "default_true")]
    pub auto_resume_studio: bool,
    #[serde(default)]
    pub skipped_update_version: String,
    #[serde(default)]
    pub studio: PersistedStudioState,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            start_minimized: true,
            auto_check_update: true,
            auto_resume_studio: true,
            skipped_update_version: String::new(),
            studio: PersistedStudioState::default(),
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPatch {
    pub close_to_tray: Option<bool>,
    pub start_minimized: Option<bool>,
    pub auto_check_update: Option<bool>,
    pub auto_resume_studio: Option<bool>,
    pub skipped_update_version: Option<String>,
    pub autostart: Option<bool>,
    pub studio: Option<PersistedStudioState>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsView {
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub auto_check_update: bool,
    pub auto_resume_studio: bool,
    pub skipped_update_version: String,
    pub autostart: bool,
    pub exe_path: String,
    pub settings_path: String,
    pub studio: PersistedStudioState,
}

#[derive(Clone)]
pub struct SettingsStore {
    inner: Arc<Mutex<AppSettings>>,
    path: PathBuf,
    mock: bool,
    mock_autostart: Arc<Mutex<bool>>,
}

impl SettingsStore {
    pub fn load(mock: bool) -> Self {
        Self::load_at(settings_file_path(), mock)
    }

    pub fn load_at(path: PathBuf, mock: bool) -> Self {
        let initial = if mock {
            AppSettings::default()
        } else {
            fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<AppSettings>(&s).ok())
                .unwrap_or_default()
        };
        Self {
            inner: Arc::new(Mutex::new(initial)),
            path,
            mock,
            mock_autostart: Arc::new(Mutex::new(false)),
        }
    }

    pub fn close_to_tray(&self) -> bool {
        self.inner.lock().unwrap().close_to_tray
    }

    pub fn start_minimized(&self) -> bool {
        self.inner.lock().unwrap().start_minimized
    }

    pub fn auto_resume_studio(&self) -> bool {
        self.inner.lock().unwrap().auto_resume_studio
    }

    pub fn studio_snapshot(&self) -> PersistedStudioState {
        self.inner.lock().unwrap().studio.clone()
    }

    pub fn data_dir(&self) -> PathBuf {
        self.path
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(app_data_dir)
    }

    pub fn view(&self) -> AppSettingsView {
        let cur = self.inner.lock().unwrap().clone();
        let autostart = if self.mock {
            *self.mock_autostart.lock().unwrap()
        } else {
            is_windows_autostart_enabled()
        };
        AppSettingsView {
            close_to_tray: cur.close_to_tray,
            start_minimized: cur.start_minimized,
            auto_check_update: cur.auto_check_update,
            auto_resume_studio: cur.auto_resume_studio,
            skipped_update_version: cur.skipped_update_version,
            autostart,
            exe_path: current_exe_display_path(),
            settings_path: self.path.to_string_lossy().to_string(),
            studio: cur.studio,
        }
    }

    fn persist_locked(&self, cfg: &AppSettings) {
        if self.mock {
            return;
        }
        if let Some(parent) = self.path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(cfg) {
            let tmp = self.path.with_extension("json.tmp");
            if fs::write(&tmp, &json).is_ok() {
                if fs::rename(&tmp, &self.path).is_err() {
                    let _ = fs::write(&self.path, &json);
                    let _ = fs::remove_file(&tmp);
                }
            } else {
                let _ = fs::write(&self.path, &json);
            }
        }
    }

    pub fn apply_patch(&self, patch: AppSettingsPatch) -> Result<AppSettingsView, String> {
        if let Some(en) = patch.autostart {
            if self.mock {
                *self.mock_autostart.lock().unwrap() = en;
            } else {
                set_windows_autostart(en)?;
            }
        }
        let next = {
            let mut cur = self.inner.lock().unwrap();
            if let Some(v) = patch.close_to_tray {
                cur.close_to_tray = v;
            }
            if let Some(v) = patch.start_minimized {
                cur.start_minimized = v;
            }
            if let Some(v) = patch.auto_check_update {
                cur.auto_check_update = v;
            }
            if let Some(v) = patch.auto_resume_studio {
                cur.auto_resume_studio = v;
            }
            if let Some(v) = patch.skipped_update_version {
                cur.skipped_update_version = v;
            }
            if let Some(mut st) = patch.studio {
                st.paint_group = st.paint_group.min(3);
                st.udp_port = st.udp_port.clamp(1024, 65535);
                st.http_port = st.http_port.clamp(1024, 65535);
                st.brightness = st.brightness.clamp(0.05, 1.0);
                cur.studio = st;
            }
            cur.clone()
        };
        self.persist_locked(&next);
        Ok(self.view())
    }

    pub fn set_studio_running(&self, was_streaming: Option<bool>, was_server_running: Option<bool>) {
        let next = {
            let mut cur = self.inner.lock().unwrap();
            if let Some(v) = was_streaming {
                cur.studio.was_streaming = v;
            }
            if let Some(v) = was_server_running {
                cur.studio.was_server_running = v;
            }
            cur.clone()
        };
        self.persist_locked(&next);
    }

    pub fn save_native_config(&self, cfg: &NativeStreamConfig, running: bool) {
        let next = {
            let mut cur = self.inner.lock().unwrap();
            cur.studio.stream_source = if cfg.mode == "layers" {
                "layers".to_string()
            } else {
                "script".to_string()
            };
            cur.studio.script_id = cfg.preset_id.clone();
            cur.studio.p1 = cfg.p1;
            cur.studio.p2 = cfg.p2;
            cur.studio.p3 = cfg.p3;
            cur.studio.brightness = cfg.brightness;
            cur.studio.include_logo = cfg.include_logo;
            cur.studio.layers = cfg.layers.clone();
            cur.studio.native_eligible = true;
            cur.studio.was_streaming = running;
            cur.clone()
        };
        self.persist_locked(&next);
    }
}

pub fn app_data_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("com.redmagic.control.rs")
}

fn settings_file_path() -> PathBuf {
    app_data_dir().join("settings.json")
}

pub fn open_directory_in_explorer(dir: &Path) -> Result<(), String> {
    let _ = fs::create_dir_all(dir);
    #[cfg(windows)]
    {
        use std::ptr::null;
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let verb = to_wide("open");
        let path_w = to_wide(&dir.to_string_lossy());
        let rc = unsafe {
            ShellExecuteW(
                std::ptr::null_mut(),
                verb.as_ptr(),
                path_w.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            ) as isize
        };
        if rc <= 32 {
            return Err(format!("打开配置目录失败 (code={rc})"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        let _ = dir;
        Err("当前平台暂不支持打开目录".to_string())
    }
}

pub fn format_autostart_command(exe_path: &str) -> String {
    let clean = exe_path.strip_prefix(r"\\?\").unwrap_or(exe_path).trim();
    format!("\"{clean}\" --autostart")
}

fn current_exe_display_path() -> String {
    std::env::current_exe()
        .ok()
        .map(|p| {
            let s = p.to_string_lossy().to_string();
            s.strip_prefix(r"\\?\").unwrap_or(&s).to_string()
        })
        .unwrap_or_default()
}

#[cfg(windows)]
fn to_wide(s: &str) -> Vec<u16> {
    use std::ffi::OsStr;
    use std::os::windows::ffi::OsStrExt;
    OsStr::new(s)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(windows)]
pub fn is_windows_autostart_enabled() -> bool {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegOpenKeyExW, RegQueryValueExW, HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE,
    };

    let subkey = to_wide(RUN_SUBKEY);
    let val_name = to_wide(AUTOSTART_VALUE_NAME);
    unsafe {
        let mut hkey: HKEY = null_mut();
        if RegOpenKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            KEY_QUERY_VALUE,
            &mut hkey,
        ) != ERROR_SUCCESS
        {
            return false;
        }
        let mut reg_type: u32 = 0;
        let mut cb_data: u32 = 0;
        let rc = RegQueryValueExW(
            hkey,
            val_name.as_ptr(),
            null_mut(),
            &mut reg_type,
            null_mut(),
            &mut cb_data,
        );
        RegCloseKey(hkey);
        rc == ERROR_SUCCESS && cb_data > 2
    }
}

#[cfg(not(windows))]
pub fn is_windows_autostart_enabled() -> bool {
    false
}

#[cfg(windows)]
pub fn set_windows_autostart(enabled: bool) -> Result<(), String> {
    use std::ptr::null_mut;
    use windows_sys::Win32::Foundation::{ERROR_FILE_NOT_FOUND, ERROR_SUCCESS};
    use windows_sys::Win32::System::Registry::{
        RegCloseKey, RegCreateKeyExW, RegDeleteValueW, RegSetValueExW, HKEY, HKEY_CURRENT_USER,
        KEY_SET_VALUE, REG_OPTION_NON_VOLATILE, REG_SZ,
    };

    let subkey = to_wide(RUN_SUBKEY);
    let val_name = to_wide(AUTOSTART_VALUE_NAME);
    unsafe {
        let mut hkey: HKEY = null_mut();
        let rc_open = RegCreateKeyExW(
            HKEY_CURRENT_USER,
            subkey.as_ptr(),
            0,
            null_mut(),
            REG_OPTION_NON_VOLATILE,
            KEY_SET_VALUE,
            null_mut(),
            &mut hkey,
            null_mut(),
        );
        if rc_open != ERROR_SUCCESS {
            return Err(format!("打开注册表启动项失败 (code={rc_open})"));
        }

        let res = if enabled {
            let exe = current_exe_display_path();
            if exe.is_empty() {
                Err("无法获取当前程序路径".to_string())
            } else {
                let cmd_wide = to_wide(&format_autostart_command(&exe));
                let byte_len = (cmd_wide.len() * std::mem::size_of::<u16>()) as u32;
                let rc = RegSetValueExW(
                    hkey,
                    val_name.as_ptr(),
                    0,
                    REG_SZ,
                    cmd_wide.as_ptr() as *const u8,
                    byte_len,
                );
                if rc == ERROR_SUCCESS {
                    Ok(())
                } else {
                    Err(format!("写入开机自启注册表失败 (code={rc})"))
                }
            }
        } else {
            let rc = RegDeleteValueW(hkey, val_name.as_ptr());
            if rc == ERROR_SUCCESS || rc == ERROR_FILE_NOT_FOUND {
                Ok(())
            } else {
                Err(format!("移除开机自启注册表失败 (code={rc})"))
            }
        };

        RegCloseKey(hkey);
        res
    }
}

#[cfg(not(windows))]
pub fn set_windows_autostart(_enabled: bool) -> Result<(), String> {
    Err("当前平台暂不支持开机自启".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_enable_close_to_tray_and_silent_autostart() {
        let def = AppSettings::default();
        assert!(def.close_to_tray);
        assert!(def.start_minimized);
        assert!(def.auto_check_update);
        assert!(def.auto_resume_studio);

        let store = SettingsStore::load(true);
        assert!(store.close_to_tray());
        let v = store
            .apply_patch(AppSettingsPatch {
                autostart: Some(true),
                close_to_tray: Some(false),
                start_minimized: Some(true),
                auto_check_update: None,
                auto_resume_studio: Some(false),
                skipped_update_version: Some("0.7.0".to_string()),
                studio: None,
            })
            .unwrap();
        assert!(v.autostart);
        assert!(!v.close_to_tray);
        assert!(v.start_minimized);
        assert!(v.auto_check_update);
        assert!(!v.auto_resume_studio);
        assert_eq!(v.skipped_update_version, "0.7.0");
    }

    #[test]
    fn persists_and_reloads_studio_and_app_settings_atomically() {
        let tmp_dir = std::env::temp_dir().join(format!(
            "rm-settings-test-{}",
            std::process::id()
        ));
        let path = tmp_dir.join("settings.json");
        let _ = fs::remove_dir_all(&tmp_dir);

        let store = SettingsStore::load_at(path.clone(), false);
        let mut studio = PersistedStudioState::default();
        studio.sub_mode = "layers".to_string();
        studio.script_id = "radar".to_string();
        studio.script_code = Some("return [255, 0, 0];".to_string());
        studio.p1 = 0.88;
        studio.brightness = 0.72;
        studio.udp_port = 28000;
        studio.stream_source = "layers".to_string();
        studio.was_streaming = true;

        store
            .apply_patch(AppSettingsPatch {
                close_to_tray: Some(true),
                skipped_update_version: Some("0.6.9".to_string()),
                studio: Some(studio),
                ..Default::default()
            })
            .unwrap();

        let reloaded = SettingsStore::load_at(path.clone(), false);
        let v = reloaded.view();
        assert_eq!(v.skipped_update_version, "0.6.9");
        assert_eq!(v.studio.sub_mode, "layers");
        assert_eq!(v.studio.script_id, "radar");
        assert_eq!(v.studio.script_code.as_deref(), Some("return [255, 0, 0];"));
        assert!((v.studio.p1 - 0.88).abs() < 1e-4);
        assert_eq!(v.studio.udp_port, 28000);
        assert!(v.studio.was_streaming);
        let native_cfg = v.studio.to_native_config().expect("layers must be native-eligible");
        assert_eq!(native_cfg.mode, "layers");

        let _ = fs::remove_dir_all(&tmp_dir);
    }

    #[test]
    fn formats_autostart_command_cleanly() {
        assert_eq!(
            format_autostart_command(r"\\?\C:\Program Files\REDMAGIC\redmagic-control.exe"),
            "\"C:\\Program Files\\REDMAGIC\\redmagic-control.exe\" --autostart"
        );
    }
}
