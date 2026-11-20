//! 纯 Rust 本地可编程控灯桥接服务（零额外网络依赖，严格绑定 127.0.0.1 回环）：
//! 1. UDP 高速二进制/JSON 通道（默认 127.0.0.1:27650）
//!    - 二进制帧: "RMGB" (4B) + flags (1B: bit0=378B 键色, bit1=60B Logo色) + RGB payload
//!    - 或裸 378B（126键RGB） / 438B（378B 键 + 60B Logo）
//!    - 或 JSON 文本: {"fill":[r,g,b]} / {"keys":[[slot,r,g,b],...],"logo":[[idx,r,g,b],...]} / {"rgb":[...]}
//! 2. HTTP + RFC 6455 WebSocket 通道（默认 127.0.0.1:27651）
//!    - GET  /api/status
//!    - POST /api/rgb  (JSON 或二进制帧)
//!    - GET  /ws       (WebSocket 实时双向推送 Text JSON / Binary 帧)

use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream, UdpSocket};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use base64::Engine;
use serde::Serialize;
use serde_json::{json, Value};

use crate::devices::keyboard::CUSTOM_RGB_LEN;
use crate::devices::manager::DeviceManager;
use crate::protocol::LOGO_RGB_LEN;

pub const DEFAULT_UDP_PORT: u16 = 27650;
pub const DEFAULT_HTTP_PORT: u16 = 27651;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ExternalServerStatus {
    pub running: bool,
    pub device_id: Option<String>,
    pub udp_port: u16,
    pub http_port: u16,
    pub packets: u64,
    pub fps: f64,
    pub last_source: String,
    pub error: Option<String>,
}

struct FrameBuffers {
    keys: [u8; CUSTOM_RGB_LEN],
    logo: [u8; LOGO_RGB_LEN],
    has_frame: bool,
    last_source: String,
    window_start: Instant,
    window_frames: u32,
    fps: f64,
    last_error: Option<String>,
}

impl Default for FrameBuffers {
    fn default() -> Self {
        Self {
            keys: [0u8; CUSTOM_RGB_LEN],
            logo: [0u8; LOGO_RGB_LEN],
            has_frame: false,
            last_source: String::new(),
            window_start: Instant::now(),
            window_frames: 0,
            fps: 0.0,
            last_error: None,
        }
    }
}

#[derive(Clone)]
pub struct ExternalRgbServer {
    running: Arc<AtomicBool>,
    packets: Arc<AtomicU64>,
    config: Arc<Mutex<(Option<String>, u16, u16, bool)>>,
    frames: Arc<Mutex<FrameBuffers>>,
}

impl ExternalRgbServer {
    pub fn new() -> Self {
        Self {
            running: Arc::new(AtomicBool::new(false)),
            packets: Arc::new(AtomicU64::new(0)),
            config: Arc::new(Mutex::new((None, DEFAULT_UDP_PORT, DEFAULT_HTTP_PORT, true))),
            frames: Arc::new(Mutex::new(FrameBuffers::default())),
        }
    }

    pub fn is_running(&self) -> bool {
        self.running.load(Ordering::SeqCst)
    }

    pub fn status(&self) -> ExternalServerStatus {
        let (device_id, udp_port, http_port, _) = self.config.lock().unwrap().clone();
        let fb = self.frames.lock().unwrap();
        let fps = if fb.window_start.elapsed() > Duration::from_secs(2) {
            0.0
        } else {
            fb.fps
        };
        ExternalServerStatus {
            running: self.running.load(Ordering::SeqCst),
            device_id,
            udp_port,
            http_port,
            packets: self.packets.load(Ordering::Relaxed),
            fps,
            last_source: fb.last_source.clone(),
            error: fb.last_error.clone(),
        }
    }

    pub fn latest_frame(&self) -> Option<(Vec<u8>, Vec<u8>)> {
        let fb = self.frames.lock().unwrap();
        if !fb.has_frame {
            return None;
        }
        Some((fb.keys.to_vec(), fb.logo.to_vec()))
    }

    pub fn start(
        &self,
        manager: DeviceManager,
        device_id: String,
        udp_port: u16,
        http_port: u16,
        include_logo: bool,
    ) -> Result<ExternalServerStatus, String> {
        if self.running.load(Ordering::SeqCst) {
            return Ok(self.status());
        }

        // 先尝试绑定端口，确保端口可用后再激活设备直推模式
        let udp = UdpSocket::bind(("127.0.0.1", udp_port))
            .map_err(|e| format!("UDP 127.0.0.1:{udp_port} 绑定失败: {e}"))?;
        let _ = udp.set_read_timeout(Some(Duration::from_millis(120)));

        let tcp = TcpListener::bind(("127.0.0.1", http_port))
            .map_err(|e| format!("HTTP/WS 127.0.0.1:{http_port} 绑定失败: {e}"))?;
        let _ = tcp.set_nonblocking(true);

        // 激活键盘实时 RAM 推流模式
        manager.start_direct_rgb(&device_id, include_logo)?;

        *self.config.lock().unwrap() = (Some(device_id.clone()), udp_port, http_port, include_logo);
        self.packets.store(0, Ordering::SeqCst);
        {
            let mut fb = self.frames.lock().unwrap();
            *fb = FrameBuffers::default();
        }
        self.running.store(true, Ordering::SeqCst);

        // 1. UDP 接收线程
        {
            let running = self.running.clone();
            let packets = self.packets.clone();
            let frames = self.frames.clone();
            let mgr = manager.clone();
            let dev_id = device_id.clone();
            thread::spawn(move || {
                let mut buf = [0u8; 4096];
                while running.load(Ordering::SeqCst) {
                    match udp.recv_from(&mut buf) {
                        Ok((n, _)) if n > 0 => {
                            let _ = handle_payload(
                                &buf[..n],
                                "UDP",
                                &mgr,
                                &dev_id,
                                include_logo,
                                &packets,
                                &frames,
                            );
                        }
                        Ok(_) => {}
                        Err(ref e)
                            if e.kind() == std::io::ErrorKind::WouldBlock
                                || e.kind() == std::io::ErrorKind::TimedOut => {}
                        Err(_) => thread::sleep(Duration::from_millis(20)),
                    }
                }
            });
        }

        // 2. HTTP / WebSocket 监听线程
        {
            let running = self.running.clone();
            let packets = self.packets.clone();
            let frames = self.frames.clone();
            let mgr = manager.clone();
            let dev_id = device_id.clone();
            let srv_ref = self.clone();
            thread::spawn(move || {
                while running.load(Ordering::SeqCst) {
                    match tcp.accept() {
                        Ok((stream, _)) => {
                            let r = running.clone();
                            let p = packets.clone();
                            let f = frames.clone();
                            let m = mgr.clone();
                            let d = dev_id.clone();
                            let s = srv_ref.clone();
                            thread::spawn(move || {
                                handle_tcp_client(stream, r, p, f, m, d, include_logo, s);
                            });
                        }
                        Err(ref e) if e.kind() == std::io::ErrorKind::WouldBlock => {
                            thread::sleep(Duration::from_millis(20));
                        }
                        Err(_) => thread::sleep(Duration::from_millis(30)),
                    }
                }
            });
        }

        Ok(self.status())
    }

    pub fn stop(&self, manager: &DeviceManager) -> ExternalServerStatus {
        let was_running = self.running.swap(false, Ordering::SeqCst);
        let dev_id = self.config.lock().unwrap().0.clone();
        if was_running {
            if let Some(id) = dev_id {
                let _ = manager.stop_direct_rgb(&id);
            }
        }
        self.status()
    }
}

/// 解析二进制（RMGB / 裸 378B / 438B）或 JSON 帧并推送到键盘 RAM
pub fn apply_external_payload(
    raw: &[u8],
    keys: &mut [u8; CUSTOM_RGB_LEN],
    logo: &mut [u8; LOGO_RGB_LEN],
) -> Result<(bool, bool), String> {
    // 1. RMGB 魔数协议
    if raw.len() >= 5 && &raw[0..4] == b"RMGB" {
        let flags = raw[4];
        let mut cur = 5usize;
        let mut has_keys = false;
        let mut has_logo = false;
        if (flags & 0x01) != 0 {
            if raw.len() < cur + CUSTOM_RGB_LEN {
                return Err("RMGB 键色长度不足 378B".to_string());
            }
            keys.copy_from_slice(&raw[cur..cur + CUSTOM_RGB_LEN]);
            cur += CUSTOM_RGB_LEN;
            has_keys = true;
        }
        if (flags & 0x02) != 0 {
            if raw.len() < cur + LOGO_RGB_LEN {
                return Err("RMGB Logo 长度不足 60B".to_string());
            }
            logo.copy_from_slice(&raw[cur..cur + LOGO_RGB_LEN]);
            has_logo = true;
        }
        return Ok((has_keys, has_logo));
    }

    // 2. 裸二进制 378B (主键) 或 438B (主键 + Logo)
    if raw.len() == CUSTOM_RGB_LEN {
        keys.copy_from_slice(raw);
        return Ok((true, false));
    }
    if raw.len() == CUSTOM_RGB_LEN + LOGO_RGB_LEN {
        keys.copy_from_slice(&raw[..CUSTOM_RGB_LEN]);
        logo.copy_from_slice(&raw[CUSTOM_RGB_LEN..]);
        return Ok((true, true));
    }

    // 3. JSON 文本协议
    let val: Value = serde_json::from_slice(raw).map_err(|e| format!("JSON 解析失败: {e}"))?;
    let mut has_keys = false;
    let mut has_logo = false;

    if let Some(fill) = val.get("fill").and_then(Value::as_array) {
        if fill.len() >= 3 {
            let r = fill[0].as_u64().unwrap_or(0).min(255) as u8;
            let g = fill[1].as_u64().unwrap_or(0).min(255) as u8;
            let b = fill[2].as_u64().unwrap_or(0).min(255) as u8;
            for c in keys.chunks_exact_mut(3) {
                c[0] = r;
                c[1] = g;
                c[2] = b;
            }
            for c in logo.chunks_exact_mut(3) {
                c[0] = r;
                c[1] = g;
                c[2] = b;
            }
            has_keys = true;
            has_logo = true;
        }
    }

    if let Some(arr) = val.get("rgb").and_then(Value::as_array) {
        for (i, v) in arr.iter().take(CUSTOM_RGB_LEN).enumerate() {
            keys[i] = v.as_u64().unwrap_or(0).min(255) as u8;
        }
        has_keys = true;
    }

    if let Some(arr) = val.get("keys").and_then(Value::as_array) {
        for item in arr {
            if let Some(tuple) = item.as_array() {
                if tuple.len() >= 4 {
                    let slot = tuple[0].as_u64().unwrap_or(999) as usize;
                    if slot < 126 {
                        let o = slot * 3;
                        keys[o] = tuple[1].as_u64().unwrap_or(0).min(255) as u8;
                        keys[o + 1] = tuple[2].as_u64().unwrap_or(0).min(255) as u8;
                        keys[o + 2] = tuple[3].as_u64().unwrap_or(0).min(255) as u8;
                        has_keys = true;
                    }
                }
            }
        }
    }

    if let Some(arr) = val.get("logoRgb").and_then(Value::as_array) {
        for (i, v) in arr.iter().take(LOGO_RGB_LEN).enumerate() {
            logo[i] = v.as_u64().unwrap_or(0).min(255) as u8;
        }
        has_logo = true;
    }

    if let Some(arr) = val.get("logo").and_then(Value::as_array) {
        for item in arr {
            if let Some(tuple) = item.as_array() {
                if tuple.len() >= 4 {
                    let idx = tuple[0].as_u64().unwrap_or(999) as usize;
                    if idx < 20 {
                        let o = idx * 3;
                        logo[o] = tuple[1].as_u64().unwrap_or(0).min(255) as u8;
                        logo[o + 1] = tuple[2].as_u64().unwrap_or(0).min(255) as u8;
                        logo[o + 2] = tuple[3].as_u64().unwrap_or(0).min(255) as u8;
                        has_logo = true;
                    }
                }
            }
        }
    }

    if !has_keys && !has_logo {
        return Err("未包含有效 RGB 字段（支持 fill / rgb / keys / logo / logoRgb）".to_string());
    }
    Ok((has_keys, has_logo))
}

fn handle_payload(
    raw: &[u8],
    source: &str,
    manager: &DeviceManager,
    device_id: &str,
    include_logo: bool,
    packets: &Arc<AtomicU64>,
    frames: &Arc<Mutex<FrameBuffers>>,
) -> Result<(), String> {
    let (keys_copy, logo_copy, send_keys, send_logo) = {
        let mut guard = frames.lock().unwrap();
        let fb = &mut *guard;
        let (hk, hl) = match apply_external_payload(raw, &mut fb.keys, &mut fb.logo) {
            Ok(flags) => flags,
            Err(e) => {
                fb.last_error = Some(e.clone());
                return Err(e);
            }
        };
        fb.has_frame = true;
        fb.last_source = source.to_string();
        fb.last_error = None;
        fb.window_frames += 1;
        let elapsed = fb.window_start.elapsed().as_secs_f64();
        if elapsed >= 0.5 {
            fb.fps = ((fb.window_frames as f64 / elapsed) * 10.0).round() / 10.0;
            fb.window_frames = 0;
            fb.window_start = Instant::now();
        }
        (fb.keys, fb.logo, hk, hl && include_logo)
    };

    packets.fetch_add(1, Ordering::Relaxed);
    let k_slice: &[u8] = if send_keys { &keys_copy } else { &[] };
    let l_opt: Option<&[u8]> = if send_logo { Some(&logo_copy) } else { None };
    if let Err(e) = manager.stream_rgb_frame(device_id, k_slice, l_opt) {
        if let Ok(mut fb) = frames.lock() {
            fb.last_error = Some(e.clone());
        }
        return Err(e);
    }
    Ok(())
}

fn handle_tcp_client(
    mut stream: TcpStream,
    running: Arc<AtomicBool>,
    packets: Arc<AtomicU64>,
    frames: Arc<Mutex<FrameBuffers>>,
    manager: DeviceManager,
    device_id: String,
    include_logo: bool,
    server: ExternalRgbServer,
) {
    let _ = stream.set_nonblocking(false);
    let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
    let _ = stream.set_write_timeout(Some(Duration::from_secs(2)));

    let mut req_buf = vec![0u8; 8192];
    let mut total = 0usize;
    let header_end;
    loop {
        match stream.read(&mut req_buf[total..]) {
            Ok(0) => return,
            Ok(n) => {
                total += n;
                if let Some(pos) = find_bytes(&req_buf[..total], b"\r\n\r\n") {
                    header_end = pos + 4;
                    break;
                }
                if total >= req_buf.len() {
                    return;
                }
            }
            Err(_) => return,
        }
    }

    let head_str = String::from_utf8_lossy(&req_buf[..header_end]);
    let mut lines = head_str.lines();
    let first = lines.next().unwrap_or("");
    let mut parts = first.split_whitespace();
    let method = parts.next().unwrap_or("");
    let path = parts.next().unwrap_or("/");

    let mut content_len = 0usize;
    let mut ws_key: Option<String> = None;
    let mut is_upgrade = false;
    for line in lines {
        if let Some((k, v)) = line.split_once(':') {
            let kl = k.trim().to_ascii_lowercase();
            let vt = v.trim();
            if kl == "content-length" {
                content_len = vt.parse().unwrap_or(0).min(65536);
            } else if kl == "sec-websocket-key" {
                ws_key = Some(vt.to_string());
            } else if kl == "upgrade" && vt.eq_ignore_ascii_case("websocket") {
                is_upgrade = true;
            }
        }
    }

    // WebSocket 握手升级 (RFC 6455)
    if is_upgrade && ws_key.is_some() {
        let accept = ws_accept_key(ws_key.as_deref().unwrap());
        let resp = format!(
            "HTTP/1.1 101 Switching Protocols\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Accept: {accept}\r\n\r\n"
        );
        if stream.write_all(resp.as_bytes()).is_err() {
            return;
        }
        let _ = stream.set_read_timeout(Some(Duration::from_millis(200)));
        run_ws_loop(stream, running, packets, frames, manager, device_id, include_logo);
        return;
    }

    // 普通 HTTP CORS 预检
    if method == "OPTIONS" {
        let _ = write_http(&mut stream, "204 No Content", "text/plain", b"");
        return;
    }

    if method == "GET" && path.starts_with("/api/status") {
        let body = serde_json::to_vec(&server.status()).unwrap_or_default();
        let _ = write_http(&mut stream, "200 OK", "application/json", &body);
        return;
    }

    if method == "POST" && path.starts_with("/api/rgb") {
        let mut body = req_buf[header_end..total].to_vec();
        while body.len() < content_len {
            let mut tmp = vec![0u8; (content_len - body.len()).min(4096)];
            match stream.read(&mut tmp) {
                Ok(0) | Err(_) => break,
                Ok(n) => body.extend_from_slice(&tmp[..n]),
            }
        }
        if body.len() > content_len && content_len > 0 {
            body.truncate(content_len);
        }
        match handle_payload(&body, "HTTP", &manager, &device_id, include_logo, &packets, &frames) {
            Ok(_) => {
                let res = serde_json::to_vec(&json!({ "ok": true })).unwrap();
                let _ = write_http(&mut stream, "200 OK", "application/json", &res);
            }
            Err(e) => {
                let res = serde_json::to_vec(&json!({ "ok": false, "error": e })).unwrap();
                let _ = write_http(&mut stream, "400 Bad Request", "application/json", &res);
            }
        }
        return;
    }

    let info = serde_json::to_vec(&json!({
        "service": "REDMAGIC Control Local RGB Bridge",
        "endpoints": ["GET /api/status", "POST /api/rgb", "WS /ws"],
        "udpPort": server.status().udp_port,
    }))
    .unwrap();
    let _ = write_http(&mut stream, "200 OK", "application/json", &info);
}

fn write_http(stream: &mut TcpStream, status: &str, ctype: &str, body: &[u8]) -> std::io::Result<()> {
    let head = format!(
        "HTTP/1.1 {status}\r\nContent-Type: {ctype}\r\nContent-Length: {}\r\nAccess-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\nAccess-Control-Allow-Headers: Content-Type\r\nConnection: close\r\n\r\n",
        body.len()
    );
    stream.write_all(head.as_bytes())?;
    if !body.is_empty() {
        stream.write_all(body)?;
    }
    Ok(())
}

fn run_ws_loop(
    mut stream: TcpStream,
    running: Arc<AtomicBool>,
    packets: Arc<AtomicU64>,
    frames: Arc<Mutex<FrameBuffers>>,
    manager: DeviceManager,
    device_id: String,
    include_logo: bool,
) {
    while running.load(Ordering::SeqCst) {
        let mut h2 = [0u8; 2];
        match stream.read_exact(&mut h2) {
            Ok(()) => {}
            Err(ref e)
                if e.kind() == std::io::ErrorKind::WouldBlock
                    || e.kind() == std::io::ErrorKind::TimedOut =>
            {
                continue;
            }
            Err(_) => break,
        }
        let opcode = h2[0] & 0x0f;
        let masked = (h2[1] & 0x80) != 0;
        let mut len = (h2[1] & 0x7f) as usize;
        if len == 126 {
            let mut ext = [0u8; 2];
            if stream.read_exact(&mut ext).is_err() {
                break;
            }
            len = u16::from_be_bytes(ext) as usize;
        } else if len == 127 {
            let mut ext = [0u8; 8];
            if stream.read_exact(&mut ext).is_err() {
                break;
            }
            len = (u64::from_be_bytes(ext) as usize).min(65536);
        }
        if len > 65536 {
            break;
        }
        let mut mask = [0u8; 4];
        if masked && stream.read_exact(&mut mask).is_err() {
            break;
        }
        let mut payload = vec![0u8; len];
        if len > 0 && stream.read_exact(&mut payload).is_err() {
            break;
        }
        if masked {
            for (i, b) in payload.iter_mut().enumerate() {
                *b ^= mask[i & 3];
            }
        }
        match opcode {
            0x08 => break, // Close
            0x09 => {
                // Ping -> Pong
                let mut pong = vec![0x8a, payload.len().min(125) as u8];
                pong.extend_from_slice(&payload[..payload.len().min(125)]);
                let _ = stream.write_all(&pong);
            }
            0x01 | 0x02 => {
                let _ = handle_payload(
                    &payload,
                    "WS",
                    &manager,
                    &device_id,
                    include_logo,
                    &packets,
                    &frames,
                );
            }
            _ => {}
        }
    }
}

fn find_bytes(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}

/// 纯 Rust RFC 6455 Sec-WebSocket-Accept (SHA-1 + Base64，零外部加密库)
fn ws_accept_key(key: &str) -> String {
    const GUID: &str = "258EAFA5-E914-47DA-95CA-C5AB0DC85B11";
    let mut input = Vec::with_capacity(key.len() + GUID.len());
    input.extend_from_slice(key.as_bytes());
    input.extend_from_slice(GUID.as_bytes());
    let digest = sha1(&input);
    base64::engine::general_purpose::STANDARD.encode(digest)
}

fn sha1(data: &[u8]) -> [u8; 20] {
    let mut h: [u32; 5] = [0x67452301, 0xEFCDAB89, 0x98BADCFE, 0x10325476, 0xC3D2E1F0];
    let bit_len = (data.len() as u64) * 8;
    let mut msg = data.to_vec();
    msg.push(0x80);
    while (msg.len() % 64) != 56 {
        msg.push(0);
    }
    msg.extend_from_slice(&bit_len.to_be_bytes());

    for chunk in msg.chunks_exact(64) {
        let mut w = [0u32; 80];
        for i in 0..16 {
            w[i] = u32::from_be_bytes([
                chunk[i * 4],
                chunk[i * 4 + 1],
                chunk[i * 4 + 2],
                chunk[i * 4 + 3],
            ]);
        }
        for i in 16..80 {
            w[i] = (w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16]).rotate_left(1);
        }
        let [mut a, mut b, mut c, mut d, mut e] = h;
        for i in 0..80 {
            let (f, k) = match i {
                0..=19 => ((b & c) | ((!b) & d), 0x5A827999u32),
                20..=39 => (b ^ c ^ d, 0x6ED9EBA1u32),
                40..=59 => ((b & c) | (b & d) | (c & d), 0x8F1BBCDCu32),
                _ => (b ^ c ^ d, 0xCA62C1D6u32),
            };
            let temp = a
                .rotate_left(5)
                .wrapping_add(f)
                .wrapping_add(e)
                .wrapping_add(k)
                .wrapping_add(w[i]);
            e = d;
            d = c;
            c = b.rotate_left(30);
            b = a;
            a = temp;
        }
        h[0] = h[0].wrapping_add(a);
        h[1] = h[1].wrapping_add(b);
        h[2] = h[2].wrapping_add(c);
        h[3] = h[3].wrapping_add(d);
        h[4] = h[4].wrapping_add(e);
    }

    let mut out = [0u8; 20];
    for (i, word) in h.iter().enumerate() {
        out[i * 4..i * 4 + 4].copy_from_slice(&word.to_be_bytes());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rfc6455_accept_key_matches_spec() {
        assert_eq!(
            ws_accept_key("dGhlIHNhbXBsZSBub25jZQ=="),
            "s3pPLMBiTxaQ9kYGzzhZRbK+xOo="
        );
    }

    #[test]
    fn parses_rmgb_binary_and_json_payloads() {
        let mut keys = [0u8; CUSTOM_RGB_LEN];
        let mut logo = [0u8; LOGO_RGB_LEN];

        let mut bin = Vec::from(&b"RMGB"[..]);
        bin.push(0x03);
        bin.extend(vec![12u8; CUSTOM_RGB_LEN]);
        bin.extend(vec![34u8; LOGO_RGB_LEN]);
        let (hk, hl) = apply_external_payload(&bin, &mut keys, &mut logo).unwrap();
        assert!(hk && hl);
        assert_eq!(keys[0], 12);
        assert_eq!(logo[0], 34);

        let j = br#"{"keys":[[0,229,57,69]],"logo":[[1,0,255,128]]}"#;
        let (hk2, hl2) = apply_external_payload(j, &mut keys, &mut logo).unwrap();
        assert!(hk2 && hl2);
        assert_eq!(&keys[0..3], &[229, 57, 69]);
        assert_eq!(&logo[3..6], &[0, 255, 128]);
    }
}
