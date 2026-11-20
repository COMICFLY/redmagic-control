/* 轻量前端主控制器：统一状态、IPC 驱动、键盘物理录入与实时硬件同步。 */

import { icon } from "./icons.js";
import { renderConnectionBanner, renderPageTelemetry, renderOverview, renderLighting, renderKeymap, renderScreen, renderHardware, renderStudio } from "./panels.js";
import { renderBatteryGauge, renderConnectionBadge } from "./stage.js";
import { LightingAnimator } from "./lighting.js";
import {
  STUDIO_SCRIPTS,
  StudioController,
  applyPaintTemplate,
  buildExternalExamplePython,
  buildNativeConfig,
  createInitialStudioState,
  hexToRgb,
  isNativeEligible,
  rgbToHex,
} from "./studio.js";

const lighting = new LightingAnimator();
const studioCtrl = new StudioController();

const TABS = [
  { id: "overview", label: "概览与性能", ico: "sliders" },
  { id: "lighting", label: "灯光", ico: "sun" },
  { id: "studio", label: "灯效编程", ico: "sparkles", kbOnly: true },
  { id: "keymap", label: "按键映射", ico: "command" },
  { id: "screen", label: "小屏图片", ico: "monitor", kbOnly: true },
  { id: "hardware", label: "硬件诊断", ico: "cpu" },
];

const CODE_TO_HEX = {
  Escape: "200029", Backquote: "200035", Tab: "20002b", CapsLock: "200039",
  Space: "20002c", Enter: "200028", Backspace: "20002a", Delete: "20004c",
  Insert: "200049", Home: "20004a", End: "20004d", PageUp: "20004b", PageDown: "20004e",
  ArrowRight: "20004f", ArrowLeft: "200050", ArrowDown: "200051", ArrowUp: "200052",
  Minus: "20002d", Equal: "20002e", BracketLeft: "20002f", BracketRight: "200030",
  Backslash: "200031", Semicolon: "200033", Quote: "200034", Comma: "200036",
  Period: "200037", Slash: "200038",
  ControlLeft: "200100", ShiftLeft: "200200", AltLeft: "200400", MetaLeft: "200800",
  ControlRight: "201000", ShiftRight: "202000", AltRight: "204000", MetaRight: "208000",
  NumLock: "200053", NumpadDivide: "200054", NumpadMultiply: "200055",
  NumpadSubtract: "200056", NumpadAdd: "200057", NumpadEnter: "200058",
  Numpad1: "200059", Numpad2: "20005a", Numpad3: "20005b", Numpad4: "20005c",
  Numpad5: "20005d", Numpad6: "20005e", Numpad7: "20005f", Numpad8: "200060",
  Numpad9: "200061", Numpad0: "200062", NumpadDecimal: "200063",
};
for (let i = 0; i < 26; i++) CODE_TO_HEX[`Key${String.fromCharCode(65 + i)}`] = `2000${(4 + i).toString(16).padStart(2, "0")}`;
for (let i = 1; i <= 9; i++) CODE_TO_HEX[`Digit${i}`] = `2000${(0x1d + i).toString(16).padStart(2, "0")}`;
CODE_TO_HEX.Digit0 = "200027";
for (let i = 1; i <= 12; i++) CODE_TO_HEX[`F${i}`] = `2000${(0x39 + i).toString(16).padStart(2, "0")}`;

const state = {
  meta: null,
  devices: [],
  activeId: null,
  tab: "overview",
  hwStates: new Map(),
  ui: {
    kbLightZone: "kb",
    selectedSlot: null,
    keyCategory: null,
    pendingHex: null,
    capturingKey: false,
    screenSourceKind: "library",
    screenSelectedUuid: null,
    screenLibImages: new Map(),
    screenPreset: "default",
    screenFit: "cover",
    screenZoom: 1,
    screenPanX: 0,
    screenPanY: 0,
    screenRotate: 0,
    screenFlipH: false,
    screenBgColor: "#08090b",
    screenText: "",
    screenCustomImg: null,
    screenCustomName: "",
    screenSyncedImg: null,
    screenDefaultImg: null,
    screenDataUrl: null,
    screenUploading: false,
    screenUploadPct: 0,
    studio: {
      ...createInitialStudioState(),
      scriptSensors: { ...STUDIO_SCRIPTS[0].sensors },
    },
  },
  busy: false,
};

const $ = (sel) => document.querySelector(sel);

function esc(str) {
  return String(str ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

function toast(title, detail = "", isError = false) {
  const stack = $("#toast-stack");
  const el = document.createElement("div");
  el.className = `toast${isError ? " is-error" : ""}`;
  el.innerHTML = `<strong>${esc(title)}</strong>${detail ? `<small>${esc(detail)}</small>` : ""}`;
  stack.appendChild(el);
  setTimeout(() => el.remove(), 2600);
}

function setSyncText(text, dotClass = null) {
  $("#sync-status").textContent = text;
  if (dotClass) {
    const dot = $("#conn-dot");
    dot.className = `status-dot ${dotClass}`;
  }
}

async function callApi(fn, ...args) {
  if (!window.rm) throw new Error("请在桌面环境中运行");
  const res = await window.rm[fn](...args);
  if (!res || !res.ok) throw new Error(res?.error || "设备通信失败");
  return res.data;
}

function currentDevice() {
  return state.devices.find((d) => d.id === state.activeId) || state.devices[0] || null;
}

function renderChrome() {
  $("#win-min").innerHTML = icon("minus", 14);
  $("#win-max").innerHTML = icon("square", 13);
  $("#win-close").innerHTML = icon("x", 14);
  $("#rescan-btn").innerHTML = icon("refresh", 14);
}

function renderNavTabs() {
  const dev = currentDevice();
  const isKb = !dev || dev.kind === "keyboard";
  if (!isKb && (state.tab === "screen" || state.tab === "studio")) state.tab = "overview";
  const visibleTabs = TABS.filter((t) => !t.kbOnly || isKb);
  $("#nav-tabs").innerHTML = visibleTabs.map((t) =>
    `<button type="button" class="nav-tab" role="tab" data-tab="${t.id}" aria-selected="${state.tab === t.id}">${icon(t.ico, 15)}<span>${t.label}</span></button>`
  ).join("");
}

function renderDeviceRail() {
  const listEl = $("#device-list");
  listEl.innerHTML = state.devices.map((d) => {
    const hw = state.hwStates.get(d.id);
    const active = d.id === state.activeId;
    const mode = hw?.connectionMode || d.connectionMode || "wired";
    const linkState = hw?.linkState || d.linkState || "online";
    const bat = hw?.battery != null ? hw.battery : d.battery;
    const chg = hw?.chargingState != null ? hw.chargingState : d.chargingState;
    return `
      <button type="button" class="device-item${active ? " is-active" : ""}${linkState === "sleeping" ? " is-sleeping" : ""}" role="option" aria-selected="${active}" data-device-id="${esc(d.id)}">
        <div class="device-thumb"><img src="${esc(d.image)}" alt="" draggable="false" /></div>
        <div class="device-item-top">
          <span class="device-name">${esc(d.name)}</span>
          ${renderConnectionBadge(mode, linkState, { mock: d.mock, compact: true })}
        </div>
        <div class="device-meta mono">
          <span>${esc(d.model)} · ${(d.pid || 0).toString(16).padStart(4, "0")}</span>
          ${renderBatteryGauge(bat, { size: "sm", chargingState: chg })}
        </div>
      </button>`;
  }).join("");

  const activeDev = currentDevice();
  const activeHw = activeDev && state.hwStates.get(activeDev.id);
  const titleBat = $("#titlebar-bat");
  if (titleBat) {
    if (activeDev) {
      const mode = activeHw?.connectionMode || activeDev.connectionMode || "wired";
      const linkState = activeHw?.linkState || activeDev.linkState || "online";
      const bat = activeHw?.battery != null ? activeHw.battery : activeDev.battery;
      const chg = activeHw?.chargingState != null ? activeHw.chargingState : activeDev.chargingState;
      titleBat.hidden = false;
      titleBat.innerHTML = `${renderConnectionBadge(mode, linkState, { mock: activeDev.mock, compact: true })}${bat != null ? renderBatteryGauge(bat, { size: "sm", chargingState: chg }) : ""}`;
    } else {
      titleBat.hidden = true;
    }
  }

  const liveDevs = state.devices.filter((d) => !d.mock);
  const onlineCount = liveDevs.filter((d) => {
    const hw = state.hwStates.get(d.id);
    const st = hw?.linkState || d.linkState;
    return st === "online" || st === "bt-readonly";
  }).length;
  const sleepingCount = liveDevs.length - onlineCount;
  if (liveDevs.length > 0) {
    $("#conn-text").textContent = sleepingCount > 0
      ? `真机 · ${onlineCount} 在线 / ${sleepingCount} 休眠`
      : `真机 · ${onlineCount} 台在线`;
  } else {
    $("#conn-text").textContent = `仿真模式 · ${state.devices.length} 台`;
  }
  $("#conn-dot").className = `status-dot ${state.busy ? "is-busy" : (liveDevs.length > 0 ? (onlineCount > 0 ? "is-live" : "is-mock") : "is-mock")}`;
  renderRailStreamCard();
}

function keyboardDevice() {
  return state.devices.find((d) => d.kind === "keyboard") || null;
}

function getStudioBgState() {
  const kb = keyboardDevice();
  return {
    studio: state.ui.studio,
    dev: kb,
    hw: kb ? state.hwStates.get(kb.id) : null,
  };
}

function studioSourceLabel(st) {
  if (st.serverStatus?.running) return `UDP ${st.udpPort} / WS ${st.httpPort}`;
  if (st.streamSource === "layers") return "多图层实时混合";
  const preset = STUDIO_SCRIPTS.find((s) => s.id === st.scriptId);
  return preset ? preset.name : "自定义 JS 脚本";
}

function renderRailStreamCard() {
  const card = $("#rail-stream-card");
  if (!card) return;
  const st = state.ui.studio;
  const active = Boolean(st.streaming || st.serverStatus?.running);
  if (!active) {
    card.hidden = true;
    card.innerHTML = "";
    return;
  }
  const kb = keyboardDevice();
  const hw = kb && state.hwStates.get(kb.id);
  const is24g = (hw?.connectionMode || kb?.connectionMode) === "2.4g";
  const fps = st.serverStatus?.running
    ? (st.serverStatus.fps || 0)
    : (st.streamFps || (is24g ? 8 : 30));
  const title = st.serverStatus?.running
    ? "外部控灯桥运行中"
    : st.nativeStreaming
      ? "纯 Rust 后台推流"
      : "JS 脚本后台推流";
  const subHint = (st.nativeStreaming || st.serverStatus?.running)
    ? `${studioSourceLabel(st)} · 挂起零浏览器占用`
    : `${studioSourceLabel(st)} · 支持托盘挂起`;

  card.hidden = false;
  card.innerHTML = `
    <button type="button" class="rail-stream-head" id="rail-jump-studio" title="点击返回灯效编程工作台">
      <span class="rail-stream-title">
        <span class="stream-pulse-dot" aria-hidden="true"></span>
        <span>${title}</span>
      </span>
      <span class="mono rail-stream-fps" id="rail-stream-fps">${fps} FPS</span>
    </button>
    <div class="rail-stream-sub" title="点击关闭或挂起至托盘将释放界面进程，后台极低占用持续推流">${esc(subHint)}</div>
    <div class="rail-stream-actions">
      <button type="button" class="btn btn-secondary btn-sm" id="rail-tray-btn" title="隐藏到 Windows 系统托盘继续推流">${icon("minus", 12)}挂起至托盘</button>
      <button type="button" class="btn btn-danger btn-sm" id="rail-stop-stream-btn" title="停止推流并恢复键盘原板载灯效">${icon("stop", 12)}停止</button>
    </div>`;
}

let nativeUpdateTimer = null;
function scheduleNativeConfigSync() {
  const st = state.ui.studio;
  if (!st.streaming || !st.nativeStreaming || !window.rm?.updateNativeRgb) return;
  if (nativeUpdateTimer) clearTimeout(nativeUpdateTimer);
  nativeUpdateTimer = setTimeout(() => {
    nativeUpdateTimer = null;
    if (state.ui.studio.streaming && state.ui.studio.nativeStreaming) {
      window.rm.updateNativeRgb(buildNativeConfig(state.ui.studio)).catch(() => {});
    }
  }, 25);
}

async function handoffCustomJsIfNeeded() {
  const st = state.ui.studio;
  if (!st.streaming || !st.nativeStreaming || isNativeEligible(st)) return;
  const kb = keyboardDevice();
  if (!kb) return;
  try {
    await callApi("stopNativeRgb");
    st.nativeStreaming = false;
    await callApi("startDirectRgb", kb.id, st.includeLogo);
    await syncStudioSensors();
    studioCtrl.ensureBackgroundLoop(getStudioBgState, updateStudioLiveStats);
    renderRailStreamCard();
  } catch (_) {}
}

async function hideWindowToTray() {
  if (!window.rm?.hideToTray) return;
  try {
    await callApi("hideToTray");
  } catch (err) {
    toast("挂起至托盘失败", err.message, true);
  }
}

function updateStudioLiveStats(st) {
  const kb = keyboardDevice();
  const hw = kb && state.hwStates.get(kb.id);
  const is24g = (hw?.connectionMode || kb?.connectionMode) === "2.4g";

  const railFps = $("#rail-stream-fps");
  if (railFps && (st.streaming || st.serverStatus?.running)) {
    const fps = st.serverStatus?.running
      ? (st.serverStatus.fps || 0)
      : (st.streamFps || (is24g ? 8 : 30));
    railFps.textContent = `${fps} FPS`;
  }

  if (state.tab !== "studio") return;

  const pill = $("#studio-live-pill");
  if (pill) {
    if (st.serverStatus?.running) {
      pill.className = "conn-badge is-24g";
      pill.textContent = `外部接口接管 · ${st.serverStatus.fps || 0} FPS`;
    } else if (st.streaming) {
      pill.className = "conn-badge is-24g";
      pill.textContent = `RAM 实时推流 · ${st.streamFps || (is24g ? 8 : 30)} FPS`;
    } else if (st.subMode === "paint") {
      pill.className = "conn-badge is-wired";
      pill.textContent = `板载逐键 · 组 ${st.paintGroup + 1}`;
    } else {
      pill.className = "conn-badge";
      pill.textContent = "仅预览 · 未推流";
    }
  }

  const errEl = $("#studio-code-err");
  if (errEl) {
    if (st.scriptError) {
      errEl.hidden = false;
      errEl.textContent = st.scriptError;
    } else {
      errEl.hidden = true;
      errEl.textContent = "";
    }
  }

  const audStatus = $("#studio-audio-status");
  if (audStatus) {
    audStatus.textContent = st.telemetry?.audio?.error || "实时采集";
  }
  const bands = st.telemetry?.audio?.bands;
  if (Array.isArray(bands)) {
    for (const bar of document.querySelectorAll(".studio-eq-bar[data-band]")) {
      const idx = Number(bar.dataset.band);
      const pct = Math.max(8, Math.round((bands[idx] || 0) * 100));
      bar.style.height = `${pct}%`;
    }
  }

  const srvState = $("#studio-srv-state");
  const srvRate = $("#studio-srv-rate");
  if (srvState && srvRate && st.serverStatus) {
    srvState.textContent = st.serverStatus.running ? "运行中 (127.0.0.1)" : "已停止";
    srvRate.textContent = `${st.serverStatus.packets || 0} 帧 · ${st.serverStatus.fps || 0} FPS`;
  }
}

async function syncStudioSensors() {
  if (!window.rm?.setStudioSensors) return;
  const st = state.ui.studio;
  let audio = false;
  let ambilight = false;
  let keys = false;

  // 1. 若后台正在实时推流，优先保障推流源所需的传感器持续工作（即使切换至其他页面或挂起至托盘）
  if (st.streaming) {
    if (st.streamSource === "layers") {
      audio = audio || Boolean(st.layers.audio.enabled);
      ambilight = ambilight || Boolean(st.layers.ambilight.enabled);
      keys = keys || Boolean(st.layers.typing.enabled);
    } else {
      audio = audio || Boolean(st.scriptSensors?.audio);
      ambilight = ambilight || Boolean(st.scriptSensors?.ambilight);
      keys = keys || Boolean(st.scriptSensors?.keys);
    }
  }
  // 2. 前台处于灯效编程页时，合并当前预览模式所需的传感器
  if (state.tab === "studio") {
    if (st.subMode === "layers") {
      audio = audio || Boolean(st.layers.audio.enabled);
      ambilight = ambilight || Boolean(st.layers.ambilight.enabled);
      keys = keys || Boolean(st.layers.typing.enabled);
    } else if (st.subMode === "script") {
      audio = audio || Boolean(st.scriptSensors?.audio);
      ambilight = ambilight || Boolean(st.scriptSensors?.ambilight);
      keys = keys || Boolean(st.scriptSensors?.keys);
    }
  }
  try {
    const res = await callApi("setStudioSensors", audio, ambilight, keys);
    st.telemetry.audio.active = Boolean(res.audio);
    st.telemetry.ambilight.active = Boolean(res.ambilight);
    st.telemetry.keys.active = Boolean(res.keys);
  } catch (_) {}
}

async function stopStudioStreamingIfNeeded(silent = false) {
  const st = state.ui.studio;
  const hadServer = Boolean(st.serverStatus?.running);
  const hadNative = Boolean(st.nativeStreaming);
  const hadStream = Boolean(st.streaming);
  if (!hadStream && !hadServer && !hadNative) return;
  const dev = keyboardDevice() || currentDevice();
  st.streaming = false;
  st.nativeStreaming = false;
  st.streamFps = 0;
  studioCtrl.stopBackgroundLoop();
  if (hadNative && window.rm?.stopNativeRgb) {
    try {
      await callApi("stopNativeRgb");
    } catch (_) {}
  }
  if (hadServer && window.rm?.stopExternalRgbServer) {
    try {
      st.serverStatus = await callApi("stopExternalRgbServer");
    } catch (_) {}
  }
  if (hadStream && !hadNative && dev && window.rm?.stopDirectRgb) {
    try {
      await callApi("stopDirectRgb", dev.id);
    } catch (_) {}
  }
  await syncStudioSensors();
  renderRailStreamCard();
  if (!silent) toast("已恢复板载灯效", "已停止后台灯效推流并还原原灯效");
}

async function loadPaintGroupFromHw(group, { forceOverwrite = true, silent = false } = {}) {
  const dev = currentDevice();
  if (!dev || dev.kind !== "keyboard") return;
  const hw = state.hwStates.get(dev.id);
  if (hw && !hw.mock && hw.supportsConfig === false) return;
  const st = state.ui.studio;
  try {
    const res = await callApi("readCustomRgb", dev.id, group);
    const bytes = Array.isArray(res?.rgb) ? res.rgb : [];
    const hasAny = bytes.some((b) => b !== 0);
    if (forceOverwrite || hasAny) {
      st.paintRgb.fill(0);
      for (let i = 0; i < Math.min(378, bytes.length); i++) {
        st.paintRgb[i] = bytes[i];
      }
    }
    st.paintLoaded = true;
    if (!silent) toast("已读取板载逐键方案", `组 ${group + 1} (${hasAny ? "已加载灯色" : "当前为空白"})`);
  } catch (err) {
    if (!silent) toast("读取板载逐键方案失败", err.message, true);
  }
}

function renderWorkspace() {
  const dev = currentDevice();
  const root = $("#workspace");
  if (!dev) {
    lighting.stop();
    studioCtrl.detach();
    root.innerHTML = `<div class="card"><h2 class="card-title">未发现设备</h2><p class="card-desc">请连接 REDMAGIC 外设（USB 有线 / 2.4G 接收器 / 蓝牙 BLE）后点击左侧刷新按钮。</p></div>`;
    return;
  }
  const hw = state.hwStates.get(dev.id);
  if (!hw) {
    lighting.stop();
    studioCtrl.detach();
    root.innerHTML = `<div class="card"><h2 class="card-title">${esc(dev.name)}</h2><p class="card-desc">正在通过 ${esc(dev.connectionLabel || "HID")} 读取板载配置…</p></div>`;
    return;
  }

  const tabMeta = TABS.find((t) => t.id === state.tab) || TABS[0];
  let bodyHtml = "";
  if (state.tab === "overview") bodyHtml = renderOverview(dev, hw, state.meta, state.ui);
  else if (state.tab === "lighting") bodyHtml = renderLighting(dev, hw, state.meta, state.ui);
  else if (state.tab === "studio") bodyHtml = renderStudio(dev, hw, state.ui);
  else if (state.tab === "keymap") bodyHtml = renderKeymap(dev, hw, state.meta, state.ui);
  else if (state.tab === "screen") bodyHtml = renderScreen(dev, hw, state.ui);
  else bodyHtml = renderHardware(dev, hw);

  const mode = hw.connectionMode || dev.connectionMode || "wired";
  const linkState = hw.linkState || dev.linkState || "online";
  root.innerHTML = `
    <div class="page-head">
      <div class="page-head-main">
        <h1 class="page-title">${esc(tabMeta.label)}</h1>
        <p class="page-sub">
          <span>${esc(dev.name)}</span>
          <span>·</span>
          <span class="mono">${esc(dev.chip)} · 0x${(dev.pid || 0).toString(16).padStart(4, "0")}</span>
          ${renderConnectionBadge(mode, linkState, { mock: dev.mock })}
        </p>
      </div>
      ${renderPageTelemetry(dev, hw, state.meta)}
    </div>
    ${renderConnectionBanner(dev, hw)}
    ${bodyHtml}
  `;

  if (state.tab === "studio" && dev.kind === "keyboard") {
    lighting.stop();
    studioCtrl.attach(
      root,
      () => ({ studio: state.ui.studio, dev: currentDevice(), hw: state.hwStates.get(currentDevice()?.id) }),
      updateStudioLiveStats
    );
    if (state.ui.studio.subMode === "paint" && !state.ui.studio.paintLoaded) {
      loadPaintGroupFromHw(state.ui.studio.paintGroup, { forceOverwrite: false, silent: true });
    }
  } else {
    studioCtrl.detach();
    lighting.start(root, dev, hw);
  }
  if (state.tab === "screen") paintScreenCanvas();
}

function loadImageAsync(src) {
  return new Promise((resolve) => {
    if (!src) return resolve(null);
    const img = new Image();
    img.onload = () => resolve(img);
    img.onerror = () => resolve(null);
    img.src = src;
  });
}

function resetScreenView() {
  state.ui.screenZoom = 1;
  state.ui.screenPanX = 0;
  state.ui.screenPanY = 0;
  state.ui.screenRotate = 0;
  state.ui.screenFlipH = false;
}

function canTransformCurrentScreen() {
  return state.ui.screenSourceKind !== "preset" || state.ui.screenPreset === "default";
}

function isScreenViewEdited() {
  return (
    Math.round((state.ui.screenZoom ?? 1) * 100) !== 100 ||
    Math.abs(state.ui.screenPanX || 0) > 0.5 ||
    Math.abs(state.ui.screenPanY || 0) > 0.5 ||
    (state.ui.screenRotate || 0) !== 0 ||
    Boolean(state.ui.screenFlipH)
  );
}

async function loadCustomImageDataUrl(dataUrl, fileName) {
  const img = await loadImageAsync(dataUrl);
  if (!img || !img.naturalWidth) {
    toast("图片解析失败", "请选择有效的 PNG/JPG/GIF/WebP/BMP 文件", true);
    return false;
  }
  state.ui.screenSourceKind = "custom";
  state.ui.screenCustomImg = img;
  state.ui.screenCustomName = fileName || "本地图片";
  resetScreenView();
  renderWorkspace();
  toast("图片已载入取景器", `${state.ui.screenCustomName} (${img.naturalWidth}×${img.naturalHeight} → 320×172)`);
  return true;
}

function loadCustomImageFile(file) {
  if (!file) return;
  if (file.type && !file.type.startsWith("image/")) {
    toast("不支持的文件格式", "请拖入 PNG / JPG / WebP / BMP / GIF 图片", true);
    return;
  }
  const reader = new FileReader();
  reader.onload = () => loadCustomImageDataUrl(String(reader.result), file.name);
  reader.onerror = () => toast("文件读取失败", file.name, true);
  reader.readAsDataURL(file);
}

async function syncScreenImagesFromState(hwData, { preferUuid = null } = {}) {
  if (!hwData?.screenImage) return;
  const sc = hwData.screenImage;
  const items = Array.isArray(sc.items) ? sc.items : [];
  const [syncedImg, defImg, ...libImgs] = await Promise.all([
    loadImageAsync(sc.currentDataUrl),
    loadImageAsync(sc.defaultDataUrl || "assets/default-screen.png"),
    ...items.map((it) => loadImageAsync(it.dataUrl)),
  ]);
  if (syncedImg) state.ui.screenSyncedImg = syncedImg;
  if (defImg) state.ui.screenDefaultImg = defImg;

  const map = new Map();
  items.forEach((it, idx) => {
    if (libImgs[idx]) map.set(it.uuid, libImgs[idx]);
  });
  state.ui.screenLibImages = map;

  if (preferUuid && map.has(preferUuid)) {
    state.ui.screenSelectedUuid = preferUuid;
    state.ui.screenSourceKind = "library";
  } else if (!state.ui.screenSelectedUuid || !map.has(state.ui.screenSelectedUuid)) {
    if (sc.currentUuid && map.has(sc.currentUuid)) {
      state.ui.screenSelectedUuid = sc.currentUuid;
      if (state.ui.screenSourceKind !== "custom" && state.ui.screenSourceKind !== "preset") {
        state.ui.screenSourceKind = "library";
      }
    } else if (items.length > 0 && map.has(items[0].uuid)) {
      state.ui.screenSelectedUuid = items[0].uuid;
    } else {
      state.ui.screenSelectedUuid = null;
      if (state.ui.screenSourceKind === "library") {
        state.ui.screenSourceKind = "preset";
        state.ui.screenPreset = "default";
      }
    }
  }
  paintScreenCanvas();
}

function drawTransformedImage(ctx, img, W, H, opts) {
  const iw = img.naturalWidth || W;
  const ih = img.naturalHeight || H;
  const fit = opts.fit || "cover";
  const zoom = Math.max(0.5, Math.min(3, Number(opts.zoom) || 1));
  const panX = Number(opts.panX) || 0;
  const panY = Number(opts.panY) || 0;
  const rot = ((Number(opts.rotate) || 0) % 360 + 360) % 360;
  const flipH = Boolean(opts.flipH);

  const effW = rot % 180 ? ih : iw;
  const effH = rot % 180 ? iw : ih;

  ctx.save();
  ctx.imageSmoothingEnabled = true;
  ctx.imageSmoothingQuality = "high";
  ctx.translate(W / 2 + panX, H / 2 + panY);
  if (flipH) ctx.scale(-1, 1);

  if (fit === "fill") {
    ctx.scale((W / effW) * zoom, (H / effH) * zoom);
    if (rot) ctx.rotate((rot * Math.PI) / 180);
  } else {
    const baseScale = fit === "cover" ? Math.max(W / effW, H / effH) : Math.min(W / effW, H / effH);
    const s = baseScale * zoom;
    if (rot) ctx.rotate((rot * Math.PI) / 180);
    ctx.scale(s, s);
  }
  ctx.drawImage(img, -iw / 2, -ih / 2, iw, ih);
  ctx.restore();
}

/* ---- 键盘 320×172 彩屏画布渲染与 Big-Endian RGB565 编码（严格对齐 Goper.exe 0x47b3f0） ---- */
function paintScreenCanvas() {
  let canvas = $("#screen-canvas");
  if (!canvas) {
    canvas = document.createElement("canvas");
    canvas.width = 320;
    canvas.height = 172;
  }
  const ctx = canvas.getContext("2d");
  const W = 320, H = 172;
  ctx.fillStyle = state.ui.screenBgColor || "#08090b";
  ctx.fillRect(0, 0, W, H);

  const kind = state.ui.screenSourceKind || "library";
  const preset = state.ui.screenPreset || "default";
  const drawOpts = {
    fit: state.ui.screenFit || "cover",
    zoom: state.ui.screenZoom ?? 1,
    panX: state.ui.screenPanX || 0,
    panY: state.ui.screenPanY || 0,
    rotate: state.ui.screenRotate || 0,
    flipH: Boolean(state.ui.screenFlipH),
  };

  let activeImg = null;
  if (kind === "custom" && state.ui.screenCustomImg?.naturalWidth > 0) {
    activeImg = state.ui.screenCustomImg;
  } else if (kind === "library") {
    activeImg = (state.ui.screenSelectedUuid && state.ui.screenLibImages.get(state.ui.screenSelectedUuid))
      || state.ui.screenSyncedImg
      || state.ui.screenDefaultImg;
  } else if (kind === "preset" && preset === "default") {
    activeImg = state.ui.screenDefaultImg || state.ui.screenSyncedImg;
  }

  if (activeImg && activeImg.naturalWidth > 0) {
    drawTransformedImage(ctx, activeImg, W, H, drawOpts);
  } else if (preset === "clock") {
    ctx.fillStyle = "#0b0c10";
    ctx.fillRect(0, 0, W, H);
    const now = new Date();
    const hh = String(now.getHours()).padStart(2, "0");
    const mm = String(now.getMinutes()).padStart(2, "0");
    ctx.fillStyle = "#eef0f4";
    ctx.font = "600 54px Consolas, monospace";
    ctx.textAlign = "center";
    ctx.fillText(`${hh}:${mm}`, W / 2, 92);
    ctx.fillStyle = "#e53945";
    ctx.font = "500 13px Consolas, monospace";
    ctx.fillText("GK001S · 1000Hz · ACTIVE", W / 2, 122);
  } else {
    const g = ctx.createLinearGradient(0, 0, W, H);
    g.addColorStop(0, "#19080b");
    g.addColorStop(0.5, "#0d0e12");
    g.addColorStop(1, "#24090d");
    ctx.fillStyle = g;
    ctx.fillRect(0, 0, W, H);
    ctx.strokeStyle = "rgba(229, 57, 69, 0.25)";
    ctx.lineWidth = 2;
    for (let x = -40; x < W + 40; x += 18) {
      ctx.beginPath(); ctx.moveTo(x, H); ctx.lineTo(x + 36, 0); ctx.stroke();
    }
    ctx.fillStyle = "#e53945";
    ctx.beginPath();
    ctx.moveTo(160, 32); ctx.lineTo(188, 52); ctx.lineTo(176, 108); ctx.lineTo(160, 122); ctx.lineTo(144, 108); ctx.lineTo(132, 52);
    ctx.closePath(); ctx.fill();
    ctx.fillStyle = "#0d0e12";
    ctx.fillRect(154, 48, 12, 48);
  }

  // 叠加底部徽标文字
  const label = (state.ui.screenText || "").trim();
  if (label) {
    ctx.fillStyle = "rgba(13, 14, 16, 0.76)";
    ctx.fillRect(0, H - 28, W, 28);
    ctx.fillStyle = "#e53945";
    ctx.fillRect(0, H - 28, 4, 28);
    ctx.fillStyle = "#eef0f4";
    ctx.font = "600 13px Consolas, 'Segoe UI', sans-serif";
    ctx.textAlign = "left";
    ctx.fillText(label, 12, H - 10);
    ctx.fillStyle = "#8c929f";
    ctx.font = "500 11px Consolas, monospace";
    ctx.textAlign = "right";
    ctx.fillText("320×172", W - 10, H - 10);
  }

  state.ui.screenDataUrl = canvas.toDataURL("image/png");
  for (const el of document.querySelectorAll(".kb-mini-screen")) {
    el.src = state.ui.screenDataUrl;
  }
  return canvas;
}

function encodeCanvasToRgb565Base64(canvas) {
  const ctx = canvas.getContext("2d");
  const { data } = ctx.getImageData(0, 0, 320, 172);
  const out = new Uint8Array(320 * 172 * 2);
  let dst = 0;
  for (let i = 0; i < data.length; i += 4) {
    const r = data[i], g = data[i + 1], b = data[i + 2];
    const rgb565 = ((r >> 3) << 11) | ((g >> 2) << 5) | (b >> 3);
    out[dst++] = (rgb565 >> 8) & 0xff; // Big-Endian MSB (Goper.exe 0x47b8cb)
    out[dst++] = rgb565 & 0xff;        // Big-Endian LSB (Goper.exe 0x47b8ee)
  }
  let binary = "";
  const chunk = 0x8000;
  for (let i = 0; i < out.length; i += chunk) {
    binary += String.fromCharCode(...out.subarray(i, i + chunk));
  }
  return btoa(binary);
}

function renderAll() {
  renderNavTabs();
  renderDeviceRail();
  renderWorkspace();
}

function syncStatusForHw(hw) {
  if (!hw) return;
  const syncLabel = hw.mock
    ? "仿真已同步"
    : hw.linkState === "sleeping"
      ? "2.4G 待机休眠"
      : hw.linkState === "bt-readonly"
        ? "蓝牙 BLE 已连接"
        : hw.connectionMode === "2.4g"
          ? "2.4G 已同步"
          : "USB 已同步";
  const syncDot = hw.mock || hw.linkState === "sleeping" ? "is-mock" : "is-live";
  setSyncText(syncLabel, syncDot);
}

async function loadDeviceState(deviceId, showToast = false) {
  state.busy = true;
  setSyncText("读取中...", "is-busy");
  try {
    const data = await callApi("readState", deviceId);
    state.hwStates.set(deviceId, data);
    if (data.kind === "keyboard") await syncScreenImagesFromState(data);
    syncStatusForHw(data);
    if (showToast) toast("读取完成", data.linkState === "sleeping" ? "设备处于 2.4G 休眠态，请唤醒后重试" : "已从板载存储同步最新配置");
  } catch (err) {
    setSyncText("读取失败");
    toast("读取设备失败", err.message, true);
  } finally {
    state.busy = false;
    renderAll();
  }
}

async function patchConfig(patch, label) {
  const dev = currentDevice();
  if (!dev) return;
  // 若用户主动调整键盘原生灯光/Logo 设置，先自动退出后台 RAM 推流使新灯效立即生效
  if (dev.kind === "keyboard" && (patch.light || patch.logo) && (state.ui.studio.streaming || state.ui.studio.serverStatus?.running)) {
    await stopStudioStreamingIfNeeded(true);
  }
  state.busy = true;
  setSyncText("写入硬件...", "is-busy");
  try {
    const res = await callApi("writeConfig", dev.id, patch);
    const hw = state.hwStates.get(dev.id);
    if (hw && res.config) hw.config = res.config;
    setSyncText(res.mock ? "仿真已写入" : "已写入并校验", res.mock ? "is-mock" : "is-live");
    if (!res.ok) toast("校验提示", "设备已响应但回读存在差异", true);
    else if (label) toast("已同步到设备", label);
  } catch (err) {
    setSyncText("写入失败");
    toast("写入失败", err.message, true);
  } finally {
    state.busy = false;
    renderAll();
  }
}

function setNested(obj, dotted, val) {
  const parts = dotted.split(".");
  let cur = obj;
  for (let i = 0; i < parts.length - 1; i++) cur = cur[parts[i]] = {};
  cur[parts[parts.length - 1]] = val;
  return obj;
}

function selectSlot(slot) {
  const dev = currentDevice();
  const hw = dev && state.hwStates.get(dev.id);
  const row = hw?.keymap?.find((k) => k.slot === slot);
  state.ui.selectedSlot = slot;
  state.ui.capturingKey = false;
  if (row) {
    state.ui.pendingHex = row.hex;
    state.ui.keyCategory = row.current.kind === "mouse" || row.current.kind === "dpi" ? "mouse"
      : row.current.kind === "media" ? "media"
      : row.current.kind === "disable" ? "disable" : "key";
  }
  renderWorkspace();
}

function bindEvents() {
  // 窗口控制
  $("#win-min").addEventListener("click", () => window.rm?.win.minimize());
  $("#win-max").addEventListener("click", () => window.rm?.win.toggleMax());
  $("#win-close").addEventListener("click", () => window.rm?.win.close());

  // 顶部导航（允许后台持续推流，切页绝不中断实时 RAM 推流）
  $("#nav-tabs").addEventListener("click", async (e) => {
    const btn = e.target.closest("[data-tab]");
    if (!btn) return;
    const prevTab = state.tab;
    state.tab = btn.dataset.tab;
    state.ui.capturingKey = false;
    if (prevTab !== state.tab && (prevTab === "studio" || state.tab === "studio")) {
      syncStudioSensors();
    }
    if (state.tab === "keymap" && state.ui.selectedSlot === null) {
      const dev = currentDevice();
      if (dev) selectSlot(dev.kind === "keyboard" ? 0 : 3);
    }
    renderAll();
  });

  // 左侧设备切换与重扫（切换至鼠标时键盘灯效推流依然在后台平滑运行）
  $("#device-list").addEventListener("click", async (e) => {
    const item = e.target.closest("[data-device-id]");
    if (!item || item.dataset.deviceId === state.activeId) return;
    state.activeId = item.dataset.deviceId;
    state.ui.selectedSlot = null;
    state.ui.keyCategory = null;
    state.ui.pendingHex = null;
    state.ui.capturingKey = false;
    renderAll();
    if (!state.hwStates.has(state.activeId)) await loadDeviceState(state.activeId);
    else syncStatusForHw(state.hwStates.get(state.activeId));
    syncStudioSensors();
    if (state.tab === "keymap") {
      const dev = currentDevice();
      if (dev) selectSlot(dev.kind === "keyboard" ? 0 : 3);
    }
  });

  // 左侧边栏后台推流控制卡片（跨页面/跨设备随时一键返回、挂起至托盘或停止推流）
  $("#rail-stream-card").addEventListener("click", async (e) => {
    const btn = e.target.closest("button");
    if (!btn) return;
    if (btn.id === "rail-jump-studio") {
      const kb = keyboardDevice();
      if (kb) state.activeId = kb.id;
      state.tab = "studio";
      renderAll();
      return;
    }
    if (btn.id === "rail-tray-btn") {
      await hideWindowToTray();
      return;
    }
    if (btn.id === "rail-stop-stream-btn") {
      await stopStudioStreamingIfNeeded(false);
      const dev = currentDevice();
      if (dev) setSyncText("已恢复板载灯效", dev.mock ? "is-mock" : "is-live");
      renderAll();
    }
  });

  $("#rescan-btn").addEventListener("click", async () => {
    try {
      state.hwStates.clear();
      state.devices = await callApi("rescanDevices");
      if (!state.devices.some((d) => d.id === state.activeId)) state.activeId = state.devices[0]?.id || null;
      renderAll();
      if (state.activeId) await loadDeviceState(state.activeId);
      toast("扫描完成", `发现 ${state.devices.length} 台设备`);
    } catch (err) {
      toast("扫描失败", err.message, true);
    }
  });

  $("#reload-state-btn").addEventListener("click", () => {
    if (state.activeId) loadDeviceState(state.activeId, true);
  });

  // 工作区委托事件（点击）
  const ws = $("#workspace");

  async function triggerPickLocalImage() {
    if (window.rm?.pickScreenFile) {
      try {
        const picked = await callApi("pickScreenFile");
        if (picked && !picked.canceled && picked.dataUrl) {
          await loadCustomImageDataUrl(picked.dataUrl, picked.name);
        }
        return;
      } catch (err) {
        toast("打开图片失败", err.message, true);
        return;
      }
    }
    $("#screen-file-input")?.click();
  }

  async function triggerPasteClipboardImage() {
    if (window.rm?.pasteScreenClipboard) {
      try {
        const res = await callApi("pasteScreenClipboard");
        if (res && !res.empty && res.dataUrl) {
          await loadCustomImageDataUrl(res.dataUrl, res.name);
        } else {
          toast("剪贴板无图片", "请先截图或复制一张图片后再粘贴", true);
        }
        return;
      } catch (err) {
        toast("剪贴板读取失败", err.message, true);
        return;
      }
    }
    toast("剪贴板粘贴仅在桌面端可用", "", true);
  }

  function currentScreenDraftName() {
    if (state.ui.screenSourceKind === "custom" && state.ui.screenCustomName) {
      return state.ui.screenCustomName;
    }
    if (state.ui.screenSourceKind === "preset") {
      return state.ui.screenPreset === "redmagic" ? "红魔战魂"
        : state.ui.screenPreset === "clock" ? "极简时钟" : "官方红魔姬";
    }
    const dev = currentDevice();
    const hw = dev && state.hwStates.get(dev.id);
    const cur = hw?.screenImage?.items?.find((it) => it.uuid === state.ui.screenSelectedUuid);
    return cur ? cur.name : "";
  }

  ws.addEventListener("click", async (e) => {
    const el = e.target.closest("button, [data-screen-lib-uuid]");
    if (!el) return;

    if (el.dataset.lightZone) {
      state.ui.kbLightZone = el.dataset.lightZone;
      renderWorkspace();
      return;
    }
    if (el.dataset.kbMode) {
      const prefix = el.dataset.kbMode;
      const modeId = Number(el.dataset.val);
      const list = prefix === "logo" ? state.meta.kbLogoModes : state.meta.kbLightModes;
      const m = list.find((x) => x.id === modeId);
      const sub = { mode: modeId };
      if (m && m.color && !m.multi) sub[prefix === "logo" ? "sevenColor" : "multiColor"] = false;
      await patchConfig({ [prefix]: sub }, `已切换至「${m?.name || modeId}」`);
      return;
    }
    if (el.dataset.kbColorMode) {
      const prefix = el.dataset.kbColorMode;
      const multi = Number(el.dataset.multi) === 1;
      const key = prefix === "logo" ? "sevenColor" : "multiColor";
      await patchConfig({ [prefix]: { [key]: multi } }, multi ? "已开启多彩循环" : "已切换为指定单色");
      return;
    }
    if (el.dataset.kbCfg) {
      await patchConfig(setNested({}, el.dataset.kbCfg, Number(el.dataset.val)), "键盘配置已写入");
      return;
    }
    if (el.dataset.kbColor) {
      const p = el.dataset.kbColor;
      const patch = p === "logo" ? { logo: { color: el.dataset.hex, sevenColor: false } } : { light: { color: el.dataset.hex, multiColor: false } };
      await patchConfig(patch, `颜色已设为 ${el.dataset.hex}`);
      return;
    }
    if (el.dataset.msMode !== undefined) {
      const modeId = Number(el.dataset.msMode);
      const m = state.meta.msLightModes.find((x) => x.id === modeId);
      const lightPatch = { mode: modeId };
      if (m && m.color && !m.multi) lightPatch.multiColor = false;
      await patchConfig({ light: lightPatch }, `鼠标灯效已切换至「${m?.name || modeId}」`);
      return;
    }
    if (el.dataset.msCfg) {
      await patchConfig(setNested({}, el.dataset.msCfg, Number(el.dataset.val)), "鼠标设置已写入");
      return;
    }
    if (el.dataset.msActiveDpi !== undefined) {
      const idx = Number(el.dataset.msActiveDpi);
      await patchConfig({ performance: { activeDpi: idx } }, `已切换至第 ${idx + 1} 档 DPI`);
      return;
    }
    if (el.dataset.msColor) {
      await patchConfig({ light: { color: el.dataset.msColor, multiColor: false } }, `鼠标灯色已设为 ${el.dataset.msColor}`);
      return;
    }
    if (el.dataset.selectSlot !== undefined) {
      selectSlot(Number(el.dataset.selectSlot));
      return;
    }
    if (el.dataset.keyCat) {
      state.ui.keyCategory = el.dataset.keyCat;
      state.ui.capturingKey = false;
      state.ui.pendingHex = el.dataset.keyCat === "disable" ? "a00000"
        : el.dataset.keyCat === "mouse" ? "100100"
        : el.dataset.keyCat === "media" ? "3000e9" : "200029";
      renderWorkspace();
      return;
    }
    if (el.id === "capture-key-btn") {
      state.ui.capturingKey = !state.ui.capturingKey;
      renderWorkspace();
      return;
    }
    if (el.id === "save-one-key-btn") {
      const dev = currentDevice();
      const slot = state.ui.selectedSlot ?? (dev.kind === "keyboard" ? 0 : 3);
      const hex = state.ui.keyCategory === "disable" ? "a00000" : ($("#key-preset-select")?.value || state.ui.pendingHex);
      if (!hex) return;
      state.busy = true;
      setSyncText("写入键位...", "is-busy");
      try {
        const res = await callApi("writeKey", dev.id, slot, hex);
        await loadDeviceState(dev.id);
        toast("键位已写入设备", `槽位 #${slot} → ${res.decoded.label}`);
      } catch (err) {
        toast("写入键位失败", err.message, true);
      } finally {
        state.busy = false;
      }
      return;
    }
    if (el.id === "reset-one-key-btn") {
      const dev = currentDevice();
      const slot = state.ui.selectedSlot ?? (dev.kind === "keyboard" ? 0 : 3);
      try {
        const res = await callApi("resetKey", dev.id, slot);
        await loadDeviceState(dev.id);
        toast("已恢复出厂默认", `槽位 #${slot} → ${res.decoded.label}`);
      } catch (err) {
        toast("恢复失败", err.message, true);
      }
      return;
    }
    if (el.id === "reset-all-keys-btn") {
      const dev = currentDevice();
      try {
        await callApi("resetAllKeys", dev.id);
        await loadDeviceState(dev.id);
        toast("全部键位已重置", `${dev.model} 已恢复出厂 ROM 键表`);
      } catch (err) {
        toast("重置失败", err.message, true);
      }
      return;
    }
    if (el.id === "copy-diag-btn") {
      const dev = currentDevice();
      const hw = state.hwStates.get(dev.id);
      navigator.clipboard?.writeText(JSON.stringify({ device: dev, state: hw }, null, 2));
      toast("已复制诊断 JSON", "可直接用于协议分析或问题反馈");
      return;
    }
    if (el.id === "wake-device-btn") {
      const dev = currentDevice();
      if (!dev) return;
      state.busy = true;
      setSyncText("唤醒探测中...", "is-busy");
      renderDeviceRail();
      try {
        const res = await callApi("wakeDevice", dev.id);
        if (res.devices) state.devices = res.devices;
        if (res.state) {
          state.hwStates.set(dev.id, res.state);
          if (res.state.kind === "keyboard") await syncScreenImagesFromState(res.state);
        }
        const refreshed = currentDevice();
        if (res.state?.linkState === "online") {
          setSyncText(res.state.connectionMode === "2.4g" ? "2.4G 已唤醒" : "硬件已连接", "is-live");
          toast("连接已就绪", `${refreshed?.name || dev.name} 已通过 ${refreshed?.connectionLabel || "HID"} 在线`);
        } else if (res.state?.linkState === "bt-readonly") {
          setSyncText("蓝牙 BLE 已连接", "is-live");
          toast("当前仍为蓝牙连接", "未检测到在线的 2.4G 或 USB 有线链路");
        } else {
          setSyncText("2.4G 仍处于休眠", "is-mock");
          toast("设备尚未唤醒", "请晃动鼠标或轻按键盘任意键后再试", true);
        }
      } catch (err) {
        setSyncText("探测失败");
        toast("唤醒探测失败", err.message, true);
      } finally {
        state.busy = false;
        renderAll();
      }
      return;
    }
    if (el.dataset.switchLink) {
      const dev = currentDevice();
      if (!dev) return;
      const targetLinkId = el.dataset.switchLink;
      state.busy = true;
      setSyncText("切换链路中...", "is-busy");
      renderDeviceRail();
      try {
        const res = await callApi("switchLink", dev.id, targetLinkId);
        if (res.devices) state.devices = res.devices;
        if (res.state) {
          state.hwStates.set(dev.id, res.state);
          if (res.state.kind === "keyboard") await syncScreenImagesFromState(res.state);
        }
        const refreshed = currentDevice();
        setSyncText(refreshed?.linkState === "online" ? `${refreshed.connectionShortLabel} 已切换` : "链路已更新", refreshed?.linkState === "online" ? "is-live" : "is-mock");
        toast("链路已切换", `${refreshed?.model || dev.model} → ${refreshed?.connectionLabel || targetLinkId}`);
      } catch (err) {
        setSyncText("切换失败");
        toast("链路切换失败", err.message, true);
      } finally {
        state.busy = false;
        renderAll();
      }
      return;
    }
    if (el.id === "pick-screen-file-btn" || el.id === "gallery-add-btn") {
      await triggerPickLocalImage();
      return;
    }
    if (el.id === "paste-screen-btn") {
      await triggerPasteClipboardImage();
      return;
    }
    if (el.id === "export-screen-btn") {
      const canvas = paintScreenCanvas();
      const a = document.createElement("a");
      a.href = canvas.toDataURL("image/png");
      a.download = "redmagic-screen-320x172.png";
      a.click();
      toast("已导出 320×172 画面", "redmagic-screen-320x172.png");
      return;
    }
    if (el.dataset.screenLibDel) {
      e.stopPropagation();
      const dev = currentDevice();
      if (!dev) return;
      const uuid = el.dataset.screenLibDel;
      try {
        const res = await callApi("deleteScreenLibraryItem", dev.id, uuid);
        const hw = state.hwStates.get(dev.id);
        if (hw && res.screenImage) {
          hw.screenImage = res.screenImage;
          if (state.ui.screenSelectedUuid === uuid) state.ui.screenSelectedUuid = null;
          await syncScreenImagesFromState(hw);
        }
        renderWorkspace();
        toast("已从图库移除", "本地壁纸列表已更新");
      } catch (err) {
        toast("删除失败", err.message, true);
      }
      return;
    }
    if (el.dataset.screenLibUuid) {
      state.ui.screenSourceKind = "library";
      state.ui.screenSelectedUuid = el.dataset.screenLibUuid;
      state.ui.screenCustomImg = null;
      state.ui.screenCustomName = "";
      resetScreenView();
      renderWorkspace();
      return;
    }
    if (el.dataset.screenPreset) {
      state.ui.screenSourceKind = "preset";
      state.ui.screenPreset = el.dataset.screenPreset;
      state.ui.screenCustomImg = null;
      state.ui.screenCustomName = "";
      resetScreenView();
      renderWorkspace();
      return;
    }
    if (el.dataset.screenFit) {
      state.ui.screenFit = el.dataset.screenFit;
      state.ui.screenPanX = 0;
      state.ui.screenPanY = 0;
      renderWorkspace();
      return;
    }
    if (el.dataset.screenBg) {
      state.ui.screenBgColor = el.dataset.screenBg;
      renderWorkspace();
      return;
    }
    if (el.id === "screen-rot-ccw") {
      state.ui.screenRotate = ((state.ui.screenRotate || 0) + 270) % 360;
      renderWorkspace();
      return;
    }
    if (el.id === "screen-rot-cw") {
      state.ui.screenRotate = ((state.ui.screenRotate || 0) + 90) % 360;
      renderWorkspace();
      return;
    }
    if (el.id === "screen-flip-h") {
      state.ui.screenFlipH = !state.ui.screenFlipH;
      renderWorkspace();
      return;
    }
    if (el.id === "screen-reset-view") {
      resetScreenView();
      renderWorkspace();
      return;
    }
    if (el.id === "screen-text-clear") {
      state.ui.screenText = "";
      renderWorkspace();
      return;
    }
    if (el.id === "save-screen-lib-btn") {
      const dev = currentDevice();
      if (!dev || dev.kind !== "keyboard") return;
      const hw = state.hwStates.get(dev.id);
      const canvas = paintScreenCanvas();
      const b64 = encodeCanvasToRgb565Base64(canvas);
      const name = currentScreenDraftName();
      try {
        const res = await callApi("saveScreenToLibrary", dev.id, b64, name);
        if (hw && res.screenImage) {
          hw.screenImage = res.screenImage;
          state.ui.screenCustomImg = null;
          state.ui.screenCustomName = "";
          resetScreenView();
          await syncScreenImagesFromState(hw, { preferUuid: res.uuid });
        }
        renderWorkspace();
        toast("已存入本机图库", "随时可一键切换并写入键盘小屏");
      } catch (err) {
        toast("保存图库失败", err.message, true);
      }
      return;
    }
    if (el.id === "upload-screen-btn") {
      const dev = currentDevice();
      if (!dev || dev.kind !== "keyboard" || state.ui.screenUploading) return;
      const hw = state.hwStates.get(dev.id);
      const canvas = paintScreenCanvas();
      const b64 = encodeCanvasToRgb565Base64(canvas);
      const untouchedLib =
        state.ui.screenSourceKind === "library" &&
        state.ui.screenSelectedUuid &&
        !isScreenViewEdited() &&
        (state.ui.screenFit || "cover") === "cover" &&
        !(state.ui.screenText || "").trim();
      const opts = {
        name: currentScreenDraftName(),
        reuseUuid: untouchedLib ? state.ui.screenSelectedUuid : null,
      };
      state.ui.screenUploading = true;
      state.ui.screenUploadPct = 0;
      state.busy = true;
      setSyncText("写入小屏...", "is-busy");
      renderWorkspace();
      try {
        const res = await callApi("uploadScreenImage", dev.id, b64, opts);
        if (hw && res.config) hw.config = res.config;
        if (hw && res.screenImage) {
          hw.screenImage = res.screenImage;
          state.ui.screenCustomImg = null;
          state.ui.screenCustomName = "";
          resetScreenView();
          await syncScreenImagesFromState(hw, { preferUuid: res.uuid });
        }
        setSyncText(res.mock ? "仿真小屏已写入" : "小屏已写入", res.mock ? "is-mock" : "is-live");
        toast("小屏图片写入完成", "已写入主扇区 0xCE0000 并同步至本机图库");
      } catch (err) {
        setSyncText("小屏写入失败");
        toast("小屏写入失败", err.message, true);
      } finally {
        state.ui.screenUploading = false;
        state.busy = false;
        renderAll();
      }
      return;
    }

    /* ---- 灯效编程工作室按钮事件 ---- */
    const st = state.ui.studio;
    if (el.dataset.studioSub) {
      const nextSub = el.dataset.studioSub;
      if (nextSub === st.subMode) return;
      st.subMode = nextSub;
      // 推流运行期间在脚本/多图层间切换时无缝热切换推流源，不断流
      if (st.streaming && (nextSub === "script" || nextSub === "layers")) {
        st.streamSource = nextSub;
        if (st.nativeStreaming) {
          if (isNativeEligible(st)) scheduleNativeConfigSync();
          else await handoffCustomJsIfNeeded();
        }
      }
      await syncStudioSensors();
      renderDeviceRail();
      renderWorkspace();
      return;
    }
    if (el.dataset.paintGroup !== undefined) {
      const g = Number(el.dataset.paintGroup);
      st.paintGroup = g;
      await loadPaintGroupFromHw(g, { forceOverwrite: true, silent: false });
      renderWorkspace();
      return;
    }
    if (el.dataset.paintTool) {
      st.paintTool = el.dataset.paintTool;
      renderWorkspace();
      return;
    }
    if (el.dataset.paintSwatch) {
      st.paintColor = el.dataset.paintSwatch;
      if (st.paintTool === "eraser") st.paintTool = "brush";
      renderWorkspace();
      return;
    }
    if (el.dataset.paintTpl) {
      const names = { redmagic: "红魔战甲", fps: "FPS WASD", moba: "MOBA QWER", cyber: "赛博渐变", white: "纯白极简", clear: "全黑清空" };
      applyPaintTemplate(st.paintRgb, el.dataset.paintTpl);
      toast("已套用逐键模板", names[el.dataset.paintTpl] || el.dataset.paintTpl);
      return;
    }
    if (el.id === "studio-paint-read-btn") {
      await loadPaintGroupFromHw(st.paintGroup, { forceOverwrite: true, silent: false });
      return;
    }
    if (el.id === "studio-paint-write-btn") {
      const dev = currentDevice();
      if (!dev || dev.kind !== "keyboard") return;
      if (st.streaming || st.serverStatus?.running) {
        await stopStudioStreamingIfNeeded(true);
      }
      state.busy = true;
      setSyncText("写入逐键配色...", "is-busy");
      renderDeviceRail();
      try {
        const res = await callApi("writeCustomRgb", dev.id, st.paintGroup, Array.from(st.paintRgb), true);
        const hw = state.hwStates.get(dev.id);
        if (hw && res.config) hw.config = res.config;
        setSyncText(res.mock ? "仿真逐键已写入" : "板载逐键已写入", res.mock ? "is-mock" : "is-live");
        toast("板载逐键配色已写入并启用", `OP 0x0b 组 ${st.paintGroup + 1} · 断电/切无线不丢失`);
      } catch (err) {
        setSyncText("写入失败");
        toast("写入逐键配色失败", err.message, true);
      } finally {
        state.busy = false;
        renderDeviceRail();
      }
      return;
    }
    if (el.dataset.studioScript) {
      const preset = STUDIO_SCRIPTS.find((s) => s.id === el.dataset.studioScript);
      if (!preset) return;
      st.scriptId = preset.id;
      st.scriptCode = preset.code;
      st.scriptError = "";
      st.p1 = preset.p1;
      st.p2 = preset.p2;
      st.p3 = preset.p3;
      st.scriptSensors = { ...preset.sensors };
      await syncStudioSensors();
      if (st.streaming) {
        if (st.nativeStreaming) {
          scheduleNativeConfigSync();
        } else if (isNativeEligible(st)) {
          // 从自定义 JS 切回内置预设时，自动切回纯 Rust 极低占用引擎
          const kb = keyboardDevice();
          if (kb) {
            try {
              await callApi("stopDirectRgb", kb.id);
              await callApi("startNativeRgb", kb.id, buildNativeConfig(st));
              st.nativeStreaming = true;
            } catch (_) {}
          }
        }
      }
      renderDeviceRail();
      renderWorkspace();
      return;
    }
    if (el.id === "studio-export-btn") {
      const payload = {
        kind: "redmagic-rgb-preset",
        version: 1,
        scriptId: st.scriptId,
        scriptCode: st.scriptCode,
        p1: st.p1,
        p2: st.p2,
        p3: st.p3,
        scriptSensors: st.scriptSensors,
        layers: st.layers,
        paintRgb: Array.from(st.paintRgb),
      };
      try {
        const res = await callApi("exportStudioPreset", `redmagic-${st.scriptId || "custom"}.json`, JSON.stringify(payload, null, 2));
        if (res && !res.canceled) {
          toast("灯效方案已导出", res.filePath || "");
        }
      } catch (err) {
        toast("导出方案失败", err.message, true);
      }
      return;
    }
    if (el.id === "studio-import-btn") {
      try {
        const res = await callApi("importStudioPreset");
        if (!res || res.canceled || !res.content) return;
        const text = String(res.content);
        if (String(res.name || "").toLowerCase().endsWith(".js")) {
          st.scriptId = "custom";
          st.scriptCode = text;
          st.scriptError = "";
          st.subMode = "script";
        } else {
          const parsed = JSON.parse(text);
          if (typeof parsed.scriptCode === "string") {
            st.scriptId = parsed.scriptId || "custom";
            st.scriptCode = parsed.scriptCode;
            st.scriptError = "";
          }
          if (typeof parsed.p1 === "number") st.p1 = parsed.p1;
          if (typeof parsed.p2 === "number") st.p2 = parsed.p2;
          if (typeof parsed.p3 === "number") st.p3 = parsed.p3;
          if (parsed.scriptSensors) st.scriptSensors = { ...st.scriptSensors, ...parsed.scriptSensors };
          if (parsed.layers) st.layers = { ...st.layers, ...parsed.layers };
          if (Array.isArray(parsed.paintRgb) && parsed.paintRgb.length >= 378) {
            for (let i = 0; i < 378; i++) st.paintRgb[i] = parsed.paintRgb[i] & 0xff;
          }
        }
        await syncStudioSensors();
        renderWorkspace();
        toast("灯效方案已导入", res.name || "");
      } catch (err) {
        toast("导入方案失败", err.message, true);
      }
      return;
    }
    if (el.id === "studio-tray-btn") {
      await hideWindowToTray();
      return;
    }
    if (el.id === "studio-stream-btn") {
      const dev = currentDevice();
      if (!dev || dev.kind !== "keyboard") return;
      if (st.streaming) {
        await stopStudioStreamingIfNeeded(true);
        setSyncText("已恢复板载灯效", dev.mock ? "is-mock" : "is-live");
        toast("已停止实时推流", "键盘已自动还原原有板载灯效");
        renderAll();
      } else {
        try {
          if (st.serverStatus?.running) {
            st.serverStatus = await callApi("stopExternalRgbServer");
          }
          st.streamSource = st.subMode === "layers" ? "layers" : "script";
          if (isNativeEligible(st) && window.rm?.startNativeRgb) {
            await callApi("startNativeRgb", dev.id, buildNativeConfig(st));
            st.streaming = true;
            st.nativeStreaming = true;
            st.streamFps = 0;
            studioCtrl.ensureBackgroundLoop(getStudioBgState, updateStudioLiveStats);
            setSyncText("纯 Rust 推流中", "is-live");
            toast("已开启纯 Rust 零损耗推流", "挂起至托盘后自动释放浏览器进程（<0.3% CPU / ~15MB 内存）");
          } else {
            await callApi("startDirectRgb", dev.id, st.includeLogo);
            st.streaming = true;
            st.nativeStreaming = false;
            st.streamFps = 0;
            await syncStudioSensors();
            studioCtrl.ensureBackgroundLoop(getStudioBgState, updateStudioLiveStats);
            setSyncText("JS 脚本推流中", "is-live");
            toast("已开启自定义 JS 实时推流", "挂起至托盘后自动关闭 GPU 合成（0% GPU 占用）");
          }
          renderAll();
        } catch (err) {
          toast("开启实时推流失败", err.message, true);
        }
      }
      return;
    }
    if (el.id === "studio-server-toggle-btn") {
      const dev = currentDevice();
      if (!dev || dev.kind !== "keyboard") return;
      try {
        if (st.serverStatus?.running) {
          st.serverStatus = await callApi("stopExternalRgbServer");
          studioCtrl.stopBackgroundLoop();
          setSyncText("已停止外部接口", dev.mock ? "is-mock" : "is-live");
          toast("外部控灯桥已停止", "键盘已恢复原板载灯效");
        } else {
          if (st.streaming) {
            st.streaming = false;
            st.nativeStreaming = false;
            st.streamFps = 0;
          }
          st.serverStatus = await callApi("startExternalRgbServer", dev.id, st.udpPort, st.httpPort, st.includeLogo);
          studioCtrl.ensureBackgroundLoop(getStudioBgState, updateStudioLiveStats);
          setSyncText("外部控灯桥运行中", "is-live");
          toast("本地回环控灯桥已启动", `UDP 127.0.0.1:${st.udpPort} · 挂起至托盘零浏览器占用`);
        }
        renderAll();
      } catch (err) {
        toast("控灯服务操作失败", err.message, true);
      }
      return;
    }
    if (el.id === "studio-copy-api-btn") {
      const sample = buildExternalExamplePython(st.udpPort, st.httpPort);
      navigator.clipboard?.writeText(sample);
      toast("已复制 Python 控灯示例代码", `UDP ${st.udpPort} / HTTP+WS ${st.httpPort}`);
      return;
    }
  });

  // 滑块拖动即时更新标签数值，小屏缩放/底色/文字输入即时重绘预览，灯效编程热更新
  ws.addEventListener("input", (e) => {
    const el = e.target;
    const st = state.ui.studio;
    if (el.id === "studio-code-editor") {
      st.scriptCode = el.value;
      if (st.streaming && st.nativeStreaming && !isNativeEligible(st)) {
        handoffCustomJsIfNeeded();
      }
      return;
    }
    if (el.dataset.studioParam) {
      const p = el.dataset.studioParam;
      st[p] = Number(el.value) / 100;
      const valEl = $(`#studio-${p}-val`);
      if (valEl) valEl.textContent = `${el.value}%`;
      scheduleNativeConfigSync();
      return;
    }
    if (el.id === "studio-bri-range") {
      st.brightness = Number(el.value) / 100;
      const valEl = $("#studio-bri-val");
      if (valEl) valEl.textContent = `${el.value}%`;
      scheduleNativeConfigSync();
      return;
    }
    if (el.id === "studio-paint-picker") {
      st.paintColor = el.value;
      if (st.paintTool === "eraser") st.paintTool = "brush";
      return;
    }
    if (el.dataset.layerRange) {
      const [lk, fk] = el.dataset.layerRange.split(".");
      if (st.layers[lk]) st.layers[lk][fk] = Number(el.value) / 100;
      scheduleNativeConfigSync();
      return;
    }
    if (el.dataset.layerField) {
      const [lk, fk] = el.dataset.layerField.split(".");
      if (st.layers[lk]) st.layers[lk][fk] = el.value;
      scheduleNativeConfigSync();
      return;
    }
    if (el.id === "studio-udp-port") {
      const v = Number(el.value);
      if (v >= 1024 && v <= 65535) st.udpPort = v;
      return;
    }
    if (el.id === "studio-http-port") {
      const v = Number(el.value);
      if (v >= 1024 && v <= 65535) st.httpPort = v;
      return;
    }
    if (el.id === "screen-zoom-range") {
      state.ui.screenZoom = Number(el.value) / 100;
      const valEl = $("#screen-zoom-val");
      if (valEl) valEl.textContent = `${el.value}%`;
      const rst = $("#screen-reset-view");
      if (rst) rst.disabled = !isScreenViewEdited();
      paintScreenCanvas();
      return;
    }
    if (el.id === "screen-bg-picker") {
      state.ui.screenBgColor = el.value;
      paintScreenCanvas();
      return;
    }
    if (el.type === "range") {
      const readout = el.previousElementSibling?.querySelector(".mono");
      if (readout) readout.textContent = `${el.value} / ${el.max}`;
    } else if (el.id === "screen-text-input") {
      const hadText = Boolean(state.ui.screenText);
      state.ui.screenText = el.value;
      paintScreenCanvas();
      if (hadText !== Boolean(el.value)) {
        const pos = el.selectionStart ?? el.value.length;
        renderWorkspace();
        const nextInput = $("#screen-text-input");
        if (nextInput) { nextInput.focus(); nextInput.setSelectionRange(pos, pos); }
      }
    }
  });

  // 逐键涂色拖拽绘制 + 交互式 320×172 画布鼠标拖拽平移构图
  let panDrag = null;
  let paintDrag = null;

  function applyPaintAtSlot(slot, mode) {
    if (!Number.isInteger(slot) || slot < 0 || slot >= 126) return;
    const st = state.ui.studio;
    const o = slot * 3;
    if (mode === "eyedropper") {
      const hex = rgbToHex(st.paintRgb[o], st.paintRgb[o + 1], st.paintRgb[o + 2]);
      st.paintColor = hex;
      st.paintTool = "brush";
      renderWorkspace();
      toast("已吸取按键颜色", hex.toUpperCase());
      return;
    }
    if (mode === "erase") {
      st.paintRgb[o] = 0;
      st.paintRgb[o + 1] = 0;
      st.paintRgb[o + 2] = 0;
    } else {
      const [r, g, b] = hexToRgb(st.paintColor);
      st.paintRgb[o] = r;
      st.paintRgb[o + 1] = g;
      st.paintRgb[o + 2] = b;
    }
  }

  ws.addEventListener("pointerdown", (e) => {
    const paintKey = e.target.closest(".kb-paint-hit");
    if (paintKey && (e.button === 0 || e.button === 2)) {
      e.preventDefault();
      const slot = Number(paintKey.dataset.paintSlot);
      const st = state.ui.studio;
      const mode = e.button === 2 ? "erase" : st.paintTool === "eyedropper" ? "eyedropper" : st.paintTool === "eraser" ? "erase" : "paint";
      applyPaintAtSlot(slot, mode);
      if (mode !== "eyedropper") {
        paintDrag = { pointerId: e.pointerId, mode, lastSlot: slot };
      }
      return;
    }

    const box = e.target.closest("#screen-drop-zone");
    if (!box || e.button !== 0 || !canTransformCurrentScreen()) return;
    e.preventDefault();
    const rect = box.getBoundingClientRect();
    panDrag = {
      pointerId: e.pointerId,
      box,
      startX: e.clientX,
      startY: e.clientY,
      basePanX: state.ui.screenPanX || 0,
      basePanY: state.ui.screenPanY || 0,
      scaleX: 320 / Math.max(1, rect.width),
      scaleY: 172 / Math.max(1, rect.height),
    };
    box.classList.add("is-dragging");
    box.setPointerCapture?.(e.pointerId);
  });

  ws.addEventListener("pointermove", (e) => {
    if (paintDrag && e.pointerId === paintDrag.pointerId) {
      const hit = document.elementFromPoint(e.clientX, e.clientY)?.closest?.(".kb-paint-hit");
      if (hit) {
        const slot = Number(hit.dataset.paintSlot);
        if (slot !== paintDrag.lastSlot) {
          paintDrag.lastSlot = slot;
          applyPaintAtSlot(slot, paintDrag.mode);
        }
      }
      return;
    }
    if (!panDrag || e.pointerId !== panDrag.pointerId) return;
    state.ui.screenPanX = Math.max(-320, Math.min(320, panDrag.basePanX + (e.clientX - panDrag.startX) * panDrag.scaleX));
    state.ui.screenPanY = Math.max(-220, Math.min(220, panDrag.basePanY + (e.clientY - panDrag.startY) * panDrag.scaleY));
    const rst = $("#screen-reset-view");
    if (rst) rst.disabled = !isScreenViewEdited();
    paintScreenCanvas();
  });

  const endPan = (e) => {
    if (paintDrag && (!e || e.pointerId === paintDrag.pointerId)) {
      paintDrag = null;
    }
    if (!panDrag || (e && e.pointerId !== panDrag.pointerId)) return;
    panDrag.box.classList.remove("is-dragging");
    try { panDrag.box.releasePointerCapture?.(panDrag.pointerId); } catch (_) {}
    panDrag = null;
  };
  window.addEventListener("pointerup", endPan);
  window.addEventListener("pointercancel", endPan);

  // 画布鼠标滚轮缩放 (50% ~ 300%)
  ws.addEventListener("wheel", (e) => {
    const box = e.target.closest("#screen-drop-zone");
    if (!box || !canTransformCurrentScreen()) return;
    e.preventDefault();
    const curPct = Math.round((state.ui.screenZoom ?? 1) * 100);
    const nextPct = Math.max(50, Math.min(300, curPct + (e.deltaY < 0 ? 10 : -10)));
    if (nextPct === curPct) return;
    state.ui.screenZoom = nextPct / 100;
    const rangeEl = $("#screen-zoom-range");
    const valEl = $("#screen-zoom-val");
    const rst = $("#screen-reset-view");
    if (rangeEl) rangeEl.value = String(nextPct);
    if (valEl) valEl.textContent = `${nextPct}%`;
    if (rst) rst.disabled = !isScreenViewEdited();
    paintScreenCanvas();
  }, { passive: false });

  // 键盘回车/空格激活图库缩略卡 + 脚本编辑器 Tab 缩进支持
  ws.addEventListener("keydown", (e) => {
    if (e.target.id === "studio-code-editor" && e.key === "Tab") {
      e.preventDefault();
      const ta = e.target;
      const start = ta.selectionStart;
      const end = ta.selectionEnd;
      ta.value = `${ta.value.slice(0, start)}  ${ta.value.slice(end)}`;
      ta.selectionStart = ta.selectionEnd = start + 2;
      state.ui.studio.scriptCode = ta.value;
      return;
    }
    if (e.key !== "Enter" && e.key !== " ") return;
    const card = e.target.closest("[data-screen-lib-uuid]");
    if (!card || e.target.closest("button")) return;
    e.preventDefault();
    card.click();
  });

  // 拖放图片文件载入（防止 Electron 默认打开文件）
  const stopDrag = (e) => {
    if (e.dataTransfer?.types?.includes("Files")) {
      e.preventDefault();
      e.stopPropagation();
    }
  };
  window.addEventListener("dragover", stopDrag);
  window.addEventListener("drop", (e) => {
    if (e.dataTransfer?.files?.length) {
      e.preventDefault();
      if (state.tab === "screen") {
        $("#screen-drop-zone")?.classList.remove("is-dragover");
        $("#screen-gallery-drop")?.classList.remove("is-dragover");
        loadCustomImageFile(e.dataTransfer.files[0]);
      }
    }
  });
  ws.addEventListener("dragover", (e) => {
    const zone = e.target.closest("#screen-drop-zone, #screen-gallery-drop");
    if (!zone || !e.dataTransfer?.types?.includes("Files")) return;
    e.preventDefault();
    zone.classList.add("is-dragover");
  });
  ws.addEventListener("dragleave", (e) => {
    const zone = e.target.closest("#screen-drop-zone, #screen-gallery-drop");
    if (zone && !zone.contains(e.relatedTarget)) zone.classList.remove("is-dragover");
  });

  // 工作区委托事件（change：开关、滑块松手、颜色选择器、下拉、数值输入）
  ws.addEventListener("change", async (e) => {
    const el = e.target;
    const st = state.ui.studio;
    if (el.id === "studio-logo-sync") {
      st.includeLogo = el.checked;
      if (st.streaming) {
        if (st.nativeStreaming) {
          scheduleNativeConfigSync();
        } else {
          const dev = currentDevice();
          if (dev && dev.kind === "keyboard") {
            try {
              await callApi("stopDirectRgb", dev.id);
              await callApi("startDirectRgb", dev.id, st.includeLogo);
            } catch (_) {}
          }
        }
      }
      return;
    }
    if (el.dataset.studioSensor) {
      st.scriptSensors[el.dataset.studioSensor] = el.checked;
      await syncStudioSensors();
      renderWorkspace();
      return;
    }
    if (el.dataset.layerToggle) {
      const lk = el.dataset.layerToggle;
      if (st.layers[lk]) st.layers[lk].enabled = el.checked;
      scheduleNativeConfigSync();
      await syncStudioSensors();
      renderWorkspace();
      return;
    }
    if (el.id === "studio-paint-picker") {
      st.paintColor = el.value;
      renderWorkspace();
      return;
    }
    if (el.id === "studio-udp-port" || el.id === "studio-http-port") {
      const clamped = Math.max(1024, Math.min(65535, Number(el.value) || (el.id === "studio-udp-port" ? 27650 : 27651)));
      if (el.id === "studio-udp-port") st.udpPort = clamped;
      else st.httpPort = clamped;
      renderWorkspace();
      return;
    }
    if (el.dataset.kbToggle) {
      await patchConfig(setNested({}, el.dataset.kbToggle, el.checked), "键盘开关已同步");
      return;
    }
    if (el.dataset.kbRange) {
      await patchConfig(setNested({}, el.dataset.kbRange, Number(el.value)), "键盘灯光参数已写入");
      return;
    }
    if (el.dataset.kbColorPicker) {
      const p = el.dataset.kbColorPicker;
      const patch = p === "logo" ? { logo: { color: el.value, sevenColor: false } } : { light: { color: el.value, multiColor: false } };
      await patchConfig(patch, `颜色已设为 ${el.value}`);
      return;
    }
    if (el.dataset.msToggle) {
      await patchConfig(setNested({}, el.dataset.msToggle, el.checked), "鼠标设置已同步");
      return;
    }
    if (el.dataset.msRange) {
      await patchConfig(setNested({}, el.dataset.msRange, Number(el.value)), "鼠标灯光参数已写入");
      return;
    }
    if (el.dataset.msColorPicker !== undefined) {
      await patchConfig({ light: { color: el.value, multiColor: false } }, `鼠标灯色已设为 ${el.value}`);
      return;
    }
    if (el.dataset.msSelect === "performance.polling") {
      const v = Number(el.value);
      await patchConfig({ performance: { pollingWired: v, polling24g: v } }, "鼠标回报率已写入");
      return;
    }
    if (el.dataset.msSleep !== undefined) {
      await patchConfig({ performance: { sleepSec: Number(el.value) } }, `休眠时间已设为 ${el.value} 秒`);
      return;
    }
    if (el.dataset.msDpiVal !== undefined) {
      const idx = Number(el.dataset.msDpiVal);
      const val = Math.max(50, Math.min(26000, Number(el.value) || 800));
      await patchConfig({ dpiStages: [{ index: idx, dpiX: val, dpiY: val }] }, `第 ${idx + 1} 档 DPI 已设为 ${val}`);
      return;
    }
    if (el.dataset.msDpiColor !== undefined) {
      const idx = Number(el.dataset.msDpiColor);
      await patchConfig({ dpiStages: [{ index: idx, color: el.value }] }, `第 ${idx + 1} 档指示色已更新`);
      return;
    }
    if (el.dataset.msDpiEn !== undefined) {
      const idx = Number(el.dataset.msDpiEn);
      await patchConfig({ dpiStages: [{ index: idx, enabled: el.checked }] }, `第 ${idx + 1} 档已${el.checked ? "启用" : "停用"}`);
      return;
    }
    if (el.id === "key-preset-select") {
      state.ui.pendingHex = el.value;
      return;
    }
    if (el.id === "screen-bg-picker") {
      state.ui.screenBgColor = el.value;
      renderWorkspace();
      return;
    }
    if (el.id === "screen-file-input" && el.files?.[0]) {
      loadCustomImageFile(el.files[0]);
      el.value = "";
    }
  });

  // 禁用浏览器默认右键调试菜单（保留原生桌面软件体验）
  document.addEventListener("contextmenu", (e) => {
    e.preventDefault();
  }, { capture: true });

  // 键盘物理录入目标键 + 屏蔽浏览器调试/刷新快捷键 + 小屏页面 Ctrl+V 快捷粘贴图片
  window.addEventListener("keydown", (e) => {
    if (state.ui.capturingKey) {
      e.preventDefault();
      e.stopPropagation();
      const hex = CODE_TO_HEX[e.code];
      state.ui.capturingKey = false;
      if (hex) {
        state.ui.pendingHex = hex;
        toast("已录入按键", `${e.code} (0x${hex})`);
      } else {
        toast("未识别该按键", e.code, true);
      }
      renderWorkspace();
      return;
    }

    // 屏蔽浏览器默认快捷键（F3/F5/F12、Ctrl+R/P/S/U/F、Ctrl+Shift+I/J/C），不影响输入框复制粘贴
    const key = e.key.toLowerCase();
    const ctrlOrMeta = e.ctrlKey || e.metaKey;
    if (
      e.key === "F5" || e.key === "F3" || e.key === "F12" ||
      (ctrlOrMeta && !e.altKey && ["r", "p", "s", "u", "f"].includes(key)) ||
      (ctrlOrMeta && e.shiftKey && ["i", "j", "c"].includes(key))
    ) {
      e.preventDefault();
      return;
    }

    if (
      state.tab === "screen" &&
      ctrlOrMeta &&
      key === "v" &&
      !["INPUT", "TEXTAREA"].includes(document.activeElement?.tagName)
    ) {
      e.preventDefault();
      triggerPasteClipboardImage();
    }
  });
}

async function boot() {
  renderChrome();
  bindEvents();
  paintScreenCanvas();
  window.rm?.onScreenProgress?.((prog) => {
    state.ui.screenUploadPct = prog.pct;
    const bar = $("#screen-upload-bar");
    const txt = $("#screen-upload-pct");
    if (bar) bar.style.width = `${prog.pct}%`;
    if (txt) txt.textContent = `${prog.pct}%`;
  });
  window.rm?.onDeviceEvent?.(async (evt) => {
    if (!evt || state.ui.screenUploading) return;
    if (evt.type === "telemetry" && Array.isArray(evt.updates)) {
      let activeChanged = false;
      for (const u of evt.updates) {
        const dev = state.devices.find((d) => d.id === u.id);
        if (dev) {
          if (u.battery != null) dev.battery = u.battery;
          if (u.chargingState != null) dev.chargingState = u.chargingState;
        }
        const hw = state.hwStates.get(u.id);
        if (hw) {
          if (u.battery != null) hw.battery = u.battery;
          if (u.chargingState != null) hw.chargingState = u.chargingState;
          if (u.id === state.activeId) activeChanged = true;
        }
      }
      renderDeviceRail();
      // 仅在非小屏/非灯效编程页刷新 KPI，避免打断取景裁切、脚本输入或拖拽涂色
      if (activeChanged && state.tab !== "screen" && state.tab !== "studio") renderWorkspace();
      return;
    }
    if (evt.type === "topology" && Array.isArray(evt.devices)) {
      const prevById = new Map(state.devices.map((d) => [d.id, d]));
      state.devices = evt.devices;
      if (!state.devices.some((d) => d.id === state.activeId)) {
        state.activeId = state.devices[0]?.id || null;
      }
      for (const d of state.devices) {
        const prev = prevById.get(d.id);
        if (prev && (prev.connectionMode !== d.connectionMode || prev.linkState !== d.linkState)) {
          const msg = d.linkState === "sleeping"
            ? `${d.model} 已进入 2.4G 休眠`
            : `${d.model} 已切换至 ${d.connectionLabel}`;
          toast("连接状态变更", msg);
          state.hwStates.delete(d.id);
        } else if (!prev && !d.mock) {
          toast("发现新外设", `${d.name} (${d.connectionLabel})`);
        }
      }
      renderAll();
      if (state.activeId && !state.hwStates.has(state.activeId)) {
        await loadDeviceState(state.activeId);
      }
      for (const d of state.devices) {
        if (d.id !== state.activeId && !state.hwStates.has(d.id)) {
          callApi("readState", d.id).then((st) => {
            state.hwStates.set(d.id, st);
            renderDeviceRail();
          }).catch(() => {});
        }
      }
    }
  });
  // 窗口最小化 / 托盘隐藏时立即冻结所有前台 rAF 与 CSS 毛玻璃合成，使 WebView2 GPU 占用归零
  const suspendForegroundVisuals = () => {
    document.body.classList.add("is-tray-hidden");
    lighting.stop();
    studioCtrl.detach();
  };
  const resumeForegroundVisuals = () => {
    if (!document.body.classList.contains("is-tray-hidden")) return;
    document.body.classList.remove("is-tray-hidden");
    renderWorkspace();
  };
  document.addEventListener("visibilitychange", () => {
    if (document.hidden) suspendForegroundVisuals();
    else resumeForegroundVisuals();
  });
  window.rm?.onTrayHidden?.(suspendForegroundVisuals);
  window.rm?.onTrayShown?.(resumeForegroundVisuals);

  window.rm?.onStudioStopped?.(() => {
    const st = state.ui.studio;
    st.streaming = false;
    st.nativeStreaming = false;
    st.streamFps = 0;
    if (st.serverStatus) st.serverStatus.running = false;
    st.telemetry.audio.active = false;
    st.telemetry.ambilight.active = false;
    st.telemetry.keys.active = false;
    studioCtrl.stopBackgroundLoop();
    const dev = currentDevice();
    if (dev) setSyncText("已恢复板载灯效", dev.mock ? "is-mock" : "is-live");
    renderAll();
    toast("已从托盘停止推流", "键盘已还原原有板载灯效");
  });
  try {
    state.meta = await callApi("getMeta");
    $("#app-version").textContent = `v${state.meta.version}`;
    state.devices = await callApi("listDevices");
    const params = new URLSearchParams(window.location.search);
    const initDev = params.get("device");
    const initTab = params.get("tab");
    const initSub = params.get("sub");
    const initScript = params.get("script");
    const initStream = params.get("streaming") === "1";
    if (initTab && TABS.some((t) => t.id === initTab)) state.tab = initTab;
    if (initSub && ["paint", "script", "layers", "api"].includes(initSub)) {
      state.ui.studio.subMode = initSub;
    }
    if (initScript) {
      const found = STUDIO_SCRIPTS.find((s) => s.id === initScript);
      if (found) {
        state.ui.studio.scriptId = found.id;
        state.ui.studio.scriptCode = found.code;
        state.ui.studio.p1 = found.p1;
        state.ui.studio.p2 = found.p2;
        state.ui.studio.p3 = found.p3;
        state.ui.studio.scriptSensors = { ...found.sensors };
      }
    }
    state.activeId = (initDev && state.devices.some((d) => d.id === initDev))
      ? initDev
      : (state.devices[0]?.id || null);
    renderAll();
    if (state.activeId) await loadDeviceState(state.activeId);

    // 若从纯 Rust 托盘挂起态（WebView2 曾被彻底销毁）重新打开界面，无缝还原后台正在运行的推流/服务状态
    if (window.rm?.getStudioStatus) {
      try {
        const srvSt = await callApi("getStudioStatus");
        const st = state.ui.studio;
        if (srvSt?.native?.running && srvSt.native.config) {
          const cfg = srvSt.native.config;
          st.streaming = true;
          st.nativeStreaming = true;
          st.streamFps = srvSt.native.fps || 0;
          st.streamSource = cfg.mode === "layers" ? "layers" : "script";
          st.subMode = st.streamSource;
          st.p1 = cfg.p1 ?? st.p1;
          st.p2 = cfg.p2 ?? st.p2;
          st.p3 = cfg.p3 ?? st.p3;
          st.brightness = cfg.brightness ?? st.brightness;
          st.includeLogo = cfg.includeLogo ?? st.includeLogo;
          if (cfg.layers) st.layers = { ...st.layers, ...cfg.layers };
          const found = STUDIO_SCRIPTS.find((s) => s.id === cfg.presetId);
          if (found) {
            st.scriptId = found.id;
            st.scriptCode = found.code;
            st.scriptSensors = { ...found.sensors };
          }
          studioCtrl.ensureBackgroundLoop(getStudioBgState, updateStudioLiveStats);
          setSyncText("纯 Rust 推流中", "is-live");
        } else if (srvSt?.server?.running) {
          st.serverStatus = srvSt.server;
          studioCtrl.ensureBackgroundLoop(getStudioBgState, updateStudioLiveStats);
          setSyncText("外部控灯桥运行中", "is-live");
        }
      } catch (_) {}
    }

    if (initStream && !state.ui.studio.streaming) {
      const kb = keyboardDevice();
      if (kb) {
        try {
          state.ui.studio.streamSource = state.ui.studio.subMode === "layers" ? "layers" : "script";
          await callApi("startNativeRgb", kb.id, buildNativeConfig(state.ui.studio));
          state.ui.studio.streaming = true;
          state.ui.studio.nativeStreaming = true;
          studioCtrl.ensureBackgroundLoop(getStudioBgState, updateStudioLiveStats);
        } catch (_) {}
      }
    }
    if (state.tab === "studio" || state.ui.studio.streaming || state.ui.studio.serverStatus?.running) {
      if (!state.ui.studio.nativeStreaming) await syncStudioSensors();
      renderAll();
    }
    // 后台预拉其余在线设备电量，左侧栏立即可见
    for (const d of state.devices) {
      if (d.id !== state.activeId) {
        callApi("readState", d.id).then((st) => {
          state.hwStates.set(d.id, st);
          renderDeviceRail();
        }).catch(() => {});
      }
    }
  } catch (err) {
    toast("初始化失败", err.message, true);
  }
}

boot();
