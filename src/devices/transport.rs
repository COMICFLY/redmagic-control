//! 统一设备传输层：
//! - WinHid：纯 Rust Win32 Overlapped HID（USB 有线 56B / 2.4G 24B + HidD_FlushQueue + OP 0xaa 唤醒）
//! - BluetoothReadOnly：标准 Windows BLE HOGP 只读通道（通过 CfgMgr32 同步 0x180F 实时电量）
//! - Mock：完整离线仿真设备（支持模拟有线 / 2.4G 24B 分包+休眠+len=0 回包 / 蓝牙只读）

use std::thread;
use std::time::{Duration, Instant};

use crate::devices::bluetooth_win::scan_bluetooth_devices;
use crate::devices::catalog::{ConnectionMode, DeviceKind};
use crate::devices::hid_win::{enumerate_hid_devices, WinHidDevice};
use crate::protocol::frame::{build_frame, is_wireless_nack, matches_header, parse_frame, HidFrame};
use crate::protocol::usage::{build_default_kb_rom, build_default_ms_rom};
use crate::protocol::{
    CHUNK_SIZE, CHUNK_SIZE_24G, HEARTBEAT, LOGO_RGB_LEN, POLL_STATUS, PREPARE_SCREEN_IMG,
    READ_CONFIG, READ_CUSTOM_RGB, READ_INFO, READ_KEYMAP_ROM, READ_KEYMAP_USER, STREAM_KEY_RGB,
    STREAM_LOGO_RGB, TX_BEGIN, TX_COMMIT, WRITE_CONFIG, WRITE_CUSTOM_RGB, WRITE_KEYMAP,
    WRITE_SCREEN_GIF_1, WRITE_SCREEN_GIF_2, WRITE_SCREEN_IMG,
};

pub const DEFAULT_KB_INFO_HEX: &str = "aa550000017e180001953332803e64000105014000ac1100";
pub const DEFAULT_MS_INFO_HEX: &str = "aa550000010c180001953332906564000105010600000000";
pub const DEFAULT_KB_CONFIG_HEX: &str = "0000040100ff00ff000000000000000000000000ff0000000000000000000000000000ff000401ff0000ffffffffff000200";
pub const DEFAULT_MS_CONFIG_HEX: &str = "000004030001ffffff00030302010100900190010000ff01002003200300ffff01004006400600ff00010060096009ff00000100800c800cffff000000000000005802000000000001000aa00500000000000000";

pub fn default_kb_config() -> Vec<u8> {
    hex::decode(DEFAULT_KB_CONFIG_HEX).unwrap_or_else(|_| vec![0u8; 50])
}

pub fn default_ms_config() -> Vec<u8> {
    hex::decode(DEFAULT_MS_CONFIG_HEX).unwrap_or_else(|_| vec![0u8; 84])
}

#[derive(Debug, Clone)]
pub enum TransportError {
    WirelessSleeping(String),
    BtConfigUnsupported(String),
    Disconnected(String),
    Io(String),
}

impl TransportError {
    pub fn is_sleeping(&self) -> bool {
        matches!(self, Self::WirelessSleeping(_))
    }

    pub fn is_abort_retry(&self) -> bool {
        matches!(
            self,
            Self::WirelessSleeping(_) | Self::BtConfigUnsupported(_) | Self::Disconnected(_)
        )
    }

    pub fn message(&self) -> &str {
        match self {
            Self::WirelessSleeping(s)
            | Self::BtConfigUnsupported(s)
            | Self::Disconnected(s)
            | Self::Io(s) => s,
        }
    }
}

impl std::fmt::Display for TransportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.message())
    }
}

pub struct MockState {
    pub kind: DeviceKind,
    pub wireless_online: bool,
    pub battery: u8,
    pub charging_state: u8,
    pub info: Vec<u8>,
    pub config: Vec<u8>,
    pub rom_keymap: Vec<u8>,
    pub user_keymap: Vec<u8>,
    pub custom_rgb_flash: Vec<u8>,
    pub direct_key_rgb: Vec<u8>,
    pub direct_logo_rgb: Vec<u8>,
    pub stream_frames: u64,
    pub screen_flash: Vec<u8>,
    pub max_observed_chunk: usize,
}

impl MockState {
    pub fn new(kind: DeviceKind, mode: ConnectionMode, wireless_online: bool, battery: Option<u8>, charging: Option<u8>) -> Self {
        let info = hex::decode(match kind {
            DeviceKind::Keyboard => DEFAULT_KB_INFO_HEX,
            DeviceKind::Mouse => DEFAULT_MS_INFO_HEX,
        })
        .unwrap_or_default();
        let config = match kind {
            DeviceKind::Keyboard => default_kb_config(),
            DeviceKind::Mouse => default_ms_config(),
        };
        let rom_keymap = match kind {
            DeviceKind::Keyboard => build_default_kb_rom(),
            DeviceKind::Mouse => build_default_ms_rom(),
        };
        let user_keymap = rom_keymap.clone();
        // 默认仿真逐键自定义灯效（4 组 × 512B）：第 0 组预置红魔绯红底色 + WASD 亮白高亮
        let mut custom_rgb_flash = if kind == DeviceKind::Keyboard {
            vec![0u8; 4 * 0x200]
        } else {
            Vec::new()
        };
        if kind == DeviceKind::Keyboard {
            for slot in 0..126 {
                let o = slot * 3;
                custom_rgb_flash[o] = 0x55;
                custom_rgb_flash[o + 1] = 0x10;
                custom_rgb_flash[o + 2] = 0x18;
            }
            // W(44), A(64), S(65), D(66), Esc(0)
            for &s in &[0usize, 44, 64, 65, 66] {
                let o = s * 3;
                custom_rgb_flash[o] = 0xe5;
                custom_rgb_flash[o + 1] = 0x39;
                custom_rgb_flash[o + 2] = 0x45;
            }
        }
        Self {
            kind,
            wireless_online,
            battery: battery.unwrap_or(if kind == DeviceKind::Mouse { 86 } else { 100 }),
            charging_state: charging.unwrap_or(if mode == ConnectionMode::Wired { 2 } else { 0 }),
            info,
            config,
            rom_keymap,
            user_keymap,
            custom_rgb_flash,
            direct_key_rgb: if kind == DeviceKind::Keyboard { vec![0u8; 126 * 3] } else { Vec::new() },
            direct_logo_rgb: if kind == DeviceKind::Keyboard { vec![0u8; LOGO_RGB_LEN] } else { Vec::new() },
            stream_frames: 0,
            screen_flash: if kind == DeviceKind::Keyboard {
                vec![0u8; 0x20000]
            } else {
                Vec::new()
            },
            max_observed_chunk: 0,
        }
    }
}

pub enum TransportBackend {
    WinHid {
        path: String,
        dev: Option<WinHidDevice>,
    },
    BluetoothReadOnly {
        device_id: String,
        battery: Option<u8>,
    },
    Mock(MockState),
}

pub struct Transport {
    pub mode: ConnectionMode,
    pub chunk_size: usize,
    pub link_state: String,
    pub supports_config: bool,
    pub bt_address: Option<String>,
    pub backend: TransportBackend,
}

impl Transport {
    pub fn new_win_hid(path: String, mode: ConnectionMode, link_state: &str, bt_address: Option<String>) -> Self {
        Self {
            mode,
            chunk_size: mode.chunk_size(),
            link_state: link_state.to_string(),
            supports_config: true,
            bt_address,
            backend: TransportBackend::WinHid { path, dev: None },
        }
    }

    pub fn new_bt_readonly(device_id: String, battery: Option<u8>, bt_address: Option<String>) -> Self {
        Self {
            mode: ConnectionMode::Bluetooth,
            chunk_size: CHUNK_SIZE_24G,
            link_state: "bt-readonly".to_string(),
            supports_config: false,
            bt_address,
            backend: TransportBackend::BluetoothReadOnly { device_id, battery },
        }
    }

    pub fn new_mock(
        kind: DeviceKind,
        mode: ConnectionMode,
        wireless_online: bool,
        supports_config: bool,
        battery: Option<u8>,
        charging: Option<u8>,
        bt_address: Option<String>,
    ) -> Self {
        let link_state = if !supports_config && mode == ConnectionMode::Bluetooth {
            "bt-readonly"
        } else if mode == ConnectionMode::Wireless24g && !wireless_online {
            "sleeping"
        } else {
            "online"
        };
        Self {
            mode,
            chunk_size: mode.chunk_size(),
            link_state: link_state.to_string(),
            supports_config,
            bt_address,
            backend: TransportBackend::Mock(MockState::new(kind, mode, wireless_online, battery, charging)),
        }
    }

    pub fn is_mock(&self) -> bool {
        matches!(self.backend, TransportBackend::Mock(_))
    }

    pub fn is_24g(&self) -> bool {
        self.mode == ConnectionMode::Wireless24g
    }

    pub fn device_path(&self) -> Option<&str> {
        match &self.backend {
            TransportBackend::WinHid { path, .. } => Some(path.as_str()),
            _ => None,
        }
    }

    #[cfg(test)]
    pub fn set_mock_online(&mut self, online: bool) {
        if let TransportBackend::Mock(ref mut m) = self.backend {
            m.wireless_online = online;
            if self.mode == ConnectionMode::Wireless24g {
                self.link_state = if online { "online" } else { "sleeping" }.to_string();
            }
        }
    }

    pub fn read_bt_battery(&mut self) -> (Option<u8>, u8) {
        match &mut self.backend {
            TransportBackend::BluetoothReadOnly { device_id, battery } => {
                let all_hid = enumerate_hid_devices();
                let found = scan_bluetooth_devices(&all_hid);
                if let Some(hit) = found.into_iter().find(|d| d.device_id == *device_id) {
                    if hit.battery.is_some() {
                        *battery = hit.battery;
                    }
                    if hit.bt_address.is_some() {
                        self.bt_address = hit.bt_address;
                    }
                    self.link_state = "bt-readonly".to_string();
                }
                (*battery, 0)
            }
            TransportBackend::Mock(m) => (Some(m.battery), 0),
            _ => (None, 0),
        }
    }

    pub fn drain(&mut self) {
        if let TransportBackend::WinHid { dev: Some(d), .. } = &self.backend {
            d.flush_queue();
        }
    }

    pub fn xfer(
        &mut self,
        op: u8,
        len: usize,
        off: u32,
        payload: Option<&[u8]>,
        timeout_ms: Option<u32>,
    ) -> Result<HidFrame, TransportError> {
        let mode = self.mode;
        let supports_config = self.supports_config;
        match &mut self.backend {
            TransportBackend::WinHid { path, dev } => {
                if dev.is_none() {
                    let opened = WinHidDevice::open(path)
                        .map_err(TransportError::Disconnected)?;
                    *dev = Some(opened);
                }
                let handle = dev.as_ref().unwrap();
                let wait_ms = timeout_ms.unwrap_or(if mode == ConnectionMode::Wired { 1800 } else { 2400 });
                let frame = build_frame(op, len, off, payload, None);

                if mode == ConnectionMode::Wireless24g {
                    // 对齐官方 Goper.exe 0x4de812：2.4G 写前清输入队列，防止读到旧帧
                    handle.flush_queue();
                }

                if let Err(e) = handle.write_frame(&frame, wait_ms) {
                    *dev = None;
                    self.link_state = "offline".to_string();
                    return Err(TransportError::Disconnected(e));
                }

                let deadline = Instant::now() + Duration::from_millis(wait_ms as u64);
                loop {
                    let now = Instant::now();
                    if now >= deadline {
                        break;
                    }
                    let rem = (deadline - now).as_millis().min(250).max(15) as u32;
                    match handle.read_frame(rem) {
                        Ok(Some(raw)) => {
                            let parsed = parse_frame(&raw);
                            if matches_header(&parsed, op, len, off) {
                                return Ok(parsed);
                            }
                            if mode == ConnectionMode::Wireless24g
                                && op != HEARTBEAT
                                && is_wireless_nack(&parsed)
                            {
                                self.link_state = "sleeping".to_string();
                                return Err(TransportError::WirelessSleeping(
                                    "2.4G 无线设备已休眠或未处于 2.4G 模式，请晃动鼠标 / 按键唤醒后重试"
                                        .to_string(),
                                ));
                            }
                        }
                        Ok(None) => continue,
                        Err(e) => {
                            *dev = None;
                            self.link_state = "offline".to_string();
                            return Err(TransportError::Disconnected(e));
                        }
                    }
                }
                Err(TransportError::Io(format!(
                    "HID 通信超时 (op=0x{op:02x}, off=0x{off:04x})"
                )))
            }

            TransportBackend::BluetoothReadOnly { .. } => Err(TransportError::BtConfigUnsupported(
                format!(
                    "当前蓝牙 BLE 连接未开放 0xFF1C 厂商指令通道 (op=0x{op:02x})，请切换至 2.4G 或 USB 有线"
                ),
            )),

            TransportBackend::Mock(m) => {
                if len > m.max_observed_chunk {
                    m.max_observed_chunk = len;
                }
                if mode == ConnectionMode::Wireless24g && len > CHUNK_SIZE_24G {
                    return Err(TransportError::Io(format!(
                        "2.4G 模拟校验失败：单包长度 0x{len:02x} 超过 24B RF 上限"
                    )));
                }
                let mut out = [0u8; CHUNK_SIZE];
                if op == HEARTBEAT {
                    out[0] = if m.wireless_online { 0xff } else { 0x00 };
                    return Ok(parse_frame(&build_frame(HEARTBEAT, 0, 0, Some(&out), Some(0))));
                }
                if mode == ConnectionMode::Wireless24g && !m.wireless_online {
                    self.link_state = "sleeping".to_string();
                    return Err(TransportError::WirelessSleeping(
                        "2.4G 无线设备已休眠或未处于 2.4G 模式，请晃动鼠标 / 按键唤醒后重试".to_string(),
                    ));
                }
                if !supports_config {
                    return Err(TransportError::BtConfigUnsupported(
                        "蓝牙 BLE 模式未开放 0xFF1C 指令通道".to_string(),
                    ));
                }

                let mut reply_len = len;
                let off_u = off as usize;
                match op {
                    POLL_STATUS => {
                        out[0] = m.battery;
                        out[1] = m.charging_state;
                        if mode == ConnectionMode::Wireless24g && m.kind == DeviceKind::Mouse {
                            reply_len = 0; // 复现真实 GS001S 2.4G (0x2277) 回包特性
                        }
                    }
                    READ_INFO => {
                        if off_u < m.info.len() {
                            let n = len.min(m.info.len() - off_u);
                            out[..n].copy_from_slice(&m.info[off_u..off_u + n]);
                        }
                    }
                    READ_CONFIG => {
                        if off_u < m.config.len() {
                            let n = len.min(m.config.len() - off_u);
                            out[..n].copy_from_slice(&m.config[off_u..off_u + n]);
                        }
                    }
                    WRITE_CONFIG => {
                        if let Some(p) = payload {
                            if off_u < m.config.len() {
                                let n = len.min(p.len()).min(m.config.len() - off_u);
                                m.config[off_u..off_u + n].copy_from_slice(&p[..n]);
                            }
                            let n = len.min(p.len()).min(out.len());
                            out[..n].copy_from_slice(&p[..n]);
                        }
                    }
                    READ_KEYMAP_ROM => {
                        if off_u < m.rom_keymap.len() {
                            let n = len.min(m.rom_keymap.len() - off_u);
                            out[..n].copy_from_slice(&m.rom_keymap[off_u..off_u + n]);
                        }
                    }
                    READ_KEYMAP_USER => {
                        if off_u < m.user_keymap.len() {
                            let n = len.min(m.user_keymap.len() - off_u);
                            out[..n].copy_from_slice(&m.user_keymap[off_u..off_u + n]);
                        }
                    }
                    WRITE_KEYMAP => {
                        if let Some(p) = payload {
                            if off_u < m.user_keymap.len() {
                                let n = len.min(p.len()).min(m.user_keymap.len() - off_u);
                                m.user_keymap[off_u..off_u + n].copy_from_slice(&p[..n]);
                            }
                            let n = len.min(p.len()).min(out.len());
                            out[..n].copy_from_slice(&p[..n]);
                        }
                    }
                    READ_CUSTOM_RGB => {
                        if off_u < m.custom_rgb_flash.len() {
                            let n = len.min(m.custom_rgb_flash.len() - off_u);
                            out[..n].copy_from_slice(&m.custom_rgb_flash[off_u..off_u + n]);
                        }
                    }
                    WRITE_CUSTOM_RGB => {
                        if let Some(p) = payload {
                            if off_u < m.custom_rgb_flash.len() {
                                let n = len.min(p.len()).min(m.custom_rgb_flash.len() - off_u);
                                m.custom_rgb_flash[off_u..off_u + n].copy_from_slice(&p[..n]);
                            }
                            let n = len.min(p.len()).min(out.len());
                            out[..n].copy_from_slice(&p[..n]);
                        }
                    }
                    STREAM_KEY_RGB => {
                        if let Some(p) = payload {
                            if off_u < m.direct_key_rgb.len() {
                                let n = len.min(p.len()).min(m.direct_key_rgb.len() - off_u);
                                m.direct_key_rgb[off_u..off_u + n].copy_from_slice(&p[..n]);
                            }
                            if off_u == 0 {
                                m.stream_frames += 1;
                            }
                        }
                    }
                    STREAM_LOGO_RGB => {
                        if let Some(p) = payload {
                            if off_u < m.direct_logo_rgb.len() {
                                let n = len.min(p.len()).min(m.direct_logo_rgb.len() - off_u);
                                m.direct_logo_rgb[off_u..off_u + n].copy_from_slice(&p[..n]);
                            }
                        }
                    }
                    PREPARE_SCREEN_IMG => {
                        m.screen_flash.fill(0);
                    }
                    WRITE_SCREEN_IMG | WRITE_SCREEN_GIF_1 | WRITE_SCREEN_GIF_2 => {
                        if let Some(p) = payload {
                            if off_u < m.screen_flash.len() {
                                let n = len.min(p.len()).min(m.screen_flash.len() - off_u);
                                m.screen_flash[off_u..off_u + n].copy_from_slice(&p[..n]);
                            }
                        }
                    }
                    _ => {}
                }

                let frame = parse_frame(&build_frame(op, reply_len, off, Some(&out), None));
                if !matches_header(&frame, op, len, off) {
                    return Err(TransportError::Io(format!(
                        "Mock 帧头校验不匹配 (op=0x{op:02x}, reply_len={reply_len}, req_len={len})"
                    )));
                }
                Ok(frame)
            }
        }
    }

    pub fn ping_wireless(&mut self, timeout_ms: u32) -> (bool, u8) {
        if !self.is_24g() {
            let on = self.link_state == "online";
            return (on, if on { 0xff } else { 0x00 });
        }
        self.drain();
        match self.xfer(HEARTBEAT, 0, 0, None, Some(timeout_ms)) {
            Ok(r) => {
                let online = r.op == HEARTBEAT
                    && r.status != 0xff
                    && r.status != 0xfe
                    && r.data[0] == 0xff;
                self.link_state = if online { "online" } else { "sleeping" }.to_string();
                (online, r.data[0])
            }
            Err(_) => {
                self.link_state = "sleeping".to_string();
                (false, 0)
            }
        }
    }

    pub fn ensure_wireless_awake(&mut self, retries: usize, interval_ms: u64) -> Result<(), TransportError> {
        if !self.is_24g() {
            return Ok(());
        }
        for i in 0..retries {
            let (online, _) = self.ping_wireless(340);
            if online {
                return Ok(());
            }
            if i + 1 < retries {
                thread::sleep(Duration::from_millis(interval_ms));
            }
        }
        self.link_state = "sleeping".to_string();
        Err(TransportError::WirelessSleeping(
            "2.4G 无线设备已休眠或未切换到 2.4G 档位，请晃动鼠标 / 按任意键唤醒后重试".to_string(),
        ))
    }

    pub fn read_blob(&mut self, op: u8, total_len: usize, base_off: u32) -> Result<Vec<u8>, TransportError> {
        let mut out = Vec::with_capacity(total_len);
        let step = self.chunk_size;
        let max_attempts = if self.mode == ConnectionMode::Wired { 1 } else { 3 };
        let timeout_ms = if self.mode == ConnectionMode::Wired { 1800 } else { 2400 };

        let mut off = 0usize;
        while off < total_len {
            let len = step.min(total_len - off);
            let mut last_err = None;
            let mut got = None;
            for _ in 0..max_attempts {
                match self.xfer(op, len, base_off + off as u32, None, Some(timeout_ms)) {
                    Ok(frame) => {
                        got = Some(frame);
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        let abort = e.is_abort_retry();
                        last_err = Some(e);
                        if abort {
                            break;
                        }
                        thread::sleep(Duration::from_millis(35));
                        self.drain();
                    }
                }
            }
            if let Some(e) = last_err {
                return Err(e);
            }
            let frame = got.unwrap();
            out.extend_from_slice(&frame.data[..len]);
            off += step;
        }
        Ok(out)
    }

    pub fn write_blob_tx(&mut self, op: u8, data: &[u8], base_off: u32) -> Result<(), TransportError> {
        if !self.supports_config {
            return Err(TransportError::BtConfigUnsupported(
                "当前蓝牙 BLE 模式未开放 0xFF1C 写配置通道，请切换至 2.4G 无线接收器或 USB 有线连接后再修改"
                    .to_string(),
            ));
        }
        if self.is_24g() {
            self.ensure_wireless_awake(6, 55)?;
        }

        let step = self.chunk_size;
        let timeout_ms = if self.mode == ConnectionMode::Wired { 1800 } else { 2600 };
        let max_attempts = if self.mode == ConnectionMode::Wired { 1 } else { 3 };

        self.drain();
        let begin_tries = if self.is_24g() { 2 } else { 1 };
        for a in 0..begin_tries {
            match self.xfer(TX_BEGIN, 0, 0, None, Some(timeout_ms)) {
                Ok(_) => break,
                Err(e) => {
                    if a + 1 >= begin_tries || e.is_abort_retry() {
                        return Err(e);
                    }
                    thread::sleep(Duration::from_millis(45));
                    self.drain();
                }
            }
        }

        let mut off = 0usize;
        while off < data.len() {
            let len = step.min(data.len() - off);
            let slice = &data[off..off + len];
            let mut last_err = None;
            let mut reply = None;
            for _ in 0..max_attempts {
                match self.xfer(op, len, base_off + off as u32, Some(slice), Some(timeout_ms)) {
                    Ok(r) => {
                        reply = Some(r);
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        let abort = e.is_abort_retry();
                        last_err = Some(e);
                        if abort {
                            break;
                        }
                        thread::sleep(Duration::from_millis(40));
                        self.drain();
                    }
                }
            }
            if let Some(e) = last_err {
                return Err(e);
            }
            let r = reply.unwrap();
            if r.status == 0xff || r.status == 0xfe {
                return Err(TransportError::Io(format!(
                    "设备拒绝写入 (op=0x{op:02x}, status=0x{:02x})",
                    r.status
                )));
            }
            off += step;
        }

        thread::sleep(Duration::from_millis(if self.is_24g() { 25 } else { 15 }));
        let commit = self.xfer(TX_COMMIT, 0, 0, None, Some(timeout_ms))?;
        if commit.status == 0xff || commit.status == 0xfe {
            return Err(TransportError::Io(format!(
                "事务提交失败 (status=0x{:02x})",
                commit.status
            )));
        }
        Ok(())
    }

    /// 零 Flash 损耗的高速实时 RAM 帧推送（OP 0x12 主键逐键 378B / OP 0x28 顶部 Logo 60B）
    /// 对齐官方 Goper.exe 0x4e1380 / 0x4e1630：不发送 TX_BEGIN/TX_COMMIT，按单帧 MTU 分包直写 RAM
    pub fn stream_blob(&mut self, op: u8, data: &[u8]) -> Result<(), TransportError> {
        if !self.supports_config {
            return Err(TransportError::BtConfigUnsupported(
                "蓝牙 BLE 模式不支持实时 RGB 流式推送，请切换至 USB 有线或 2.4G 无线".to_string(),
            ));
        }
        let step = self.chunk_size;
        let timeout_ms = if self.mode == ConnectionMode::Wired { 140 } else { 260 };
        let mut off = 0usize;
        while off < data.len() {
            let len = step.min(data.len() - off);
            let slice = &data[off..off + len];
            let r = self.xfer(op, len, off as u32, Some(slice), Some(timeout_ms))?;
            if r.status == 0xff || r.status == 0xfe {
                return Err(TransportError::Io(format!(
                    "实时灯效帧被设备拒绝 (op=0x{op:02x}, status=0x{:02x})",
                    r.status
                )));
            }
            off += step;
        }
        Ok(())
    }
}
