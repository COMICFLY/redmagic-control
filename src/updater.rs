//! 纯 Rust 原生 Windows HTTPS 更新检查（WinHTTP）与安全外链打开（ShellExecuteW）：
//! - 零 reqwest / rustls / OpenSSL 依赖，自动复用 Windows 系统代理与 TLS 证书链；
//! - 直接查询 GitHub Releases API (`COMICFLY/redmagic-control`) 并执行严格 SemVer 比较。

use serde::Serialize;
use serde_json::Value;

pub const REPO_OWNER: &str = "COMICFLY";
pub const REPO_NAME: &str = "redmagic-control";
pub const REPO_URL: &str = "https://github.com/COMICFLY/redmagic-control";
pub const RELEASES_URL: &str = "https://github.com/COMICFLY/redmagic-control/releases";
pub const AUTHOR_URL: &str = "https://github.com/COMICFLY";

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateCheckResult {
    pub current_version: String,
    pub latest_version: String,
    pub tag_name: String,
    pub has_update: bool,
    pub release_name: String,
    pub release_notes: String,
    pub release_url: String,
    pub download_url: Option<String>,
    pub asset_name: Option<String>,
    pub asset_size: Option<u64>,
    pub published_at: String,
    pub checked_at: String,
}

pub fn parse_semver(raw: &str) -> (u64, u64, u64) {
    let cleaned = raw.trim().trim_start_matches(['v', 'V']);
    let main = cleaned.split('-').next().unwrap_or(cleaned);
    let mut parts = main.split('.');
    let major = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let minor = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    let patch = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
    (major, minor, patch)
}

pub fn is_newer_version(current: &str, latest: &str) -> bool {
    parse_semver(latest) > parse_semver(current)
}

pub fn parse_github_release_json(
    body: &str,
    current_version: &str,
) -> Result<UpdateCheckResult, String> {
    let v: Value =
        serde_json::from_str(body).map_err(|e| format!("解析 GitHub Release JSON 失败: {e}"))?;
    let tag_name = v
        .get("tag_name")
        .and_then(Value::as_str)
        .ok_or_else(|| "GitHub 响应缺少 tag_name 字段".to_string())?
        .to_string();
    let latest_version = tag_name.trim().trim_start_matches(['v', 'V']).to_string();
    let release_name = v
        .get("name")
        .and_then(Value::as_str)
        .filter(|s| !s.trim().is_empty())
        .unwrap_or(&tag_name)
        .to_string();
    let release_notes = v
        .get("body")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();
    let release_url = v
        .get("html_url")
        .and_then(Value::as_str)
        .unwrap_or(RELEASES_URL)
        .to_string();
    let published_at = v
        .get("published_at")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string();

    let mut download_url = None;
    let mut asset_name = None;
    let mut asset_size = None;
    if let Some(assets) = v.get("assets").and_then(Value::as_array) {
        if let Some(exe) = assets.iter().find(|a| {
            a.get("name")
                .and_then(Value::as_str)
                .map(|n| n.to_ascii_lowercase().ends_with(".exe"))
                .unwrap_or(false)
        }) {
            download_url = exe
                .get("browser_download_url")
                .and_then(Value::as_str)
                .map(String::from);
            asset_name = exe.get("name").and_then(Value::as_str).map(String::from);
            asset_size = exe.get("size").and_then(Value::as_u64);
        }
    }

    Ok(UpdateCheckResult {
        current_version: current_version.to_string(),
        has_update: is_newer_version(current_version, &latest_version),
        latest_version,
        tag_name,
        release_name,
        release_notes,
        release_url,
        download_url,
        asset_name,
        asset_size,
        published_at,
        checked_at: chrono::Local::now().format("%H:%M:%S").to_string(),
    })
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
struct WinHttpHandle(*mut std::ffi::c_void);

#[cfg(windows)]
impl Drop for WinHttpHandle {
    fn drop(&mut self) {
        if !self.0.is_null() {
            unsafe {
                windows_sys::Win32::Networking::WinHttp::WinHttpCloseHandle(self.0);
            }
        }
    }
}

#[cfg(windows)]
fn winhttp_get_https(host: &str, path: &str, user_agent: &str) -> Result<String, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpAddRequestHeaders, WinHttpConnect, WinHttpOpen, WinHttpOpenRequest,
        WinHttpQueryDataAvailable, WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse,
        WinHttpSendRequest, WinHttpSetTimeouts, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
        WINHTTP_ACCESS_TYPE_DEFAULT_PROXY, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    };

    let ua_w = to_wide(user_agent);
    let host_w = to_wide(host);
    let path_w = to_wide(path);
    let verb_w = to_wide("GET");
    let hdrs_w = to_wide("Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n");

    unsafe {
        let mut h_sess = WinHttpOpen(
            ua_w.as_ptr(),
            WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY,
            null(),
            null(),
            0,
        );
        if h_sess.is_null() {
            h_sess = WinHttpOpen(
                ua_w.as_ptr(),
                WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
                null(),
                null(),
                0,
            );
        }
        if h_sess.is_null() {
            return Err(format!("WinHTTP 初始化失败 (err={})", GetLastError()));
        }
        let _sess = WinHttpHandle(h_sess);
        let _ = WinHttpSetTimeouts(h_sess, 6000, 6000, 6000, 8000);

        let h_conn = WinHttpConnect(h_sess, host_w.as_ptr(), 443, 0);
        if h_conn.is_null() {
            return Err(format!("连接 GitHub 服务器失败 (err={})", GetLastError()));
        }
        let _conn = WinHttpHandle(h_conn);

        let h_req = WinHttpOpenRequest(
            h_conn,
            verb_w.as_ptr(),
            path_w.as_ptr(),
            null(),
            null(),
            null(),
            WINHTTP_FLAG_SECURE,
        );
        if h_req.is_null() {
            return Err(format!("创建 HTTPS 请求失败 (err={})", GetLastError()));
        }
        let _req = WinHttpHandle(h_req);

        let _ = WinHttpAddRequestHeaders(h_req, hdrs_w.as_ptr(), u32::MAX, WINHTTP_ADDREQ_FLAG_ADD);

        if WinHttpSendRequest(h_req, null(), 0, null_mut(), 0, 0, 0) == 0 {
            return Err(format!("发送更新检查请求失败，请检查网络连接 (err={})", GetLastError()));
        }
        if WinHttpReceiveResponse(h_req, null_mut()) == 0 {
            return Err(format!("等待 GitHub 响应超时 (err={})", GetLastError()));
        }

        let mut status_code: u32 = 0;
        let mut status_len: u32 = std::mem::size_of::<u32>() as u32;
        let ok_status = WinHttpQueryHeaders(
            h_req,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            &mut status_code as *mut u32 as *mut std::ffi::c_void,
            &mut status_len,
            null_mut(),
        );
        if ok_status != 0 && status_code != 200 {
            return Err(format!("GitHub API 返回 HTTP {status_code}"));
        }

        let mut body = Vec::with_capacity(8192);
        const MAX_BODY: usize = 1024 * 1024;
        loop {
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(h_req, &mut avail) == 0 || avail == 0 {
                break;
            }
            let to_read = (avail as usize).min(16384);
            let mut chunk = vec![0u8; to_read];
            let mut got: u32 = 0;
            if WinHttpReadData(
                h_req,
                chunk.as_mut_ptr() as *mut std::ffi::c_void,
                to_read as u32,
                &mut got,
            ) == 0
                || got == 0
            {
                break;
            }
            body.extend_from_slice(&chunk[..got as usize]);
            if body.len() >= MAX_BODY {
                break;
            }
        }
        String::from_utf8(body).map_err(|e| format!("响应 UTF-8 解码失败: {e}"))
    }
}

pub fn check_latest_release(current_version: &str) -> Result<UpdateCheckResult, String> {
    let path = format!("/repos/{REPO_OWNER}/{REPO_NAME}/releases/latest");
    let ua = format!("REDMAGIC-Control/{current_version}");
    #[cfg(windows)]
    let raw = winhttp_get_https("api.github.com", &path, &ua)?;
    #[cfg(not(windows))]
    let raw: String = {
        let _ = (path, ua);
        return Err("当前平台暂未实现更新检查".to_string());
    };
    parse_github_release_json(&raw, current_version)
}

/// 仅允许安全打开 https:// 网页链接，使用系统默认浏览器打开
pub fn open_external_url(url: &str) -> Result<(), String> {
    let trimmed = url.trim();
    if !trimmed.starts_with("https://") {
        return Err("仅允许打开 HTTPS 安全链接".to_string());
    }
    #[cfg(windows)]
    {
        use std::ptr::{null, null_mut};
        use windows_sys::Win32::UI::Shell::ShellExecuteW;
        use windows_sys::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

        let op_w = to_wide("open");
        let url_w = to_wide(trimmed);
        let rc = unsafe {
            ShellExecuteW(
                null_mut(),
                op_w.as_ptr(),
                url_w.as_ptr(),
                null(),
                null(),
                SW_SHOWNORMAL,
            )
        } as isize;
        if rc <= 32 {
            return Err(format!("无法启动默认浏览器 (code={rc})"));
        }
        Ok(())
    }
    #[cfg(not(windows))]
    {
        Err("当前平台暂不支持打开外部链接".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn semver_comparison_handles_v_prefix_and_multidigit() {
        assert!(is_newer_version("0.4.3", "v0.5.0"));
        assert!(is_newer_version("0.9.9", "v0.10.0"));
        assert!(!is_newer_version("0.5.0", "v0.5.0"));
        assert!(!is_newer_version("0.5.1", "v0.5.0"));
    }

    #[test]
    fn parses_github_release_payload_and_finds_exe_asset() {
        let sample = r###"{
            "tag_name": "v0.6.0",
            "name": "REDMAGIC Control v0.6.0",
            "body": "## Updates\n- New feature",
            "html_url": "https://github.com/COMICFLY/redmagic-control/releases/tag/v0.6.0",
            "published_at": "2026-11-20T15:00:00Z",
            "assets": [
                {
                    "name": "redmagic-control-v0.6.0-windows-x64.exe",
                    "size": 9899520,
                    "browser_download_url": "https://github.com/COMICFLY/redmagic-control/releases/download/v0.6.0/redmagic-control-v0.6.0-windows-x64.exe"
                }
            ]
        }"###;
        let res = parse_github_release_json(sample, "0.5.0").unwrap();
        assert!(res.has_update);
        assert_eq!(res.latest_version, "0.6.0");
        assert_eq!(
            res.asset_name.as_deref(),
            Some("redmagic-control-v0.6.0-windows-x64.exe")
        );
        assert_eq!(res.asset_size, Some(9899520));
    }
}
