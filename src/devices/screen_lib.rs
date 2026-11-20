//! 键盘 320×172 小屏本机图库与官方 Goper 持久化兼容层：
//! - Big-Endian RGB565 <-> 标准 24-bit 320×172 BMP 编码（165,174 字节）；
//! - 读写 `%LOCALAPPDATA%\Goper\keyboard\ScreenFileList.ini`，与官方驱动无缝双向同步。

use std::fs;
use std::path::{Path, PathBuf};

use base64::Engine;
use chrono::Local;
use serde::{Deserialize, Serialize};

pub const SCREEN_W: usize = 320;
pub const SCREEN_H: usize = 172;
pub const SCREEN_PIXEL_BYTES: usize = SCREEN_W * SCREEN_H * 2; // 110,080 字节
pub const SCREEN_FLASH_BLOCK: usize = 0x20000; // 131,072 字节 (128KB)

const GOPER_DEFAULT_BMP: &str = r"C:\Program Files (x86)\Goper\DefaultData\DefaultScreenFile.bmp";
const EMBEDDED_DEFAULT_PNG: &[u8] = include_bytes!("../../ui/assets/default-screen.png");

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IniEntry {
    #[serde(rename = "ScreenFileUUID", default)]
    pub uuid: String,
    #[serde(rename = "ScreenFileislink", default)]
    pub is_link: i32,
    #[serde(rename = "ScreenFilelinkmame", default = "default_ext")]
    pub ext: String,
    #[serde(rename = "ScreenFilename", default)]
    pub name: String,
}

fn default_ext() -> String {
    ".bmp".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct ScreenIniFile {
    #[serde(rename = "ScreenFileList", default)]
    list: Vec<IniEntry>,
    #[serde(rename = "ScreenFileSelect", default)]
    select: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ScreenLibraryItem {
    pub index: usize,
    pub uuid: String,
    pub name: String,
    pub is_linked: bool,
    pub file_path: Option<String>,
    pub data_url: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PersistedScreenState {
    pub has_linked_cache: bool,
    pub current_uuid: Option<String>,
    pub current_data_url: Option<String>,
    pub current_name: String,
    pub default_data_url: Option<String>,
    pub items: Vec<ScreenLibraryItem>,
}

fn goper_kb_dir() -> Option<PathBuf> {
    std::env::var_os("LOCALAPPDATA").map(|p| PathBuf::from(p).join("Goper").join("keyboard"))
}

fn file_to_data_url(path: &Path) -> Option<String> {
    let bytes = fs::read(path).ok()?;
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mime = match ext.as_str() {
        "png" => "image/png",
        "gif" => "image/gif",
        "jpg" | "jpeg" => "image/jpeg",
        "webp" => "image/webp",
        _ => "image/bmp",
    };
    let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
    Some(format!("data:{mime};base64,{b64}"))
}

pub fn rgb565_to_bmp(rgb565: &[u8]) -> Vec<u8> {
    let row_stride = SCREEN_W * 3; // 960B (天然 4B 对齐)
    let img_size = row_stride * SCREEN_H;
    let header_size = 54usize;
    let mut bmp = vec![0u8; header_size + img_size];

    bmp[0] = b'B';
    bmp[1] = b'M';
    bmp[2..6].copy_from_slice(&((header_size + img_size) as u32).to_le_bytes());
    bmp[10..14].copy_from_slice(&(header_size as u32).to_le_bytes());

    bmp[14..18].copy_from_slice(&40u32.to_le_bytes());
    bmp[18..22].copy_from_slice(&(SCREEN_W as i32).to_le_bytes());
    bmp[22..26].copy_from_slice(&(SCREEN_H as i32).to_le_bytes());
    bmp[26..28].copy_from_slice(&1u16.to_le_bytes());
    bmp[28..30].copy_from_slice(&24u16.to_le_bytes());
    bmp[34..38].copy_from_slice(&(img_size as u32).to_le_bytes());

    for y in 0..SCREEN_H {
        let dst_y = SCREEN_H - 1 - y;
        let dst_row = header_size + dst_y * row_stride;
        let src_row = y * SCREEN_W * 2;
        for x in 0..SCREEN_W {
            if src_row + x * 2 + 1 >= rgb565.len() {
                break;
            }
            let hi = rgb565[src_row + x * 2] as u16;
            let lo = rgb565[src_row + x * 2 + 1] as u16;
            let val = (hi << 8) | lo;
            let r5 = ((val >> 11) & 0x1f) as u8;
            let g6 = ((val >> 5) & 0x3f) as u8;
            let b5 = (val & 0x1f) as u8;
            let dst = dst_row + x * 3;
            bmp[dst] = (b5 << 3) | (b5 >> 2);
            bmp[dst + 1] = (g6 << 2) | (g6 >> 4);
            bmp[dst + 2] = (r5 << 3) | (r5 >> 2);
        }
    }
    bmp
}

pub fn make_goper_uuid() -> String {
    Local::now().format("%Y_%m_%d_%H_%M_%S_%3f").to_string()
}

pub fn sanitize_label(name: &str, fallback: &str) -> String {
    let trimmed = name.trim();
    let without_ext = match trimmed.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && ext.len() <= 5 && !ext.contains(['/', '\\', ' ']) => stem,
        _ => trimmed,
    };
    let clean = without_ext.trim();
    if clean.is_empty() {
        fallback.to_string()
    } else {
        clean.chars().take(28).collect()
    }
}

fn read_ini(dir: &Path) -> ScreenIniFile {
    let ini_path = dir.join("ScreenFileList.ini");
    let Ok(txt) = fs::read_to_string(ini_path) else {
        return ScreenIniFile::default();
    };
    serde_json::from_str(&txt).unwrap_or_default()
}

fn write_ini(dir: &Path, mut ini: ScreenIniFile) {
    let _ = fs::create_dir_all(dir);
    if ini.list.is_empty() {
        ini.select = 0;
    } else {
        ini.select = ini.select.min(ini.list.len() - 1);
    }
    if let Ok(json_txt) = serde_json::to_string_pretty(&ini) {
        let _ = fs::write(dir.join("ScreenFileList.ini"), json_txt);
    }
}

pub fn read_persisted_screen_state() -> PersistedScreenState {
    let default_data_url = file_to_data_url(Path::new(GOPER_DEFAULT_BMP)).or_else(|| {
        let b64 = base64::engine::general_purpose::STANDARD.encode(EMBEDDED_DEFAULT_PNG);
        Some(format!("data:image/png;base64,{b64}"))
    });

    let mut items = Vec::new();
    let mut select_idx = 0usize;

    if let Some(dir) = goper_kb_dir() {
        let ini = read_ini(&dir);
        select_idx = ini.select;
        for (idx, entry) in ini.list.iter().enumerate() {
            let ext = if entry.ext.is_empty() { ".bmp" } else { entry.ext.as_str() };
            let file_path = dir.join(format!("{}{}", entry.uuid, ext));
            if let Some(data_url) = file_to_data_url(&file_path) {
                items.push(ScreenLibraryItem {
                    index: idx,
                    uuid: entry.uuid.clone(),
                    name: if entry.name.is_empty() {
                        format!("ScreenFile_{idx}")
                    } else {
                        entry.name.clone()
                    },
                    is_linked: entry.is_link == 1,
                    file_path: Some(file_path.to_string_lossy().to_string()),
                    data_url,
                });
            }
        }
    }

    let linked = items.iter().find(|it| it.is_linked);
    let fallback_item = items.get(select_idx).or_else(|| items.first());
    let current_uuid = linked
        .map(|it| it.uuid.clone())
        .or_else(|| fallback_item.map(|it| it.uuid.clone()));
    let current_data_url = linked
        .map(|it| it.data_url.clone())
        .or_else(|| fallback_item.map(|it| it.data_url.clone()))
        .or_else(|| default_data_url.clone());
    let current_name = if let Some(l) = linked {
        format!("本机已同步 ({})", l.name)
    } else {
        "本机无写入记录（硬件不支持 USB 读屏，暂显示默认预览）".to_string()
    };

    PersistedScreenState {
        has_linked_cache: linked.is_some(),
        current_uuid,
        current_data_url,
        current_name,
        default_data_url,
        items,
    }
}

pub fn save_screen_library_bmp(
    rgb565: &[u8],
    name: &str,
    link: bool,
    reuse_uuid: Option<&str>,
) -> (Option<String>, PersistedScreenState) {
    let Some(dir) = goper_kb_dir() else {
        return (None, read_persisted_screen_state());
    };
    let _ = fs::create_dir_all(&dir);
    let mut ini = read_ini(&dir);

    if let Some(ru) = reuse_uuid {
        if let Some(idx) = ini.list.iter().position(|it| it.uuid == ru) {
            if link {
                for (i, it) in ini.list.iter_mut().enumerate() {
                    it.is_link = if i == idx { 1 } else { 0 };
                }
                ini.select = idx;
                write_ini(&dir, ini);
            }
            return (Some(ru.to_string()), read_persisted_screen_state());
        }
    }

    let uuid = make_goper_uuid();
    let bmp_path = dir.join(format!("{uuid}.bmp"));
    if fs::write(&bmp_path, rgb565_to_bmp(rgb565)).is_err() {
        return (None, read_persisted_screen_state());
    }

    if link {
        for it in &mut ini.list {
            it.is_link = 0;
        }
    }
    let fallback_name = format!("ScreenFile_{}", ini.list.len());
    ini.list.push(IniEntry {
        uuid: uuid.clone(),
        is_link: if link { 1 } else { 0 },
        ext: ".bmp".to_string(),
        name: sanitize_label(name, &fallback_name),
    });
    if link {
        ini.select = ini.list.len() - 1;
    }
    write_ini(&dir, ini);
    (Some(uuid), read_persisted_screen_state())
}

pub fn delete_screen_library_bmp(uuid: &str) -> PersistedScreenState {
    if let Some(dir) = goper_kb_dir() {
        let mut ini = read_ini(&dir);
        if let Some(idx) = ini.list.iter().position(|it| it.uuid == uuid) {
            let removed = ini.list.remove(idx);
            let ext = if removed.ext.is_empty() { ".bmp" } else { removed.ext.as_str() };
            let _ = fs::remove_file(dir.join(format!("{}{}", removed.uuid, ext)));
            write_ini(&dir, ini);
        }
    }
    read_persisted_screen_state()
}
