//! 应用偏好设置持久化 + 纯 Rust Win32 注册表开机自启管理（HKCU\...\Run）：
//! - 默认开启「关闭窗口时挂起至系统托盘」（closeToTray = true）与「开机自启静默挂起」（startMinimized = true）；
//! - 零外部 crate、零管理员提权需求，直接操作当前用户 HKCU 启动项并持久化至 %LOCALAPPDATA%\com.redmagic.control.rs\settings.json。

use std::fs;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use serde::{Deserialize, Serialize};

const AUTOSTART_VALUE_NAME: &str = "REDMAGICControl";
#[cfg(windows)]
const RUN_SUBKEY: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";

fn default_true() -> bool {
    true
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
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            close_to_tray: true,
            start_minimized: true,
            auto_check_update: true,
        }
    }
}

#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsPatch {
    pub close_to_tray: Option<bool>,
    pub start_minimized: Option<bool>,
    pub auto_check_update: Option<bool>,
    pub autostart: Option<bool>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AppSettingsView {
    pub close_to_tray: bool,
    pub start_minimized: bool,
    pub auto_check_update: bool,
    pub autostart: bool,
    pub exe_path: String,
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
        let path = settings_file_path();
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
            autostart,
            exe_path: current_exe_display_path(),
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
            cur.clone()
        };
        if !self.mock {
            if let Some(parent) = self.path.parent() {
                let _ = fs::create_dir_all(parent);
            }
            if let Ok(json) = serde_json::to_string_pretty(&next) {
                let _ = fs::write(&self.path, json);
            }
        }
        Ok(self.view())
    }
}

fn settings_file_path() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("com.redmagic.control.rs")
        .join("settings.json")
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

        let store = SettingsStore::load(true);
        assert!(store.close_to_tray());
        let v = store
            .apply_patch(AppSettingsPatch {
                autostart: Some(true),
                close_to_tray: Some(false),
                start_minimized: Some(true),
                auto_check_update: None,
            })
            .unwrap();
        assert!(v.autostart);
        assert!(!v.close_to_tray);
        assert!(v.start_minimized);
        assert!(v.auto_check_update);
    }

    #[test]
    fn formats_autostart_command_cleanly() {
        assert_eq!(
            format_autostart_command(r"\\?\C:\Program Files\REDMAGIC\redmagic-control.exe"),
            "\"C:\\Program Files\\REDMAGIC\\redmagic-control.exe\" --autostart"
        );
    }
}
