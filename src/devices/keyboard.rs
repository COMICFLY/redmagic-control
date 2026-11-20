//! GK001S 键盘真机适配器（50B 配置块 OP 0x05/0x06 + 378B 键表 OP 0x07/0x08/0x09 + 128KB 小屏 OP 0x23/0x25）。

use std::thread;
use std::time::Duration;

use base64::Engine;
use serde_json::{json, Value};

use crate::devices::catalog::ConnectionMode;
use crate::devices::screen_lib::{
    delete_screen_library_bmp, make_goper_uuid, read_persisted_screen_state, rgb565_to_bmp,
    sanitize_label, save_screen_library_bmp, PersistedScreenState, ScreenLibraryItem,
    SCREEN_FLASH_BLOCK, SCREEN_H, SCREEN_PIXEL_BYTES, SCREEN_W,
};
use crate::devices::transport::{default_kb_config, Transport, TransportError};
use crate::protocol::usage::{build_default_kb_rom, bytes_to_hex3, decode_assignment, hex3_to_bytes};
use crate::protocol::{
    CHUNK_SIZE, POLL_STATUS, PREPARE_SCREEN_IMG, READ_CONFIG, READ_INFO, READ_KEYMAP_ROM,
    READ_KEYMAP_USER, WRITE_CONFIG, WRITE_KEYMAP, WRITE_SCREEN_IMG,
};

pub const CONFIG_LEN: usize = 0x32; // 50 字节
pub const KEY_SLOTS: usize = 126;
pub const KEYMAP_LEN: usize = KEY_SLOTS * 3; // 378 字节

fn clamp_u8(v: i64, lo: i64, hi: i64) -> u8 {
    v.clamp(lo, hi) as u8
}

fn rgb_to_hex(r: u8, g: u8, b: u8) -> String {
    format!("#{r:02x}{g:02x}{b:02x}")
}

fn hex_to_rgb(s: &str) -> (u8, u8, u8) {
    let m = s.trim().trim_start_matches('#');
    if m.len() == 6 {
        if let (Ok(r), Ok(g), Ok(b)) = (
            u8::from_str_radix(&m[0..2], 16),
            u8::from_str_radix(&m[2..4], 16),
            u8::from_str_radix(&m[4..6], 16),
        ) {
            return (r, g, b);
        }
    }
    (229, 57, 69)
}

pub fn decode_kb_config(buf: &[u8]) -> Value {
    let b = |i: usize| -> u8 { buf.get(i).copied().unwrap_or(0) };
    json!({
        "profile": b(0),
        "light": {
            "mode": b(1),
            "brightness": clamp_u8(b(2) as i64, 0, 4),
            "speed": clamp_u8(2 - b(3) as i64, 0, 2),
            "direction": b(4),
            "multiColor": b(5) != 0,
            "color": rgb_to_hex(b(6), b(7), b(8)),
        },
        "logo": {
            "mode": b(28),
            "color": rgb_to_hex(b(34), b(35), b(36)),
            "brightness": clamp_u8(b(37) as i64, 0, 4),
            "speed": clamp_u8(3 - b(38) as i64, 0, 3),
            "sevenColor": b(39) != 0,
        },
        "system": {
            "wasdSwap": b(19) != 0,
            "key6": b(20) != 0,
            "winLock": b(21) != 0,
            "pollingRate": b(22),
            "lowPower": b(32) != 0,
            "altF4": b(33) != 0,
            "sleep": b(40),
            "wallpaper": b(47),
        },
        "screen": {
            "width": SCREEN_W,
            "height": SCREEN_H,
            "isGif": b(29) != 0,
            "frameCount": if b(31) == 0 { 1 } else { b(31) },
            "screenSleep": b(41),
            "wallpaperFlags": [b(42), b(43), b(44), b(45), b(46)],
            "wallpaperSlot": clamp_u8(b(47) as i64, 0, 4),
        },
        "rawHex": hex::encode(buf),
    })
}

pub fn apply_kb_config_patch(base: &[u8], patch: &Value) -> Vec<u8> {
    let mut buf = if base.len() >= CONFIG_LEN {
        base[..CONFIG_LEN].to_vec()
    } else {
        let mut v = vec![0u8; CONFIG_LEN];
        v[..base.len()].copy_from_slice(base);
        v
    };

    if let Some(l) = patch.get("light").and_then(Value::as_object) {
        if let Some(v) = l.get("mode").and_then(Value::as_i64) {
            buf[1] = (v & 0xff) as u8;
        }
        if let Some(v) = l.get("brightness").and_then(Value::as_i64) {
            buf[2] = clamp_u8(v, 0, 4);
        }
        if let Some(v) = l.get("speed").and_then(Value::as_i64) {
            buf[3] = clamp_u8(2 - v.clamp(0, 2), 0, 2);
        }
        if let Some(v) = l.get("direction").and_then(Value::as_i64) {
            buf[4] = (v & 0xff) as u8;
        }
        if let Some(v) = l.get("multiColor").and_then(Value::as_bool) {
            buf[5] = if v { if base.get(5).copied().unwrap_or(0) != 0 { base[5] } else { 0xff } } else { 0x00 };
        }
        if let Some(s) = l.get("color").and_then(Value::as_str) {
            let (r, g, b) = hex_to_rgb(s);
            buf[6] = r;
            buf[7] = g;
            buf[8] = b;
        }
    }

    if let Some(g) = patch.get("logo").and_then(Value::as_object) {
        if let Some(v) = g.get("mode").and_then(Value::as_i64) {
            buf[28] = (v & 0xff) as u8;
        }
        if let Some(s) = g.get("color").and_then(Value::as_str) {
            let (r, gr, b) = hex_to_rgb(s);
            buf[34] = r;
            buf[35] = gr;
            buf[36] = b;
        }
        if let Some(v) = g.get("brightness").and_then(Value::as_i64) {
            buf[37] = clamp_u8(v, 0, 4);
        }
        if let Some(v) = g.get("speed").and_then(Value::as_i64) {
            buf[38] = clamp_u8(3 - v.clamp(0, 3), 0, 3);
        }
        if let Some(v) = g.get("sevenColor").and_then(Value::as_bool) {
            buf[39] = if v { if base.get(39).copied().unwrap_or(0) != 0 { base[39] } else { 0xff } } else { 0x00 };
        }
    }

    if let Some(s) = patch.get("system").and_then(Value::as_object) {
        if let Some(v) = s.get("wasdSwap").and_then(Value::as_bool) {
            buf[19] = u8::from(v);
        }
        if let Some(v) = s.get("winLock").and_then(Value::as_bool) {
            buf[21] = u8::from(v);
        }
        if let Some(v) = s.get("pollingRate").and_then(Value::as_i64) {
            buf[22] = clamp_u8(v, 0, 3);
        }
        if let Some(v) = s.get("lowPower").and_then(Value::as_bool) {
            buf[32] = u8::from(v);
        }
        if let Some(v) = s.get("altF4").and_then(Value::as_bool) {
            buf[33] = u8::from(v);
        }
        if let Some(v) = s.get("sleep").and_then(Value::as_i64) {
            buf[40] = clamp_u8(v, 0, 255);
        }
    }

    if let Some(sc) = patch.get("screen").and_then(Value::as_object) {
        if let Some(v) = sc.get("isGif").and_then(Value::as_bool) {
            buf[29] = u8::from(v);
        }
        if let Some(v) = sc.get("frameCount").and_then(Value::as_i64) {
            buf[31] = clamp_u8(v, 1, 50);
        }
        if let Some(v) = sc.get("screenSleep").and_then(Value::as_i64) {
            buf[41] = clamp_u8(v, 0, 255);
        }
        // GK001S 固件 OP 0x25 固定写入 Flash 0xCE0000（槽位 0）；保持 buf[47]=0 避免黑屏
        buf[47] = 0;
    }

    buf
}

pub struct KeyboardAdapter {
    pub t: Transport,
    pub link_id: String,
    rom_cache: Option<Vec<u8>>,
    last_state: Option<Value>,
    mock_screen: Option<PersistedScreenState>,
}

impl KeyboardAdapter {
    pub fn new(transport: Transport, link_id: String, cached_state: Option<Value>) -> Self {
        Self {
            t: transport,
            link_id,
            rom_cache: None,
            last_state: cached_state,
            mock_screen: None,
        }
    }

    fn ensure_writable(&mut self) -> Result<(), TransportError> {
        if !self.t.supports_config {
            return Err(TransportError::BtConfigUnsupported(
                "当前蓝牙 BLE 模式未开放 0xFF1C 写配置通道，请切换至 2.4G 接收器或 USB 有线后修改"
                    .to_string(),
            ));
        }
        if self.t.is_24g() {
            self.t.ensure_wireless_awake(6, 55)?;
        }
        Ok(())
    }

    fn build_fallback_state(&self, link_state: &str, battery: Option<u8>, charging_state: Option<u8>) -> Value {
        if let Some(mut prev) = self.last_state.clone() {
            if let Some(obj) = prev.as_object_mut() {
                obj.insert("mock".into(), json!(self.t.is_mock()));
                obj.insert("connectionMode".into(), json!(self.t.mode.as_str()));
                obj.insert("linkState".into(), json!(link_state));
                obj.insert("supportsConfig".into(), json!(false));
                obj.insert("supportsScreen".into(), json!(false));
                obj.insert("chunkSize".into(), json!(self.t.chunk_size));
                obj.insert("btAddress".into(), json!(self.t.bt_address));
                if let Some(b) = battery {
                    obj.insert("battery".into(), json!(b));
                }
                obj.insert("chargingState".into(), json!(charging_state));
                obj.insert("staleConfig".into(), json!(true));
            }
            return prev;
        }

        let rom_buf = build_default_kb_rom();
        let mut keymap = Vec::with_capacity(KEY_SLOTS);
        for slot in 0..KEY_SLOTS {
            let h = bytes_to_hex3(&rom_buf, slot * 3);
            let dec = decode_assignment(&h);
            keymap.push(json!({
                "slot": slot,
                "hex": h,
                "defaultHex": h,
                "modified": false,
                "current": dec,
                "fallback": dec,
            }));
        }
        json!({
            "kind": "keyboard",
            "mock": self.t.is_mock(),
            "connectionMode": self.t.mode.as_str(),
            "linkState": link_state,
            "supportsConfig": false,
            "supportsScreen": false,
            "chunkSize": self.t.chunk_size,
            "btAddress": self.t.bt_address,
            "battery": battery,
            "chargingState": charging_state,
            "capabilityHex": Value::Null,
            "hwKeyCount": KEY_SLOTS,
            "config": decode_kb_config(&default_kb_config()),
            "screenImage": read_persisted_screen_state(),
            "keymap": keymap,
            "staleConfig": true,
        })
    }

    pub fn read_battery(&mut self) -> Result<Value, TransportError> {
        if self.t.mode == ConnectionMode::Bluetooth && !self.t.supports_config {
            let (bat, chg) = self.t.read_bt_battery();
            return Ok(json!({
                "battery": bat,
                "chargingState": chg,
                "connectionMode": "bluetooth",
                "linkState": "bt-readonly",
            }));
        }
        if self.t.is_24g() {
            let (online, _) = self.t.ping_wireless(340);
            if !online {
                return Ok(json!({
                    "battery": Value::Null,
                    "chargingState": Value::Null,
                    "connectionMode": "2.4g",
                    "linkState": "sleeping",
                }));
            }
        }
        let timeout = if self.t.is_24g() { 900 } else { 700 };
        let r = self.t.xfer(POLL_STATUS, 0x06, 0, None, Some(timeout))?;
        let bat = if r.data[0] <= 100 { Some(r.data[0]) } else { None };
        let chg = if r.data[1] <= 2 { r.data[1] } else { 0 };
        Ok(json!({
            "battery": bat,
            "chargingState": chg,
            "connectionMode": self.t.mode.as_str(),
            "linkState": "online",
        }))
    }

    pub fn read_state(&mut self) -> Result<Value, TransportError> {
        if self.t.mode == ConnectionMode::Bluetooth && !self.t.supports_config {
            let (bat, chg) = self.t.read_bt_battery();
            return Ok(self.build_fallback_state("bt-readonly", bat, Some(chg)));
        }
        if self.t.is_24g() {
            if let Err(e) = self.t.ensure_wireless_awake(4, 55) {
                if e.is_sleeping() {
                    return Ok(self.build_fallback_state("sleeping", None, None));
                }
                return Err(e);
            }
        }

        self.t.drain();
        let poll_ms = if self.t.is_24g() { 1200 } else { 900 };
        let bat_res = self.t.xfer(POLL_STATUS, 0x06, 0, None, Some(poll_ms)).ok();
        let info_buf = self.t.read_blob(READ_INFO, 0x22, 0).ok();
        let cfg_buf = self.t.read_blob(READ_CONFIG, CONFIG_LEN, 0)?;
        let user_km = self.t.read_blob(READ_KEYMAP_USER, KEYMAP_LEN, 0)?;
        let rom_km = if let Some(ref c) = self.rom_cache {
            c.clone()
        } else {
            let r = self.t.read_blob(READ_KEYMAP_ROM, KEYMAP_LEN, 0)?;
            self.rom_cache = Some(r.clone());
            r
        };

        let mut keymap = Vec::with_capacity(KEY_SLOTS);
        for slot in 0..KEY_SLOTS {
            let cur_hex = bytes_to_hex3(&user_km, slot * 3);
            let def_hex = bytes_to_hex3(&rom_km, slot * 3);
            keymap.push(json!({
                "slot": slot,
                "hex": cur_hex,
                "defaultHex": def_hex,
                "modified": cur_hex != def_hex,
                "current": decode_assignment(&cur_hex),
                "fallback": decode_assignment(&def_hex),
            }));
        }

        let battery = bat_res.as_ref().and_then(|r| (r.data[0] <= 100).then_some(r.data[0]));
        let charging = bat_res.as_ref().and_then(|r| (r.data[1] <= 2).then_some(r.data[1]));
        let capability_hex = info_buf
            .as_ref()
            .map(|b| hex::encode(&b[..b.len().min(24)]));
        let hw_key_count = info_buf
            .as_ref()
            .and_then(|b| (b.len() > 5 && b[5] > 0).then_some(b[5] as usize))
            .unwrap_or(KEY_SLOTS);
        let screen_img = if self.t.is_mock() {
            self.mock_screen
                .clone()
                .unwrap_or_else(read_persisted_screen_state)
        } else {
            read_persisted_screen_state()
        };

        let full = json!({
            "kind": "keyboard",
            "mock": self.t.is_mock(),
            "connectionMode": self.t.mode.as_str(),
            "linkState": "online",
            "supportsConfig": self.t.supports_config,
            "supportsScreen": self.t.is_mock() || self.t.mode == ConnectionMode::Wired,
            "chunkSize": self.t.chunk_size,
            "btAddress": self.t.bt_address,
            "battery": battery,
            "chargingState": charging,
            "capabilityHex": capability_hex,
            "hwKeyCount": hw_key_count,
            "config": decode_kb_config(&cfg_buf),
            "screenImage": screen_img,
            "keymap": keymap,
            "staleConfig": false,
        });
        self.last_state = Some(full.clone());
        Ok(full)
    }

    pub fn write_config(&mut self, patch: &Value) -> Result<Value, TransportError> {
        self.ensure_writable()?;
        let current = self.t.read_blob(READ_CONFIG, CONFIG_LEN, 0)?;
        let next = apply_kb_config_patch(&current, patch);
        self.t.write_blob_tx(WRITE_CONFIG, &next, 0)?;
        let verify = self.t.read_blob(READ_CONFIG, CONFIG_LEN, 0)?;
        let decoded = decode_kb_config(&verify);
        if let Some(ref mut st) = self.last_state {
            if let Some(obj) = st.as_object_mut() {
                obj.insert("config".into(), decoded.clone());
            }
        }
        Ok(json!({
            "ok": verify == next,
            "mock": self.t.is_mock(),
            "config": decoded,
        }))
    }

    pub fn write_key(&mut self, slot: usize, assignment_hex: &str) -> Result<Value, TransportError> {
        self.ensure_writable()?;
        if slot >= KEY_SLOTS {
            return Err(TransportError::Io(format!("无效键盘槽位：{slot}")));
        }
        let mut next = self.t.read_blob(READ_KEYMAP_USER, KEYMAP_LEN, 0)?;
        let bytes = hex3_to_bytes(assignment_hex);
        next[slot * 3..slot * 3 + 3].copy_from_slice(&bytes);
        self.t.write_blob_tx(WRITE_KEYMAP, &next, 0)?;
        let verify = self.t.read_blob(READ_KEYMAP_USER, KEYMAP_LEN, 0)?;
        let got_hex = bytes_to_hex3(&verify, slot * 3);
        Ok(json!({
            "ok": got_hex == hex::encode(bytes),
            "mock": self.t.is_mock(),
            "slot": slot,
            "hex": got_hex,
            "decoded": decode_assignment(&got_hex),
        }))
    }

    pub fn reset_key(&mut self, slot: usize) -> Result<Value, TransportError> {
        self.ensure_writable()?;
        if slot >= KEY_SLOTS {
            return Err(TransportError::Io(format!("无效键盘槽位：{slot}")));
        }
        let rom = if let Some(ref c) = self.rom_cache {
            c.clone()
        } else {
            let r = self.t.read_blob(READ_KEYMAP_ROM, KEYMAP_LEN, 0)?;
            self.rom_cache = Some(r.clone());
            r
        };
        let def_hex = bytes_to_hex3(&rom, slot * 3);
        let mut next = self.t.read_blob(READ_KEYMAP_USER, KEYMAP_LEN, 0)?;
        next[slot * 3..slot * 3 + 3].copy_from_slice(&hex3_to_bytes(&def_hex));
        self.t.write_blob_tx(WRITE_KEYMAP, &next, 0)?;
        let verify = self.t.read_blob(READ_KEYMAP_USER, KEYMAP_LEN, 0)?;
        let got_hex = bytes_to_hex3(&verify, slot * 3);
        Ok(json!({
            "ok": got_hex == def_hex,
            "mock": self.t.is_mock(),
            "slot": slot,
            "hex": got_hex,
            "decoded": decode_assignment(&got_hex),
        }))
    }

    pub fn reset_all_keys(&mut self) -> Result<Value, TransportError> {
        self.ensure_writable()?;
        let rom = if let Some(ref c) = self.rom_cache {
            c.clone()
        } else {
            let r = self.t.read_blob(READ_KEYMAP_ROM, KEYMAP_LEN, 0)?;
            self.rom_cache = Some(r.clone());
            r
        };
        self.t.write_blob_tx(WRITE_KEYMAP, &rom, 0)?;
        let verify = self.t.read_blob(READ_KEYMAP_USER, KEYMAP_LEN, 0)?;
        Ok(json!({
            "ok": verify == rom,
            "mock": self.t.is_mock(),
        }))
    }

    pub fn upload_screen_image<F>(
        &mut self,
        raw_rgb565: &[u8],
        name: &str,
        reuse_uuid: Option<&str>,
        mut on_progress: F,
    ) -> Result<Value, TransportError>
    where
        F: FnMut(u32, usize, usize),
    {
        if !self.t.is_mock() && self.t.mode != ConnectionMode::Wired {
            return Err(TransportError::Io(format!(
                "小屏 128KB 固件写入仅支持 USB 有线直连（当前为 {} 模式），请接入 USB 数据线后重试",
                self.t.mode.label()
            )));
        }
        if raw_rgb565.len() < SCREEN_PIXEL_BYTES {
            return Err(TransportError::Io(format!(
                "小屏图像数据不足：需要 {SCREEN_PIXEL_BYTES} 字节 (320×172 RGB565)，实际 {}",
                raw_rgb565.len()
            )));
        }

        let mut flash_block = vec![0u8; SCREEN_FLASH_BLOCK];
        let copy_n = raw_rgb565.len().min(SCREEN_FLASH_BLOCK);
        flash_block[..copy_n].copy_from_slice(&raw_rgb565[..copy_n]);

        self.t.drain();
        let current_cfg = self.t.read_blob(READ_CONFIG, CONFIG_LEN, 0)?;
        let next_cfg = apply_kb_config_patch(
            &current_cfg,
            &json!({ "screen": { "isGif": false, "frameCount": 1 } }),
        );
        self.t.write_blob_tx(WRITE_CONFIG, &next_cfg, 0)?;

        // OP 0x23: 准备小屏静态图片扇区 (固件 0x000284da Echo 握手)
        self.t.xfer(PREPARE_SCREEN_IMG, 0, 0, None, Some(2500))?;

        // OP 0x25: 56B 分包流式写入 128KB 至 0x00CE0000..0x00D00000 (2,341 包)
        let mut last_pct: i32 = -1;
        let mut off = 0usize;
        while off < SCREEN_FLASH_BLOCK {
            let len = CHUNK_SIZE.min(SCREEN_FLASH_BLOCK - off);
            let chunk = &flash_block[off..off + len];
            let mut last_err = None;
            for _ in 0..3 {
                match self.t.xfer(WRITE_SCREEN_IMG, len, off as u32, Some(chunk), Some(2500)) {
                    Ok(_) => {
                        last_err = None;
                        break;
                    }
                    Err(e) => {
                        last_err = Some(e);
                        thread::sleep(Duration::from_millis(30));
                    }
                }
            }
            if let Some(e) = last_err {
                return Err(e);
            }
            if off == 0 && !self.t.is_mock() {
                // 固件 0x00028b18 擦除 0xCE/0xCF 两个 64KB 扇区 + Goper.exe 0x4e209b 等待 1000ms
                thread::sleep(Duration::from_millis(1000));
            }
            let sent = off + len;
            let pct = ((sent * 100) / SCREEN_FLASH_BLOCK).min(100) as i32;
            if pct != last_pct && (pct % 2 == 0 || pct == 100) {
                last_pct = pct;
                on_progress(pct as u32, sent, SCREEN_FLASH_BLOCK);
            }
            off += CHUNK_SIZE;
        }

        let (saved_uuid, screen_image) = if self.t.is_mock() {
            let bmp = rgb565_to_bmp(raw_rgb565);
            let data_url = format!(
                "data:image/bmp;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bmp)
            );
            let mut st = self
                .mock_screen
                .clone()
                .unwrap_or_else(read_persisted_screen_state);
            let mut uuid_out = reuse_uuid.map(str::to_string);
            let existing_idx = uuid_out
                .as_deref()
                .and_then(|u| st.items.iter().position(|it| it.uuid == u));
            if let Some(idx) = existing_idx {
                for (i, it) in st.items.iter_mut().enumerate() {
                    it.is_linked = i == idx;
                }
            } else {
                let u = make_goper_uuid();
                for it in &mut st.items {
                    it.is_linked = false;
                }
                let fb = format!("ScreenFile_{}", st.items.len());
                st.items.push(ScreenLibraryItem {
                    index: st.items.len(),
                    uuid: u.clone(),
                    name: sanitize_label(name, &fb),
                    is_linked: true,
                    file_path: None,
                    data_url: data_url.clone(),
                });
                uuid_out = Some(u);
            }
            let linked_name = st
                .items
                .iter()
                .find(|it| it.is_linked)
                .map(|it| it.name.clone())
                .unwrap_or_else(|| "Mock".to_string());
            st.has_linked_cache = true;
            st.current_uuid = uuid_out.clone();
            st.current_data_url = Some(data_url);
            st.current_name = format!("本机已同步 ({linked_name})");
            self.mock_screen = Some(st.clone());
            (uuid_out, st)
        } else {
            let saved = save_screen_library_bmp(raw_rgb565, name, true, reuse_uuid);
            // 固件 SysTick (0x0004091e: cmp.w r1, #0x7d0) 需静默满 2000ms 才清零 flag_4794 并刷新 LCD
            thread::sleep(Duration::from_millis(2100));
            saved
        };

        Ok(json!({
            "ok": true,
            "mock": self.t.is_mock(),
            "slot": 0,
            "uuid": saved_uuid,
            "bytesSent": SCREEN_FLASH_BLOCK,
            "config": decode_kb_config(&next_cfg),
            "screenImage": screen_image,
        }))
    }

    pub fn save_screen_to_library(&mut self, raw_rgb565: &[u8], name: &str) -> Result<Value, TransportError> {
        if raw_rgb565.len() < SCREEN_PIXEL_BYTES {
            return Err(TransportError::Io(format!(
                "小屏图像数据不足：需要 {SCREEN_PIXEL_BYTES} 字节"
            )));
        }
        if self.t.is_mock() {
            let bmp = rgb565_to_bmp(raw_rgb565);
            let data_url = format!(
                "data:image/bmp;base64,{}",
                base64::engine::general_purpose::STANDARD.encode(&bmp)
            );
            let mut st = self
                .mock_screen
                .clone()
                .unwrap_or_else(read_persisted_screen_state);
            let uuid = make_goper_uuid();
            let fb = format!("ScreenFile_{}", st.items.len());
            st.items.push(ScreenLibraryItem {
                index: st.items.len(),
                uuid: uuid.clone(),
                name: sanitize_label(name, &fb),
                is_linked: false,
                file_path: None,
                data_url,
            });
            self.mock_screen = Some(st.clone());
            return Ok(json!({ "ok": true, "uuid": uuid, "screenImage": st }));
        }
        let (uuid, screen_image) = save_screen_library_bmp(raw_rgb565, name, false, None);
        Ok(json!({ "ok": true, "uuid": uuid, "screenImage": screen_image }))
    }

    pub fn delete_screen_library_item(&mut self, uuid: &str) -> Result<Value, TransportError> {
        if uuid.is_empty() {
            return Err(TransportError::Io("缺少图片 UUID".to_string()));
        }
        if self.t.is_mock() {
            let mut st = self
                .mock_screen
                .clone()
                .unwrap_or_else(read_persisted_screen_state);
            st.items.retain(|it| it.uuid != uuid);
            let linked = st.items.iter().find(|it| it.is_linked).cloned();
            let fallback = st.items.first().cloned();
            st.has_linked_cache = linked.is_some();
            st.current_uuid = linked
                .as_ref()
                .map(|x| x.uuid.clone())
                .or_else(|| fallback.as_ref().map(|x| x.uuid.clone()));
            st.current_data_url = linked
                .as_ref()
                .map(|x| x.data_url.clone())
                .or_else(|| fallback.as_ref().map(|x| x.data_url.clone()))
                .or_else(|| st.default_data_url.clone());
            st.current_name = linked
                .map(|x| format!("本机已同步 ({})", x.name))
                .unwrap_or_else(|| "本机无写入记录（硬件不支持 USB 读屏，暂显示默认预览）".to_string());
            self.mock_screen = Some(st.clone());
            return Ok(json!({ "ok": true, "screenImage": st }));
        }
        let screen_image = delete_screen_library_bmp(uuid);
        Ok(json!({ "ok": true, "screenImage": screen_image }))
    }
}
