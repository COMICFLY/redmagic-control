//! 硬件 3 字节大端键值（Assignment）编解码与 GK001S / GS001S 出厂 ROM 默认表。

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct DecodedAssignment {
    pub hex: String,
    pub kind: String,
    pub label: String,
}

pub fn hid_key_label(code: u8) -> Option<&'static str> {
    match code {
        0x04 => Some("A"), 0x05 => Some("B"), 0x06 => Some("C"), 0x07 => Some("D"),
        0x08 => Some("E"), 0x09 => Some("F"), 0x0a => Some("G"), 0x0b => Some("H"),
        0x0c => Some("I"), 0x0d => Some("J"), 0x0e => Some("K"), 0x0f => Some("L"),
        0x10 => Some("M"), 0x11 => Some("N"), 0x12 => Some("O"), 0x13 => Some("P"),
        0x14 => Some("Q"), 0x15 => Some("R"), 0x16 => Some("S"), 0x17 => Some("T"),
        0x18 => Some("U"), 0x19 => Some("V"), 0x1a => Some("W"), 0x1b => Some("X"),
        0x1c => Some("Y"), 0x1d => Some("Z"),
        0x1e => Some("1"), 0x1f => Some("2"), 0x20 => Some("3"), 0x21 => Some("4"),
        0x22 => Some("5"), 0x23 => Some("6"), 0x24 => Some("7"), 0x25 => Some("8"),
        0x26 => Some("9"), 0x27 => Some("0"),
        0x28 => Some("Enter"), 0x29 => Some("Esc"), 0x2a => Some("Backspace"),
        0x2b => Some("Tab"), 0x2c => Some("Space"),
        0x2d => Some("-"), 0x2e => Some("="), 0x2f => Some("["), 0x30 => Some("]"),
        0x31 => Some("\\"), 0x32 => Some("#"), 0x33 => Some(";"), 0x34 => Some("'"),
        0x35 => Some("`"), 0x36 => Some(","), 0x37 => Some("."), 0x38 => Some("/"),
        0x39 => Some("Caps Lock"),
        0x3a => Some("F1"), 0x3b => Some("F2"), 0x3c => Some("F3"), 0x3d => Some("F4"),
        0x3e => Some("F5"), 0x3f => Some("F6"), 0x40 => Some("F7"), 0x41 => Some("F8"),
        0x42 => Some("F9"), 0x43 => Some("F10"), 0x44 => Some("F11"), 0x45 => Some("F12"),
        0x46 => Some("Print"), 0x47 => Some("Scroll"), 0x48 => Some("Pause"),
        0x49 => Some("Insert"), 0x4a => Some("Home"), 0x4b => Some("Page Up"),
        0x4c => Some("Delete"), 0x4d => Some("End"), 0x4e => Some("Page Down"),
        0x4f => Some("Right"), 0x50 => Some("Left"), 0x51 => Some("Down"), 0x52 => Some("Up"),
        0x53 => Some("Num Lock"), 0x54 => Some("Numpad /"), 0x55 => Some("Numpad *"),
        0x56 => Some("Numpad -"), 0x57 => Some("Numpad +"), 0x58 => Some("Numpad Enter"),
        0x59 => Some("Numpad 1"), 0x5a => Some("Numpad 2"), 0x5b => Some("Numpad 3"),
        0x5c => Some("Numpad 4"), 0x5d => Some("Numpad 5"), 0x5e => Some("Numpad 6"),
        0x5f => Some("Numpad 7"), 0x60 => Some("Numpad 8"), 0x61 => Some("Numpad 9"),
        0x62 => Some("Numpad 0"), 0x63 => Some("Numpad ."), 0x64 => Some("ISO \\"),
        0x65 => Some("Menu"),
        _ => None,
    }
}

const MODIFIERS: &[(u8, &str)] = &[
    (0x01, "Ctrl"),
    (0x02, "Shift"),
    (0x04, "Alt"),
    (0x08, "Win"),
    (0x10, "Ctrl Right"),
    (0x20, "Shift Right"),
    (0x40, "Alt Right"),
    (0x80, "Win Right"),
];

pub fn bytes_to_hex3(buf: &[u8], offset: usize) -> String {
    if offset + 3 > buf.len() {
        return "000000".to_string();
    }
    format!(
        "{:02x}{:02x}{:02x}",
        buf[offset],
        buf[offset + 1],
        buf[offset + 2]
    )
}

pub fn hex3_to_bytes(hex_str: &str) -> [u8; 3] {
    let clean = hex_str.trim().trim_start_matches("0x").trim_start_matches("0X");
    let padded = format!("{:0>6}", clean.to_ascii_lowercase());
    let mut out = [0u8; 3];
    if let Ok(decoded) = hex::decode(&padded[..6]) {
        if decoded.len() == 3 {
            out.copy_from_slice(&decoded);
        }
    }
    out
}

pub fn decode_assignment(hex3: &str) -> DecodedAssignment {
    let hex = format!("{:0>6}", hex3.trim().to_ascii_lowercase());
    let hex = hex[..6].to_string();

    let special = match hex.as_str() {
        "a00000" => Some(("disable", "已禁用")),
        "a00100" => Some(("special", "Fn")),
        "a02200" => Some(("special", "旋钮")),
        "000000" | "200000" => Some(("special", "空")),
        _ => None,
    };
    if let Some((kind, label)) = special {
        return DecodedAssignment {
            hex,
            kind: kind.to_string(),
            label: label.to_string(),
        };
    }

    let mouse = match hex.as_str() {
        "100100" => Some(("mouse", "鼠标左键")),
        "100200" => Some(("mouse", "鼠标右键")),
        "100400" => Some(("mouse", "鼠标中键")),
        "100800" => Some(("mouse", "鼠标后退")),
        "101000" => Some(("mouse", "鼠标前进")),
        "130100" => Some(("dpi", "DPI +")),
        "130200" => Some(("dpi", "DPI -")),
        "130300" => Some(("dpi", "DPI 循环")),
        _ => None,
    };
    if let Some((kind, label)) = mouse {
        return DecodedAssignment {
            hex,
            kind: kind.to_string(),
            label: label.to_string(),
        };
    }

    let media = match hex.as_str() {
        "3000e9" => Some("音量加"),
        "3000ea" => Some("音量减"),
        "3000e2" => Some("静音"),
        "3000cd" => Some("播放 / 暂停"),
        "3000b5" => Some("下一曲"),
        "3000b6" => Some("上一曲"),
        "3000b7" => Some("停止"),
        _ => None,
    };
    if let Some(label) = media {
        return DecodedAssignment {
            hex,
            kind: "media".to_string(),
            label: label.to_string(),
        };
    }

    let bytes = hex3_to_bytes(&hex);
    let (b0, b1, b2) = (bytes[0], bytes[1], bytes[2]);
    if b0 == 0x20 {
        if b1 != 0 && b2 == 0 {
            if let Some((_, label)) = MODIFIERS.iter().find(|(m, _)| *m == b1) {
                return DecodedAssignment {
                    hex,
                    kind: "key".to_string(),
                    label: (*label).to_string(),
                };
            }
        }
        if b1 == 0 {
            if let Some(label) = hid_key_label(b2) {
                return DecodedAssignment {
                    hex,
                    kind: "key".to_string(),
                    label: label.to_string(),
                };
            }
        }
        if b1 != 0 {
            if let Some(key_label) = hid_key_label(b2) {
                let mods: Vec<String> = MODIFIERS
                    .iter()
                    .filter(|(m, _)| (b1 & *m) != 0)
                    .map(|(_, n)| n.replace(" Right", ""))
                    .collect();
                return DecodedAssignment {
                    hex,
                    kind: "combo".to_string(),
                    label: format!("{}+{}", mods.join("+"), key_label),
                };
            }
        }
    }

    DecodedAssignment {
        label: format!("0x{}", hex.to_ascii_uppercase()),
        hex,
        kind: "raw".to_string(),
    }
}

const GK001S_ROM_ENTRIES: &[(usize, &str)] = &[
    (0, "200029"), (1, "200035"), (2, "20002b"), (3, "200039"), (4, "200200"), (5, "200100"),
    (6, "20003a"), (7, "20001e"), (8, "200014"), (9, "200004"), (10, "200064"), (11, "200800"),
    (12, "20003b"), (13, "20001f"), (14, "20001a"), (15, "200016"), (16, "20001d"), (17, "200400"),
    (18, "20003c"), (19, "200020"), (20, "200008"), (21, "200007"), (22, "20001b"), (23, "200090"),
    (24, "20003d"), (25, "200021"), (26, "200015"), (27, "200009"), (28, "200006"), (30, "20003e"),
    (31, "200022"), (32, "200017"), (33, "20000a"), (34, "200019"), (35, "20002c"), (36, "20003f"),
    (37, "200023"), (38, "20001c"), (39, "20000b"), (40, "200005"), (42, "200040"), (43, "200024"),
    (44, "200018"), (45, "20000d"), (46, "200011"), (48, "200041"), (49, "200025"), (50, "20000c"),
    (51, "20000e"), (52, "200010"), (54, "200042"), (55, "200026"), (56, "200012"), (57, "20000f"),
    (58, "200036"), (59, "204000"), (60, "200043"), (61, "200027"), (62, "200013"), (63, "200033"),
    (64, "200037"), (65, "a00100"), (66, "200044"), (67, "20002d"), (68, "20002f"), (69, "200034"),
    (70, "200038"), (72, "200045"), (73, "20002e"), (74, "200030"), (75, "200032"), (76, "200087"),
    (77, "201000"), (79, "20002a"), (80, "200031"), (81, "200028"), (82, "202000"), (83, "200050"),
    (85, "200049"), (86, "20004c"), (89, "200051"), (91, "20004a"), (92, "20004e"), (94, "200052"),
    (95, "20004f"), (97, "20004d"), (98, "20004b"), (103, "200053"), (104, "20005f"), (105, "20005c"),
    (106, "200059"), (107, "200062"), (109, "200054"), (110, "200060"), (111, "20005d"), (112, "20005a"),
    (113, "200063"), (115, "200055"), (116, "200061"), (117, "20005e"), (118, "20005b"), (121, "200056"),
    (122, "200057"), (123, "200085"), (124, "200058"), (125, "a02200"),
];

pub fn build_default_kb_rom() -> Vec<u8> {
    let mut buf = vec![0u8; 126 * 3];
    for slot in 0..126 {
        let hex_code = GK001S_ROM_ENTRIES
            .iter()
            .find(|(idx, _)| *idx == slot)
            .map(|(_, h)| *h)
            .unwrap_or("200000");
        let b = hex3_to_bytes(hex_code);
        buf[slot * 3..slot * 3 + 3].copy_from_slice(&b);
    }
    buf
}

pub fn build_default_ms_rom() -> Vec<u8> {
    hex::decode("10010010020010040010080010100013030000000000000000000000000100005761726e")
        .unwrap_or_else(|_| vec![0u8; 36])
}
