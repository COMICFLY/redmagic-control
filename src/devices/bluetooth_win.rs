//! 纯 Rust Win32 原生蓝牙 BLE / BT HID 探测与实时电量采集（零 PowerShell 子进程）：
//! - 直接调用 SetupAPI + CfgMgr32 (`SetupDiGetClassDevsW` + `CM_Get_DevNode_PropertyW`)；
//! - 读取 `DEVPKEY_Bluetooth_Battery` (BLE Battery Service 0x180F)、`DEVPKEY_Device_IsConnected` 与蓝牙 MAC；
//! - 全程耗时 < 3ms，与 `hid_win::enumerate_hid_devices()` 结果自动归并。

use std::collections::HashMap;

use crate::devices::catalog::{
    is_bluetooth_hid_path, match_bt_catalog, DeviceCatalogEntry, VENDOR_PAGE, VENDOR_USAGE,
};
use crate::devices::hid_win::HidDeviceInfo;

#[derive(Debug, Clone)]
pub struct BluetoothDeviceRecord {
    pub device_id: &'static str,
    pub meta: &'static DeviceCatalogEntry,
    pub product_name: String,
    pub vid: u16,
    pub pid: u16,
    pub bt_address: Option<String>,
    pub battery: Option<u8>,
    pub connected: bool,
    pub vendor_hid_path: Option<String>,
    pub hid_path: Option<String>,
    pub instance_id: String,
}

pub fn format_bt_address(raw_addr: Option<&str>, instance_id: Option<&str>) -> Option<String> {
    let mut hex_digits: String = raw_addr
        .unwrap_or("")
        .chars()
        .filter(|c| c.is_ascii_hexdigit())
        .collect::<String>()
        .to_ascii_uppercase();

    if hex_digits.len() != 12 {
        if let Some(iid) = instance_id {
            // 从 BTHLEDevice 实例 ID 中提取 12 位 MAC，例如 Dev_e4a1b2c3d4e5 或 _e4a1b2c3d4e5
            let bytes = iid.as_bytes();
            for i in 0..bytes.len() {
                if bytes[i] == b'_' && i + 13 <= bytes.len() {
                    let cand = &iid[i + 1..i + 13];
                    let after_ok = i + 13 == bytes.len()
                        || matches!(bytes[i + 13], b'\\' | b'&' | b'#' | b'_');
                    if after_ok && cand.chars().all(|c| c.is_ascii_hexdigit()) {
                        hex_digits = cand.to_ascii_uppercase();
                        break;
                    }
                }
            }
        }
    }

    if hex_digits.len() != 12 {
        return None;
    }
    Some(format!(
        "{}:{}:{}:{}:{}:{}",
        &hex_digits[0..2],
        &hex_digits[2..4],
        &hex_digits[4..6],
        &hex_digits[6..8],
        &hex_digits[8..10],
        &hex_digits[10..12]
    ))
}

pub fn parse_vid_pid_from_id(text: &str) -> (u16, u16) {
    let up = text.to_ascii_uppercase();
    let parse_after = |markers: &[&str]| -> u16 {
        for m in markers {
            if let Some(pos) = up.find(m) {
                let rest = &up[pos + m.len()..];
                if rest.len() >= 4 && rest[..4].chars().all(|c| c.is_ascii_hexdigit()) {
                    if let Ok(v) = u16::from_str_radix(&rest[..4], 16) {
                        return v;
                    }
                }
            }
        }
        0
    };
    let vid = parse_after(&["VID&02", "VID_", "VID&"]);
    let pid = parse_after(&["PID_", "PID&"]);
    (vid, pid)
}

#[derive(Debug)]
struct PnpBtRow {
    name: String,
    instance_id: String,
    battery: Option<u8>,
    connected: bool,
    address: Option<String>,
}

#[cfg(windows)]
fn query_windows_pnp_bluetooth() -> Vec<PnpBtRow> {
    use std::ptr::null;
    use windows_sys::core::GUID;
    use windows_sys::Win32::Devices::DeviceAndDriverInstallation::{
        CM_Get_DevNode_PropertyW, CM_Get_Device_IDW, SetupDiDestroyDeviceInfoList,
        SetupDiEnumDeviceInfo, SetupDiGetClassDevsW, CR_SUCCESS, DIGCF_PRESENT,
        SP_DEVINFO_DATA,
    };
    use windows_sys::Win32::Devices::Properties::{
        DEVPKEY_Device_FriendlyName, DEVPROPKEY, DEVPROP_TYPE_BOOLEAN, DEVPROP_TYPE_BYTE,
        DEVPROP_TYPE_STRING, DEVPROP_TYPE_UINT32,
    };
    use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;

    // GUID_DEVCLASS_BLUETOOTH = {e0cbf06c-cd8b-4647-bb8a-263b43f0f974}
    const GUID_BLUETOOTH: GUID = GUID {
        data1: 0xe0cbf06c,
        data2: 0xcd8b,
        data3: 0x4647,
        data4: [0xbb, 0x8a, 0x26, 0x3b, 0x43, 0xf0, 0xf9, 0x74],
    };
    // GUID_DEVCLASS_HIDCLASS = {745a17a0-74d3-11d0-b6fe-00a0c90f57da}
    const GUID_HIDCLASS: GUID = GUID {
        data1: 0x745a17a0,
        data2: 0x74d3,
        data3: 0x11d0,
        data4: [0xb6, 0xfe, 0x00, 0xa0, 0xc9, 0x0f, 0x57, 0xda],
    };
    // DEVPKEY_Bluetooth_Battery = {104EA319-6EE2-4701-BD47-8DDBF425BBE5}, 2
    const PKEY_BT_BATTERY: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID {
            data1: 0x104ea319,
            data2: 0x6ee2,
            data3: 0x4701,
            data4: [0xbd, 0x47, 0x8d, 0xdb, 0xf4, 0x25, 0xbb, 0xe5],
        },
        pid: 2,
    };
    // DEVPKEY_Device_IsConnected = {83DA6326-97A6-4088-9453-A1923F573B29}, 15
    const PKEY_IS_CONNECTED: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID {
            data1: 0x83da6326,
            data2: 0x97a6,
            data3: 0x4088,
            data4: [0x94, 0x53, 0xa1, 0x92, 0x3f, 0x57, 0x3b, 0x29],
        },
        pid: 15,
    };
    // DEVPKEY_Bluetooth_DeviceAddress = {2BD67D8B-8BEB-48D5-87E0-6CDA3428040A}, 1
    const PKEY_BT_ADDRESS: DEVPROPKEY = DEVPROPKEY {
        fmtid: GUID {
            data1: 0x2bd67d8b,
            data2: 0x8beb,
            data3: 0x48d5,
            data4: [0x87, 0xe0, 0x6c, 0xda, 0x34, 0x28, 0x04, 0x0a],
        },
        pid: 1,
    };

    unsafe fn read_prop_string(dev_inst: u32, key: &DEVPROPKEY) -> Option<String> {
        let mut prop_type = 0u32;
        let mut buf = [0u8; 512];
        let mut size = buf.len() as u32;
        let cr = CM_Get_DevNode_PropertyW(
            dev_inst,
            key,
            &mut prop_type,
            buf.as_mut_ptr(),
            &mut size,
            0,
        );
        if cr == CR_SUCCESS && prop_type == DEVPROP_TYPE_STRING && size >= 2 {
            let wlen = (size as usize) / 2;
            let mut w = Vec::with_capacity(wlen);
            for chunk in buf[..wlen * 2].chunks_exact(2) {
                let u = u16::from_le_bytes([chunk[0], chunk[1]]);
                if u == 0 {
                    break;
                }
                w.push(u);
            }
            let s = String::from_utf16_lossy(&w).trim().to_string();
            if !s.is_empty() {
                return Some(s);
            }
        }
        None
    }

    unsafe fn read_prop_u8(dev_inst: u32, key: &DEVPROPKEY) -> Option<u8> {
        let mut prop_type = 0u32;
        let mut buf = [0u8; 8];
        let mut size = buf.len() as u32;
        let cr = CM_Get_DevNode_PropertyW(
            dev_inst,
            key,
            &mut prop_type,
            buf.as_mut_ptr(),
            &mut size,
            0,
        );
        if cr != CR_SUCCESS {
            return None;
        }
        if prop_type == DEVPROP_TYPE_BYTE && size >= 1 {
            Some(buf[0])
        } else if prop_type == DEVPROP_TYPE_UINT32 && size >= 4 {
            let v = u32::from_le_bytes([buf[0], buf[1], buf[2], buf[3]]);
            (v <= 100).then_some(v as u8)
        } else {
            None
        }
    }

    unsafe fn read_prop_bool(dev_inst: u32, key: &DEVPROPKEY) -> bool {
        let mut prop_type = 0u32;
        let mut buf = [0u8; 4];
        let mut size = buf.len() as u32;
        let cr = CM_Get_DevNode_PropertyW(
            dev_inst,
            key,
            &mut prop_type,
            buf.as_mut_ptr(),
            &mut size,
            0,
        );
        cr == CR_SUCCESS && prop_type == DEVPROP_TYPE_BOOLEAN && size >= 1 && buf[0] != 0
    }

    let mut rows = Vec::new();
    for class_guid in [&GUID_BLUETOOTH, &GUID_HIDCLASS] {
        unsafe {
            let hinfo = SetupDiGetClassDevsW(class_guid, null(), std::ptr::null_mut(), DIGCF_PRESENT);
            if hinfo == INVALID_HANDLE_VALUE as isize || hinfo == 0 {
                continue;
            }
            let mut idx = 0u32;
            loop {
                let mut dev_data: SP_DEVINFO_DATA = std::mem::zeroed();
                dev_data.cbSize = std::mem::size_of::<SP_DEVINFO_DATA>() as u32;
                if SetupDiEnumDeviceInfo(hinfo, idx, &mut dev_data) == 0 {
                    break;
                }
                idx += 1;

                let mut id_buf = [0u16; 320];
                if CM_Get_Device_IDW(dev_data.DevInst, id_buf.as_mut_ptr(), id_buf.len() as u32, 0)
                    != CR_SUCCESS
                {
                    continue;
                }
                let end = id_buf.iter().position(|&c| c == 0).unwrap_or(id_buf.len());
                let instance_id = String::from_utf16_lossy(&id_buf[..end]);
                let up_id = instance_id.to_ascii_uppercase();
                if !up_id.contains("BTHLE") && !up_id.contains("BTHENUM") {
                    continue;
                }

                let name = read_prop_string(dev_data.DevInst, &DEVPKEY_Device_FriendlyName)
                    .unwrap_or_default();
                let up_name = name.to_ascii_uppercase();
                let relevant = up_name.contains("GK001")
                    || up_name.contains("GS001")
                    || up_name.contains("REDMAGIC")
                    || up_name.contains("REDMACIC")
                    || up_id.contains("320F")
                    || up_id.contains("05AC&PID_024F");
                if !relevant {
                    continue;
                }

                let battery = read_prop_u8(dev_data.DevInst, &PKEY_BT_BATTERY).filter(|&b| b <= 100);
                let connected = read_prop_bool(dev_data.DevInst, &PKEY_IS_CONNECTED);
                let address = read_prop_string(dev_data.DevInst, &PKEY_BT_ADDRESS);

                rows.push(PnpBtRow {
                    name,
                    instance_id,
                    battery,
                    connected,
                    address,
                });
            }
            SetupDiDestroyDeviceInfoList(hinfo);
        }
    }
    rows
}

#[cfg(not(windows))]
fn query_windows_pnp_bluetooth() -> Vec<PnpBtRow> {
    Vec::new()
}

pub fn scan_bluetooth_devices(all_hid: &[HidDeviceInfo]) -> Vec<BluetoothDeviceRecord> {
    let pnp_rows = query_windows_pnp_bluetooth();
    let mut grouped: HashMap<&'static str, BluetoothDeviceRecord> = HashMap::new();
    let mut addr_to_meta: HashMap<String, &'static DeviceCatalogEntry> = HashMap::new();

    for row in pnp_rows {
        let (vid, pid) = parse_vid_pid_from_id(&row.instance_id);
        let addr = format_bt_address(row.address.as_deref(), Some(&row.instance_id));
        let meta = match_bt_catalog(&row.name, vid, pid)
            .or_else(|| addr.as_ref().and_then(|a| addr_to_meta.get(a).copied()));
        let Some(meta) = meta else { continue };
        if let Some(ref a) = addr {
            addr_to_meta.insert(a.clone(), meta);
        }

        let entry = grouped.entry(meta.id).or_insert_with(|| BluetoothDeviceRecord {
            device_id: meta.id,
            meta,
            product_name: String::new(),
            vid: 0,
            pid: 0,
            bt_address: None,
            battery: None,
            connected: false,
            vendor_hid_path: None,
            hid_path: None,
            instance_id: row.instance_id.clone(),
        });

        let low = row.name.to_ascii_lowercase();
        if !row.name.is_empty()
            && (entry.product_name.is_empty()
                || low.contains("gk001")
                || low.contains("gs001")
                || low.contains("redmagic"))
        {
            entry.product_name = row.name;
        }
        if vid != 0 && entry.vid == 0 {
            entry.vid = vid;
        }
        if pid != 0 && entry.pid == 0 {
            entry.pid = pid;
        }
        if entry.bt_address.is_none() {
            entry.bt_address = addr;
        }
        if row.connected {
            entry.connected = true;
        }
        if let Some(b) = row.battery {
            entry.battery = Some(b);
        }
    }

    for d in all_hid {
        if !is_bluetooth_hid_path(&d.path) {
            continue;
        }
        let addr = format_bt_address(None, Some(&d.path));
        let meta = match_bt_catalog(&d.product, d.vendor_id, d.product_id)
            .or_else(|| addr.as_ref().and_then(|a| addr_to_meta.get(a).copied()));
        let Some(meta) = meta else { continue };

        let entry = grouped.entry(meta.id).or_insert_with(|| BluetoothDeviceRecord {
            device_id: meta.id,
            meta,
            product_name: String::new(),
            vid: d.vendor_id,
            pid: d.product_id,
            bt_address: addr.clone(),
            battery: None,
            connected: true,
            vendor_hid_path: None,
            hid_path: None,
            instance_id: d.path.clone(),
        });

        entry.connected = true;
        if entry.product_name.is_empty() && !d.product.is_empty() {
            entry.product_name = d.product.clone();
        }
        if d.vendor_id != 0 {
            entry.vid = d.vendor_id;
        }
        if d.product_id != 0 {
            entry.pid = d.product_id;
        }
        if entry.bt_address.is_none() {
            entry.bt_address = addr;
        }
        if d.usage_page == VENDOR_PAGE && d.usage == VENDOR_USAGE {
            entry.vendor_hid_path = Some(d.path.clone());
        } else if entry.hid_path.is_none() {
            entry.hid_path = Some(d.path.clone());
        }
    }

    grouped.into_values().filter(|g| g.connected).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_ble_mac_and_matches_gk001s_only() {
        let mac = format_bt_address(
            None,
            Some("BTHLEDevice\\{00001812-0000-1000-8000-00805f9b34fb}_Dev_VID&0205ac_PID&024f_a1b2c3d4e5f6\\8&1234"),
        );
        assert_eq!(mac.as_deref(), Some("A1:B2:C3:D4:E5:F6"));
        assert_eq!(match_bt_catalog("GK001S 5.1", 0x05ac, 0x024f).map(|d| d.id), Some("kb-1s"));
        assert_eq!(match_bt_catalog("GS001S", 0x320f, 0x2277).map(|d| d.id), Some("ms-1s"));
        assert!(match_bt_catalog("Apple Magic Keyboard", 0x05ac, 0x024f).is_none());
    }
}
