//! 64 字节固定 HID 帧编解码：
//!   [0]     0x04       Report ID
//!   [1..3]  Checksum   uint16_le = sum(frame[3..64]) & 0xFFFF
//!   [3]     Opcode     指令码
//!   [4]     Length     本次有效负载长度 (<= 0x38 有线 / <= 0x18 2.4G)
//!   [5..7]  Offset     uint16_le 偏移地址
//!   [7]     Status/Sub 发送填 (off >> 16) & 0xff；接收 0x00=OK，0xff/0xfe=Error
//!   [8..64] Payload    有效数据区（最多 56B，未用补 0）

use super::{
    FRAME_SIZE, HEARTBEAT, POLL_STATUS, REPORT_ID, STREAM_KEY_RGB, STREAM_LOGO_RGB, WIRELESS_NACK,
};

#[derive(Debug, Clone)]
pub struct HidFrame {
    pub report_id: u8,
    #[allow(dead_code)]
    pub checksum: u16,
    pub op: u8,
    pub len: u8,
    pub off: u16,
    pub status: u8,
    pub data: [u8; 56],
    #[allow(dead_code)]
    pub raw: [u8; FRAME_SIZE],
}

pub fn calc_checksum(buf: &[u8]) -> u16 {
    let mut sum: u16 = 0;
    for &b in buf.iter().skip(3) {
        sum = sum.wrapping_add(b as u16);
    }
    sum
}

pub fn build_frame(op: u8, len: usize, off: u32, payload: Option<&[u8]>, sub: Option<u8>) -> [u8; FRAME_SIZE] {
    let mut frame = [0u8; FRAME_SIZE];
    frame[0] = REPORT_ID;
    frame[3] = op;
    frame[4] = (len & 0xff) as u8;
    let off_lo = (off & 0xffff) as u16;
    frame[5..7].copy_from_slice(&off_lo.to_le_bytes());
    frame[7] = sub.unwrap_or(((off >> 16) & 0xff) as u8);
    if let Some(src) = payload {
        let n = src.len().min(FRAME_SIZE - 8);
        frame[8..8 + n].copy_from_slice(&src[..n]);
    }
    let cs = calc_checksum(&frame);
    frame[1..3].copy_from_slice(&cs.to_le_bytes());
    frame
}

pub fn parse_frame(bytes: &[u8]) -> HidFrame {
    let mut full = [0u8; FRAME_SIZE];
    if bytes.len() >= FRAME_SIZE {
        full.copy_from_slice(&bytes[..FRAME_SIZE]);
    } else if !bytes.is_empty() {
        full[0] = REPORT_ID;
        let n = bytes.len().min(FRAME_SIZE - 1);
        full[1..1 + n].copy_from_slice(&bytes[..n]);
    }
    let mut data = [0u8; 56];
    data.copy_from_slice(&full[8..FRAME_SIZE]);
    HidFrame {
        report_id: full[0],
        checksum: u16::from_le_bytes([full[1], full[2]]),
        op: full[3],
        len: full[4],
        off: u16::from_le_bytes([full[5], full[6]]),
        status: full[7],
        data,
        raw: full,
    }
}

pub fn matches_header(frame: &HidFrame, op: u8, len: usize, off: u32) -> bool {
    if frame.report_id != REPORT_ID || frame.op != op {
        return false;
    }
    // 2.4G 无线唤醒 / 在线探测 (OP 0xaa)
    if op == HEARTBEAT {
        return frame.off == 0;
    }
    // 电量/状态查询 (OP 0x1a)：2.4G 鼠标 (0x2277) 实测回复 len=0x00
    if op == POLL_STATUS {
        return frame.off == (off & 0xffff) as u16
            && (frame.len == (len & 0xff) as u8 || frame.len == 0);
    }
    // 实时 RAM 逐键/Logo 灯效流（对齐 Goper.exe 0x4dfb15：仅校验 op 回包一致即可）
    if op == STREAM_KEY_RGB || op == STREAM_LOGO_RGB {
        return true;
    }
    if op == 0x21 || op == 0x25 || op == 0x2c {
        return frame.len == (len & 0xff) as u8
            && frame.off == (off & 0xffff) as u16
            && frame.status == ((off >> 16) & 0xff) as u8;
    }
    if (0x21..=0x2c).contains(&op) {
        return true;
    }
    frame.len == (len & 0xff) as u8 && frame.off == (off & 0xffff) as u16
}

/// 2.4G 接收器在键鼠休眠或未切至 2.4G 档位时，对非 0xaa 指令立即回包 op=0x50
pub fn is_wireless_nack(frame: &HidFrame) -> bool {
    frame.report_id == REPORT_ID && frame.op == WIRELESS_NACK && frame.len == 0 && frame.off == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::{POLL_STATUS, READ_CONFIG, WRITE_SCREEN_IMG};

    #[test]
    fn checksum_and_parse_match_goper_trace() {
        let raw = build_frame(READ_CONFIG, 0x32, 0, None, None);
        assert_eq!(raw[0], 0x04);
        assert_eq!(u16::from_le_bytes([raw[1], raw[2]]), 0x0037);
        let parsed = parse_frame(&raw);
        assert_eq!(parsed.checksum, calc_checksum(&raw));
        assert!(matches_header(&parsed, READ_CONFIG, 0x32, 0));
    }

    #[test]
    fn poll_status_accepts_24g_len_zero_reply() {
        let raw = build_frame(POLL_STATUS, 0, 0, Some(&[0x4d, 0x00]), Some(0));
        let parsed = parse_frame(&raw);
        assert!(matches_header(&parsed, POLL_STATUS, 0x06, 0));
        assert_eq!(parsed.data[0], 0x4d);
    }

    #[test]
    fn screen_write_matches_24bit_offset() {
        let raw = build_frame(WRITE_SCREEN_IMG, 0x38, 0x010038, None, None);
        let parsed = parse_frame(&raw);
        assert_eq!(parsed.status, 0x01);
        assert!(matches_header(&parsed, WRITE_SCREEN_IMG, 0x38, 0x010038));
        assert!(!matches_header(&parsed, WRITE_SCREEN_IMG, 0x38, 0x000038));
    }

    #[test]
    fn detects_wireless_nack() {
        let raw = build_frame(WIRELESS_NACK, 0, 0, None, None);
        assert!(is_wireless_nack(&parse_frame(&raw)));
    }
}
