# Changelog

All notable changes to **REDMAGIC Control** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.3.1] - 2026-11-20

### Changed
- **Zero-scroll single-viewport layout at `1280×820`**: Reflowed keyboard Keymap (`按键映射`) with a balanced `780px` stage cap and side-by-side physical key capture + preset selector (`km-picker-grid`), compacted Mini-Screen (`小屏图片`) cropper bezel/toolbar (`396px`) and preview proportions, and added bounded internal scroll to multi-link list in Hardware Diagnostics (`硬件诊断`), so all 9 views fit cleanly without page scrollbars while remaining freely resizable.
- **Documentation & dev preview**: Added `?device=...&tab=...` (`REDMAGIC_VIEW`) initial view routing and embedded updated frosted-glass showcase screenshots in `README.md`.

## [0.3.0] - 2026-11-20

### Added
- **Pure Rust + Tauri v2 architecture**: Complete native rewrite replacing Electron and `node-hid`, producing a single ~9 MB standalone Windows executable.
- **Native Win32 Overlapped HID backend** (`src/devices/hid_win.rs`): Zero C `hidapi` dependency, `CancelIoEx` + `GetOverlappedResult` deterministic I/O cancellation and RAII handle lifecycle to eliminate heap corruption crashes.
- **Native Win32 Bluetooth BLE backend** (`src/devices/bluetooth_win.rs`): SetupAPI + CfgMgr32 direct discovery and `0x180F` battery reading without spawning PowerShell subprocesses.
- **Three-mode connection manager** (`src/devices/manager.rs`): Unified USB wired (`56B`), 2.4G wireless (`24B` chunking + `OP 0xaa` wake/sleep probing + `OP 0x50` NACK handling), and BLE read-only battery links with priority aggregation and hotplug/telemetry monitoring.
- **GK001S & GS001S full device controls**: Performance & polling rate, RGB lighting preview & capability-aware controls, physical keymap editor with factory ROM diff/reset, and 320×172 TFT mini-screen image cropper/library (`0xCE0000` wired flash upload + `ScreenFileList.ini` sync).
- **Windows 11 Acrylic + obsidian frosted glass UI**: Native DWM Acrylic backdrop and rounded corners, layered CSS glassmorphism across all UI components, compact inline header telemetry pills replacing redundant KPI cards, and unified vector SVG battery gauges with left-aligned charging bolts.
- **Startup resilience**: Windows named mutex single-instance guard, automatic fallback WebView2 data directory on `0x800700AA` busy locks, and `--probe` CLI read-only self-check mode.
