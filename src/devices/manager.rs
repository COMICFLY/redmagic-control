//! 纯 Rust 三模设备管理器：
//! - 统一发现 USB 有线 / 2.4G 无线接收器 / 蓝牙 BLE 链路，按优先级自动选择活跃通道；
//! - 全局互斥锁保护拓扑变更与每设备独占会话，零句柄泄漏、零多线程竞争；
//! - 后台监控线程每 4 秒低开销比对 HID 路径指纹（不反复开关闲置接收器句柄），15 秒刷新在线电量与充电状态。

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use serde::Serialize;
use serde_json::{json, Value};

use crate::devices::bluetooth_win::scan_bluetooth_devices;
use crate::devices::catalog::{
    infer_connection_mode, is_bluetooth_hid_path, match_catalog_by_hid, ConnectionMode,
    DeviceCatalogEntry, DeviceKind, DEVICES, VENDOR_PAGE, VENDOR_USAGE, VIDS,
};
use crate::devices::hid_win::{enumerate_hid_devices, HidDeviceInfo};
use crate::devices::keyboard::{KeyboardAdapter, SavedDirectLightState};
use crate::devices::mouse::MouseAdapter;
use crate::devices::transport::Transport;
use crate::protocol::POLL_STATUS;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceLinkInfo {
    pub link_id: String,
    pub mode: ConnectionMode,
    pub label: String,
    pub short_label: String,
    pub state: String,
    pub online: bool,
    pub active: bool,
    pub supports_config: bool,
    pub supports_screen: bool,
    pub chunk_size: usize,
    pub vid: u16,
    pub pid: u16,
    pub product_name: String,
    pub path: Option<String>,
    pub bt_address: Option<String>,
    pub battery: Option<u8>,
    pub charging_state: Option<u8>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeviceDescriptor {
    pub id: String,
    pub kind: String,
    pub name: String,
    pub model: String,
    pub chip: String,
    pub firmware: String,
    pub image: String,
    pub card: String,
    pub key_count: usize,
    pub mock: bool,
    pub connected: bool,
    pub connection_mode: ConnectionMode,
    pub connection_label: String,
    pub connection_short_label: String,
    pub link_id: String,
    pub link_state: String,
    pub supports_config: bool,
    pub supports_screen: bool,
    pub chunk_size: usize,
    pub vid: u16,
    pub pid: u16,
    pub product_name: String,
    pub path: Option<String>,
    pub bt_address: Option<String>,
    pub battery: Option<u8>,
    pub charging_state: Option<u8>,
    pub links: Vec<DeviceLinkInfo>,
}

pub enum DeviceSession {
    Keyboard(KeyboardAdapter),
    Mouse(MouseAdapter),
}

impl DeviceSession {
    fn link_id(&self) -> &str {
        match self {
            Self::Keyboard(a) => &a.link_id,
            Self::Mouse(a) => &a.link_id,
        }
    }

    fn device_path(&self) -> Option<&str> {
        match self {
            Self::Keyboard(a) => a.t.device_path(),
            Self::Mouse(a) => a.t.device_path(),
        }
    }

    fn ping_wireless(&mut self, timeout_ms: u32) -> (bool, u8) {
        match self {
            Self::Keyboard(a) => a.t.ping_wireless(timeout_ms),
            Self::Mouse(a) => a.t.ping_wireless(timeout_ms),
        }
    }

    fn read_battery(&mut self) -> Result<Value, String> {
        match self {
            Self::Keyboard(a) => a.read_battery().map_err(|e| e.to_string()),
            Self::Mouse(a) => a.read_battery().map_err(|e| e.to_string()),
        }
    }

    fn read_state(&mut self) -> Result<Value, String> {
        match self {
            Self::Keyboard(a) => a.read_state().map_err(|e| e.to_string()),
            Self::Mouse(a) => a.read_state().map_err(|e| e.to_string()),
        }
    }

    fn write_config(&mut self, patch: &Value) -> Result<Value, String> {
        match self {
            Self::Keyboard(a) => a.write_config(patch).map_err(|e| e.to_string()),
            Self::Mouse(a) => a.write_config(patch).map_err(|e| e.to_string()),
        }
    }

    fn write_key(&mut self, slot: usize, hex: &str) -> Result<Value, String> {
        match self {
            Self::Keyboard(a) => a.write_key(slot, hex).map_err(|e| e.to_string()),
            Self::Mouse(a) => a.write_key(slot, hex).map_err(|e| e.to_string()),
        }
    }

    fn reset_key(&mut self, slot: usize) -> Result<Value, String> {
        match self {
            Self::Keyboard(a) => a.reset_key(slot).map_err(|e| e.to_string()),
            Self::Mouse(a) => a.reset_key(slot).map_err(|e| e.to_string()),
        }
    }

    fn reset_all_keys(&mut self) -> Result<Value, String> {
        match self {
            Self::Keyboard(a) => a.reset_all_keys().map_err(|e| e.to_string()),
            Self::Mouse(a) => a.reset_all_keys().map_err(|e| e.to_string()),
        }
    }
}

struct ManagerInner {
    force_mock: bool,
    cached_devices: Option<Vec<DeviceDescriptor>>,
    sessions: HashMap<String, DeviceSession>,
    preferred_links: HashMap<String, String>,
    last_known_states: HashMap<String, Value>,
    saved_direct_light: HashMap<String, SavedDirectLightState>,
    last_raw_sig: String,
    last_topo_sig: String,
}

#[derive(Clone)]
pub struct DeviceManager {
    inner: Arc<Mutex<ManagerInner>>,
    screen_uploading: Arc<AtomicBool>,
    rgb_streaming: Arc<AtomicBool>,
    monitor_stop: Arc<AtomicBool>,
}

fn make_link_id(mode: ConnectionMode, pid: u16, path_or_addr: &str) -> String {
    let segs: Vec<&str> = path_or_addr.split('#').filter(|s| !s.is_empty()).collect();
    let uniq = if segs.len() >= 3 { segs[2] } else { path_or_addr };
    let mut hash: u32 = 2166136261;
    for b in path_or_addr.as_bytes() {
        hash = (hash ^ (*b as u32)).wrapping_mul(16777619);
    }
    let clean: String = uniq.chars().filter(|c| c.is_ascii_alphanumeric()).take(10).collect();
    let tag = if clean.is_empty() {
        format!("{hash:08x}")
    } else {
        clean
    };
    format!("{}:{pid:04x}:{tag}", mode.as_str())
}

fn link_rank(link: &DeviceLinkInfo) -> i32 {
    match (link.mode, link.state.as_str(), link.supports_config) {
        (ConnectionMode::Wired, "online", _) => 100,
        (ConnectionMode::Wireless24g, "online", _) => 90,
        (ConnectionMode::Bluetooth, "online", true) => 80,
        (ConnectionMode::Bluetooth, "bt-readonly", _) => 60,
        (ConnectionMode::Wired, _, _) => 50,
        (ConnectionMode::Wireless24g, "sleeping", _) => 30,
        _ => 10,
    }
}

fn compute_raw_path_sig(all_hid: &[HidDeviceInfo]) -> String {
    let mut parts: Vec<String> = all_hid
        .iter()
        .filter(|d| {
            (VIDS.contains(&d.vendor_id) && d.usage_page == VENDOR_PAGE && d.usage == VENDOR_USAGE)
                || is_bluetooth_hid_path(&d.path)
        })
        .map(|d| format!("{}:{}:{}:{}:{}", d.vendor_id, d.product_id, d.usage_page, d.usage, d.path))
        .collect();
    parts.sort();
    parts.join("|")
}

fn compute_topo_sig(devices: &[DeviceDescriptor]) -> String {
    devices
        .iter()
        .map(|d| {
            let ls: Vec<String> = d.links.iter().map(|l| format!("{}={}", l.link_id, l.state)).collect();
            format!(
                "{}|{}|{}|{}|{}|{}",
                d.id,
                d.mock,
                d.connection_mode.as_str(),
                d.link_state,
                d.pid,
                ls.join(",")
            )
        })
        .collect::<Vec<_>>()
        .join(";")
}

struct ProbeResult {
    online: bool,
    link_state: String,
    battery: Option<u8>,
    charging_state: Option<u8>,
}

fn probe_vendor_link(
    sessions: &mut HashMap<String, DeviceSession>,
    path: &str,
    mode: ConnectionMode,
    wake_retries: usize,
) -> ProbeResult {
    // 优先复用已打开会话，避免重复打开同一 HID 接口
    for sess in sessions.values_mut() {
        if sess.device_path() == Some(path) {
            if mode == ConnectionMode::Wireless24g {
                let mut online = false;
                for i in 0..wake_retries {
                    let (ok, _) = sess.ping_wireless(340);
                    if ok {
                        online = true;
                        break;
                    }
                    if i + 1 < wake_retries {
                        thread::sleep(Duration::from_millis(55));
                    }
                }
                if !online {
                    return ProbeResult {
                        online: false,
                        link_state: "sleeping".to_string(),
                        battery: None,
                        charging_state: None,
                    };
                }
            }
            if let Ok(b) = sess.read_battery() {
                let ls = b
                    .get("linkState")
                    .and_then(Value::as_str)
                    .unwrap_or("online")
                    .to_string();
                return ProbeResult {
                    online: ls == "online",
                    link_state: ls,
                    battery: b.get("battery").and_then(Value::as_u64).map(|x| x as u8),
                    charging_state: b.get("chargingState").and_then(Value::as_u64).map(|x| x as u8),
                };
            }
            return ProbeResult {
                online: false,
                link_state: if mode == ConnectionMode::Wireless24g {
                    "sleeping"
                } else {
                    "offline"
                }
                .to_string(),
                battery: None,
                charging_state: None,
            };
        }
    }

    // 临时句柄随作用域结束自动 Drop 关闭，Win32 CancelIoEx + CloseHandle 零竞态
    let mut t = Transport::new_win_hid(path.to_string(), mode, "online", None);
    if mode == ConnectionMode::Wireless24g {
        let mut online = false;
        for i in 0..wake_retries {
            let (ok, _) = t.ping_wireless(340);
            if ok {
                online = true;
                break;
            }
            if i + 1 < wake_retries {
                thread::sleep(Duration::from_millis(55));
            }
        }
        if !online {
            return ProbeResult {
                online: false,
                link_state: "sleeping".to_string(),
                battery: None,
                charging_state: None,
            };
        }
        let r = t.xfer(POLL_STATUS, 0x06, 0, None, Some(700)).ok();
        return ProbeResult {
            online: true,
            link_state: "online".to_string(),
            battery: r.as_ref().and_then(|f| (f.data[0] <= 100).then_some(f.data[0])),
            charging_state: r.as_ref().map(|f| if f.data[1] <= 2 { f.data[1] } else { 0 }),
        };
    }

    match t.xfer(POLL_STATUS, 0x06, 0, None, Some(550)) {
        Ok(r) if r.op == POLL_STATUS && r.data[0] <= 100 => ProbeResult {
            online: true,
            link_state: "online".to_string(),
            battery: Some(r.data[0]),
            charging_state: Some(if r.data[1] <= 2 { r.data[1] } else { 0 }),
        },
        _ => ProbeResult {
            online: false,
            link_state: "offline".to_string(),
            battery: None,
            charging_state: None,
        },
    }
}

impl DeviceManager {
    pub fn new(force_mock: bool) -> Self {
        Self {
            inner: Arc::new(Mutex::new(ManagerInner {
                force_mock,
                cached_devices: None,
                sessions: HashMap::new(),
                preferred_links: HashMap::new(),
                last_known_states: HashMap::new(),
                saved_direct_light: HashMap::new(),
                last_raw_sig: String::new(),
                last_topo_sig: String::new(),
            })),
            screen_uploading: Arc::new(AtomicBool::new(false)),
            rgb_streaming: Arc::new(AtomicBool::new(false)),
            monitor_stop: Arc::new(AtomicBool::new(false)),
        }
    }

    fn scan_locked(inner: &mut ManagerInner, force_scan: bool, wake_device_id: Option<&str>) -> Vec<DeviceDescriptor> {
        if !force_scan {
            if let Some(ref c) = inner.cached_devices {
                return c.clone();
            }
        }

        let all_hid = if inner.force_mock {
            Vec::new()
        } else {
            enumerate_hid_devices()
        };
        inner.last_raw_sig = compute_raw_path_sig(&all_hid);

        let vendor_ifaces: Vec<&HidDeviceInfo> = all_hid
            .iter()
            .filter(|d| {
                (VIDS.contains(&d.vendor_id) || is_bluetooth_hid_path(&d.path))
                    && d.usage_page == VENDOR_PAGE
                    && d.usage == VENDOR_USAGE
            })
            .collect();

        let bt_devices = if inner.force_mock {
            Vec::new()
        } else {
            scan_bluetooth_devices(&all_hid)
        };

        let mut by_id: HashMap<&'static str, (&'static DeviceCatalogEntry, Vec<DeviceLinkInfo>)> =
            HashMap::new();

        for iface in vendor_ifaces {
            let Some(meta) = match_catalog_by_hid(iface.product_id, &iface.product) else {
                continue;
            };
            let mode = infer_connection_mode(meta, iface.product_id, &iface.product, &iface.path);
            let wake_retries = if wake_device_id == Some(meta.id) && mode == ConnectionMode::Wireless24g {
                8
            } else {
                1
            };
            let probe = probe_vendor_link(&mut inner.sessions, &iface.path, mode, wake_retries);
            let entry = by_id.entry(meta.id).or_insert_with(|| (meta, Vec::new()));
            entry.1.push(DeviceLinkInfo {
                link_id: make_link_id(mode, iface.product_id, &iface.path),
                mode,
                label: mode.label().to_string(),
                short_label: mode.short_label().to_string(),
                state: probe.link_state,
                online: probe.online,
                active: false,
                supports_config: probe.online,
                supports_screen: meta.kind == DeviceKind::Keyboard
                    && mode == ConnectionMode::Wired
                    && probe.online,
                chunk_size: mode.chunk_size(),
                vid: iface.vendor_id,
                pid: iface.product_id,
                product_name: if iface.product.is_empty() {
                    meta.name.to_string()
                } else {
                    iface.product.clone()
                },
                path: Some(iface.path.clone()),
                bt_address: None,
                battery: probe.battery,
                charging_state: probe.charging_state,
            });
        }

        for bt in bt_devices {
            let entry = by_id.entry(bt.meta.id).or_insert_with(|| (bt.meta, Vec::new()));
            if let Some(existing) = entry.1.iter_mut().find(|l| l.mode == ConnectionMode::Bluetooth) {
                if bt.bt_address.is_some() {
                    existing.bt_address = bt.bt_address;
                }
                if existing.battery.is_none() {
                    existing.battery = bt.battery;
                }
                continue;
            }
            let pid = if bt.pid != 0 { bt.pid } else { bt.meta.pids[0] };
            let vid = if bt.vid != 0 { bt.vid } else { bt.meta.vids[0] };
            let id_seed = bt.bt_address.as_deref().unwrap_or(&bt.instance_id);
            entry.1.push(DeviceLinkInfo {
                link_id: make_link_id(ConnectionMode::Bluetooth, pid, id_seed),
                mode: ConnectionMode::Bluetooth,
                label: ConnectionMode::Bluetooth.label().to_string(),
                short_label: ConnectionMode::Bluetooth.short_label().to_string(),
                state: "bt-readonly".to_string(),
                online: true,
                active: false,
                supports_config: false,
                supports_screen: false,
                chunk_size: ConnectionMode::Bluetooth.chunk_size(),
                vid,
                pid,
                product_name: if bt.product_name.is_empty() {
                    format!("{} BLE", bt.meta.model)
                } else {
                    bt.product_name
                },
                path: bt.hid_path,
                bt_address: bt.bt_address,
                battery: bt.battery,
                charging_state: Some(0),
            });
        }

        // 按固定目录顺序（键盘在前、鼠标在后）组装输出
        let mut out = Vec::new();
        for meta in DEVICES {
            let Some((_, mut links)) = by_id.remove(meta.id) else {
                continue;
            };
            links.sort_by(|a, b| link_rank(b).cmp(&link_rank(a)));

            let pref = inner.preferred_links.get(meta.id).cloned();
            let chosen_idx = pref
                .as_deref()
                .and_then(|pid| links.iter().position(|l| l.link_id == pid))
                .filter(|&i| links[i].online || !links[0].online)
                .unwrap_or(0);

            for (i, l) in links.iter_mut().enumerate() {
                l.active = i == chosen_idx;
            }
            let chosen = links[chosen_idx].clone();

            out.push(DeviceDescriptor {
                id: meta.id.to_string(),
                kind: meta.kind.as_str().to_string(),
                name: meta.name.to_string(),
                model: meta.model.to_string(),
                chip: meta.chip.to_string(),
                firmware: meta.firmware.to_string(),
                image: meta.image.to_string(),
                card: meta.card.to_string(),
                key_count: meta.key_count,
                mock: false,
                connected: chosen.online || chosen.state == "sleeping",
                connection_mode: chosen.mode,
                connection_label: chosen.label,
                connection_short_label: chosen.short_label,
                link_id: chosen.link_id,
                link_state: chosen.state,
                supports_config: chosen.supports_config,
                supports_screen: chosen.supports_screen,
                chunk_size: chosen.chunk_size,
                vid: chosen.vid,
                pid: chosen.pid,
                product_name: chosen.product_name,
                path: chosen.path,
                bt_address: chosen.bt_address,
                battery: chosen.battery,
                charging_state: chosen.charging_state,
                links,
            });
        }

        if out.is_empty() {
            for meta in DEVICES {
                let mode = if meta.kind == DeviceKind::Mouse {
                    ConnectionMode::Wireless24g
                } else {
                    ConnectionMode::Wired
                };
                let pid = if mode == ConnectionMode::Wireless24g {
                    meta.wireless24g_pid
                } else {
                    meta.wired_pid
                };
                let link_id = make_link_id(mode, pid, "mock");
                let bat = if meta.kind == DeviceKind::Mouse { 86 } else { 100 };
                let chg = if mode == ConnectionMode::Wired { 2 } else { 0 };
                let link = DeviceLinkInfo {
                    link_id: link_id.clone(),
                    mode,
                    label: mode.label().to_string(),
                    short_label: mode.short_label().to_string(),
                    state: "online".to_string(),
                    online: true,
                    active: true,
                    supports_config: true,
                    supports_screen: meta.kind == DeviceKind::Keyboard,
                    chunk_size: mode.chunk_size(),
                    vid: meta.vids[0],
                    pid,
                    product_name: meta.name.to_string(),
                    path: None,
                    bt_address: None,
                    battery: Some(bat),
                    charging_state: Some(chg),
                };
                out.push(DeviceDescriptor {
                    id: meta.id.to_string(),
                    kind: meta.kind.as_str().to_string(),
                    name: meta.name.to_string(),
                    model: meta.model.to_string(),
                    chip: meta.chip.to_string(),
                    firmware: meta.firmware.to_string(),
                    image: meta.image.to_string(),
                    card: meta.card.to_string(),
                    key_count: meta.key_count,
                    mock: true,
                    connected: false,
                    connection_mode: mode,
                    connection_label: mode.label().to_string(),
                    connection_short_label: mode.short_label().to_string(),
                    link_id,
                    link_state: "online".to_string(),
                    supports_config: true,
                    supports_screen: meta.kind == DeviceKind::Keyboard,
                    chunk_size: mode.chunk_size(),
                    vid: meta.vids[0],
                    pid,
                    product_name: meta.name.to_string(),
                    path: None,
                    bt_address: None,
                    battery: Some(bat),
                    charging_state: Some(chg),
                    links: vec![link],
                });
            }
        }

        // 活动链路切换时自动丢弃旧会话
        for dev in &out {
            let changed = inner
                .sessions
                .get(&dev.id)
                .map(|s| s.link_id() != dev.link_id)
                .unwrap_or(false);
            if changed {
                inner.sessions.remove(&dev.id);
            }
        }

        inner.last_topo_sig = compute_topo_sig(&out);
        inner.cached_devices = Some(out.clone());
        out
    }

    fn ensure_session<'a>(inner: &'a mut ManagerInner, device_id: &str) -> Result<&'a mut DeviceSession, String> {
        if !inner.sessions.contains_key(device_id) {
            let devices = Self::scan_locked(inner, false, None);
            let entry = devices
                .into_iter()
                .find(|d| d.id == device_id)
                .ok_or_else(|| format!("未找到设备：{device_id}"))?;
            let kind = if entry.kind == "mouse" {
                DeviceKind::Mouse
            } else {
                DeviceKind::Keyboard
            };
            let transport = if entry.mock {
                Transport::new_mock(
                    kind,
                    entry.connection_mode,
                    true,
                    true,
                    entry.battery,
                    entry.charging_state,
                    entry.bt_address.clone(),
                )
            } else if entry.connection_mode == ConnectionMode::Bluetooth && !entry.supports_config {
                Transport::new_bt_readonly(
                    entry.id.clone(),
                    entry.battery,
                    entry.bt_address.clone(),
                )
            } else if let Some(path) = entry.path.clone() {
                Transport::new_win_hid(
                    path,
                    entry.connection_mode,
                    &entry.link_state,
                    entry.bt_address.clone(),
                )
            } else {
                Transport::new_mock(kind, entry.connection_mode, true, true, entry.battery, entry.charging_state, None)
            };

            let cached = inner.last_known_states.get(device_id).cloned();
            let sess = match kind {
                DeviceKind::Keyboard => {
                    DeviceSession::Keyboard(KeyboardAdapter::new(transport, entry.link_id.clone(), cached))
                }
                DeviceKind::Mouse => {
                    DeviceSession::Mouse(MouseAdapter::new(transport, entry.link_id.clone(), cached))
                }
            };
            inner.sessions.insert(device_id.to_string(), sess);
        }
        Ok(inner.sessions.get_mut(device_id).unwrap())
    }

    pub fn list_devices(&self, force_scan: bool) -> Vec<DeviceDescriptor> {
        let mut inner = self.inner.lock().unwrap();
        Self::scan_locked(&mut inner, force_scan, None)
    }

    pub fn rescan_devices(&self) -> Vec<DeviceDescriptor> {
        let mut inner = self.inner.lock().unwrap();
        inner.sessions.clear();
        Self::scan_locked(&mut inner, true, None)
    }

    pub fn wake_device(&self, device_id: &str) -> Result<Value, String> {
        let (devices, device, state) = {
            let mut inner = self.inner.lock().unwrap();
            inner.sessions.remove(device_id);
            let devices = Self::scan_locked(&mut inner, true, Some(device_id));
            let dev = devices.iter().find(|d| d.id == device_id).cloned();
            let st = if dev.is_some() {
                Self::ensure_session(&mut inner, device_id)
                    .and_then(|s| s.read_state())
                    .ok()
            } else {
                None
            };
            if let Some(ref s) = st {
                if s.get("staleConfig").and_then(Value::as_bool) != Some(true) {
                    inner.last_known_states.insert(device_id.to_string(), s.clone());
                }
            }
            (devices, dev, st)
        };
        Ok(json!({ "devices": devices, "device": device, "state": state }))
    }

    pub fn switch_link(&self, device_id: &str, link_id: &str) -> Result<Value, String> {
        let (devices, device, state) = {
            let mut inner = self.inner.lock().unwrap();
            inner.preferred_links.insert(device_id.to_string(), link_id.to_string());
            inner.sessions.remove(device_id);
            let devices = Self::scan_locked(&mut inner, true, Some(device_id));
            let dev = devices.iter().find(|d| d.id == device_id).cloned();
            let st = if dev.is_some() {
                Self::ensure_session(&mut inner, device_id)
                    .and_then(|s| s.read_state())
                    .ok()
            } else {
                None
            };
            if let Some(ref s) = st {
                if s.get("staleConfig").and_then(Value::as_bool) != Some(true) {
                    inner.last_known_states.insert(device_id.to_string(), s.clone());
                }
            }
            (devices, dev, st)
        };
        Ok(json!({ "devices": devices, "device": device, "state": state }))
    }

    pub fn read_state(&self, device_id: &str) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        let st = Self::ensure_session(&mut inner, device_id)?.read_state()?;
        if st.get("staleConfig").and_then(Value::as_bool) != Some(true) {
            inner.last_known_states.insert(device_id.to_string(), st.clone());
        }
        if let Some(ref mut devs) = inner.cached_devices {
            if let Some(d) = devs.iter_mut().find(|d| d.id == device_id) {
                if let Some(ls) = st.get("linkState").and_then(Value::as_str) {
                    d.link_state = ls.to_string();
                }
                if let Some(sc) = st.get("supportsConfig").and_then(Value::as_bool) {
                    d.supports_config = sc;
                }
                if let Some(b) = st.get("battery").and_then(Value::as_u64) {
                    d.battery = Some(b as u8);
                }
                if let Some(c) = st.get("chargingState").and_then(Value::as_u64) {
                    d.charging_state = Some(c as u8);
                }
            }
        }
        Ok(st)
    }

    pub fn read_battery(&self, device_id: &str) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        Self::ensure_session(&mut inner, device_id)?.read_battery()
    }

    pub fn write_config(&self, device_id: &str, patch: &Value) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        Self::ensure_session(&mut inner, device_id)?.write_config(patch)
    }

    pub fn write_key(&self, device_id: &str, slot: usize, hex: &str) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        Self::ensure_session(&mut inner, device_id)?.write_key(slot, hex)
    }

    pub fn reset_key(&self, device_id: &str, slot: usize) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        Self::ensure_session(&mut inner, device_id)?.reset_key(slot)
    }

    pub fn reset_all_keys(&self, device_id: &str) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        Self::ensure_session(&mut inner, device_id)?.reset_all_keys()
    }

    pub fn upload_screen_image<F>(
        &self,
        device_id: &str,
        raw_rgb565: &[u8],
        name: &str,
        reuse_uuid: Option<&str>,
        on_progress: F,
    ) -> Result<Value, String>
    where
        F: FnMut(u32, usize, usize),
    {
        self.screen_uploading.store(true, Ordering::SeqCst);
        let res = {
            let mut inner = self.inner.lock().unwrap();
            match Self::ensure_session(&mut inner, device_id)? {
                DeviceSession::Keyboard(kb) => kb
                    .upload_screen_image(raw_rgb565, name, reuse_uuid, on_progress)
                    .map_err(|e| e.to_string()),
                DeviceSession::Mouse(_) => Err("该设备不支持小屏图像载入".to_string()),
            }
        };
        self.screen_uploading.store(false, Ordering::SeqCst);
        res
    }

    pub fn save_screen_to_library(&self, device_id: &str, raw_rgb565: &[u8], name: &str) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        match Self::ensure_session(&mut inner, device_id)? {
            DeviceSession::Keyboard(kb) => kb
                .save_screen_to_library(raw_rgb565, name)
                .map_err(|e| e.to_string()),
            DeviceSession::Mouse(_) => Err("该设备不支持小屏图库".to_string()),
        }
    }

    pub fn delete_screen_library_item(&self, device_id: &str, uuid: &str) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        match Self::ensure_session(&mut inner, device_id)? {
            DeviceSession::Keyboard(kb) => kb
                .delete_screen_library_item(uuid)
                .map_err(|e| e.to_string()),
            DeviceSession::Mouse(_) => Err("该设备不支持小屏图库".to_string()),
        }
    }

    pub fn read_custom_rgb(&self, device_id: &str, group: u8) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        match Self::ensure_session(&mut inner, device_id)? {
            DeviceSession::Keyboard(kb) => kb.read_custom_rgb(group).map_err(|e| e.to_string()),
            DeviceSession::Mouse(_) => Err("该设备不支持逐键自定义 RGB".to_string()),
        }
    }

    pub fn write_custom_rgb(
        &self,
        device_id: &str,
        group: u8,
        rgb_bytes: &[u8],
        activate: bool,
    ) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        match Self::ensure_session(&mut inner, device_id)? {
            DeviceSession::Keyboard(kb) => kb
                .write_custom_rgb(group, rgb_bytes, activate)
                .map_err(|e| e.to_string()),
            DeviceSession::Mouse(_) => Err("该设备不支持逐键自定义 RGB".to_string()),
        }
    }

    pub fn start_direct_rgb(&self, device_id: &str, include_logo: bool) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        if inner.saved_direct_light.contains_key(device_id) {
            self.rgb_streaming.store(true, Ordering::SeqCst);
            return Ok(json!({ "ok": true, "alreadyActive": true }));
        }
        let saved = match Self::ensure_session(&mut inner, device_id)? {
            DeviceSession::Keyboard(kb) => kb
                .enter_direct_rgb(include_logo)
                .map_err(|e| e.to_string())?,
            DeviceSession::Mouse(_) => return Err("该设备不支持实时 RAM 逐键推流".to_string()),
        };
        inner.saved_direct_light.insert(device_id.to_string(), saved);
        self.rgb_streaming.store(true, Ordering::SeqCst);
        Ok(json!({
            "ok": true,
            "includeLogo": include_logo,
            "prevLightMode": saved.prev_light_mode,
            "prevLogoMode": saved.prev_logo_mode,
        }))
    }

    pub fn stop_direct_rgb(&self, device_id: &str) -> Result<Value, String> {
        let mut inner = self.inner.lock().unwrap();
        let saved = inner.saved_direct_light.remove(device_id);
        if inner.saved_direct_light.is_empty() {
            self.rgb_streaming.store(false, Ordering::SeqCst);
        }
        let Some(saved) = saved else {
            return Ok(json!({ "ok": true, "restored": false }));
        };
        match Self::ensure_session(&mut inner, device_id)? {
            DeviceSession::Keyboard(kb) => kb.exit_direct_rgb(saved).map_err(|e| e.to_string()),
            DeviceSession::Mouse(_) => Ok(json!({ "ok": true, "restored": false })),
        }
    }

    pub fn stream_rgb_frame(
        &self,
        device_id: &str,
        keys_rgb: &[u8],
        logo_rgb: Option<&[u8]>,
    ) -> Result<(), String> {
        let mut inner = self.inner.lock().unwrap();
        match Self::ensure_session(&mut inner, device_id)? {
            DeviceSession::Keyboard(kb) => kb
                .stream_rgb_frame(keys_rgb, logo_rgb)
                .map_err(|e| e.to_string()),
            DeviceSession::Mouse(_) => Err("该设备不支持实时 RAM 逐键推流".to_string()),
        }
    }

    #[cfg(test)]
    pub fn is_direct_rgb_active(&self, device_id: &str) -> bool {
        self.inner
            .lock()
            .map(|i| i.saved_direct_light.contains_key(device_id))
            .unwrap_or(false)
    }

    pub fn has_any_direct_rgb(&self) -> bool {
        self.inner
            .lock()
            .map(|i| !i.saved_direct_light.is_empty())
            .unwrap_or(false)
    }

    pub fn stop_all_direct_rgb(&self) {
        self.rgb_streaming.store(false, Ordering::SeqCst);
        if let Ok(mut inner) = self.inner.lock() {
            let saved_list: Vec<(String, SavedDirectLightState)> =
                inner.saved_direct_light.drain().collect();
            for (dev_id, saved) in saved_list {
                if let Some(DeviceSession::Keyboard(kb)) = inner.sessions.get_mut(&dev_id) {
                    let _ = kb.exit_direct_rgb(saved);
                }
            }
        }
    }

    pub fn close_all_sessions(&self) {
        self.monitor_stop.store(true, Ordering::SeqCst);
        self.rgb_streaming.store(false, Ordering::SeqCst);
        if let Ok(mut inner) = self.inner.lock() {
            // 退出软件前自动恢复正在实时推流的键盘原板载灯效，避免键盘停留在空白 RAM 帧
            let saved_list: Vec<(String, SavedDirectLightState)> =
                inner.saved_direct_light.drain().collect();
            for (dev_id, saved) in saved_list {
                if let Some(DeviceSession::Keyboard(kb)) = inner.sessions.get_mut(&dev_id) {
                    let _ = kb.exit_direct_rgb(saved);
                }
            }
            inner.sessions.clear();
        }
    }

    /// 启动轻量后台热插拔 + 2.4G 休眠唤醒 + 15 秒电量遥测监控线程
    pub fn start_monitor<F>(&self, on_event: F)
    where
        F: Fn(Value) + Send + 'static,
    {
        let force_mock = self.inner.lock().unwrap().force_mock;
        if force_mock {
            return;
        }
        let mgr = self.clone();
        thread::spawn(move || {
            let mut last_telemetry = Instant::now();
            let mut last_sleep_poll = Instant::now();
            while !mgr.monitor_stop.load(Ordering::SeqCst) {
                thread::sleep(Duration::from_secs(4));
                if mgr.monitor_stop.load(Ordering::SeqCst)
                    || mgr.screen_uploading.load(Ordering::SeqCst)
                    || mgr.rgb_streaming.load(Ordering::SeqCst)
                {
                    continue;
                }

                // 1. 仅用无状态枚举比对 USB/BT 路径指纹（绝不反复开关闲置接收器句柄）
                let all_hid = enumerate_hid_devices();
                let raw_sig = compute_raw_path_sig(&all_hid);

                let (raw_changed, has_active_sleeping) = {
                    let inner = mgr.inner.lock().unwrap();
                    let changed = raw_sig != inner.last_raw_sig;
                    let sleeping = inner
                        .cached_devices
                        .as_ref()
                        .map(|ds| {
                            ds.iter().any(|d| {
                                !d.mock
                                    && d.connection_mode == ConnectionMode::Wireless24g
                                    && d.link_state == "sleeping"
                            })
                        })
                        .unwrap_or(false);
                    (changed, sleeping)
                };

                let need_wake = has_active_sleeping && last_sleep_poll.elapsed() >= Duration::from_secs(8);
                if raw_changed || need_wake {
                    if need_wake {
                        last_sleep_poll = Instant::now();
                    }
                    let (changed_topo, devices) = {
                        let mut inner = mgr.inner.lock().unwrap();
                        let prev = inner.last_topo_sig.clone();
                        let devs = Self::scan_locked(&mut inner, true, None);
                        (inner.last_topo_sig != prev, devs)
                    };
                    if changed_topo {
                        on_event(json!({ "type": "topology", "devices": devices }));
                    }
                    continue;
                }

                // 2. 每 15 秒复用已打开会话刷新在线电量与充电状态
                if last_telemetry.elapsed() >= Duration::from_secs(15) {
                    last_telemetry = Instant::now();
                    let mut updates = Vec::new();
                    let mut state_flipped = false;
                    let ids: Vec<String> = {
                        let inner = mgr.inner.lock().unwrap();
                        inner
                            .cached_devices
                            .as_ref()
                            .map(|ds| {
                                ds.iter()
                                    .filter(|d| !d.mock && d.link_state != "sleeping")
                                    .map(|d| d.id.clone())
                                    .collect()
                            })
                            .unwrap_or_default()
                    };

                    for id in ids {
                        if mgr.screen_uploading.load(Ordering::SeqCst)
                            || mgr.rgb_streaming.load(Ordering::SeqCst)
                        {
                            break;
                        }
                        if let Ok(b) = mgr.read_battery(&id) {
                            let mut inner = mgr.inner.lock().unwrap();
                            if let Some(ref mut devs) = inner.cached_devices {
                                if let Some(d) = devs.iter_mut().find(|x| x.id == id) {
                                    if let Some(v) = b.get("battery").and_then(Value::as_u64) {
                                        d.battery = Some(v as u8);
                                    }
                                    if let Some(v) = b.get("chargingState").and_then(Value::as_u64) {
                                        d.charging_state = Some(v as u8);
                                    }
                                    if let Some(ls) = b.get("linkState").and_then(Value::as_str) {
                                        if ls != d.link_state {
                                            d.link_state = ls.to_string();
                                            state_flipped = true;
                                        }
                                    }
                                    updates.push(json!({
                                        "id": id,
                                        "battery": b.get("battery"),
                                        "chargingState": b.get("chargingState"),
                                        "connectionMode": b.get("connectionMode").unwrap_or(&json!(d.connection_mode.as_str())),
                                        "linkState": b.get("linkState").unwrap_or(&json!(d.link_state)),
                                    }));
                                }
                            }
                        }
                    }

                    if state_flipped {
                        let devs = mgr.list_devices(true);
                        on_event(json!({ "type": "topology", "devices": devs }));
                    } else if !updates.is_empty() {
                        on_event(json!({ "type": "telemetry", "updates": updates }));
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::CHUNK_SIZE_24G;

    #[test]
    fn mock_end_to_end_wired_24g_and_bt() {
        let mgr = DeviceManager::new(true);
        let devs = mgr.list_devices(true);
        assert_eq!(devs.len(), 2);

        // 1. 键盘配置 + 改键 + 恢复 + 小屏写入
        let kb_st = mgr.read_state("kb-1s").unwrap();
        assert_eq!(kb_st["battery"], 100);
        assert_eq!(kb_st["keymap"].as_array().unwrap().len(), 126);

        let wr = mgr
            .write_config(
                "kb-1s",
                &json!({
                    "light": { "mode": 5, "brightness": 2, "speed": 2, "multiColor": false, "color": "#e53945" },
                    "system": { "winLock": true, "altF4": true, "pollingRate": 1 }
                }),
            )
            .unwrap();
        assert_eq!(wr["ok"], true);
        assert_eq!(wr["config"]["light"]["mode"], 5);
        assert_eq!(wr["config"]["system"]["winLock"], true);

        let kw = mgr.write_key("kb-1s", 0, "200035").unwrap();
        assert_eq!(kw["ok"], true);
        let kr = mgr.reset_key("kb-1s", 0).unwrap();
        assert_eq!(kr["hex"], "200029");

        let dummy_rgb = vec![0xf8u8; 320 * 172 * 2];
        let mut last_pct = 0;
        let sc = mgr
            .upload_screen_image("kb-1s", &dummy_rgb, "RustTest.png", None, |pct, _, _| {
                last_pct = pct;
            })
            .unwrap();
        assert_eq!(sc["ok"], true);
        assert_eq!(last_pct, 100);
        assert_eq!(sc["bytesSent"], 0x20000);

        // 2. 鼠标 2.4G 24B 分包 + OP 0x1a len=0 + 休眠唤醒
        let ms_st = mgr.read_state("ms-1s").unwrap();
        assert_eq!(ms_st["connectionMode"], "2.4g");
        assert_eq!(ms_st["chunkSize"], CHUNK_SIZE_24G);
        assert_eq!(ms_st["battery"], 86);

        let ms_wr = mgr
            .write_config(
                "ms-1s",
                &json!({
                    "performance": { "polling24g": 6, "activeDpi": 3, "lod": 2, "sleepSec": 300 },
                    "dpiStages": [{ "index": 0, "dpiX": 1250, "dpiY": 1250, "color": "#e53945" }]
                }),
            )
            .unwrap();
        assert_eq!(ms_wr["ok"], true);
        assert_eq!(ms_wr["config"]["dpiStages"][0]["dpiX"], 1250);
        assert_eq!(ms_wr["config"]["performance"]["polling24g"], 6);

        // 3. 2.4G 休眠平滑降级与写保护
        let mut t24 = Transport::new_mock(
            DeviceKind::Mouse,
            ConnectionMode::Wireless24g,
            true,
            true,
            Some(75),
            Some(0),
            None,
        );
        t24.set_mock_online(false);
        let mut ms24 = MouseAdapter::new(t24, "2.4g:2277:test".into(), None);
        let sleep_st = ms24.read_state().unwrap();
        assert_eq!(sleep_st["linkState"], "sleeping");
        assert_eq!(sleep_st["supportsConfig"], false);
        assert!(ms24.write_config(&json!({"performance": {"activeDpi": 1}})).is_err());

        // 4. 蓝牙只读保护
        let tbt = Transport::new_mock(
            DeviceKind::Keyboard,
            ConnectionMode::Bluetooth,
            true,
            false,
            Some(82),
            Some(0),
            Some("A1:B2:C3:D4:E5:F6".into()),
        );
        let mut kb_bt = KeyboardAdapter::new(tbt, "bt:test".into(), None);
        let kb_bt_st = kb_bt.read_state().unwrap();
        assert_eq!(kb_bt_st["linkState"], "bt-readonly");
        assert_eq!(kb_bt_st["battery"], 82);
        assert!(kb_bt.write_config(&json!({"system": {"winLock": true}})).is_err());

        // 5. 逐键自定义 RGB 板载写入 (OP 0x0a/0x0b) + 实时 RAM 推流 (OP 0x12/0x28) 与退出恢复
        let mut custom_rgb = vec![0u8; 126 * 3];
        custom_rgb[0..3].copy_from_slice(&[229, 57, 69]);
        custom_rgb[42..45].copy_from_slice(&[0, 210, 255]);
        let wr_rgb = mgr.write_custom_rgb("kb-1s", 1, &custom_rgb, true).unwrap();
        assert_eq!(wr_rgb["ok"], true);
        assert_eq!(wr_rgb["config"]["light"]["mode"], 18);
        assert_eq!(wr_rgb["config"]["light"]["customGroup"], 1);
        let rd_rgb = mgr.read_custom_rgb("kb-1s", 1).unwrap();
        assert_eq!(rd_rgb["rgb"][0], 229);
        assert_eq!(rd_rgb["rgb"][43], 210);

        let st_dir = mgr.start_direct_rgb("kb-1s", true).unwrap();
        assert_eq!(st_dir["ok"], true);
        assert_eq!(st_dir["prevLightMode"], 18);
        assert!(mgr.is_direct_rgb_active("kb-1s"));
        let logo_rgb = vec![255u8; 60];
        mgr.stream_rgb_frame("kb-1s", &custom_rgb, Some(&logo_rgb)).unwrap();
        let stop_dir = mgr.stop_direct_rgb("kb-1s").unwrap();
        assert_eq!(stop_dir["ok"], true);
        assert_eq!(stop_dir["config"]["light"]["mode"], 18);
        assert!(!mgr.is_direct_rgb_active("kb-1s"));

        // 6. 托盘一键停止所有后台推流并恢复原板载灯效
        mgr.start_direct_rgb("kb-1s", true).unwrap();
        assert!(mgr.has_any_direct_rgb());
        mgr.stop_all_direct_rgb();
        assert!(!mgr.has_any_direct_rgb());
        let after_tray = mgr.read_state("kb-1s").unwrap();
        assert_eq!(after_tray["config"]["light"]["mode"], 18);
    }
}
