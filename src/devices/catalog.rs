//! REDMAGIC 外设目录、三模连接能力与灯效/回报率元数据表。

use serde::Serialize;
use serde_json::{json, Value};

use crate::protocol::{CHUNK_SIZE, CHUNK_SIZE_24G};

pub const VIDS: &[u16] = &[0x320f, 0x0c45];
pub const VENDOR_PAGE: u16 = 0xff1c;
pub const VENDOR_USAGE: u16 = 0x92;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum ConnectionMode {
    #[serde(rename = "wired")]
    Wired,
    #[serde(rename = "2.4g")]
    Wireless24g,
    #[serde(rename = "bluetooth")]
    Bluetooth,
}

impl ConnectionMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Wired => "wired",
            Self::Wireless24g => "2.4g",
            Self::Bluetooth => "bluetooth",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Wired => "USB 有线",
            Self::Wireless24g => "2.4G 无线",
            Self::Bluetooth => "蓝牙 BLE",
        }
    }

    pub fn short_label(self) -> &'static str {
        match self {
            Self::Wired => "有线",
            Self::Wireless24g => "2.4G",
            Self::Bluetooth => "蓝牙",
        }
    }

    pub fn chunk_size(self) -> usize {
        match self {
            Self::Wired => CHUNK_SIZE,
            Self::Wireless24g | Self::Bluetooth => CHUNK_SIZE_24G,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DeviceKind {
    Keyboard,
    Mouse,
}

impl DeviceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Keyboard => "keyboard",
            Self::Mouse => "mouse",
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BtPnpId {
    pub vid: u16,
    pub pid: u16,
    pub require_name: bool,
}

#[derive(Debug, Clone, Copy)]
pub struct DeviceCatalogEntry {
    pub id: &'static str,
    pub kind: DeviceKind,
    pub name: &'static str,
    pub model: &'static str,
    pub chip: &'static str,
    pub firmware: &'static str,
    pub image: &'static str,
    pub card: &'static str,
    pub vids: &'static [u16],
    pub pids: &'static [u16],
    pub wired_pid: u16,
    pub wireless24g_pid: u16,
    pub key_count: usize,
    pub bt_names: &'static [&'static str],
    pub bt_pnp_ids: &'static [BtPnpId],
}

pub const DEVICES: &[DeviceCatalogEntry] = &[
    DeviceCatalogEntry {
        id: "kb-1s",
        kind: DeviceKind::Keyboard,
        name: "REDMAGIC Keyboard 1S",
        model: "GK001S",
        chip: "VS11K16A",
        firmware: "V0112",
        image: "assets/redmagic-keyboard.png",
        card: "assets/device-kb-card.png",
        vids: &[0x320f, 0x0c45],
        pids: &[0x5134, 0x5135],
        wired_pid: 0x5134,
        wireless24g_pid: 0x5135,
        key_count: 126,
        bt_names: &["gk001s", "redmagic gk001", "redmacic gk001", "redmagic keyboard"],
        bt_pnp_ids: &[
            BtPnpId { vid: 0x05ac, pid: 0x024f, require_name: true },
            BtPnpId { vid: 0x320f, pid: 0x5134, require_name: false },
            BtPnpId { vid: 0x320f, pid: 0x5135, require_name: false },
        ],
    },
    DeviceCatalogEntry {
        id: "ms-1s",
        kind: DeviceKind::Mouse,
        name: "REDMAGIC Mouse 1S",
        model: "GS001S",
        chip: "VS11K17A",
        firmware: "V0112",
        image: "assets/redmagic-mouse.png",
        card: "assets/device-ms-card.png",
        vids: &[0x320f, 0x0c45],
        pids: &[0x2276, 0x2277],
        wired_pid: 0x2276,
        wireless24g_pid: 0x2277,
        key_count: 12,
        bt_names: &["gs001s", "redmagic gs001", "redmagic mouse"],
        bt_pnp_ids: &[
            BtPnpId { vid: 0x320f, pid: 0x2276, require_name: false },
            BtPnpId { vid: 0x320f, pid: 0x2277, require_name: false },
        ],
    },
];

pub fn is_bluetooth_hid_path(path: &str) -> bool {
    let p = path.to_ascii_lowercase();
    p.contains("bthle") || p.contains("bthenum") || p.contains("{00001812-0000-1000-8000-00805f9b34fb}")
}

pub fn match_catalog_by_hid(pid: u16, product: &str) -> Option<&'static DeviceCatalogEntry> {
    if let Some(d) = DEVICES.iter().find(|d| d.pids.contains(&pid)) {
        return Some(d);
    }
    let low = product.to_ascii_lowercase();
    if low.contains("gk001") || low.contains("keyboard") {
        return Some(&DEVICES[0]);
    }
    if low.contains("gs001") || low.contains("mouse") {
        return Some(&DEVICES[1]);
    }
    None
}

pub fn match_bt_catalog(name: &str, vid: u16, pid: u16) -> Option<&'static DeviceCatalogEntry> {
    let low = name.to_ascii_lowercase();
    for meta in DEVICES {
        if meta.bt_names.iter().any(|kw| low.contains(kw)) {
            return Some(meta);
        }
        if vid != 0 && pid != 0 {
            if meta
                .bt_pnp_ids
                .iter()
                .any(|p| p.vid == vid && p.pid == pid && !p.require_name)
            {
                return Some(meta);
            }
        }
    }
    None
}

pub fn infer_connection_mode(meta: &DeviceCatalogEntry, pid: u16, product: &str, path: &str) -> ConnectionMode {
    if is_bluetooth_hid_path(path) {
        return ConnectionMode::Bluetooth;
    }
    if pid == meta.wired_pid {
        return ConnectionMode::Wired;
    }
    if pid == meta.wireless24g_pid {
        return ConnectionMode::Wireless24g;
    }
    let low = product.to_ascii_lowercase();
    if low.contains("2.4g") || low.contains("dongle") || low.contains("receiver") {
        return ConnectionMode::Wireless24g;
    }
    if low.contains("5.1") || low.contains("5.0") || low.contains("bt") || low.contains("ble") {
        return ConnectionMode::Bluetooth;
    }
    ConnectionMode::Wired
}

pub fn get_metadata_json(version: &str) -> Value {
    json!({
        "version": version,
        "connectionModes": {
            "wired": { "mode": "wired", "label": "USB 有线", "shortLabel": "有线", "chunkSize": CHUNK_SIZE, "priority": 1 },
            "2.4g": { "mode": "2.4g", "label": "2.4G 无线", "shortLabel": "2.4G", "chunkSize": CHUNK_SIZE_24G, "priority": 2 },
            "bluetooth": { "mode": "bluetooth", "label": "蓝牙 BLE", "shortLabel": "蓝牙", "chunkSize": CHUNK_SIZE_24G, "priority": 3 }
        },
        "kbLightModes": [
            { "id": 0,  "name": "随波逐流", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": true,  "dirLabels": ["由左至右", "由右至左"] },
            { "id": 1,  "name": "彩云纷飞", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": true,  "dirLabels": ["由左至右", "由右至左"] },
            { "id": 2,  "name": "峰回路转", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": true,  "dirLabels": ["逆时针", "顺时针"] },
            { "id": 3,  "name": "光谱",     "color": false, "multi": false, "brightness": true,  "speed": true,  "dir": false },
            { "id": 4,  "name": "呼吸",     "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": false },
            { "id": 5,  "name": "常亮",     "color": true,  "multi": true,  "brightness": true,  "speed": false, "dir": false },
            { "id": 6,  "name": "泛起涟漪", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": false },
            { "id": 7,  "name": "踏雪无痕", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": false },
            { "id": 8,  "name": "奔逸绝尘", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": false },
            { "id": 9,  "name": "百花争艳", "color": false, "multi": false, "brightness": true,  "speed": true,  "dir": false },
            { "id": 10, "name": "繁星点点", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": false },
            { "id": 11, "name": "川流不息", "color": false, "multi": false, "brightness": true,  "speed": true,  "dir": true,  "dirLabels": ["由上至下", "由下至上"] },
            { "id": 12, "name": "大鹏展翅", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": false },
            { "id": 13, "name": "厚积薄发", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": false },
            { "id": 14, "name": "落雨纷纷", "color": true,  "multi": true,  "brightness": true,  "speed": true,  "dir": false },
            { "id": 15, "name": "左右逢源", "color": true,  "multi": true,  "brightness": true,  "speed": false, "dir": false },
            { "id": 16, "name": "众志成城", "color": false, "multi": false, "brightness": true,  "speed": false, "dir": false, "zzccGroup": true },
            { "id": 17, "name": "速度激情", "color": false, "multi": false, "brightness": true,  "speed": false, "dir": true,  "dirLabels": ["由内至外", "由外至内"] },
            { "id": 18, "name": "自定义",   "color": true,  "multi": false, "brightness": true,  "speed": false, "dir": false, "customGroup": true },
            { "id": 19, "name": "全灭",     "color": false, "multi": false, "brightness": false, "speed": false, "dir": false }
        ],
        "kbLogoModes": [
            { "id": 0, "name": "随波逐流", "color": true,  "multi": true,  "brightness": true,  "speed": true  },
            { "id": 1, "name": "光谱",     "color": false, "multi": false, "brightness": true,  "speed": false },
            { "id": 2, "name": "呼吸",     "color": true,  "multi": true,  "brightness": true,  "speed": true  },
            { "id": 3, "name": "常亮",     "color": true,  "multi": false, "brightness": true,  "speed": false },
            { "id": 4, "name": "全灭",     "color": false, "multi": false, "brightness": false, "speed": false }
        ],
        "msLightModes": [
            { "id": 0, "name": "随波逐流", "color": false, "multi": false, "brightness": true,  "speed": true  },
            { "id": 1, "name": "流光模式", "color": false, "multi": false, "brightness": true,  "speed": true  },
            { "id": 2, "name": "呼吸",     "color": true,  "multi": false, "brightness": true,  "speed": true  },
            { "id": 3, "name": "常亮",     "color": true,  "multi": false, "brightness": true,  "speed": false },
            { "id": 4, "name": "全灭",     "color": false, "multi": false, "brightness": false, "speed": false }
        ],
        "kbPollingRates": [
            { "id": 0, "hz": 1000 }, { "id": 1, "hz": 500 }, { "id": 2, "hz": 250 }, { "id": 3, "hz": 125 }
        ],
        "msPollingRates": [
            { "id": 0, "hz": 125 }, { "id": 1, "hz": 250 }, { "id": 2, "hz": 500 }, { "id": 3, "hz": 1000 },
            { "id": 4, "hz": 2000 }, { "id": 5, "hz": 4000 }, { "id": 6, "hz": 8000 }
        ],
        "mouseSlots": [
            { "slot": 0, "code": "L",   "name": "左键",       "x": 35, "y": 26 },
            { "slot": 1, "code": "R",   "name": "右键",       "x": 65, "y": 26 },
            { "slot": 2, "code": "M",   "name": "滚轮中键",   "x": 50, "y": 24 },
            { "slot": 3, "code": "B4",  "name": "侧键 · 后退", "x": 25, "y": 54 },
            { "slot": 4, "code": "B5",  "name": "侧键 · 前进", "x": 25, "y": 42 },
            { "slot": 5, "code": "DPI", "name": "DPI 切换键", "x": 50, "y": 38 }
        ]
    })
}
