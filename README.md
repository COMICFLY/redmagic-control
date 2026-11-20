# REDMAGIC Control

轻量级红魔外设桌面控制中心 — **纯 Rust + Tauri v2** 原生实现（无 Node.js / 无 Electron / 无 C `hidapi` 运行时依赖），支持 **REDMAGIC 氘锋机械键盘 `GK001S`** 与 **REDMAGIC 电竞鼠标 `GS001S`**。

## 核心特性

- **纯 Rust Win32 原生驱动栈**
  - 原生 Win32 Overlapped HID（`CM_Get_Device_Interface_ListW` + `HidD_*` + `CancelIoEx` 确定性取消 + RAII 自动句柄清理），杜绝并发读写竞态与内存损坏闪退。
  - 原生 Win32 SetupAPI / CfgMgr32 蓝牙 BLE 发现，直接读取系统 `DEVPKEY_Bluetooth_Battery`（GATT `0x180F` 电池服务）、连接状态与 MAC 地址，无需外部 PowerShell 子进程。
- **三模连接管理（USB 有线 / 2.4G 无线 / 蓝牙 BLE）**
  - 自动按 `USB 有线 → 2.4G 在线 → 蓝牙 BLE → 2.4G 休眠待机` 优先级聚合多链路，支持在硬件诊断页手动切换通道与一键唤醒。
  - 完整适配 `56B`（有线）与 `24B`（2.4G RF 分包）协议 MTU、`OP 0xaa` 在线握手探测、`OP 0x1a` 电量/充电态解码与 `OP 0x50` NACK 保护。
- **全功能外设控制**
  - **概览与性能**：键盘回报率（125–1000 Hz）、Win 键锁、Alt+F4 屏蔽、WASD 互换、低功耗开关；鼠标 5 档板载 DPI（50–26000）、有线/2.4G 回报率（最高 8000 Hz）、LOD、休眠时间与波纹控制。
  - **灯光管理**：键盘主灯区 + 顶部 26 格 Logo 灯条独立控制；鼠标三灯区实时联动预览，严格按固件能力表动态显隐色彩/速度/方向/亮度控件。
  - **按键映射**：全宽物理键位热区点击改键、物理按键快速录入、对照出厂 ROM（`OP 0x07`）单键/全键恢复。
  - **320×172 小屏图片选择器（GK001S）**：支持本地选图、拖拽导入、剪贴板粘贴（`Ctrl+V`）、平移/缩放/旋转/翻转裁切、本机图库（同步 `%LOCALAPPDATA%\Goper\keyboard\ScreenFileList.ini`），安全写入静态壁纸扇区 `0xCE0000`（仅 USB 有线直连模式开放）。
- **暗黑曜石毛玻璃界面**
  - Windows 11 原生 DWM Acrylic 毛玻璃底透与圆角边框 + 全套组件多层 `backdrop-filter` 磨砂玻璃设计。
  - 单实例互斥锁保护与 WebView2 数据目录占用自动回退。

## 快速构建与运行

### 环境要求

- Windows 10/11（x86_64）
- Rust 工具链（`rustup` / `cargo`，edition 2021）
- Microsoft Edge WebView2 Runtime（Windows 11 默认内置）

### 常用命令

```powershell
# 运行单元测试（协议帧、蓝牙解析、Mock 端到端）
cargo test

# 真机/仿真命令行只读自检（不弹 GUI 窗口，绝不写设备 Flash）
cargo run -- --probe

# 仿真模式预览界面（无真机硬件也可运行）
$env:MOCK_MODE="1"; cargo run

# 构建独立 Release 可执行文件（约 9 MB）
cargo build --release
# 输出路径：target\release\redmagic-control.exe
```

## 目录结构

```text
├── Cargo.toml               # Rust 依赖与 Release 优化配置
├── tauri.conf.json          # Tauri v2 应用与安全配置
├── capabilities/            # Tauri v2 最小权限声明
├── icons/                   # 应用图标资源
├── src/
│   ├── main.rs              # 入口、单实例锁、Win11 DWM Acrylic、WebView2 容错与 --probe 模式
│   ├── commands.rs          # Tauri v2 IPC 命令层（设备读写、文件选择、剪贴板读图）
│   ├── protocol/            # 64B HID 帧编解码、校验和、按键编码与出厂 ROM 表
│   └── devices/             # Win32 HID/BLE 底层、键盘/鼠标适配器、三模管理器与小屏图库
└── ui/                      # 原生轻量前端（无打包器、零第三方前端框架依赖）
    ├── index.html
    ├── assets/
    └── src/renderer/        # tokens.css / styles.css / app.js / panels.js / stage.js / lighting.js
```

## 版本管理规范

本项目遵循 [Semantic Versioning (SemVer)](https://semver.org/) 规范管理版本：
- `Cargo.toml` 与 `tauri.conf.json` 中的 `version` 保持同步；
- 每次发版同步更新 [CHANGELOG.md](./CHANGELOG.md) 并创建对应的 annotated git tag（如 `v0.3.0`、`v0.3.1`）。
