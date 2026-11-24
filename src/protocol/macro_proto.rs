//! 板载宏协议编解码器（严格对齐官方 Goper.exe V1.35 `0x515d40` / `0x524190` / `0x48a5f0` / `0x44543a`）：
//!
//! 1. **OP 0x15 宏表二进制布局**：
//!    - `0x00..0x02`: `0x55AA_u16.to_le_bytes()` (`[0xAA, 0x55]`)
//!    - `0x02..0x04`: `total_len: u16` (LE) = `16 + N*2 + sum(4 + E_i*4)`
//!    - `0x04..0x06`: `macro_count: u16` (LE) = `N`
//!    - `0x06..0x10`: 10 字节保留 0
//!    - `0x10..0x10 + N*2`: `N` 个 `u16` (LE) 偏移量，指向每条宏块起始位置
//!    - 每条宏块：`[E_lo, E_hi, 0x00, 0x00]` + `E_i × 4B` 事件数组
//!    - 容量上限：由 `READ_INFO (OP 0x03)` 第 6 字节给出 `max_bytes = info[6] << 7`（GK001S / GS001S 均为 `0x18 << 7 = 3072B`）
//!
//! 2. **4 字节宏事件编码 (`Goper.exe 0x48a5f0`)**：
//!    - `ev[0..2]` = `delay_ms.to_le_bytes()` (`0..=65535 ms`)
//!    - `ev[2]` = `((down as u8) << 7) | (type_code & 0x7F)`
//!    - `ev[3]` = `button_or_delta`
//!      - 标准键盘键 (`kind = "key"`): `type_code = 0x0A`, `code = USB HID Usage ID (0x04..0x65)`
//!      - 修饰键 (`kind = "modifier"`): `type_code = 0x09`, `code = HID Modifier Bitmask (0x01..0x80)`
//!      - 鼠标按键 (`kind = "mouse"`): `type_code = 0x01`, `code = Mouse Bitmask (0x01..0x10)`
//!      - 鼠标横向移动 (`kind = "moveX"`): `type_code = 0x04`, `down = true(左 -X)/false(右 +X)`, `code = 1..=255`
//!      - 鼠标纵向移动 (`kind = "moveY"`): `type_code = 0x05`, `down = true(上 -Y)/false(下 +Y)`, `code = 1..=255`
//!
//! 3. **OP 0x09 键位绑定 3 字节编码 (`Goper.exe 0x44543a`)**：
//!    - 模式 `0..=3`: `[0x70, macro_index, mode]`
//!      - `0`: 正常（单次播放）
//!      - `1`: 释放停止（松开按键后播完本次）
//!      - `2`: 按下停止（按一下循环，再按一下停止）
//!      - `3`: 释放立即停止（按住循环，松开即停）
//!    - 模式 `4` (播放指定次数 `1..=255`): `[0x71, macro_index, repeat_count]`

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::protocol::usage::hid_key_label;
use crate::protocol::DEFAULT_MACRO_FLASH_BYTES;

pub const EVENT_TYPE_MOUSE: u8 = 0x01;
pub const EVENT_TYPE_MOVE_X: u8 = 0x04;
pub const EVENT_TYPE_MOVE_Y: u8 = 0x05;
pub const EVENT_TYPE_MODIFIER: u8 = 0x09;
pub const EVENT_TYPE_KEY: u8 = 0x0A;

pub const TRIGGER_ONCE: u8 = 0;
pub const TRIGGER_STOP_ON_RELEASE: u8 = 1;
pub const TRIGGER_TOGGLE: u8 = 2;
pub const TRIGGER_STOP_IMMEDIATE: u8 = 3;
pub const TRIGGER_REPEAT_COUNT: u8 = 4;

fn default_fixed_delay() -> u16 {
    20
}

fn default_delay_mode() -> String {
    "recorded".to_string()
}

fn default_library_version() -> u32 {
    1
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroEvent {
    pub kind: String,
    pub code: u8,
    pub down: bool,
    pub delay_ms: u16,
    #[serde(default)]
    pub label: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroDef {
    pub id: String,
    pub name: String,
    #[serde(default = "default_delay_mode")]
    pub delay_mode: String,
    #[serde(default = "default_fixed_delay")]
    pub fixed_delay_ms: u16,
    #[serde(default)]
    pub events: Vec<MacroEvent>,
    #[serde(default)]
    pub updated_at: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct MacroLibrary {
    #[serde(default = "default_library_version")]
    pub version: u32,
    #[serde(default)]
    pub macros: Vec<MacroDef>,
}

impl Default for MacroLibrary {
    fn default() -> Self {
        Self {
            version: default_library_version(),
            macros: Vec::new(),
        }
    }
}

pub fn macro_flash_capacity_from_info(info: Option<&[u8]>) -> usize {
    info.and_then(|b| b.get(6).copied())
        .filter(|&v| v > 0)
        .map(|v| (v as usize) << 7)
        .unwrap_or(DEFAULT_MACRO_FLASH_BYTES)
}

pub fn modifier_label(bit: u8) -> Option<&'static str> {
    match bit {
        0x01 => Some("左 Ctrl"),
        0x02 => Some("左 Shift"),
        0x04 => Some("左 Alt"),
        0x08 => Some("左 Win"),
        0x10 => Some("右 Ctrl"),
        0x20 => Some("右 Shift"),
        0x40 => Some("右 Alt"),
        0x80 => Some("右 Win"),
        _ => None,
    }
}

pub fn mouse_button_label(bit: u8) -> Option<&'static str> {
    match bit {
        0x01 => Some("鼠标左键"),
        0x02 => Some("鼠标右键"),
        0x04 => Some("鼠标中键"),
        0x08 => Some("鼠标后退"),
        0x10 => Some("鼠标前进"),
        _ => None,
    }
}

pub fn event_label(kind: &str, code: u8, down: bool) -> String {
    match kind {
        "key" => hid_key_label(code)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Key 0x{code:02X}")),
        "modifier" => modifier_label(code)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Mod 0x{code:02X}")),
        "mouse" => mouse_button_label(code)
            .map(str::to_string)
            .unwrap_or_else(|| format!("Mouse 0x{code:02X}")),
        "moveX" => format!("鼠标{} {} px", if down { "左移" } else { "右移" }, code.max(1)),
        "moveY" => format!("鼠标{} {} px", if down { "上移" } else { "下移" }, code.max(1)),
        _ => format!("0x{code:02X}"),
    }
}

pub fn normalize_macro_event(ev: &MacroEvent) -> Option<MacroEvent> {
    let kind = match ev.kind.as_str() {
        "key" | "modifier" | "mouse" | "moveX" | "moveY" => ev.kind.clone(),
        _ => return None,
    };
    let code = if kind == "moveX" || kind == "moveY" {
        ev.code.max(1)
    } else if ev.code == 0 {
        return None;
    } else {
        ev.code
    };
    let label = if ev.label.trim().is_empty() {
        event_label(&kind, code, ev.down)
    } else {
        ev.label.trim().to_string()
    };
    Some(MacroEvent {
        kind,
        code,
        down: ev.down,
        delay_ms: ev.delay_ms,
        label,
    })
}

pub fn normalize_macro_def(m: &MacroDef, fallback_idx: usize) -> MacroDef {
    let id = if m.id.trim().is_empty() {
        format!("macro-{}", fallback_idx + 1)
    } else {
        m.id.trim().to_string()
    };
    let name = if m.name.trim().is_empty() {
        format!("宏 {}", fallback_idx + 1)
    } else {
        m.name.trim().chars().take(40).collect()
    };
    let delay_mode = match m.delay_mode.as_str() {
        "recorded" | "fixed" | "none" => m.delay_mode.clone(),
        _ => "recorded".to_string(),
    };
    let fixed_delay_ms = m.fixed_delay_ms.clamp(1, 10_000);
    let events = m.events.iter().filter_map(normalize_macro_event).collect();
    MacroDef {
        id,
        name,
        delay_mode,
        fixed_delay_ms,
        events,
        updated_at: m.updated_at,
    }
}

pub fn encode_macro_event(ev: &MacroEvent) -> Result<[u8; 4], String> {
    let type_code = match ev.kind.as_str() {
        "key" => EVENT_TYPE_KEY,
        "modifier" => EVENT_TYPE_MODIFIER,
        "mouse" => EVENT_TYPE_MOUSE,
        "moveX" => EVENT_TYPE_MOVE_X,
        "moveY" => EVENT_TYPE_MOVE_Y,
        other => return Err(format!("不支持的宏事件类型: {other}")),
    };
    let delay_bytes = ev.delay_ms.to_le_bytes();
    let flags_type = (if ev.down { 0x80 } else { 0x00 }) | (type_code & 0x7F);
    Ok([delay_bytes[0], delay_bytes[1], flags_type, ev.code])
}

pub fn compute_macro_table_size(macros: &[MacroDef]) -> usize {
    let n = macros.len();
    let body: usize = macros.iter().map(|m| 4 + m.events.len() * 4).sum();
    16 + n * 2 + body
}

/// 将宏列表编码为官方 OP 0x15 板载宏表二进制块
pub fn encode_macro_table(macros: &[MacroDef], max_bytes: usize) -> Result<Vec<u8>, String> {
    let n = macros.len();
    if n > 255 {
        return Err(format!("宏数量超出硬件上限：最多 255 条，当前 {n} 条"));
    }
    let total_len = compute_macro_table_size(macros);
    if total_len > max_bytes {
        let over_bytes = total_len - max_bytes;
        let over_events = (over_bytes + 3) / 4;
        return Err(format!(
            "宏数据超出设备板载 Flash 容量（{total_len} B / {max_bytes} B），请精简约 {over_events} 条动作或删除不用的宏"
        ));
    }
    if total_len > u16::MAX as usize {
        return Err(format!("宏表总长度超出 64KB 协议上限：{total_len} B"));
    }

    let mut buf = vec![0u8; total_len];
    // 16B 表头
    buf[0..2].copy_from_slice(&0x55AA_u16.to_le_bytes()); // [0xAA, 0x55]
    buf[2..4].copy_from_slice(&(total_len as u16).to_le_bytes());
    buf[4..6].copy_from_slice(&(n as u16).to_le_bytes());

    let mut cursor = 16 + n * 2;
    for (i, m) in macros.iter().enumerate() {
        let off_pos = 16 + i * 2;
        buf[off_pos..off_pos + 2].copy_from_slice(&(cursor as u16).to_le_bytes());

        let ev_count = m.events.len();
        if ev_count > u16::MAX as usize {
            return Err(format!("宏「{}」动作数超出上限", m.name));
        }
        buf[cursor..cursor + 2].copy_from_slice(&(ev_count as u16).to_le_bytes());
        buf[cursor + 2] = 0;
        buf[cursor + 3] = 0;
        cursor += 4;

        for ev in &m.events {
            let encoded = encode_macro_event(ev)?;
            buf[cursor..cursor + 4].copy_from_slice(&encoded);
            cursor += 4;
        }
    }

    debug_assert_eq!(cursor, total_len);
    Ok(buf)
}

/// 编码 3 字节宏键位绑定值（对齐 Goper.exe `0x44543a`）
pub fn encode_macro_assignment(
    macro_index: u8,
    trigger_mode: u8,
    repeat_count: u8,
) -> Result<[u8; 3], String> {
    match trigger_mode {
        TRIGGER_ONCE | TRIGGER_STOP_ON_RELEASE | TRIGGER_TOGGLE | TRIGGER_STOP_IMMEDIATE => {
            Ok([0x70, macro_index, trigger_mode])
        }
        TRIGGER_REPEAT_COUNT => Ok([0x71, macro_index, repeat_count.max(1)]),
        other => Err(format!("无效的宏触发模式：{other}")),
    }
}

pub fn encode_macro_assignment_hex(
    macro_index: u8,
    trigger_mode: u8,
    repeat_count: u8,
) -> Result<String, String> {
    let b = encode_macro_assignment(macro_index, trigger_mode, repeat_count)?;
    Ok(format!("{:02x}{:02x}{:02x}", b[0], b[1], b[2]))
}

/// 解析官方 Goper `.mac` JSON 或 REDMAGIC Control 导出的宏 JSON
pub fn parse_imported_macros(raw: &str, fallback_name: &str) -> Result<Vec<MacroDef>, String> {
    let val: Value =
        serde_json::from_str(raw.trim()).map_err(|e| format!("宏文件 JSON 解析失败: {e}"))?;

    // 1. REDMAGIC Control 完整宏库格式: { "macros": [ ... ] }
    if let Some(arr) = val.get("macros").and_then(Value::as_array) {
        let mut out = Vec::new();
        for (i, item) in arr.iter().enumerate() {
            let m: MacroDef = serde_json::from_value(item.clone())
                .map_err(|e| format!("第 {} 条宏格式错误: {e}", i + 1))?;
            out.push(normalize_macro_def(&m, i));
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    // 2. REDMAGIC Control 单条宏格式: { "name": "...", "events": [ ... ] }
    if val.get("events").and_then(Value::as_array).is_some() {
        let m: MacroDef =
            serde_json::from_value(val.clone()).map_err(|e| format!("单条宏解析失败: {e}"))?;
        return Ok(vec![normalize_macro_def(&m, 0)]);
    }

    // 3. 官方 Goper.exe `.mac` 格式: { "Events": [ { "Action": "down", "Button": 4, "Delay": "10", "Type": 10 }, ... ] }
    if let Some(arr) = val.get("Events").and_then(Value::as_array) {
        let mut events = Vec::with_capacity(arr.len());
        for ev in arr {
            let action = ev.get("Action").and_then(Value::as_str).unwrap_or("down");
            let down = action.eq_ignore_ascii_case("down");
            let button = ev
                .get("Button")
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0) as u8;
            let delay_ms = ev
                .get("Delay")
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0)
                .min(u16::MAX as u64) as u16;
            let type_code = ev
                .get("Type")
                .and_then(|v| v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok())))
                .unwrap_or(0) as u8;

            let kind = match type_code {
                EVENT_TYPE_KEY => "key",
                EVENT_TYPE_MODIFIER => "modifier",
                EVENT_TYPE_MOUSE => "mouse",
                EVENT_TYPE_MOVE_X => "moveX",
                EVENT_TYPE_MOVE_Y => "moveY",
                _ => continue,
            };
            if let Some(norm) = normalize_macro_event(&MacroEvent {
                kind: kind.to_string(),
                code: button,
                down,
                delay_ms,
                label: String::new(),
            }) {
                events.push(norm);
            }
        }
        let name = val
            .get("Name")
            .and_then(Value::as_str)
            .filter(|s| !s.trim().is_empty())
            .unwrap_or(fallback_name)
            .to_string();
        return Ok(vec![normalize_macro_def(
            &MacroDef {
                id: String::new(),
                name,
                delay_mode: "recorded".to_string(),
                fixed_delay_ms: 20,
                events,
                updated_at: 0,
            },
            0,
        )]);
    }

    // 4. 顶层数组格式: [ MacroDef, ... ]
    if let Some(arr) = val.as_array() {
        let mut out = Vec::new();
        for (i, item) in arr.iter().enumerate() {
            let m: MacroDef = serde_json::from_value(item.clone())
                .map_err(|e| format!("第 {} 条宏格式错误: {e}", i + 1))?;
            out.push(normalize_macro_def(&m, i));
        }
        if !out.is_empty() {
            return Ok(out);
        }
    }

    Err("未识别的宏文件格式（支持 REDMAGIC Control JSON 与官方 Goper .mac 文件）".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_event_matches_goper_48a5f0() {
        // 标准按键 A (0x04) 按下, 延时 25ms (0x0019)
        let ev_key_down = MacroEvent {
            kind: "key".into(),
            code: 0x04,
            down: true,
            delay_ms: 25,
            label: "A".into(),
        };
        assert_eq!(encode_macro_event(&ev_key_down).unwrap(), [0x19, 0x00, 0x8A, 0x04]);

        // 标准按键 A 弹起, 延时 300ms (0x012C)
        let ev_key_up = MacroEvent {
            kind: "key".into(),
            code: 0x04,
            down: false,
            delay_ms: 300,
            label: "A".into(),
        };
        assert_eq!(encode_macro_event(&ev_key_up).unwrap(), [0x2C, 0x01, 0x0A, 0x04]);

        // 左 Ctrl (0x01) 修饰键按下 (Type = 9 -> 0x89)
        let ev_mod = MacroEvent {
            kind: "modifier".into(),
            code: 0x01,
            down: true,
            delay_ms: 10,
            label: "左 Ctrl".into(),
        };
        assert_eq!(encode_macro_event(&ev_mod).unwrap(), [0x0A, 0x00, 0x89, 0x01]);

        // 鼠标左键按下 (Type = 1 -> 0x81, Button = 0x01)
        let ev_ms = MacroEvent {
            kind: "mouse".into(),
            code: 0x01,
            down: true,
            delay_ms: 15,
            label: "鼠标左键".into(),
        };
        assert_eq!(encode_macro_event(&ev_ms).unwrap(), [0x0F, 0x00, 0x81, 0x01]);

        // 鼠标下移 12px (Type = 5, down=false -> 0x05)
        let ev_move_down = MacroEvent {
            kind: "moveY".into(),
            code: 12,
            down: false,
            delay_ms: 8,
            label: String::new(),
        };
        assert_eq!(encode_macro_event(&ev_move_down).unwrap(), [0x08, 0x00, 0x05, 12]);

        // 鼠标左移 20px (Type = 4, down=true -> 0x84)
        let ev_move_left = MacroEvent {
            kind: "moveX".into(),
            code: 20,
            down: true,
            delay_ms: 5,
            label: String::new(),
        };
        assert_eq!(encode_macro_event(&ev_move_left).unwrap(), [0x05, 0x00, 0x84, 20]);
    }

    #[test]
    fn encode_macro_table_layout_matches_goper_515d40() {
        let m0 = MacroDef {
            id: "m1".into(),
            name: "Tap A".into(),
            delay_mode: "recorded".into(),
            fixed_delay_ms: 20,
            events: vec![
                MacroEvent { kind: "key".into(), code: 0x04, down: true, delay_ms: 20, label: "A".into() },
                MacroEvent { kind: "key".into(), code: 0x04, down: false, delay_ms: 0, label: "A".into() },
            ],
            updated_at: 0,
        };
        let m1 = MacroDef {
            id: "m2".into(),
            name: "Click".into(),
            delay_mode: "fixed".into(),
            fixed_delay_ms: 10,
            events: vec![
                MacroEvent { kind: "mouse".into(), code: 0x01, down: true, delay_ms: 10, label: "左键".into() },
                MacroEvent { kind: "mouse".into(), code: 0x01, down: false, delay_ms: 10, label: "左键".into() },
            ],
            updated_at: 0,
        };

        let blob = encode_macro_table(&[m0, m1], 3072).unwrap();
        // total_len = 16 + 2*2 + (4 + 2*4) + (4 + 2*4) = 16 + 4 + 12 + 12 = 44 (0x002C)
        assert_eq!(blob.len(), 44);
        // Header: AA 55, 2C 00, 02 00, 10 zero bytes
        assert_eq!(&blob[0..6], &[0xAA, 0x55, 0x2C, 0x00, 0x02, 0x00]);
        assert_eq!(&blob[6..16], &[0u8; 10]);
        // Offset table: m0 at 20 (0x0014), m1 at 32 (0x0020)
        assert_eq!(u16::from_le_bytes([blob[16], blob[17]]), 20);
        assert_eq!(u16::from_le_bytes([blob[18], blob[19]]), 32);
        // Block 0 @ 20: event_count=2, 0, 0, then 2 events
        assert_eq!(&blob[20..24], &[0x02, 0x00, 0x00, 0x00]);
        assert_eq!(&blob[24..28], &[0x14, 0x00, 0x8A, 0x04]);
        assert_eq!(&blob[28..32], &[0x00, 0x00, 0x0A, 0x04]);
        // Block 1 @ 32: event_count=2, 0, 0, then 2 events
        assert_eq!(&blob[32..36], &[0x02, 0x00, 0x00, 0x00]);
        assert_eq!(&blob[36..40], &[0x0A, 0x00, 0x81, 0x01]);
        assert_eq!(&blob[40..44], &[0x0A, 0x00, 0x01, 0x01]);
    }

    #[test]
    fn encode_macro_table_enforces_capacity() {
        let huge = MacroDef {
            id: "big".into(),
            name: "Huge".into(),
            delay_mode: "fixed".into(),
            fixed_delay_ms: 10,
            events: vec![
                MacroEvent { kind: "key".into(), code: 0x04, down: true, delay_ms: 10, label: "A".into() };
                800
            ],
            updated_at: 0,
        };
        let err = encode_macro_table(&[huge], 3072).unwrap_err();
        assert!(err.contains("超出设备板载 Flash 容量"));
    }

    #[test]
    fn macro_capacity_parsed_from_real_read_info() {
        let kb_info = hex::decode("aa550000017e180001953332803e64000105014000ac1100").unwrap();
        let ms_info = hex::decode("aa550000010c180001953332906564000105010600000000").unwrap();
        assert_eq!(macro_flash_capacity_from_info(Some(&kb_info)), 3072);
        assert_eq!(macro_flash_capacity_from_info(Some(&ms_info)), 3072);
        assert_eq!(macro_flash_capacity_from_info(None), 3072);
    }

    #[test]
    fn macro_assignment_encoding_matches_goper_44543a() {
        assert_eq!(encode_macro_assignment_hex(0, TRIGGER_ONCE, 1).unwrap(), "700000");
        assert_eq!(encode_macro_assignment_hex(2, TRIGGER_STOP_ON_RELEASE, 1).unwrap(), "700201");
        assert_eq!(encode_macro_assignment_hex(3, TRIGGER_TOGGLE, 1).unwrap(), "700302");
        assert_eq!(encode_macro_assignment_hex(1, TRIGGER_STOP_IMMEDIATE, 1).unwrap(), "700103");
        assert_eq!(encode_macro_assignment_hex(4, TRIGGER_REPEAT_COUNT, 5).unwrap(), "710405");
    }

    #[test]
    fn parse_goper_mac_json_file() {
        let goper_json = r#"{
            "Events": [
                { "Action": "down", "Button": 1, "Delay": "10", "Type": 9 },
                { "Action": "down", "Button": 6, "Delay": "15", "Type": 10 },
                { "Action": "up", "Button": 6, "Delay": "10", "Type": 10 },
                { "Action": "up", "Button": 1, "Delay": "0", "Type": 9 }
            ]
        }"#;
        let parsed = parse_imported_macros(goper_json, "复制宏").unwrap();
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].name, "复制宏");
        assert_eq!(parsed[0].events.len(), 4);
        assert_eq!(parsed[0].events[0].kind, "modifier");
        assert_eq!(parsed[0].events[0].code, 1);
        assert_eq!(parsed[0].events[0].label, "左 Ctrl");
        assert_eq!(parsed[0].events[1].kind, "key");
        assert_eq!(parsed[0].events[1].code, 6);
        assert_eq!(parsed[0].events[1].label, "C");
    }
}
