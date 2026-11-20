# Changelog

All notable changes to **REDMAGIC Control** will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.4.1] - 2026-11-20

### Added
- **Background-suspended RGB streaming & Windows System Tray**:
  - **Native Windows System Tray (`tray-icon`)**: Left-click/double-click restores the main window; right-click menu provides `打开 REDMAGIC Control`, `停止灯效推流（恢复板载）`, and `退出程序`. Re-launching the executable also un-hides and focuses a tray-suspended window.
  - **Smart close-to-tray when streaming**: Clicking the window close button (`×`) while real-time RAM streaming or the local UDP/HTTP/WebSocket bridge is active automatically suspends the app to the system tray instead of stopping your lighting; when idle, closing exits immediately.
  - **Zero-throttle background push loop**: Passed WebView2 `--disable-background-timer-throttling --disable-renderer-backgrounding --disable-backgrounding-occluded-windows` flags and decoupled the persistent background hardware push timer from the foreground `requestAnimationFrame` DOM preview loop, maintaining steady `30 FPS` (USB wired) / `8 FPS` (2.4G) streaming even when switching tabs/devices, minimizing, gaming fullscreen, or suspended in the tray.
  - **Global sidebar streaming status card + one-click `后台挂起`**: Added a live glowing status card in the left rail visible across all tabs and devices (jump back to Studio, suspend to tray, or stop streaming in one click) plus a dedicated `后台挂起` button on the Studio control bar. Manually modifying onboard keyboard lighting automatically stops background streaming to avoid conflicts.

## [0.4.0] - 2026-11-20

### Added
- **Programmable RGB Studio (`灯效编程`)** for `GK001S` with 4 unified sub-modes fitting cleanly inside `1280×820` without page scrolling:
  1. **Onboard Per-Key Static RGB Painter (`逐键涂色`, `OP 0x0a` / `OP 0x0b`)**: Click or drag-paint across all 100 physical keys (brush / right-click eraser / eyedropper), 4 independent onboard Flash slots (`group 0..3`, offset `group * 0x200`, `378 B`), and 6 quick zone templates (RedMagic, FPS WASD, MOBA QWER, Cyber Gradient, Pure White, Clear). Persists offline across Bluetooth, 2.4G, and power cycles.
  2. **JS Shader Programmable Lighting Engine + Zero-Wear Volatile RAM Streaming (`脚本编程`, `OP 0x12` + `OP 0x28`)**: Live hot-reloading `render(x, y, t, ctx) -> [r, g, b]` editor with 6 built-in presets (Cyber Aurora, RedMagic Radar, Typing Ripple, Music Spectrum, Screen Ambilight, Digital Matrix), real-time `p1/p2/p3` sliders, `.json`/`.js` preset import/export, 60 FPS UI preview, and direct volatile RAM streaming (`30 FPS` USB wired / `8 FPS` 2.4G, 100 keys `378 B` + top 20-LED Logo strip `60 B`, zero Flash wear, automatic onboard lighting restore on stop or exit).
  3. **Multi-Layer Compositor + Pure-Rust Windows System Linkage Engines (`多图层`)**:
     - `src/studio/audio.rs`: Pure-Rust Windows WASAPI loopback capture with built-in 512-point Radix-2 FFT 16-band spectrum + adaptive AGC and live equalizer bar.
     - `src/studio/ambilight.rs`: Pure-Rust Win32 GDI `16×6` primary screen color sampler + 20-LED top edge mapper with vibrance boost and EMA smoothing.
     - `src/studio/keyhook.rs`: Pure-Rust 60 Hz Win32 `GetAsyncKeyState` global key activity & coordinate pulse mapper for typing ripples.
  4. **Local Loopback External RGB Bridge API (`外部接口`, `src/studio/server.rs`)**: Strictly bound to `127.0.0.1`, exposing high-speed UDP (`27650`, `<1 ms` `RMGB` binary frame + JSON) and HTTP / RFC 6455 WebSocket (`27651`: `GET /api/status`, `POST /api/rgb`, `WS /ws`) with live packet/FPS telemetry, stage preview, and copyable Python starter snippet.
- **Background monitor guard during streaming**: Automatically pauses background HID telemetry polling while real-time RAM streaming is active, and restores original keyboard `light.mode` / `logo.mode` cleanly on tab switch, stop, or app exit.

## [0.3.2] - 2026-11-20

### Fixed
- **Disabled browser context menu & DevTools in desktop UI**: Disabled WebView2 native default right-click menu (`SetAreDefaultContextMenusEnabled(false)`), DevTools (`SetAreDevToolsEnabled(false)`), status bar hover tooltip, and page zoom controls at both the Win32 `ICoreWebView2Settings` layer and DOM `contextmenu`/`keydown` layer (blocking `F3`/`F5`/`F12`/`Ctrl+R`/`Ctrl+P`/`Ctrl+S`/`Ctrl+U`/`Ctrl+F`/`Ctrl+Shift+I` while preserving standard text copy/paste and Keymap physical key capture). Set `REDMAGIC_DEVTOOLS=1` when debugging locally.

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
