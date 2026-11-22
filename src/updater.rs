//! Windows 原生 HTTPS 更新检查、流式下载校验与无感自替换升级（WinHTTP）：
//! - 复用 Windows 系统代理与 TLS 证书链，直接查询 GitHub Releases API (`COMICFLY/redmagic-control`)；
//! - 支持一键下载新版本 `.exe`、校验精确文件大小与 PE 可执行头 (`MZ` + `PE\0\0`)；
//! - 通过新下载程序自身的 `--apply-update` 模式等待旧进程退出、原子备份替换主程序并无缝重启（零 `.bat` 黑框、完整支持中文与空格路径）。

use std::fs::{self, File};
use std::io::{BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::Value;

pub const REPO_OWNER: &str = "COMICFLY";
pub const REPO_NAME: &str = "redmagic-control";
pub const REPO_URL: &str = "https://github.com/COMICFLY/redmagic-control";
pub const RELEASES_URL: &str = "https://github.com/COMICFLY/redmagic-control/releases";
pub const AUTHOR_URL: &str = "https://github.com/COMICFLY";

const MIN_EXE_BYTES: u64 = 1_000_000; // 至少 1 MB，杜绝 HTML 错误页误替换
const MAX_EXE_BYTES: u64 = 200 * 1024 * 1024; // 上限 200 MB

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

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateProgress {
    pub stage: String,
    pub downloaded: u64,
    pub total: Option<u64>,
    pub pct: u8,
    pub speed_bps: u64,
    pub message: String,
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

/// 解析并校验官方 GitHub Release 下载链接，仅允许 HTTPS 官方仓库或 GitHub Release CDN 域名
pub fn validate_download_url(url: &str) -> Result<(String, u16, String), String> {
    let trimmed = url.trim();
    let rest = trimmed
        .strip_prefix("https://")
        .ok_or_else(|| "更新下载地址必须为 HTTPS 安全链接".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(idx) => (&rest[..idx], &rest[idx..]),
        None => return Err("更新下载地址缺少文件路径".to_string()),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err("更新下载地址主机名无效".to_string());
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((h, p)) if !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()) => {
            let port_num: u16 = p.parse().map_err(|_| "更新地址端口无效".to_string())?;
            (h, port_num)
        }
        _ => (authority, 443u16),
    };
    let host_low = host.to_ascii_lowercase();
    let path_low = path.to_ascii_lowercase();
    let expected_prefix = format!(
        "/{}/{}/releases/download/",
        REPO_OWNER.to_ascii_lowercase(),
        REPO_NAME.to_ascii_lowercase()
    );
    let allowed = (host_low == "github.com" && path_low.starts_with(&expected_prefix))
        || host_low == "objects.githubusercontent.com"
        || host_low == "release-assets.githubusercontent.com";
    if !allowed {
        return Err(format!("拒绝从非官方来源下载更新: {host}"));
    }
    Ok((host.to_string(), port, path.to_string()))
}

pub fn verify_pe_header_reader<R: Read + Seek>(reader: &mut R) -> Result<(), String> {
    let mut dos = [0u8; 64];
    reader
        .seek(SeekFrom::Start(0))
        .map_err(|e| format!("读取更新文件头失败: {e}"))?;
    reader
        .read_exact(&mut dos)
        .map_err(|_| "下载文件不完整（DOS 头缺失）".to_string())?;
    if &dos[0..2] != b"MZ" {
        return Err("下载文件不是有效的 Windows 可执行程序 (缺少 MZ 头)".to_string());
    }
    let pe_off = u32::from_le_bytes([dos[0x3c], dos[0x3d], dos[0x3e], dos[0x3f]]) as u64;
    if !(0x40..=4096).contains(&pe_off) {
        return Err("下载文件 PE 偏移异常".to_string());
    }
    let mut sig = [0u8; 4];
    reader
        .seek(SeekFrom::Start(pe_off))
        .map_err(|_| "下载文件不完整（无法定位 PE 签名）".to_string())?;
    reader
        .read_exact(&mut sig)
        .map_err(|_| "下载文件不完整（PE 签名截断）".to_string())?;
    if &sig != b"PE\0\0" {
        return Err("下载文件不是有效的 Windows PE 程序".to_string());
    }
    Ok(())
}

pub fn verify_downloaded_exe(path: &Path, expected_size: Option<u64>) -> Result<u64, String> {
    let meta = fs::metadata(path).map_err(|e| format!("无法读取已下载文件属性: {e}"))?;
    let size = meta.len();
    if size < MIN_EXE_BYTES {
        return Err(format!(
            "下载文件体积异常偏小 ({size} B)，可能未下载完整"
        ));
    }
    if size > MAX_EXE_BYTES {
        return Err(format!("下载文件体积超出上限 ({size} B)"));
    }
    if let Some(exp) = expected_size.filter(|&n| n > 0) {
        if size != exp {
            return Err(format!(
                "文件大小校验不匹配（预期 {exp} B，实收 {size} B），请重试下载"
            ));
        }
    }
    let mut f = File::open(path).map_err(|e| format!("无法打开已下载文件进行校验: {e}"))?;
    verify_pe_header_reader(&mut f)?;
    Ok(size)
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_os_string();
    s.push(suffix);
    PathBuf::from(s)
}

pub fn staged_update_path(target_exe: &Path) -> PathBuf {
    append_suffix(target_exe, ".new")
}

pub fn temp_download_path(target_exe: &Path) -> PathBuf {
    append_suffix(target_exe, ".new.tmp")
}

pub fn backup_update_path(target_exe: &Path) -> PathBuf {
    append_suffix(target_exe, ".old")
}

pub fn swap_stage_path(target_exe: &Path) -> PathBuf {
    append_suffix(target_exe, ".stage")
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
fn open_winhttp_session(user_agent: &str) -> Result<WinHttpHandle, String> {
    use std::ptr::null;
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpOpen, WINHTTP_ACCESS_TYPE_AUTOMATIC_PROXY, WINHTTP_ACCESS_TYPE_DEFAULT_PROXY,
    };

    let ua_w = to_wide(user_agent);
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
        Ok(WinHttpHandle(h_sess))
    }
}

#[cfg(windows)]
fn winhttp_get_https(host: &str, path: &str, user_agent: &str) -> Result<String, String> {
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpAddRequestHeaders, WinHttpConnect, WinHttpOpenRequest, WinHttpQueryDataAvailable,
        WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
        WinHttpSetTimeouts, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE,
        WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    };

    let host_w = to_wide(host);
    let path_w = to_wide(path);
    let verb_w = to_wide("GET");
    let hdrs_w =
        to_wide("Accept: application/vnd.github+json\r\nX-GitHub-Api-Version: 2022-11-28\r\n");

    let sess = open_winhttp_session(user_agent)?;
    unsafe {
        let _ = WinHttpSetTimeouts(sess.0, 6000, 6000, 6000, 8000);

        let h_conn = WinHttpConnect(sess.0, host_w.as_ptr(), 443, 0);
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
            return Err(format!(
                "发送更新检查请求失败，请检查网络连接 (err={})",
                GetLastError()
            ));
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

#[cfg(windows)]
fn winhttp_download_to_file<F>(
    host: &str,
    port: u16,
    path: &str,
    user_agent: &str,
    dest_file: &Path,
    expected_size: Option<u64>,
    cancel: &AtomicBool,
    on_progress: &mut F,
) -> Result<u64, String>
where
    F: FnMut(UpdateProgress),
{
    use std::ptr::{null, null_mut};
    use windows_sys::Win32::Foundation::GetLastError;
    use windows_sys::Win32::Networking::WinHttp::{
        WinHttpAddRequestHeaders, WinHttpConnect, WinHttpOpenRequest, WinHttpQueryDataAvailable,
        WinHttpQueryHeaders, WinHttpReadData, WinHttpReceiveResponse, WinHttpSendRequest,
        WinHttpSetOption, WinHttpSetTimeouts, WINHTTP_ADDREQ_FLAG_ADD, WINHTTP_FLAG_SECURE,
        WINHTTP_OPTION_REDIRECT_POLICY, WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP,
        WINHTTP_QUERY_CONTENT_LENGTH, WINHTTP_QUERY_FLAG_NUMBER, WINHTTP_QUERY_STATUS_CODE,
    };

    on_progress(UpdateProgress {
        stage: "connecting".to_string(),
        downloaded: 0,
        total: expected_size,
        pct: 0,
        speed_bps: 0,
        message: "正在连接 GitHub 下载服务器…".to_string(),
    });

    let sess = open_winhttp_session(user_agent)?;
    let host_w = to_wide(host);
    let path_w = to_wide(path);
    let verb_w = to_wide("GET");
    let hdrs_w = to_wide("Accept: application/octet-stream, */*\r\n");

    unsafe {
        // 放宽流式下载超时，适应大文件与跨境代理链路
        let _ = WinHttpSetTimeouts(sess.0, 12_000, 15_000, 15_000, 60_000);
        let policy: u32 = WINHTTP_OPTION_REDIRECT_POLICY_DISALLOW_HTTPS_TO_HTTP;
        let _ = WinHttpSetOption(
            sess.0,
            WINHTTP_OPTION_REDIRECT_POLICY,
            &policy as *const u32 as *const std::ffi::c_void,
            std::mem::size_of::<u32>() as u32,
        );

        let h_conn = WinHttpConnect(sess.0, host_w.as_ptr(), port, 0);
        if h_conn.is_null() {
            return Err(format!("连接下载服务器失败 (err={})", GetLastError()));
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
            return Err(format!("创建下载请求失败 (err={})", GetLastError()));
        }
        let _req = WinHttpHandle(h_req);

        let _ = WinHttpAddRequestHeaders(h_req, hdrs_w.as_ptr(), u32::MAX, WINHTTP_ADDREQ_FLAG_ADD);

        if WinHttpSendRequest(h_req, null(), 0, null_mut(), 0, 0, 0) == 0 {
            return Err(format!(
                "无法连接 GitHub 下载更新，请检查网络或代理设置 (err={})",
                GetLastError()
            ));
        }
        if WinHttpReceiveResponse(h_req, null_mut()) == 0 {
            return Err(format!("等待下载响应超时 (err={})", GetLastError()));
        }

        let mut status_code: u32 = 0;
        let mut status_len: u32 = std::mem::size_of::<u32>() as u32;
        if WinHttpQueryHeaders(
            h_req,
            WINHTTP_QUERY_STATUS_CODE | WINHTTP_QUERY_FLAG_NUMBER,
            null(),
            &mut status_code as *mut u32 as *mut std::ffi::c_void,
            &mut status_len,
            null_mut(),
        ) != 0
            && status_code != 200
        {
            return Err(format!("下载服务器返回 HTTP {status_code}"));
        }

        let mut cl_buf = [0u16; 32];
        let mut cl_len = (cl_buf.len() * 2) as u32;
        let header_total = if WinHttpQueryHeaders(
            h_req,
            WINHTTP_QUERY_CONTENT_LENGTH,
            null(),
            cl_buf.as_mut_ptr() as *mut std::ffi::c_void,
            &mut cl_len,
            null_mut(),
        ) != 0
        {
            let chars = (cl_len as usize) / 2;
            String::from_utf16_lossy(&cl_buf[..chars])
                .trim()
                .parse::<u64>()
                .ok()
        } else {
            None
        };
        let total = header_total.or(expected_size).filter(|&n| n > 0);

        let file = File::create(dest_file).map_err(|e| {
            format!("无法写入更新临时文件（请确认程序所在目录有写入权限）: {e}")
        })?;
        let mut writer = BufWriter::with_capacity(64 * 1024, file);
        let mut downloaded: u64 = 0;
        let mut buf = vec![0u8; 64 * 1024];
        let started = Instant::now();
        let mut last_emit = Instant::now() - Duration::from_secs(1);

        loop {
            if cancel.load(Ordering::Relaxed) {
                return Err("已取消下载更新".to_string());
            }
            let mut avail: u32 = 0;
            if WinHttpQueryDataAvailable(h_req, &mut avail) == 0 {
                return Err(format!("下载流中断 (err={})", GetLastError()));
            }
            if avail == 0 {
                break;
            }
            let to_read = (avail as usize).min(buf.len());
            let mut got: u32 = 0;
            if WinHttpReadData(
                h_req,
                buf.as_mut_ptr() as *mut std::ffi::c_void,
                to_read as u32,
                &mut got,
            ) == 0
            {
                return Err(format!("读取更新数据失败 (err={})", GetLastError()));
            }
            if got == 0 {
                break;
            }
            writer
                .write_all(&buf[..got as usize])
                .map_err(|e| format!("写入本地更新文件失败: {e}"))?;
            downloaded += got as u64;
            if downloaded > MAX_EXE_BYTES {
                return Err("下载文件超出最大体积限制，已中止".to_string());
            }

            if last_emit.elapsed() >= Duration::from_millis(120) {
                last_emit = Instant::now();
                let elapsed = started.elapsed().as_secs_f64().max(0.05);
                let speed = (downloaded as f64 / elapsed) as u64;
                let pct = total
                    .map(|t| ((downloaded.saturating_mul(97)) / t).min(97) as u8)
                    .unwrap_or(50);
                on_progress(UpdateProgress {
                    stage: "downloading".to_string(),
                    downloaded,
                    total,
                    pct,
                    speed_bps: speed,
                    message: "正在下载新版本…".to_string(),
                });
            }
        }

        writer
            .flush()
            .map_err(|e| format!("刷新更新文件缓存失败: {e}"))?;
        if let Ok(inner) = writer.into_inner() {
            let _ = inner.sync_all();
        }
        Ok(downloaded)
    }
}

/// 下载并校验新版 `.exe`，成功时返回落盘在同目录的 `<exe>.new` 路径
pub fn download_and_stage_update<F>(
    download_url: &str,
    expected_size: Option<u64>,
    current_exe: &Path,
    current_version: &str,
    cancel: &AtomicBool,
    mut on_progress: F,
) -> Result<PathBuf, String>
where
    F: FnMut(UpdateProgress),
{
    let (host, port, path) = validate_download_url(download_url)?;
    let tmp_path = temp_download_path(current_exe);
    let staged_path = staged_update_path(current_exe);

    // 预检目录写入权限，避免下载完 10MB 才发现无权限写目录
    if let Err(e) = File::create(&tmp_path) {
        return Err(format!(
            "程序所在目录无写入权限（如位于 Program Files 请以管理员身份运行，或移至用户目录）：{e}"
        ));
    }
    let _ = fs::remove_file(&tmp_path);

    let ua = format!("REDMAGIC-Control/{current_version}");
    #[cfg(windows)]
    let dl_res = winhttp_download_to_file(
        &host,
        port,
        &path,
        &ua,
        &tmp_path,
        expected_size,
        cancel,
        &mut on_progress,
    );
    #[cfg(not(windows))]
    let dl_res: Result<u64, String> = {
        let _ = (host, port, path, ua, cancel);
        Err("当前平台暂不支持自动更新".to_string())
    };

    let downloaded = match dl_res {
        Ok(n) => n,
        Err(e) => {
            let _ = fs::remove_file(&tmp_path);
            return Err(e);
        }
    };

    on_progress(UpdateProgress {
        stage: "verifying".to_string(),
        downloaded,
        total: expected_size.or(Some(downloaded)),
        pct: 98,
        speed_bps: 0,
        message: "正在校验新版本程序完整性…".to_string(),
    });

    if let Err(e) = verify_downloaded_exe(&tmp_path, expected_size) {
        let _ = fs::remove_file(&tmp_path);
        return Err(e);
    }

    let _ = fs::remove_file(&staged_path);
    if let Err(e) = fs::rename(&tmp_path, &staged_path) {
        let _ = fs::remove_file(&tmp_path);
        return Err(format!("无法暂存更新文件: {e}"));
    }

    Ok(staged_path)
}

/// 启动刚下载并校验通过的 `.new` 程序执行原位替换并自动重启主程序（零批处理黑框）
pub fn launch_staged_updater(
    staged_exe: &Path,
    target_exe: &Path,
    current_pid: u32,
) -> Result<(), String> {
    Command::new(staged_exe)
        .arg("--apply-update")
        .arg("--target")
        .arg(target_exe)
        .arg("--pid")
        .arg(current_pid.to_string())
        .spawn()
        .map_err(|e| format!("无法启动更新安装程序: {e}"))?;
    Ok(())
}

/// 原子备份替换目标 `.exe`：先复制到同目录 `.stage`，旧文件移为 `.old`，再原子改名为目标文件；失败自动回滚
pub fn replace_target_atomically(source_exe: &Path, target_exe: &Path) -> Result<PathBuf, String> {
    let stage_path = swap_stage_path(target_exe);
    let backup_path = backup_update_path(target_exe);
    let mut last_err = String::new();

    for attempt in 0..35 {
        let _ = fs::remove_file(&stage_path);
        if let Err(e) = fs::copy(source_exe, &stage_path) {
            last_err = format!("复制新版本文件失败: {e}");
            std::thread::sleep(Duration::from_millis(150));
            continue;
        }
        let _ = fs::remove_file(&backup_path);

        let had_target = target_exe.exists();
        if had_target {
            if let Err(e) = fs::rename(target_exe, &backup_path) {
                last_err = format!("等待旧程序释放文件锁失败 (attempt {attempt}): {e}");
                let _ = fs::remove_file(&stage_path);
                std::thread::sleep(Duration::from_millis(150));
                continue;
            }
        }

        match fs::rename(&stage_path, target_exe) {
            Ok(()) => return Ok(backup_path),
            Err(e) => {
                last_err = format!("原子替换主程序失败: {e}");
                if had_target && backup_path.exists() {
                    let _ = fs::rename(&backup_path, target_exe);
                }
                let _ = fs::remove_file(&stage_path);
                std::thread::sleep(Duration::from_millis(150));
            }
        }
    }

    Err(last_err)
}

#[cfg(windows)]
fn wait_for_pid_exit(pid: u32, timeout_ms: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{
        OpenProcess, WaitForSingleObject, PROCESS_SYNCHRONIZE,
    };

    if pid == 0 {
        return;
    }
    unsafe {
        let h = OpenProcess(PROCESS_SYNCHRONIZE, 0, pid);
        if !h.is_null() {
            let _ = WaitForSingleObject(h, timeout_ms);
            CloseHandle(h);
        }
    }
}

#[cfg(not(windows))]
fn wait_for_pid_exit(_pid: u32, _timeout_ms: u32) {}

fn log_update_error(msg: &str) {
    let log_path = std::env::temp_dir().join("redmagic-control-update.log");
    if let Ok(mut f) = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(log_path)
    {
        let _ = writeln!(
            f,
            "[{}] {msg}",
            chrono::Local::now().format("%Y-%m-%d %H:%M:%S")
        );
    }
}

/// 若当前进程以 `--apply-update --target <exe> --pid <pid>` 启动，执行原位替换并重启新版主程序，返回 `true` 指示 `main` 直接退出
pub fn run_apply_update_mode(args: &[String]) -> bool {
    if !args.iter().any(|a| a == "--apply-update") {
        return false;
    }

    let mut target: Option<PathBuf> = None;
    let mut parent_pid: u32 = 0;
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--target" && i + 1 < args.len() {
            target = Some(PathBuf::from(&args[i + 1]));
            i += 2;
        } else if args[i] == "--pid" && i + 1 < args.len() {
            parent_pid = args[i + 1].parse().unwrap_or(0);
            i += 2;
        } else {
            i += 1;
        }
    }

    let Some(target_exe) = target else {
        log_update_error("--apply-update 缺少 --target 参数");
        return true;
    };
    let Ok(self_exe) = std::env::current_exe() else {
        log_update_error("无法获取当前更新程序路径");
        return true;
    };

    wait_for_pid_exit(parent_pid, 15_000);
    std::thread::sleep(Duration::from_millis(120));

    match replace_target_atomically(&self_exe, &target_exe) {
        Ok(backup_path) => {
            let _ = Command::new(&target_exe)
                .arg("--cleanup-update")
                .arg(&self_exe)
                .arg(&backup_path)
                .spawn();
        }
        Err(e) => {
            log_update_error(&format!("自动更新替换失败: {e}"));
            // 替换失败时重新拉起原程序，避免用户界面消失
            let _ = Command::new(&target_exe).spawn();
        }
    }
    true
}

/// 异步清理上次自动更新遗留的 `.new` / `.old` / `.stage` / `.new.tmp` 临时文件，保持程序目录整洁
pub fn cleanup_update_artifacts_async(args: &[String]) {
    let mut paths: Vec<PathBuf> = Vec::new();
    if let Some(pos) = args.iter().position(|a| a == "--cleanup-update") {
        for arg in &args[pos + 1..] {
            if !arg.starts_with("--") && !arg.is_empty() {
                paths.push(PathBuf::from(arg));
            }
        }
    }
    if let Ok(cur) = std::env::current_exe() {
        paths.push(staged_update_path(&cur));
        paths.push(temp_download_path(&cur));
        paths.push(backup_update_path(&cur));
        paths.push(swap_stage_path(&cur));
    }

    // 仅在确实存在需要清理的临时文件时才启动短生命周期清理线程
    let existing: Vec<PathBuf> = paths.into_iter().filter(|p| p.exists()).collect();
    if existing.is_empty() {
        return;
    }

    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        for _ in 0..12 {
            let mut remaining = false;
            for p in &existing {
                if p.exists() && fs::remove_file(p).is_err() {
                    remaining = true;
                }
            }
            if !remaining {
                break;
            }
            std::thread::sleep(Duration::from_millis(300));
        }
    });
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

    #[test]
    fn validates_only_official_github_https_download_urls() {
        assert!(validate_download_url(
            "https://github.com/COMICFLY/redmagic-control/releases/download/v0.7.4/redmagic-control-v0.7.4-windows-x64.exe"
        )
        .is_ok());
        assert!(validate_download_url(
            "https://release-assets.githubusercontent.com/github-production-release-asset/12345"
        )
        .is_ok());
        assert!(validate_download_url("http://github.com/COMICFLY/redmagic-control/releases/download/v0.7.4/a.exe").is_err());
        assert!(validate_download_url("https://evil.example.com/redmagic-control.exe").is_err());
        assert!(validate_download_url("https://github.com/other/repo/releases/download/v1.0.0/a.exe").is_err());
    }

    #[test]
    fn verifies_pe_header_and_atomic_replace_with_backup() {
        let mut fake_pe = vec![0u8; 256];
        fake_pe[0] = b'M';
        fake_pe[1] = b'Z';
        fake_pe[0x3c..0x40].copy_from_slice(&0x80u32.to_le_bytes());
        fake_pe[0x80..0x84].copy_from_slice(b"PE\0\0");
        let mut cur = std::io::Cursor::new(&fake_pe);
        assert!(verify_pe_header_reader(&mut cur).is_ok());

        let html = b"<!DOCTYPE html><html>404 Not Found</html>".to_vec();
        let mut bad = std::io::Cursor::new(&html);
        assert!(verify_pe_header_reader(&mut bad).is_err());

        let dir = std::env::temp_dir().join(format!(
            "rm-updater-test-{}",
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let target = dir.join("redmagic-control.exe");
        let staged = staged_update_path(&target);
        fs::write(&target, b"OLD_BINARY_V1").unwrap();
        fs::write(&staged, b"NEW_BINARY_V2").unwrap();

        let backup = replace_target_atomically(&staged, &target).unwrap();
        assert_eq!(fs::read(&target).unwrap(), b"NEW_BINARY_V2");
        assert_eq!(fs::read(&backup).unwrap(), b"OLD_BINARY_V1");
        assert!(!swap_stage_path(&target).exists());
        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    #[ignore]
    fn live_winhttp_downloads_and_verifies_latest_github_release() {
        let info = check_latest_release("0.0.1").expect("check_latest_release failed");
        let url = info.download_url.expect("missing exe asset url");
        let dir = std::env::temp_dir().join(format!("rm-live-dl-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let fake_exe = dir.join("redmagic-control.exe");
        fs::write(&fake_exe, b"old").unwrap();

        let cancel = AtomicBool::new(false);
        let mut last_pct = 0u8;
        let mut events = 0usize;
        let staged = download_and_stage_update(
            &url,
            info.asset_size,
            &fake_exe,
            "0.0.1",
            &cancel,
            |p| {
                events += 1;
                last_pct = p.pct;
            },
        )
        .expect("live download_and_stage_update failed");

        assert!(staged.exists());
        assert!(events >= 2);
        assert!(last_pct >= 98);
        let got_size = fs::metadata(&staged).unwrap().len();
        if let Some(exp) = info.asset_size {
            assert_eq!(got_size, exp);
        }
        println!(
            "LIVE_DL_OK: tag={} bytes={} events={} staged={}",
            info.tag_name,
            got_size,
            events,
            staged.display()
        );
        let _ = fs::remove_dir_all(&dir);
    }
}
