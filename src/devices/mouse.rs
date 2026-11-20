//! GS001S 鼠标真机适配器（84B 配置块 OP 0x05/0x06 + 36B 键表 OP 0x07/0x08/0x09）。

use serde_json::{json, Value};

use crate::devices::catalog::ConnectionMode;
use crate::devices::transport::{default_ms_config, Transport, TransportError};
use crate::protocol::usage::{build_default_ms_rom, bytes_to_hex3, decode_assignment, hex3_to_bytes};
use crate::protocol::{
    POLL_STATUS, READ_CONFIG, READ_INFO, READ_KEYMAP_ROM, READ_KEYMAP_USER, WRITE_CONFIG,
    WRITE_KEYMAP,
};

pub const CONFIG_LEN: usize = 0x54; // 84 字节
pub const KEY_SLOTS: usize = 12;
pub const KEYMAP_LEN: usize = 0x24; // 36 字节

const DEFAULT_STAGE_DPI: [u16; 5] = [400, 800, 1600, 2400, 3200];
const DEFAULT_STAGE_COLORS: [&str; 5] = ["#0000ff", "#00ffff", "#00ff00", "#ff0000", "#ffff00"];

fn clamp_u8(v: i64, lo: i64, hi: i64) -> u8 {
    v.clamp(lo, hi) as u8
}

fn clamp_u16(v: i64, lo: i64, hi: i64) -> u16 {
    v.clamp(lo, hi) as u16
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

pub fn decode_ms_config(buf: &[u8]) -> Value {
    let b = |i: usize| -> u8 { buf.get(i).copied().unwrap_or(0) };
    let u16_le = |i: usize| -> u16 { u16::from_le_bytes([b(i), b(i + 1)]) };

    let mut dpi_stages = Vec::with_capacity(5);
    for i in 0..5 {
        let o = 14 + i * 9;
        let raw_x = u16_le(o + 2);
        let raw_y = u16_le(o + 4);
        let has_rgb = (b(o + 6) | b(o + 7) | b(o + 8)) != 0;
        dpi_stages.push(json!({
            "index": i,
            "enabled": if raw_x > 0 { b(o) != 0 } else { true },
            "xySplit": b(o + 1) != 0,
            "dpiX": if raw_x > 0 { raw_x } else { DEFAULT_STAGE_DPI[i] },
            "dpiY": if raw_y > 0 { raw_y } else { DEFAULT_STAGE_DPI[i] },
            "color": if has_rgb { rgb_to_hex(b(o + 6), b(o + 7), b(o + 8)) } else { DEFAULT_STAGE_COLORS[i].to_string() },
        }));
    }

    json!({
        "profile": b(0),
        "light": {
            "mode": b(1),
            "brightness": clamp_u8(b(2) as i64, 0, 4),
            "speed": clamp_u8(b(3) as i64, 0, 4),
            "direction": b(4),
            "multiColor": b(5) != 0,
            "color": rgb_to_hex(b(6), b(7), b(8)),
        },
        "performance": {
            "polling24g": b(10),
            "pollingWired": b(11),
            "activeDpi": clamp_u8(b(12) as i64, 0, 4),
            "lod": b(13),
            "ripple": b(62) != 0,
            "sleepSec": u16_le(65),
        },
        "dpiStages": dpi_stages,
        "rawHex": hex::encode(buf),
    })
}

pub fn apply_ms_config_patch(base: &[u8], patch: &Value) -> Vec<u8> {
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
            buf[3] = clamp_u8(v, 0, 4);
        }
        if let Some(v) = l.get("direction").and_then(Value::as_i64) {
            buf[4] = (v & 0xff) as u8;
        }
        if let Some(v) = l.get("multiColor").and_then(Value::as_bool) {
            buf[5] = u8::from(v);
        }
        if let Some(s) = l.get("color").and_then(Value::as_str) {
            let (r, g, b) = hex_to_rgb(s);
            buf[6] = r;
            buf[7] = g;
            buf[8] = b;
        }
    }

    if let Some(p) = patch.get("performance").and_then(Value::as_object) {
        if let Some(v) = p.get("polling24g").and_then(Value::as_i64) {
            buf[10] = clamp_u8(v, 0, 6);
        }
        if let Some(v) = p.get("pollingWired").and_then(Value::as_i64) {
            buf[11] = clamp_u8(v, 0, 6);
        }
        if let Some(v) = p.get("activeDpi").and_then(Value::as_i64) {
            buf[12] = clamp_u8(v, 0, 4);
        }
        if let Some(v) = p.get("lod").and_then(Value::as_i64) {
            buf[13] = clamp_u8(v, 1, 2);
        }
        if let Some(v) = p.get("ripple").and_then(Value::as_bool) {
            buf[62] = u8::from(v);
        }
        if let Some(v) = p.get("sleepSec").and_then(Value::as_i64) {
            buf[65..67].copy_from_slice(&clamp_u16(v, 0, 3600).to_le_bytes());
        }
    }

    if let Some(arr) = patch.get("dpiStages").and_then(Value::as_array) {
        for st in arr {
            let Some(idx) = st.get("index").and_then(Value::as_u64).map(|x| x as usize) else {
                continue;
            };
            if idx >= 5 {
                continue;
            }
            let o = 14 + idx * 9;
            if let Some(v) = st.get("enabled").and_then(Value::as_bool) {
                buf[o] = u8::from(v);
            }
            if let Some(v) = st.get("xySplit").and_then(Value::as_bool) {
                buf[o + 1] = u8::from(v);
            }
            if let Some(v) = st.get("dpiX").and_then(Value::as_i64) {
                buf[o + 2..o + 4].copy_from_slice(&clamp_u16(v, 50, 26000).to_le_bytes());
            }
            if let Some(v) = st.get("dpiY").and_then(Value::as_i64) {
                buf[o + 4..o + 6].copy_from_slice(&clamp_u16(v, 50, 26000).to_le_bytes());
            }
            if let Some(s) = st.get("color").and_then(Value::as_str) {
                let (r, g, b) = hex_to_rgb(s);
                buf[o + 6] = r;
                buf[o + 7] = g;
                buf[o + 8] = b;
            }
        }
    }

    buf
}

pub struct MouseAdapter {
    pub t: Transport,
    pub link_id: String,
    rom_cache: Option<Vec<u8>>,
    last_state: Option<Value>,
}

impl MouseAdapter {
    pub fn new(transport: Transport, link_id: String, cached_state: Option<Value>) -> Self {
        Self {
            t: transport,
            link_id,
            rom_cache: None,
            last_state: cached_state,
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

        let rom_buf = build_default_ms_rom();
        let mut keymap = Vec::with_capacity(6);
        for slot in 0..6 {
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
            "kind": "mouse",
            "mock": self.t.is_mock(),
            "connectionMode": self.t.mode.as_str(),
            "linkState": link_state,
            "supportsConfig": false,
            "chunkSize": self.t.chunk_size,
            "btAddress": self.t.bt_address,
            "battery": battery,
            "chargingState": charging_state,
            "capabilityHex": Value::Null,
            "hwKeyCount": KEY_SLOTS,
            "config": decode_ms_config(&default_ms_config()),
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

        let mut keymap = Vec::with_capacity(6);
        for slot in 0..6 {
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

        let full = json!({
            "kind": "mouse",
            "mock": self.t.is_mock(),
            "connectionMode": self.t.mode.as_str(),
            "linkState": "online",
            "supportsConfig": self.t.supports_config,
            "chunkSize": self.t.chunk_size,
            "btAddress": self.t.bt_address,
            "battery": battery,
            "chargingState": charging,
            "capabilityHex": capability_hex,
            "hwKeyCount": hw_key_count,
            "config": decode_ms_config(&cfg_buf),
            "keymap": keymap,
            "staleConfig": false,
        });
        self.last_state = Some(full.clone());
        Ok(full)
    }

    pub fn write_config(&mut self, patch: &Value) -> Result<Value, TransportError> {
        self.ensure_writable()?;
        let current = self.t.read_blob(READ_CONFIG, CONFIG_LEN, 0)?;
        let next = apply_ms_config_patch(&current, patch);
        self.t.write_blob_tx(WRITE_CONFIG, &next, 0)?;
        let verify = self.t.read_blob(READ_CONFIG, CONFIG_LEN, 0)?;
        let decoded = decode_ms_config(&verify);
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
        if slot >= 6 {
            return Err(TransportError::Io(format!("无效鼠标按键槽位：{slot}")));
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
        if slot >= 6 {
            return Err(TransportError::Io(format!("无效鼠标按键槽位：{slot}")));
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
}
