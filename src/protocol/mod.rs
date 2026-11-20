//! REDMAGIC / Eevision (NeoUSBlib) USB & 2.4G HID 协议指令表与帧编解码。

pub mod frame;
pub mod usage;

pub const REPORT_ID: u8 = 0x04;
pub const FRAME_SIZE: usize = 64;
pub const CHUNK_SIZE: usize = 0x38; // USB 有线单包最大有效负载 56 字节 (Goper.exe 0x4e2830 mode=1)
pub const CHUNK_SIZE_24G: usize = 0x18; // 2.4G 无线单包最大有效负载 24 字节 (32B RF 帧 - 8B 头, mode=2)
pub const WIRELESS_NACK: u8 = 0x50; // 2.4G 接收器在设备休眠/离线时立即返回的 NACK 操作码

// 事务控制
pub const TX_BEGIN: u8 = 0x01;
pub const TX_COMMIT: u8 = 0x02;

// 设备信息与状态
pub const READ_INFO: u8 = 0x03;
pub const POLL_STATUS: u8 = 0x1a;
pub const HEARTBEAT: u8 = 0xaa;

// 配置块读写
pub const READ_CONFIG: u8 = 0x05;
pub const WRITE_CONFIG: u8 = 0x06;

// 键位映射表读写
pub const READ_KEYMAP_ROM: u8 = 0x07;
pub const READ_KEYMAP_USER: u8 = 0x08;
pub const WRITE_KEYMAP: u8 = 0x09;

// 逐键自定义 RGB（板载 Flash 持久化，支持多组槽位 group * 0x200）
pub const READ_CUSTOM_RGB: u8 = 0x0a;
pub const WRITE_CUSTOM_RGB: u8 = 0x0b;
pub const CUSTOM_RGB_GROUP_STRIDE: u32 = 0x200;

// 实时 RAM 逐键 / Logo 灯条流式推送（零 Flash 擦写损耗，无需 TX_BEGIN/TX_COMMIT）
// 需将 light.mode = 29 (0x1d) 激活主键盘实时流，logo.mode = 5 激活顶部 20 颗 Logo 实时流
pub const STREAM_KEY_RGB: u8 = 0x12;
pub const STREAM_LOGO_RGB: u8 = 0x28;
pub const KB_MODE_CUSTOM: u8 = 18;
pub const KB_MODE_DIRECT: u8 = 29;
pub const KB_LOGO_MODE_DIRECT: u8 = 5;
pub const LOGO_LED_COUNT: usize = 20;
pub const LOGO_RGB_LEN: usize = LOGO_LED_COUNT * 3; // 60 字节

// 扩展与小屏指令（严禁调用 0x22 / 0x24 / 0x2A 出厂字库擦写指令！）
pub const WRITE_SCREEN_GIF_1: u8 = 0x21;
pub const PREPARE_SCREEN_IMG: u8 = 0x23;
pub const WRITE_SCREEN_IMG: u8 = 0x25;
pub const WRITE_SCREEN_GIF_2: u8 = 0x2c;
