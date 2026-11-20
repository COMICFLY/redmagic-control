/* 四个核心功能视图渲染：概览与性能 / 灯光 / 按键映射 / 硬件诊断。
 * 所有控件均对应真实硬件协议字段，零无意义占位开关。 */

import { icon } from "./icons.js";
import { renderConnectionBadge, renderKeyboardStage, renderMouseStage } from "./stage.js";
import { STUDIO_SCRIPTS, buildExternalExamplePython, formatFps } from "./studio.js";

const SWATCHES = [
  "#e53945", "#ff6b2c", "#f5c518", "#22c55e", "#14b8a6",
  "#0ea5e9", "#6366f1", "#a855f7", "#ec4899", "#ffffff",
];

export const KEY_PRESETS = [
  ["200029", "Esc"], ["20002b", "Tab"], ["200039", "Caps Lock"], ["20002c", "Space"],
  ["200028", "Enter"], ["20002a", "Backspace"], ["20004c", "Delete"], ["200049", "Insert"],
  ["20004a", "Home"], ["20004d", "End"], ["20004b", "Page Up"], ["20004e", "Page Down"],
  ["20003a", "F1"], ["20003b", "F2"], ["20003c", "F3"], ["20003d", "F4"],
  ["20003e", "F5"], ["20003f", "F6"], ["200040", "F7"], ["200041", "F8"],
  ["200042", "F9"], ["200043", "F10"], ["200044", "F11"], ["200045", "F12"],
  ["200100", "左 Ctrl"], ["200200", "左 Shift"], ["200400", "左 Alt"], ["200800", "左 Win"],
  ["201000", "右 Ctrl"], ["202000", "右 Shift"], ["204000", "右 Alt"],
  ..."ABCDEFGHIJKLMNOPQRSTUVWXYZ".split("").map((ch, i) => [`2000${(4 + i).toString(16).padStart(2, "0")}`, ch]),
  ..."1234567890".split("").map((ch, i) => [`2000${(0x1e + i).toString(16).padStart(2, "0")}`, ch]),
];

export const MOUSE_PRESETS = [
  ["100100", "鼠标左键"], ["100200", "鼠标右键"], ["100400", "鼠标中键"],
  ["100800", "鼠标后退"], ["101000", "鼠标前进"],
  ["130100", "DPI +"], ["130200", "DPI -"], ["130300", "DPI 循环"],
];

export const MEDIA_PRESETS = [
  ["3000e9", "音量加"], ["3000ea", "音量减"], ["3000e2", "静音"],
  ["3000cd", "播放 / 暂停"], ["3000b5", "下一曲"], ["3000b6", "上一曲"], ["3000b7", "停止"],
];

function esc(str) {
  return String(str ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

function toggleSwitch(id, checked, dataAttr) {
  return `<label class="switch"><input type="checkbox" id="${id}" ${dataAttr}${checked ? " checked" : ""} /><span class="switch-track"></span></label>`;
}

function chargingLabel(state) {
  if (state === 1) return "充电中";
  if (state === 2) return "已充满";
  if (state === 0) return "电池供电";
  return "--";
}

function linkStateLabel(state) {
  if (state === "online") return "在线";
  if (state === "sleeping") return "休眠 / 未切至该档";
  if (state === "bt-readonly") return "蓝牙已连接 (只读)";
  return "离线";
}

/* ---- 无线状态提示横幅（2.4G 休眠唤醒 / 蓝牙 BLE 只读说明） ---- */
export function renderConnectionBanner(device, hwState) {
  if (!hwState || hwState.mock) return "";
  if (hwState.linkState === "sleeping") {
    return `
      <div class="link-banner is-sleeping" role="status">
        <div class="link-banner-ico">${icon("moon", 16)}</div>
        <div class="link-banner-copy">
          <strong>2.4G 无线设备处于休眠或待机状态</strong>
          <span>已检测到 <span class="mono">0x${(device.pid || 0).toString(16).padStart(4, "0")}</span> 接收器，但设备未响应 <span class="mono">OP 0xaa</span> 握手。请晃动鼠标 / 按任意键唤醒，或确认底部开关位于 2.4G 档位。</span>
        </div>
        <button type="button" class="btn btn-primary btn-sm" id="wake-device-btn">${icon("refresh", 13)}唤醒并重连</button>
      </div>`;
  }
  if (hwState.linkState === "bt-readonly") {
    const mac = hwState.btAddress || device.btAddress;
    return `
      <div class="link-banner is-bt" role="status">
        <div class="link-banner-ico">${icon("bluetooth", 16)}</div>
        <div class="link-banner-copy">
          <strong>当前通过蓝牙 BLE 连接${mac ? ` · <span class="mono">${esc(mac)}</span>` : ""}</strong>
          <span>已自动通过系统 BLE 电池服务同步实时电量。受设备固件限制，蓝牙模式未开放 <span class="mono">0xFF1C</span> 参数写通道；修改灯效、改键、DPI 或小屏壁纸请切换至 2.4G 接收器或 USB 有线。</span>
        </div>
        <button type="button" class="btn btn-secondary btn-sm" id="wake-device-btn">${icon("refresh", 13)}检测 2.4G / 有线</button>
      </div>`;
  }
  return "";
}

/* ---- 页头紧凑状态胶囊（取代冗余大卡片，与页标题同行零纵向占位） ---- */
export function renderPageTelemetry(device, hwState, meta) {
  const modCount = (hwState?.keymap || []).filter((k) => k.modified).length;

  if (device.kind === "keyboard") {
    const cfg = hwState?.config;
    const poll = meta.kbPollingRates.find((p) => p.id === cfg?.system?.pollingRate)?.hz ?? 1000;
    const modeName = meta.kbLightModes.find((m) => m.id === cfg?.light?.mode)?.name ?? `模式 ${cfg?.light?.mode ?? 0}`;
    return `
      <div class="page-telemetry" aria-label="设备实时参数">
        <div class="telemetry-pill"><span class="telemetry-k">回报率</span><strong class="telemetry-v mono">${poll} Hz</strong></div>
        <div class="telemetry-pill"><span class="telemetry-k">背光</span><strong class="telemetry-v">${esc(modeName)} <span class="mono telemetry-sub">${cfg?.light?.brightness ?? 0}/4</span></strong></div>
        <div class="telemetry-pill"><span class="telemetry-k">改键</span><strong class="telemetry-v mono">${modCount}</strong></div>
        ${cfg?.system?.winLock ? `<div class="telemetry-pill is-accent"><span class="telemetry-k">Win</span><strong class="telemetry-v">已锁定</strong></div>` : ""}
      </div>`;
  }

  const cfg = hwState?.config;
  const activeStage = cfg?.dpiStages?.[cfg?.performance?.activeDpi ?? 0];
  const is24g = (hwState?.connectionMode || device.connectionMode) === "2.4g";
  const activePollId = is24g ? cfg?.performance?.polling24g : cfg?.performance?.pollingWired;
  const poll = meta.msPollingRates.find((p) => p.id === activePollId)?.hz ?? 1000;
  return `
    <div class="page-telemetry" aria-label="设备实时参数">
      <div class="telemetry-pill"><span class="dpi-mini-dot" style="background:${esc(activeStage?.color || "#e53945")};margin-right:0"></span><span class="telemetry-k">DPI</span><strong class="telemetry-v mono">${activeStage?.dpiX ?? 1600}</strong></div>
      <div class="telemetry-pill"><span class="telemetry-k">回报率</span><strong class="telemetry-v mono">${poll} Hz</strong></div>
      <div class="telemetry-pill"><span class="telemetry-k">LOD</span><strong class="telemetry-v mono">${cfg?.performance?.lod ?? 1} mm</strong></div>
      <div class="telemetry-pill"><span class="telemetry-k">休眠</span><strong class="telemetry-v mono">${cfg?.performance?.sleepSec ?? 600}s</strong></div>
    </div>`;
}

/* ---- 视图 1：概览与性能 ---- */
export function renderOverview(device, hwState, meta, ui = {}) {
  const readOnly = !hwState.mock && hwState.supportsConfig === false;
  if (device.kind === "keyboard") {
    const sys = hwState.config.system;
    const pollBtns = meta.kbPollingRates.map((p) =>
      `<button type="button" class="seg-btn mono" data-kb-cfg="system.pollingRate" data-val="${p.id}" aria-pressed="${sys.pollingRate === p.id}">${p.hz}</button>`
    ).join("");

    return `
      <div class="workbench">
        <section class="card">
          <div class="card-head">
            <div><h2 class="card-title">${esc(device.name)}</h2><p class="card-desc">${readOnly ? "当前为只读预览 · 切换至有线或 2.4G 后可即时写入" : "实时配置预览 · 修改右侧参数即刻写入设备"}</p></div>
          </div>
          ${renderKeyboardStage(device, hwState, { showLightPreview: true, screenDataUrl: ui.screenDataUrl })}
        </section>

        <section class="card${readOnly ? " is-readonly" : ""}">
          <div class="card-head"><div><h2 class="card-title">性能与系统开关</h2><p class="card-desc">写入 50B 配置块 (OP 0x06 · ${hwState.chunkSize === 24 ? "2.4G 24B 分包" : "USB 56B 帧"})</p></div></div>
          <div class="stack">
            <div class="stack-sm">
              <span class="field-label">回报率 (Hz)</span>
              <div class="seg is-full">${pollBtns}</div>
            </div>
            <div>
              <div class="toggle-row"><div class="toggle-copy"><strong>锁定 Win 键</strong><small>防止全屏游戏时误触弹出开始菜单</small></div>${toggleSwitch("kb-winlock", sys.winLock, 'data-kb-toggle="system.winLock"')}</div>
              <div class="toggle-row"><div class="toggle-copy"><strong>屏蔽 Alt + F4</strong><small>阻止游戏过程意外关闭窗口</small></div>${toggleSwitch("kb-altf4", sys.altF4, 'data-kb-toggle="system.altF4"')}</div>
              <div class="toggle-row"><div class="toggle-copy"><strong>WASD 与方向键互换</strong><small>硬件级键位切换</small></div>${toggleSwitch("kb-wasd", sys.wasdSwap, 'data-kb-toggle="system.wasdSwap"')}</div>
              <div class="toggle-row"><div class="toggle-copy"><strong>低功耗模式</strong><small>降低背光与无线功耗</small></div>${toggleSwitch("kb-lowpower", sys.lowPower, 'data-kb-toggle="system.lowPower"')}</div>
            </div>
          </div>
        </section>
      </div>`;
  }

  // 鼠标概览与性能（DPI 五档直写 + 回报率 + LOD + 波纹 + 休眠）
  const perf = hwState.config.performance;
  const is24g = (hwState.connectionMode || device.connectionMode) === "2.4g";
  const activePollVal = is24g ? perf.polling24g : perf.pollingWired;
  const dpiHtml = hwState.config.dpiStages.map((st) => {
    const active = perf.activeDpi === st.index;
    return `
      <div class="dpi-item${active ? " is-active" : ""}">
        <button type="button" class="dpi-idx mono" data-ms-active-dpi="${st.index}" title="切换到第 ${st.index + 1} 档" aria-pressed="${active}">${st.index + 1}</button>
        <input type="color" class="color-dot-input" value="${esc(st.color)}" data-ms-dpi-color="${st.index}" aria-label="第 ${st.index + 1} 档指示色" />
        <div class="row-start">
          <input type="number" class="input dpi-num" min="50" max="26000" step="50" value="${st.dpiX}" data-ms-dpi-val="${st.index}" aria-label="第 ${st.index + 1} 档 DPI" />
          <span class="field-label mono">DPI</span>
        </div>
        ${toggleSwitch(`ms-dpi-en-${st.index}`, st.enabled, `data-ms-dpi-en="${st.index}"`)}
      </div>`;
  }).join("");

  const pollOptions = meta.msPollingRates.map((p) =>
    `<option value="${p.id}"${activePollVal === p.id ? " selected" : ""}>${p.hz} Hz</option>`
  ).join("");

  return `
    <div class="workbench">
      <section class="card">
        <div class="card-head">
          <div><h2 class="card-title">${esc(device.name)}</h2><p class="card-desc">${readOnly ? "当前为只读预览 · 唤醒 2.4G 或接入有线后可修改" : "支持 50–26000 任意 DPI 直写板载存储"}</p></div>
        </div>
        ${renderMouseStage(device, hwState, meta, { showLightPreview: true })}
      </section>

      <section class="card${readOnly ? " is-readonly" : ""}">
        <div class="card-head"><div><h2 class="card-title">DPI 档位与传感器</h2><p class="card-desc">写入 84B 配置块 (OP 0x06 · ${hwState.chunkSize === 24 ? "2.4G 24B 分包" : "USB 56B 帧"})</p></div></div>
        <div class="stack">
          <div class="stack-sm">
            <span class="field-label">5 档板载 DPI（点击序号切换生效，回车确认数值）</span>
            <div class="dpi-list">${dpiHtml}</div>
          </div>
          <div class="row">
            <div class="stack-sm" style="flex:1">
              <label class="field-label" for="ms-polling">回报率 (${is24g ? "2.4G 无线生效" : "USB 有线生效"})</label>
              <select class="select mono" id="ms-polling" data-ms-select="performance.polling">${pollOptions}</select>
            </div>
            <div class="stack-sm" style="flex:1">
              <span class="field-label">静默高度 (LOD)</span>
              <div class="seg is-full">
                <button type="button" class="seg-btn mono" data-ms-cfg="performance.lod" data-val="1" aria-pressed="${perf.lod === 1}">1 mm</button>
                <button type="button" class="seg-btn mono" data-ms-cfg="performance.lod" data-val="2" aria-pressed="${perf.lod === 2}">2 mm</button>
              </div>
            </div>
          </div>
          <div class="row">
            <div class="stack-sm" style="flex:1">
              <label class="field-label" for="ms-sleep">休眠时间 (秒)</label>
              <input class="input mono" id="ms-sleep" type="number" min="30" max="3600" step="30" value="${perf.sleepSec}" data-ms-sleep />
            </div>
            <div class="stack-sm" style="flex:1">
              <span class="field-label">波纹控制</span>
              <div class="row" style="height:34px">
                <span class="card-desc" style="margin:0">${perf.ripple ? "已开启" : "已关闭"}</span>
                ${toggleSwitch("ms-ripple", perf.ripple, 'data-ms-toggle="performance.ripple"')}
              </div>
            </div>
          </div>
        </div>
      </section>
    </div>`;
}

/* ---- 视图 2：灯光管理（严格按 Goper.exe 逆向能力表动态显隐颜色/多彩/速度/方向/亮度） ---- */
function renderDynamicLightControls({ prefix, isKb, isLogo, activeMode, cfg }) {
  const isOff = !activeMode.brightness && !activeMode.color && !activeMode.speed;
  if (isOff) {
    return `<div class="mode-hint-box"><strong>当前灯光已关闭</strong><span>点击上方任意灯效模式即可点亮并调节参数。</span></div>`;
  }

  const multiField = isLogo ? "sevenColor" : "multiColor";
  const isMulti = activeMode.multi ? Boolean(cfg[multiField]) : false;
  const maxSpeed = isKb ? (isLogo ? 3 : 2) : 4;
  const blocks = [];

  // 1. 颜色与多彩区块（仅支持改色的模式才渲染色板，内置光谱模式明确提示固件自动配色）
  if (!activeMode.color) {
    blocks.push(`
      <div class="mode-hint-box">
        <strong>内置多彩光谱模式</strong>
        <span>「${esc(activeMode.name)}」由固件自动演进色彩，不支持手动指定单色。</span>
      </div>`);
  } else {
    if (activeMode.multi) {
      const toggleAttr = isKb ? `data-kb-color-mode="${prefix}"` : `data-ms-color-mode="light"`;
      blocks.push(`
        <div class="stack-sm">
          <span class="field-label">色彩模式</span>
          <div class="seg is-full">
            <button type="button" class="seg-btn" ${toggleAttr} data-multi="1" aria-pressed="${isMulti}">${isLogo ? "7 色循环" : "随机多彩"}</button>
            <button type="button" class="seg-btn" ${toggleAttr} data-multi="0" aria-pressed="${!isMulti}">指定单色</button>
          </div>
        </div>`);
    }

    const swatchHtml = SWATCHES.map((hex) => {
      const attr = isKb ? `data-kb-color="${prefix}" data-hex="${hex}"` : `data-ms-color="${hex}"`;
      const pressed = !isMulti && String(cfg.color || "").toLowerCase() === hex;
      return `<button type="button" class="swatch" style="--sw:${hex}" ${attr} aria-label="选择颜色 ${hex}" aria-pressed="${pressed}"></button>`;
    }).join("");

    const pickerAttr = isKb ? `data-kb-color-picker="${prefix}"` : `data-ms-color-picker`;
    blocks.push(`
      <div class="stack-sm">
        <div class="row">
          <span class="field-label">${isMulti ? "预设单色（点击色块即切换为单色）" : "主灯效颜色"}</span>
          <span class="mono field-label">${esc(String(cfg.color || "#E53945").toUpperCase())}</span>
        </div>
        <div class="swatch-grid">
          ${swatchHtml}
          <input type="color" class="color-dot-input" value="${esc(cfg.color)}" ${pickerAttr} aria-label="自定义颜色" />
        </div>
      </div>`);
  }

  // 2. 灯效方向（仅波浪/彩云/环绕/滚动/极速等有方向位模式展示）
  if (isKb && !isLogo && activeMode.dir) {
    const dir = Number(cfg.direction) || 0;
    blocks.push(`
      <div class="stack-sm">
        <span class="field-label">流动方向</span>
        <div class="seg is-full">
          <button type="button" class="seg-btn" data-kb-cfg="light.direction" data-val="0" aria-pressed="${dir === 0}">左 → 右</button>
          <button type="button" class="seg-btn" data-kb-cfg="light.direction" data-val="1" aria-pressed="${dir === 1}">右 → 左</button>
        </div>
      </div>`);
  }

  // 3. 亮度
  if (activeMode.brightness) {
    const rangeAttr = isKb ? `data-kb-range="${prefix}.brightness"` : `data-ms-range="light.brightness"`;
    blocks.push(`
      <div class="stack-sm">
        <div class="row"><span class="field-label">亮度等级</span><span class="mono field-label">${cfg.brightness} / 4</span></div>
        <input type="range" min="0" max="4" step="1" value="${cfg.brightness}" ${rangeAttr} aria-label="灯光亮度" />
      </div>`);
  }

  // 4. 动态速度（恒亮/常亮/光谱/扫描/逐浪/极速/自定义等静态或固定速率模式自动隐藏）
  if (activeMode.speed) {
    const rangeAttr = isKb ? `data-kb-range="${prefix}.speed"` : `data-ms-range="light.speed"`;
    blocks.push(`
      <div class="stack-sm">
        <div class="row"><span class="field-label">动态速度</span><span class="mono field-label">${cfg.speed} / ${maxSpeed}</span></div>
        <input type="range" min="0" max="${maxSpeed}" step="1" value="${cfg.speed}" ${rangeAttr} aria-label="灯光速度" />
      </div>`);
  }

  return blocks.join("");
}

export function renderLighting(device, hwState, meta, ui) {
  const readOnly = !hwState.mock && hwState.supportsConfig === false;
  if (device.kind === "keyboard") {
    const zone = ui.kbLightZone || "kb";
    const isLogo = zone === "logo";
    const modes = isLogo ? meta.kbLogoModes : meta.kbLightModes;
    const cfg = isLogo ? hwState.config.logo : hwState.config.light;
    const prefix = isLogo ? "logo" : "light";
    const activeMode = modes.find((m) => m.id === cfg.mode) || modes[0];

    const modeGrid = modes.map((m) =>
      `<button type="button" class="mode-btn" data-kb-mode="${prefix}" data-val="${m.id}" aria-pressed="${cfg.mode === m.id}">${esc(m.name)}</button>`
    ).join("");

    const controlsHtml = renderDynamicLightControls({
      prefix,
      isKb: true,
      isLogo,
      activeMode,
      cfg,
    });

    return `
      <div class="workbench">
        <section class="card">
          <div class="card-head">
            <div><h2 class="card-title">灯光预览</h2><p class="card-desc">${isLogo ? "顶部 26 格 Logo 灯条独立控制" : "100 键边缘导光实时同步 · 不遮挡键帽字符"}</p></div>
            <div class="seg">
              <button type="button" class="seg-btn" data-light-zone="kb" aria-pressed="${!isLogo}">键盘背光</button>
              <button type="button" class="seg-btn" data-light-zone="logo" aria-pressed="${isLogo}">Logo 灯条</button>
            </div>
          </div>
          ${renderKeyboardStage(device, hwState, { showLightPreview: true, screenDataUrl: ui.screenDataUrl })}
        </section>

        <section class="card${readOnly ? " is-readonly" : ""}">
          <div class="card-head">
            <div><h2 class="card-title">${isLogo ? "Logo 灯条设置" : "背光灯效设置"}</h2><p class="card-desc">当前模式：${esc(activeMode.name)} · 仅展示该模式硬件生效项</p></div>
          </div>
          <div class="stack">
            <div class="stack-sm">
              <span class="field-label">灯效模式</span>
              <div class="mode-grid">${modeGrid}</div>
            </div>
            ${controlsHtml}
          </div>
        </section>
      </div>`;
  }

  const light = hwState.config.light;
  const activeMode = meta.msLightModes.find((m) => m.id === light.mode) || meta.msLightModes[0];
  const modeGrid = meta.msLightModes.map((m) =>
    `<button type="button" class="mode-btn" data-ms-mode="${m.id}" aria-pressed="${light.mode === m.id}">${esc(m.name)}</button>`
  ).join("");

  const controlsHtml = renderDynamicLightControls({
    prefix: "light",
    isKb: false,
    isLogo: false,
    activeMode,
    cfg: light,
  });

  return `
    <div class="workbench">
      <section class="card">
        <div class="card-head"><div><h2 class="card-title">鼠标灯效预览</h2><p class="card-desc">滚轮、中段 V 型导光翼与尾部 Logo 实时同步</p></div></div>
        ${renderMouseStage(device, hwState, meta, { showLightPreview: true })}
      </section>

      <section class="card${readOnly ? " is-readonly" : ""}">
        <div class="card-head"><div><h2 class="card-title">灯光参数</h2><p class="card-desc">当前模式：${esc(activeMode.name)} · 写入 84B 配置块 (OP 0x06)</p></div></div>
        <div class="stack">
          <div class="stack-sm">
            <span class="field-label">灯效模式</span>
            <div class="mode-grid">${modeGrid}</div>
          </div>
          ${controlsHtml}
        </div>
      </section>
    </div>`;
}

/* ---- 视图 3：按键映射 ---- */
export function renderKeymap(device, hwState, meta, ui) {
  const km = hwState.keymap || [];
  const defaultSlot = device.kind === "keyboard" ? 0 : 3;
  const selectedSlot = ui.selectedSlot ?? defaultSlot;
  const row = km.find((k) => k.slot === selectedSlot) || km[0];
  const category = ui.keyCategory || (device.kind === "mouse" ? "mouse" : "key");
  const pendingHex = ui.pendingHex || row?.hex || "200029";

  let pickerHtml = "";
  if (category === "key") {
    const opts = KEY_PRESETS.map(([hex, label]) =>
      `<option value="${hex}"${pendingHex === hex ? " selected" : ""}>${esc(label)} (0x${hex})</option>`
    ).join("");
    if (device.kind === "keyboard") {
      pickerHtml = `
        <div class="km-picker-grid">
          <button type="button" class="capture-btn is-compact${ui.capturingKey ? " is-listening" : ""}" id="capture-key-btn">
            ${icon("command", 14)}
            <span>${ui.capturingKey ? "请按物理按键…" : "点击后按物理键录入"}</span>
          </button>
          <select class="select mono" id="key-preset-select" aria-label="从标准键表选择">${opts}</select>
        </div>`;
    } else {
      pickerHtml = `
        <div class="stack-sm">
          <button type="button" class="capture-btn${ui.capturingKey ? " is-listening" : ""}" id="capture-key-btn">
            ${icon("command", 15)}
            <span>${ui.capturingKey ? "请按下键盘上的目标按键…" : "点击后按物理按键快速录入"}</span>
          </button>
          <label class="field-label" for="key-preset-select">或从标准键表选择</label>
          <select class="select mono" id="key-preset-select">${opts}</select>
        </div>`;
    }
  } else if (category === "mouse") {
    const opts = MOUSE_PRESETS.map(([hex, label]) =>
      `<option value="${hex}"${pendingHex === hex ? " selected" : ""}>${esc(label)}</option>`
    ).join("");
    pickerHtml = `<div class="stack-sm"><label class="field-label" for="key-preset-select">鼠标与 DPI 功能</label><select class="select" id="key-preset-select">${opts}</select></div>`;
  } else if (category === "media") {
    const opts = MEDIA_PRESETS.map(([hex, label]) =>
      `<option value="${hex}"${pendingHex === hex ? " selected" : ""}>${esc(label)}</option>`
    ).join("");
    pickerHtml = `<div class="stack-sm"><label class="field-label" for="key-preset-select">多媒体控制</label><select class="select" id="key-preset-select">${opts}</select></div>`;
  } else {
    pickerHtml = `<p class="card-desc">写入 <span class="mono">0xA00000</span>，按下该物理按键将不触发任何输出。</p>`;
  }

  if (device.kind === "keyboard") {
    const stageHtml = renderKeyboardStage(device, hwState, { interactiveKeys: true, selectedSlot, large: true, screenDataUrl: ui.screenDataUrl });
    return `
      <div class="keymap-stack">
        <section class="card">
          <div class="card-head">
            <div><h2 class="card-title">键盘物理按键映射</h2><p class="card-desc">全宽放大视图 · OP 0x08 读当前态 · OP 0x07 对照出厂 ROM · OP 0x09 写入</p></div>
            <button type="button" class="btn btn-danger" id="reset-all-keys-btn">${icon("rotateCcw", 14)}重置全部键位</button>
          </div>
          ${stageHtml}
        </section>

        <section class="card keymap-inspector-bar">
          <div class="km-col-info">
            <div class="row">
              <div>
                <h2 class="card-title">槽位 <span class="mono">#${row?.slot ?? 0}</span></h2>
                <p class="card-desc">${row?.modified ? "已偏离出厂默认配置" : "与出厂默认一致"}</p>
              </div>
              <span class="keycap">${esc(row?.current?.label ?? "--")}</span>
            </div>
            <div class="spec-list">
              <div class="spec-item"><dt>出厂默认 (0x07)</dt><dd class="mono">${esc(row?.fallback?.label ?? "--")}</dd></div>
              <div class="spec-item"><dt>当前生效 (0x08)</dt><dd class="mono">${esc(row?.current?.label ?? "--")}</dd></div>
            </div>
          </div>

          <div class="km-col-edit">
            <div class="stack-sm">
              <span class="field-label">功能类型</span>
              <div class="seg is-full">
                <button type="button" class="seg-btn" data-key-cat="key" aria-pressed="${category === "key"}">键盘</button>
                <button type="button" class="seg-btn" data-key-cat="mouse" aria-pressed="${category === "mouse"}">鼠标</button>
                <button type="button" class="seg-btn" data-key-cat="media" aria-pressed="${category === "media"}">媒体</button>
                <button type="button" class="seg-btn" data-key-cat="disable" aria-pressed="${category === "disable"}">禁用</button>
              </div>
            </div>
            ${pickerHtml}
          </div>

          <div class="km-col-actions">
            <button type="button" class="btn btn-primary" id="save-one-key-btn">${icon("check", 14)}写入设备</button>
            <button type="button" class="btn btn-secondary" id="reset-one-key-btn">${icon("rotateCcw", 14)}恢复此键默认</button>
          </div>
        </section>
      </div>`;
  }

  const stageHtml = renderMouseStage(device, hwState, meta, { interactiveKeys: true, selectedSlot, showLightPreview: true });

  return `
    <div class="workbench is-keymap">
      <section class="card">
        <div class="card-head">
          <div><h2 class="card-title">物理按键映射</h2><p class="card-desc">OP 0x08 读取当前生效值 · OP 0x07 对照出厂 ROM · OP 0x09 写入</p></div>
          <button type="button" class="btn btn-danger" id="reset-all-keys-btn">${icon("rotateCcw", 14)}重置全部键位</button>
        </div>
        ${stageHtml}
      </section>

      <aside class="card">
        <div class="card-head">
          <div>
            <h2 class="card-title">槽位 <span class="mono">#${row?.slot ?? 0}</span></h2>
            <p class="card-desc">${row?.modified ? "已偏离出厂默认配置" : "与出厂默认一致"}</p>
          </div>
          <span class="keycap">${esc(row?.current?.label ?? "--")}</span>
        </div>

        <div class="spec-list">
          <div class="spec-item"><dt>出厂默认 (0x07)</dt><dd class="mono">${esc(row?.fallback?.label ?? "--")}</dd></div>
          <div class="spec-item"><dt>当前生效 (0x08)</dt><dd class="mono">${esc(row?.current?.label ?? "--")}</dd></div>
        </div>

        <div class="stack-sm">
          <span class="field-label">功能类型</span>
          <div class="seg is-full">
            <button type="button" class="seg-btn" data-key-cat="key" aria-pressed="${category === "key"}">键盘</button>
            <button type="button" class="seg-btn" data-key-cat="mouse" aria-pressed="${category === "mouse"}">鼠标</button>
            <button type="button" class="seg-btn" data-key-cat="media" aria-pressed="${category === "media"}">媒体</button>
            <button type="button" class="seg-btn" data-key-cat="disable" aria-pressed="${category === "disable"}">禁用</button>
          </div>
        </div>

        ${pickerHtml}

        <div class="row">
          <button type="button" class="btn btn-secondary" id="reset-one-key-btn">${icon("rotateCcw", 14)}恢复此键</button>
          <button type="button" class="btn btn-primary" id="save-one-key-btn" style="flex:1">${icon("check", 14)}写入设备</button>
        </div>
      </aside>
    </div>`;
}

/* ---- 视图 4：键盘 320×172 小屏图片选择器与裁切工作台（OP 0x23 + OP 0x25 Big-Endian RGB565） ---- */
const SCREEN_BG_SWATCHES = ["#08090b", "#141619", "#ffffff", "#26080c"];

export function renderScreen(device, hwState, ui) {
  const scImg = hwState.screenImage || {};
  const libItems = Array.isArray(scImg.items) ? scImg.items : [];
  const sourceKind = ui.screenSourceKind || "library"; // "library" | "custom" | "preset"
  const selectedUuid = ui.screenSelectedUuid || scImg.currentUuid || null;
  const preset = ui.screenPreset || "default";
  const fit = ui.screenFit || "cover";
  const zoomPct = Math.round((ui.screenZoom ?? 1) * 100);
  const rotate = ui.screenRotate || 0;
  const flipH = Boolean(ui.screenFlipH);
  const bgColor = ui.screenBgColor || "#08090b";
  const text = ui.screenText ?? "";
  const uploading = Boolean(ui.screenUploading);
  const pct = ui.screenUploadPct ?? 0;

  const isEdited = zoomPct !== 100 || (ui.screenPanX || 0) !== 0 || (ui.screenPanY || 0) !== 0 || rotate !== 0 || flipH;

  let activeTitle = "官方红魔姬";
  let sourceDim = "";
  if (sourceKind === "custom" && ui.screenCustomImg) {
    activeTitle = ui.screenCustomName || "本地载入图片";
    sourceDim = `${ui.screenCustomImg.naturalWidth}×${ui.screenCustomImg.naturalHeight}`;
  } else if (sourceKind === "library" && selectedUuid) {
    const cur = libItems.find((it) => it.uuid === selectedUuid);
    activeTitle = cur ? cur.name : (scImg.currentName || "默认预览");
    sourceDim = "320×172";
  } else if (sourceKind === "preset") {
    activeTitle = preset === "redmagic" ? "红魔战魂" : preset === "clock" ? "极简时钟" : "官方红魔姬";
    sourceDim = "320×172";
  }

  const canTransform = sourceKind !== "preset" || preset === "default";

  // 本机壁纸图库缩略图网格
  const libCardsHtml = libItems.map((it) => {
    const isActive = sourceKind === "library" && selectedUuid === it.uuid;
    return `
      <div class="gallery-card${isActive ? " is-active" : ""}" role="button" tabindex="0" data-screen-lib-uuid="${esc(it.uuid)}" title="${esc(it.name)}">
        <div class="gallery-thumb">
          <img src="${esc(it.dataUrl)}" alt="${esc(it.name)}" draggable="false" />
          ${it.isLinked ? `<span class="gallery-badge">已同步</span>` : ""}
          <button type="button" class="gallery-del" data-screen-lib-del="${esc(it.uuid)}" title="从本机图库删除">${icon("trash", 12)}</button>
        </div>
        <div class="gallery-foot">
          <span class="gallery-name">${esc(it.name)}</span>
        </div>
      </div>`;
  }).join("");

  const bgSwatchesHtml = SCREEN_BG_SWATCHES.map((hex) =>
    `<button type="button" class="bg-dot" style="--sw:${hex}" data-screen-bg="${hex}" aria-pressed="${bgColor.toLowerCase() === hex}" title="底色 ${hex}"></button>`
  ).join("");

  const now = new Date();
  const hhmm = `${String(now.getHours()).padStart(2, "0")}:${String(now.getMinutes()).padStart(2, "0")}`;

  return `
    <div class="workbench is-screen">
      <section class="card">
        <div class="card-head">
          <div class="card-head-text">
            <h2 class="card-title">小屏取景与裁切预览</h2>
            <p class="card-desc">在画面上拖拽平移构图 · 滚轮缩放 · 支持直接拖入图片或 <span class="mono">Ctrl+V</span> 粘贴</p>
          </div>
          <div class="card-actions">
            <input type="file" id="screen-file-input" accept="image/png,image/jpeg,image/gif,image/webp,image/bmp" hidden />
            <button type="button" class="btn btn-primary btn-sm" id="pick-screen-file-btn">${icon("image", 14)}选择图片</button>
            <button type="button" class="btn btn-secondary btn-sm" id="paste-screen-btn" title="从剪贴板粘贴图片 (Ctrl+V)">${icon("clipboard", 14)}粘贴</button>
            <button type="button" class="btn btn-secondary btn-sm" id="export-screen-btn" title="导出 320×172 PNG">${icon("download", 14)}导出</button>
          </div>
        </div>

        <div class="screen-editor-block">
          <div class="screen-bezel">
            <div class="screen-bezel-top">
              <span class="mono">TFT 320×172${sourceDim ? ` · 原图 ${sourceDim}` : ""}</span>
              <span class="mono screen-bezel-name" title="${esc(activeTitle)}">${esc(activeTitle)}</span>
            </div>
            <div class="screen-canvas-box${canTransform ? " is-pannable" : ""}" id="screen-drop-zone">
              <canvas id="screen-canvas" width="320" height="172"></canvas>
              <div class="screen-grid-overlay" aria-hidden="true"></div>
              <div class="screen-drop-overlay" aria-hidden="true">
                ${icon("image", 20)}
                <span>松开鼠标载入图片到 320×172 小屏</span>
              </div>
            </div>
          </div>

          <div class="screen-crop-toolbar">
            <div class="screen-tool-row">
              <div class="seg">
                <button type="button" class="seg-btn" data-screen-fit="cover" aria-pressed="${fit === "cover"}">铺满裁切</button>
                <button type="button" class="seg-btn" data-screen-fit="contain" aria-pressed="${fit === "contain"}">完整居中</button>
                <button type="button" class="seg-btn" data-screen-fit="fill" aria-pressed="${fit === "fill"}">拉伸填满</button>
              </div>
              <div class="tool-icon-group">
                <button type="button" class="tool-icon-btn" id="screen-rot-ccw" title="逆时针旋转 90°" ${canTransform ? "" : "disabled"}>${icon("rotateCcw", 14)}</button>
                <button type="button" class="tool-icon-btn" id="screen-rot-cw" title="顺时针旋转 90°" ${canTransform ? "" : "disabled"}>${icon("rotateCw", 14)}</button>
                <button type="button" class="tool-icon-btn" id="screen-flip-h" aria-pressed="${flipH}" title="水平翻转" ${canTransform ? "" : "disabled"}>${icon("flipH", 14)}</button>
                <button type="button" class="tool-icon-btn" id="screen-reset-view" title="重置缩放与位置" ${isEdited ? "" : "disabled"}>${icon("refresh", 14)}</button>
              </div>
            </div>

            <div class="screen-tool-row is-zoom">
              <div class="zoom-control">
                <span class="field-label nowrap">缩放</span>
                <input type="range" id="screen-zoom-range" min="50" max="300" step="5" value="${zoomPct}" ${canTransform ? "" : "disabled"} aria-label="画面缩放倍率" />
                <span class="mono field-label zoom-readout" id="screen-zoom-val">${zoomPct}%</span>
              </div>
              <div class="bg-control" title="留白区背景填充色">
                <span class="field-label nowrap">底色</span>
                <div class="bg-dot-row">
                  ${bgSwatchesHtml}
                  <input type="color" class="color-dot-input" id="screen-bg-picker" value="${esc(bgColor)}" title="自定义底色" />
                </div>
              </div>
            </div>
          </div>
        </div>

        ${renderKeyboardStage(device, hwState, { showLightPreview: true, screenDataUrl: ui.screenDataUrl })}
      </section>

      <section class="card">
        <div class="card-head">
          <div class="card-head-text">
            <h2 class="card-title">图片选择器</h2>
            <p class="card-desc">本机图库与 <span class="mono">ScreenFileList.ini</span> 实时同步</p>
          </div>
          <div class="card-actions">
            <button type="button" class="btn btn-secondary btn-sm" id="save-screen-lib-btn" title="将当前裁切结果保存到本机图库">${icon("bookmark", 13)}存入图库</button>
          </div>
        </div>

        <div class="stack">
          <div class="stack-sm">
            <div class="row">
              <span class="field-label">本机图库 (<span class="mono">${libItems.length}</span> 张)</span>
              <span class="field-hint">点击切换 · 悬停右上角可移除</span>
            </div>
            <div class="screen-gallery-grid" id="screen-gallery-drop">
              ${libCardsHtml}
              <button type="button" class="gallery-add-tile" id="gallery-add-btn">
                ${icon("plus", 18)}
                <span>导入本地图片</span>
                <small>PNG / JPG / WebP / BMP</small>
              </button>
            </div>
          </div>

          <div class="stack-sm">
            <span class="field-label">内置预设画面</span>
            <div class="preset-gallery-grid">
              <button type="button" class="preset-card${sourceKind === "preset" && preset === "default" ? " is-active" : ""}" data-screen-preset="default">
                <div class="preset-thumb">
                  ${scImg.defaultDataUrl ? `<img src="${esc(scImg.defaultDataUrl)}" alt="官方红魔姬" draggable="false" />` : `<div class="preset-art is-redmagic"></div>`}
                </div>
                <span class="preset-name">官方红魔姬</span>
              </button>
              <button type="button" class="preset-card${sourceKind === "preset" && preset === "redmagic" ? " is-active" : ""}" data-screen-preset="redmagic">
                <div class="preset-thumb preset-art is-redmagic">
                  <svg viewBox="0 0 320 172" aria-hidden="true"><path d="M160 30 192 52 178 112 160 128 142 112 128 52Z" fill="#e53945"/><rect x="154" y="48" width="12" height="50" fill="#12090c"/></svg>
                </div>
                <span class="preset-name">红魔战魂</span>
              </button>
              <button type="button" class="preset-card${sourceKind === "preset" && preset === "clock" ? " is-active" : ""}" data-screen-preset="clock">
                <div class="preset-thumb preset-art is-clock mono">
                  <strong>${hhmm}</strong>
                  <small>GK001S</small>
                </div>
                <span class="preset-name">极简时钟</span>
              </button>
            </div>
          </div>

          <div class="stack-sm">
            <label class="field-label" for="screen-text-input">底部叠加角标（可选，默认留空保持原图纯净）</label>
            <div class="input-with-clear">
              <input type="text" class="input mono" id="screen-text-input" maxlength="24" value="${esc(text)}" placeholder="例如 REDMAGIC · 留空不叠加" />
              ${text ? `<button type="button" class="input-clear-btn" id="screen-text-clear" title="清空文字">${icon("x", 13)}</button>` : ""}
            </div>
          </div>

          <div class="screen-upload-box">
            ${!hwState.mock && hwState.connectionMode !== "wired" ? `
              <div class="screen-wire-note">
                ${icon("usb", 14)}
                <span>小屏 128KB 固件写入需使用 <strong>USB 有线直连</strong>（当前为 ${hwState.connectionMode === "2.4g" ? "2.4G 无线" : "蓝牙 BLE"}）。你仍可在此裁切并存入本机图库，插入数据线后一键同步。</span>
              </div>` : ""}
            <div class="row">
              <span class="field-label">传输：<span class="mono">128 KB Flash (2,341 帧)</span></span>
              <span class="mono field-label nowrap" id="screen-upload-pct">${uploading ? `${pct}%` : (!hwState.mock && hwState.connectionMode !== "wired" ? "需有线连接" : "就绪")}</span>
            </div>
            <div class="progress-track">
              <div class="progress-fill" id="screen-upload-bar" style="width:${uploading ? pct : 0}%"></div>
            </div>
            <button type="button" class="btn btn-primary" id="upload-screen-btn" ${uploading || (!hwState.mock && hwState.connectionMode !== "wired") ? "disabled" : ""} style="width:100%">
              ${icon("upload", 14)}
              <span>${uploading ? `正在写入键盘小屏 (${pct}%)…` : (!hwState.mock && hwState.connectionMode !== "wired" ? "请接入 USB 有线以写入键盘小屏" : "将当前画面写入键盘小屏并保存")}</span>
            </button>
            <p class="screen-foot-note">固定写入静态壁纸扇区 <span class="mono">0xCE0000</span>（绝不触碰系统 UI 字库），完成后约 2 秒刷新物理屏幕；硬件不支持 USB 读屏，已同步记录保存在本机图库。</p>
          </div>
        </div>
      </section>
    </div>`;
}

/* ---- 视图 5：硬件与协议诊断 ---- */
export function renderHardware(device, hwState) {
  const mode = hwState.connectionMode || device.connectionMode || "wired";
  const linkState = hwState.linkState || device.linkState || "online";
  const links = Array.isArray(device.links) ? device.links : [];
  const transportName = hwState.mock
    ? "离线仿真 (MockTransport)"
    : linkState === "bt-readonly"
      ? "Windows BLE HOGP + BAS 0x180F"
      : mode === "2.4g"
        ? "2.4G 无线接收器 (24B RF 分包 + OP 0xaa)"
        : mode === "bluetooth"
          ? "蓝牙 HID 厂商通道 (0xFF1C/0x92)"
          : "USB HID 有线直连 (56B 帧)";

  const linksHtml = links.map((l) => {
    const isAct = l.active || l.linkId === device.linkId;
    const btnText = isAct
      ? (l.state === "sleeping" ? "唤醒探测" : "当前通道")
      : (l.state === "sleeping" ? "探测并切换" : "切换通道");
    return `
      <div class="link-row${isAct ? " is-active" : ""}">
        <div class="link-row-main">
          ${renderConnectionBadge(l.mode, l.state, { mock: device.mock })}
          <div class="link-row-meta mono">
            <span>PID 0x${(l.pid || 0).toString(16).padStart(4, "0")}</span>
            <span>·</span>
            <span>${l.chunkSize || 56}B/帧</span>
            ${l.btAddress ? `<span>· ${esc(l.btAddress)}</span>` : ""}
            ${l.battery != null ? `<span>· ${l.battery}%</span>` : ""}
          </div>
        </div>
        <div class="link-row-right">
          <span class="link-state-tag is-${esc(l.state)}">${esc(linkStateLabel(l.state))}</span>
          ${!device.mock ? `<button type="button" class="btn btn-secondary btn-sm" ${isAct && l.state !== "sleeping" ? "disabled" : ""} data-switch-link="${esc(l.linkId)}">${btnText}</button>` : ""}
        </div>
      </div>`;
  }).join("");

  return `
    <div class="workbench">
      <section class="card">
        <div class="card-head">
          <div><h2 class="card-title">设备硬件与三模连接信息</h2><p class="card-desc">来自 HID 枚举、OP 0xaa 无线探测与 OP 0x03 能力帧</p></div>
          <button type="button" class="btn btn-secondary" id="copy-diag-btn">${icon("copy", 14)}复制诊断 JSON</button>
        </div>
        <dl class="spec-list">
          <div class="spec-item"><dt>设备型号</dt><dd>${esc(device.name)} (${esc(device.model)})</dd></div>
          <div class="spec-item"><dt>主控方案</dt><dd class="mono">${esc(device.chip)}</dd></div>
          <div class="spec-item"><dt>当前连接模式</dt><dd>${renderConnectionBadge(mode, linkState, { mock: device.mock })}</dd></div>
          <div class="spec-item"><dt>链路状态 / 充电</dt><dd>${esc(linkStateLabel(linkState))} · ${esc(chargingLabel(hwState.chargingState))}</dd></div>
          <div class="spec-item"><dt>VID / PID</dt><dd class="mono">0x${(device.vid || 0).toString(16).padStart(4, "0")} / 0x${(device.pid || 0).toString(16).padStart(4, "0")}</dd></div>
          <div class="spec-item"><dt>单帧 MTU / 键槽</dt><dd class="mono">${hwState.chunkSize || device.chunkSize || 56} B · ${hwState.hwKeyCount ?? "--"} 键槽</dd></div>
          <div class="spec-item"><dt>传输驱动</dt><dd>${esc(transportName)}</dd></div>
          <div class="spec-item"><dt>固件 / 蓝牙 MAC</dt><dd class="mono">${esc(device.firmware)}${hwState.btAddress || device.btAddress ? ` · ${esc(hwState.btAddress || device.btAddress)}` : ""}</dd></div>
        </dl>
        <div class="stack-sm">
          <div class="row">
            <span class="field-label">已检测到的物理 / 无线链路 (${links.length})</span>
            ${!device.mock ? `<button type="button" class="btn btn-secondary btn-sm" id="wake-device-btn">${icon("refresh", 12)}重新探测</button>` : ""}
          </div>
          <div class="link-list">${linksHtml || `<p class="card-desc">暂无可用链路</p>`}</div>
        </div>
        <div class="stack-sm">
          <span class="field-label">当前活动 HID 路径</span>
          <pre class="hex-dump">${esc(device.path || (linkState === "bt-readonly" ? "(Windows BLE HOGP 系统通道)" : "(Mock 虚拟通道)"))}</pre>
        </div>
      </section>

      <section class="card">
        <div class="card-head"><div><h2 class="card-title">原始寄存器快照</h2><p class="card-desc">用于核对板载配置与协议调试</p></div></div>
        <div class="stack-sm">
          <span class="field-label">OP 0x03 能力块 (AA55)</span>
          <pre class="hex-dump">${esc(hwState.capabilityHex || "--")}</pre>
        </div>
        <div class="stack-sm">
          <span class="field-label">OP 0x05 配置块 (${device.kind === "keyboard" ? "50B" : "84B"})</span>
          <pre class="hex-dump">${esc(hwState.config?.rawHex || "--")}</pre>
        </div>
      </section>
    </div>`;
}

/* ---- 视图 6：灯效编程工作室（板载逐键 OP 0x0b / JS Shader 实时流 OP 0x12+0x28 / 多图层联动 / 本地 UDP+WS 接口） ---- */
export function renderStudio(device, hwState, ui) {
  const st = ui.studio;
  const sub = st.subMode || "script";
  const readOnly = !hwState.mock && hwState.supportsConfig === false;
  const is24g = (hwState.connectionMode || device.connectionMode) === "2.4g";
  const targetFpsLabel = is24g ? "2.4G · 8 FPS" : "USB 有线 · 30 FPS";
  const isAudioOn = st.telemetry?.audio?.active || (sub === "layers" && st.layers.audio.enabled);

  // 左下状态胶囊
  let livePillHtml = `<span class="conn-badge" id="studio-live-pill">仅预览 · 未推流</span>`;
  if (st.serverStatus?.running) {
    livePillHtml = `<span class="conn-badge is-24g" id="studio-live-pill">外部接口接管 · ${formatFps(st.serverStatus.fps)} FPS</span>`;
  } else if (st.streaming) {
    livePillHtml = `<span class="conn-badge is-24g" id="studio-live-pill">RAM 实时推流 · ${formatFps(st.streamFps || (is24g ? 8 : 30))} FPS</span>`;
  } else if (sub === "paint") {
    livePillHtml = `<span class="conn-badge is-wired" id="studio-live-pill">板载逐键 · 组 ${st.paintGroup + 1}</span>`;
  }

  const stageHint = sub === "paint"
    ? "左键拖拽涂色 · 右键擦除 · OP 0x0b"
    : sub === "api"
      ? "实时预览 UDP / WS / HTTP 推送帧"
      : `OP 0x12+0x28 零损耗 RAM · ${targetFpsLabel}`;

  // 音频频谱可视化微条（仅启用音频感知时渲染）
  const bands = st.telemetry?.audio?.bands || new Array(16).fill(0);
  const audioMeterHtml = isAudioOn ? `
    <div class="studio-audio-meter" id="studio-audio-meter" aria-label="WASAPI 16 段实时频谱">
      <div class="studio-audio-head">
        <span class="field-label">${icon("music", 12)} WASAPI 系统回环 16 段频谱</span>
        <span class="field-hint mono" id="studio-audio-status">${st.telemetry?.audio?.error ? esc(st.telemetry.audio.error) : "实时采集"}</span>
      </div>
      <div class="studio-eq-bars">
        ${bands.map((v, idx) => `<span class="studio-eq-bar" data-band="${idx}" style="height:${Math.max(8, Math.round(v * 100))}%"></span>`).join("")}
      </div>
    </div>` : "";

  // 左下方实时推流控制台
  let leftFootHtml = "";
  if (sub === "paint") {
    leftFootHtml = `
      <div class="studio-stream-bar">
        <div class="studio-bar-copy">
          <strong>板载 Flash 逐键存储（OP 0x0a / OP 0x0b）</strong>
          <span>写入后无需后台运行，断电或切无线仍生效。</span>
        </div>
        <div class="card-actions">
          <button type="button" class="btn btn-secondary btn-sm" id="studio-paint-read-btn" ${readOnly ? "disabled" : ""}>${icon("refresh", 13)}读取板载</button>
          <button type="button" class="btn btn-primary btn-sm" id="studio-paint-write-btn" ${readOnly ? "disabled" : ""}>${icon("upload", 13)}写入并启用</button>
        </div>
      </div>`;
  } else if (sub === "api") {
    const running = Boolean(st.serverStatus?.running);
    leftFootHtml = `
      <div class="studio-stream-bar">
        <div class="studio-bar-copy">
          <strong>${running ? `本地接口运行中 · 已收 ${st.serverStatus.packets || 0} 帧` : "本地回环控灯桥未启动"}</strong>
          <span class="mono">UDP 127.0.0.1:${st.udpPort} · HTTP/WS 127.0.0.1:${st.httpPort}</span>
        </div>
        <div class="card-actions">
          ${running ? `<button type="button" class="btn btn-secondary btn-sm" id="studio-tray-btn" title="隐藏到 Windows 系统托盘继续后台运行">${icon("minus", 13)}后台挂起</button>` : ""}
          <button type="button" class="btn ${running ? "btn-danger" : "btn-primary"} btn-sm" id="studio-server-toggle-btn" ${readOnly ? "disabled" : ""}>
            ${running ? `${icon("stop", 13)}停止服务` : `${icon("play", 13)}启动本地服务`}
          </button>
        </div>
      </div>`;
  } else {
    leftFootHtml = `
      <div class="studio-stream-bar">
        <div class="studio-stream-left">
          <button type="button" class="btn ${st.streaming ? "btn-danger" : "btn-primary"} btn-sm" id="studio-stream-btn" ${readOnly ? "disabled" : ""}>
            ${st.streaming ? `${icon("stop", 13)}停止推流` : `${icon("play", 13)}开始实时推流到真机`}
          </button>
          ${st.streaming ? `<button type="button" class="btn btn-secondary btn-sm" id="studio-tray-btn" title="最小化到系统托盘继续 30FPS 推流（点关闭按钮也会自动挂起）">${icon("minus", 13)}后台挂起</button>` : ""}
          <label class="studio-check">
            <input type="checkbox" id="studio-logo-sync"${st.includeLogo ? " checked" : ""} />
            <span>Logo 20 灯</span>
          </label>
        </div>
        <div class="studio-bri-ctrl">
          <span class="field-label nowrap">亮度</span>
          <input type="range" id="studio-bri-range" min="15" max="100" step="5" value="${Math.round((st.brightness ?? 1) * 100)}" aria-label="推流主亮度" />
          <span class="mono field-label" id="studio-bri-val">${Math.round((st.brightness ?? 1) * 100)}%</span>
        </div>
      </div>`;
  }

  // 右侧 4 个子模式面板
  let rightBodyHtml = "";
  if (sub === "paint") {
    const swHtml = SWATCHES.map((hex) =>
      `<button type="button" class="swatch" style="--sw:${hex}" data-paint-swatch="${hex}" aria-pressed="${st.paintColor.toLowerCase() === hex}" aria-label="画笔颜色 ${hex}"></button>`
    ).join("");
    const grpBtns = [0, 1, 2, 3].map((g) =>
      `<button type="button" class="seg-btn mono" data-paint-group="${g}" aria-pressed="${st.paintGroup === g}">组 ${g + 1}</button>`
    ).join("");
    rightBodyHtml = `
      <div class="stack">
        <div class="stack-sm">
          <span class="field-label">板载存储分组（OP 0x0b · 4 组独立槽位）</span>
          <div class="seg is-full">${grpBtns}</div>
        </div>

        <div class="stack-sm">
          <span class="field-label">画笔工具（支持在左侧键盘直接拖拽涂色）</span>
          <div class="seg is-full">
            <button type="button" class="seg-btn" data-paint-tool="brush" aria-pressed="${st.paintTool === "brush"}">${icon("brush", 13)} 画笔</button>
            <button type="button" class="seg-btn" data-paint-tool="eraser" aria-pressed="${st.paintTool === "eraser"}">${icon("eraser", 13)} 橡皮擦</button>
            <button type="button" class="seg-btn" data-paint-tool="eyedropper" aria-pressed="${st.paintTool === "eyedropper"}">${icon("pipette", 13)} 吸管取色</button>
          </div>
        </div>

        <div class="stack-sm">
          <div class="row">
            <span class="field-label">画笔颜色</span>
            <span class="mono field-label">${esc(st.paintColor.toUpperCase())}</span>
          </div>
          <div class="swatch-grid">
            ${swHtml}
            <input type="color" class="color-dot-input" id="studio-paint-picker" value="${esc(st.paintColor)}" aria-label="自定义画笔颜色" />
          </div>
        </div>

        <div class="stack-sm">
          <span class="field-label">一键分区模板</span>
          <div class="studio-tpl-grid">
            <button type="button" class="mode-btn" data-paint-tpl="redmagic">红魔战甲</button>
            <button type="button" class="mode-btn" data-paint-tpl="fps">FPS WASD</button>
            <button type="button" class="mode-btn" data-paint-tpl="moba">MOBA QWER</button>
            <button type="button" class="mode-btn" data-paint-tpl="cyber">赛博渐变</button>
            <button type="button" class="mode-btn" data-paint-tpl="white">纯白极简</button>
            <button type="button" class="mode-btn" data-paint-tpl="clear">全黑清空</button>
          </div>
        </div>

        <div class="mode-hint-box">
          <strong>持久化说明</strong>
          <span>点击左侧「写入并启用」后将 378B 逐键色彩写入 Flash 并切换至「自定义」灯效，关闭软件仍永久保持。</span>
        </div>
      </div>`;
  } else if (sub === "script") {
    const activePreset = STUDIO_SCRIPTS.find((s) => s.id === st.scriptId) || STUDIO_SCRIPTS[0];
    const presetBtns = STUDIO_SCRIPTS.map((s) =>
      `<button type="button" class="mode-btn" data-studio-script="${esc(s.id)}" aria-pressed="${st.scriptId === s.id}">${esc(s.name)}</button>`
    ).join("");
    const labels = activePreset.labels || ["参数 P1", "参数 P2", "参数 P3"];
    rightBodyHtml = `
      <div class="stack">
        <div class="stack-sm">
          <div class="row">
            <span class="field-label">脚本预设</span>
            <div class="card-actions">
              <button type="button" class="btn btn-secondary btn-sm" id="studio-import-btn">${icon("folder", 12)}导入</button>
              <button type="button" class="btn btn-secondary btn-sm" id="studio-export-btn">${icon("download", 12)}导出</button>
            </div>
          </div>
          <div class="studio-preset-grid">${presetBtns}</div>
        </div>

        <div class="stack-sm">
          <div class="row">
            <span class="field-label mono">render(x, y, t, ctx) → [r, g, b]</span>
            <span class="field-hint">实时热更新</span>
          </div>
          <textarea class="studio-code mono" id="studio-code-editor" spellcheck="false">${esc(st.scriptCode)}</textarea>
          <div class="studio-code-err" id="studio-code-err"${st.scriptError ? "" : " hidden"}>${esc(st.scriptError)}</div>
        </div>

        <div class="studio-param-grid">
          <label class="studio-param">
            <div class="row"><span class="field-label">${esc(labels[0])}</span><span class="mono field-hint" id="studio-p1-val">${Math.round(st.p1 * 100)}%</span></div>
            <input type="range" data-studio-param="p1" min="0" max="100" value="${Math.round(st.p1 * 100)}" />
          </label>
          <label class="studio-param">
            <div class="row"><span class="field-label">${esc(labels[1])}</span><span class="mono field-hint" id="studio-p2-val">${Math.round(st.p2 * 100)}%</span></div>
            <input type="range" data-studio-param="p2" min="0" max="100" value="${Math.round(st.p2 * 100)}" />
          </label>
          <label class="studio-param">
            <div class="row"><span class="field-label">${esc(labels[2])}</span><span class="mono field-hint" id="studio-p3-val">${Math.round(st.p3 * 100)}%</span></div>
            <input type="range" data-studio-param="p3" min="0" max="100" value="${Math.round(st.p3 * 100)}" />
          </label>
        </div>

        <div class="studio-sensor-row">
          <span class="field-label">系统联动源</span>
          <div class="studio-sensor-checks">
            <label class="studio-check"><input type="checkbox" data-studio-sensor="audio"${st.telemetry?.audio?.active ? " checked" : ""} /><span>WASAPI 音频</span></label>
            <label class="studio-check"><input type="checkbox" data-studio-sensor="ambilight"${st.telemetry?.ambilight?.active ? " checked" : ""} /><span>屏幕拾色</span></label>
            <label class="studio-check"><input type="checkbox" data-studio-sensor="keys"${st.telemetry?.keys?.active ? " checked" : ""} /><span>全局按键</span></label>
          </div>
        </div>
      </div>`;
  } else if (sub === "layers") {
    const L = st.layers;
    rightBodyHtml = `
      <div class="studio-layers-list">
        <div class="studio-layer-card${L.gradient.enabled ? " is-on" : ""}">
          <div class="row">
            <strong>1. 动态渐变基底</strong>
            <div class="row-start">
              <input type="color" class="color-dot-input" value="${esc(L.gradient.colorA)}" data-layer-field="gradient.colorA" title="渐变色 A" />
              <input type="color" class="color-dot-input" value="${esc(L.gradient.colorB)}" data-layer-field="gradient.colorB" title="渐变色 B" />
              ${toggleSwitch("layer-grad-en", L.gradient.enabled, 'data-layer-toggle="gradient"')}
            </div>
          </div>
          <div class="studio-layer-sliders">
            <label><span>速度</span><input type="range" min="0" max="100" value="${Math.round(L.gradient.speed * 100)}" data-layer-range="gradient.speed" /></label>
            <label><span>强度</span><input type="range" min="0" max="100" value="${Math.round(L.gradient.opacity * 100)}" data-layer-range="gradient.opacity" /></label>
          </div>
        </div>

        <div class="studio-layer-card${L.audio.enabled ? " is-on" : ""}">
          <div class="row">
            <strong>2. WASAPI 音乐频谱</strong>
            <div class="row-start">
              <input type="color" class="color-dot-input" value="${esc(L.audio.color)}" data-layer-field="audio.color" title="频谱柱颜色" />
              ${toggleSwitch("layer-aud-en", L.audio.enabled, 'data-layer-toggle="audio"')}
            </div>
          </div>
          <div class="studio-layer-sliders">
            <label><span>灵敏</span><input type="range" min="0" max="100" value="${Math.round(L.audio.sensitivity * 100)}" data-layer-range="audio.sensitivity" /></label>
            <label><span>叠加</span><input type="range" min="0" max="100" value="${Math.round(L.audio.opacity * 100)}" data-layer-range="audio.opacity" /></label>
          </div>
        </div>

        <div class="studio-layer-card${L.ambilight.enabled ? " is-on" : ""}">
          <div class="row">
            <strong>3. 屏幕流光溢彩 (16×6)</strong>
            ${toggleSwitch("layer-ambi-en", L.ambilight.enabled, 'data-layer-toggle="ambilight"')}
          </div>
          <div class="studio-layer-sliders is-single">
            <label><span>融合度</span><input type="range" min="0" max="100" value="${Math.round(L.ambilight.opacity * 100)}" data-layer-range="ambilight.opacity" /></label>
          </div>
        </div>

        <div class="studio-layer-card${L.typing.enabled ? " is-on" : ""}">
          <div class="row">
            <strong>4. 全局打字霓虹涟漪</strong>
            <div class="row-start">
              <input type="color" class="color-dot-input" value="${esc(L.typing.color)}" data-layer-field="typing.color" title="涟漪颜色" />
              ${toggleSwitch("layer-typ-en", L.typing.enabled, 'data-layer-toggle="typing"')}
            </div>
          </div>
          <div class="studio-layer-sliders">
            <label><span>波速</span><input type="range" min="0" max="100" value="${Math.round(L.typing.speed * 100)}" data-layer-range="typing.speed" /></label>
            <label><span>亮度</span><input type="range" min="0" max="100" value="${Math.round(L.typing.opacity * 100)}" data-layer-range="typing.opacity" /></label>
          </div>
        </div>
      </div>`;
  } else {
    const srv = st.serverStatus || {};
    const pySample = buildExternalExamplePython(st.udpPort, st.httpPort);
    rightBodyHtml = `
      <div class="stack">
        <div class="row">
          <div class="stack-sm" style="flex:1">
            <label class="field-label" for="studio-udp-port">UDP 二进制/JSON 端口</label>
            <input class="input mono" id="studio-udp-port" type="number" min="1024" max="65535" value="${st.udpPort}" ${srv.running ? "disabled" : ""} />
          </div>
          <div class="stack-sm" style="flex:1">
            <label class="field-label" for="studio-http-port">HTTP / WebSocket 端口</label>
            <input class="input mono" id="studio-http-port" type="number" min="1024" max="65535" value="${st.httpPort}" ${srv.running ? "disabled" : ""} />
          </div>
        </div>

        <div class="studio-api-stats">
          <div class="spec-item"><dt>服务状态</dt><dd class="mono" id="studio-srv-state">${srv.running ? "运行中 (127.0.0.1)" : "已停止"}</dd></div>
          <div class="spec-item"><dt>已收帧 / 实时帧率</dt><dd class="mono" id="studio-srv-rate">${srv.packets || 0} 帧 · ${formatFps(srv.fps)} FPS</dd></div>
        </div>

        <div class="stack-sm">
          <div class="row">
            <span class="field-label">协议与调用示例（Python / Node / C++）</span>
            <button type="button" class="btn btn-secondary btn-sm" id="studio-copy-api-btn">${icon("copy", 12)}复制代码</button>
          </div>
          <pre class="hex-dump studio-api-sample">${esc(pySample)}</pre>
        </div>

        <p class="screen-foot-note">严格绑定 <span class="mono">127.0.0.1</span> 仅限本机进程访问；支持 <span class="mono">RMGB</span> 二进制帧（&lt;1ms 延迟）、<span class="mono">POST /api/rgb</span> 及 <span class="mono">ws://127.0.0.1:${st.httpPort}/ws</span>。</p>
      </div>`;
  }

  return `
    <div class="workbench is-studio">
      <section class="card">
        <div class="card-head">
          <div class="card-head-text">
            <h2 class="card-title">实时灯效舞台</h2>
            <p class="card-desc">${sub === "paint" ? "点击或拖过任意键帽实时填色，支持 4 组板载方案持久化" : "60 FPS 实时预览 · 开启推流直达 100 键与 20 颗顶部 Logo 灯"}</p>
          </div>
          ${livePillHtml}
        </div>
        ${renderKeyboardStage(device, hwState, {
          showLightPreview: true,
          paintKeys: sub === "paint",
          screenDataUrl: ui.screenDataUrl,
          customHint: stageHint,
        })}
        ${audioMeterHtml}
        ${leftFootHtml}
      </section>

      <section class="card${readOnly && sub !== "script" ? " is-readonly" : ""}">
        <div class="seg is-full">
          <button type="button" class="seg-btn" data-studio-sub="paint" aria-pressed="${sub === "paint"}">${icon("brush", 13)} 逐键涂色</button>
          <button type="button" class="seg-btn" data-studio-sub="script" aria-pressed="${sub === "script"}">${icon("code", 13)} 脚本编程</button>
          <button type="button" class="seg-btn" data-studio-sub="layers" aria-pressed="${sub === "layers"}">${icon("layers", 13)} 多图层</button>
          <button type="button" class="seg-btn" data-studio-sub="api" aria-pressed="${sub === "api"}">${icon("radio", 13)} 外部接口</button>
        </div>
        ${rightBodyHtml}
      </section>
    </div>`;
}

/* ---- 视图 7：关于软件、作者名片与在线更新中心 ---- */
function formatBytesMb(bytes) {
  if (!bytes || !Number.isFinite(bytes)) return "";
  return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
}

function formatIsoDate(iso) {
  if (!iso) return "";
  const m = /^(\d{4}-\d{2}-\d{2})/.exec(String(iso));
  return m ? m[1] : String(iso);
}

function formatReleaseNotes(md) {
  const src = String(md || "").trim();
  if (!src) return `<p class="notes-empty">暂无更新说明，点击下方按钮前往 GitHub Release 页面查看详情。</p>`;
  const lines = src.split(/\r?\n/);
  const out = [];
  let inList = false;
  const inlineFmt = (s) => esc(s)
    .replace(/\*\*(.+?)\*\*/g, "<strong>$1</strong>")
    .replace(/`([^`]+)`/g, '<code class="mono">$1</code>');

  for (const raw of lines) {
    const line = raw.trim();
    if (!line) {
      if (inList) { out.push("</ul>"); inList = false; }
      continue;
    }
    if (/^#{1,3}\s+/.test(line)) {
      if (inList) { out.push("</ul>"); inList = false; }
      out.push(`<h4>${inlineFmt(line.replace(/^#{1,3}\s+/, ""))}</h4>`);
    } else if (/^[-*•]\s+/.test(line) || /^\d+\.\s+/.test(line)) {
      if (!inList) { out.push("<ul>"); inList = true; }
      out.push(`<li>${inlineFmt(line.replace(/^(?:[-*•]|\d+\.)\s+/, ""))}</li>`);
    } else {
      if (inList) { out.push("</ul>"); inList = false; }
      out.push(`<p>${inlineFmt(line)}</p>`);
    }
  }
  if (inList) out.push("</ul>");
  return out.join("");
}

export function renderAbout(meta, update) {
  const version = meta?.version || "0.5.0";
  const author = meta?.author || "COMICFLY";
  const authorUrl = meta?.authorUrl || "https://github.com/COMICFLY";
  const repoUrl = meta?.repoUrl || "https://github.com/COMICFLY/redmagic-control";
  const releasesUrl = meta?.releasesUrl || `${repoUrl}/releases`;
  const issuesUrl = `${repoUrl}/issues`;

  const checking = Boolean(update?.checking);
  const info = update?.info || null;
  const err = update?.error || "";
  const autoCheck = update?.autoCheck !== false;

  let statusClass = "is-idle";
  let statusIcon = "info";
  let statusTitle = `当前运行版本 v${version}`;
  let statusDesc = "点击「立即检查更新」连接 GitHub Releases 检测最新版本";
  if (checking) {
    statusClass = "is-checking";
    statusIcon = "refresh";
    statusTitle = "正在检查更新…";
    statusDesc = "通过 Windows 原生 WinHTTP HTTPS 访问 api.github.com";
  } else if (err) {
    statusClass = "is-error";
    statusIcon = "info";
    statusTitle = "更新检查失败";
    statusDesc = err;
  } else if (info?.hasUpdate) {
    statusClass = "is-available";
    statusIcon = "arrowUpCircle";
    statusTitle = `发现新版本 v${info.latestVersion}`;
    const sizeText = info.assetSize ? ` · ${formatBytesMb(info.assetSize)}` : "";
    const dateText = info.publishedAt ? ` · ${formatIsoDate(info.publishedAt)}` : "";
    statusDesc = `当前 v${info.currentVersion} → 最新 v${info.latestVersion}${sizeText}${dateText}`;
  } else if (info) {
    statusClass = "is-latest";
    statusIcon = "check";
    statusTitle = `已是最新版本 (v${version})`;
    statusDesc = `最近检查：${info.checkedAt || "刚刚"} · 远端最新 ${info.tagName || `v${version}`}`;
  }

  const notesHtml = info?.releaseNotes
    ? formatReleaseNotes(info.releaseNotes)
    : `<h4>REDMAGIC Control v${esc(version)} 核心特性</h4>
       <ul>
         <li><strong>纯 Rust + Tauri v2</strong>：移除 Node.js / Electron / C hidapi，单文件独立运行约 9.4 MB。</li>
         <li><strong>全三模底层通信</strong>：Win32 Overlapped HID + 2.4G 24B 分包唤醒 + SetupAPI 蓝牙 BLE 电量。</li>
         <li><strong>灯效编程工作室</strong>：板载逐键涂色 (OP 0x0a/0x0b) + 纯 Rust 零 WebView2 托盘挂起推流 (OP 0x12/0x28)。</li>
         <li><strong>320×172 小屏与在线更新</strong>：彩屏壁纸裁切烧录 (0xCE0000) + 原生 WinHTTP 自动更新提醒。</li>
       </ul>`;

  return `
    <div class="workbench is-about">
      <section class="card">
        <div class="about-hero">
          <div class="about-brand-badge" aria-hidden="true">R</div>
          <div class="about-hero-copy">
            <div class="about-title-row">
              <h2 class="about-app-name">REDMAGIC Control</h2>
              <span class="conn-badge is-wired mono">v${esc(version)}</span>
            </div>
            <p class="card-desc">红魔氘锋外设轻量原生桌面控制中心 · 开源免安装单文件</p>
            <div class="about-tech-pills mono">
              <span class="chip">Pure Rust</span>
              <span class="chip">Tauri v2</span>
              <span class="chip">Win32 Native HID / BLE</span>
              <span class="chip">Zero Telemetry</span>
            </div>
          </div>
        </div>

        <div class="author-card">
          <div class="author-avatar-wrap">
            <img class="author-avatar" src="assets/author-avatar.png" alt="${esc(author)}" draggable="false" />
          </div>
          <div class="author-info">
            <div class="author-name-row">
              <strong class="author-name">${esc(author)}</strong>
              <span class="author-role">Author · Maintainer</span>
            </div>
            <button type="button" class="btn btn-secondary btn-sm" data-open-url="${esc(authorUrl)}">
              ${icon("github", 13)}访问 GitHub 主页${icon("externalLink", 12)}
            </button>
          </div>
        </div>

        <div class="about-repo-box">
          <div class="row">
            <span class="field-label">开源项目仓库地址</span>
            <button type="button" class="btn btn-secondary btn-sm" id="about-copy-repo-btn">${icon("copy", 12)}复制链接</button>
          </div>
          <div class="about-repo-url mono">${esc(repoUrl)}</div>
          <div class="about-link-row">
            <button type="button" class="btn btn-primary btn-sm" data-open-url="${esc(repoUrl)}">${icon("github", 13)}项目主页</button>
            <button type="button" class="btn btn-secondary btn-sm" data-open-url="${esc(releasesUrl)}">${icon("download", 13)}历史发行版</button>
            <button type="button" class="btn btn-secondary btn-sm" data-open-url="${esc(issuesUrl)}">${icon("externalLink", 13)}反馈建议</button>
          </div>
        </div>

        <dl class="spec-list">
          <div class="spec-item"><dt>支持外设</dt><dd>GK001S 键盘 · GS001S 鼠标</dd></div>
          <div class="spec-item"><dt>原生通信栈</dt><dd class="mono">Win32 HID · BLE · WinHTTP</dd></div>
        </dl>
      </section>

      <section class="card">
        <div class="card-head">
          <div class="card-head-text">
            <h2 class="card-title">版本检查与更新</h2>
            <p class="card-desc">直连 GitHub Releases 检查最新发行版，发现新版本自动弹窗提醒</p>
          </div>
          <span class="chip mono">${info?.tagName ? `最新 ${esc(info.tagName)}` : `v${esc(version)}`}</span>
        </div>

        <div class="update-status-banner ${statusClass}">
          <div class="update-status-ico${checking ? " is-spin" : ""}">${icon(statusIcon, 20)}</div>
          <div class="update-status-copy">
            <strong>${esc(statusTitle)}</strong>
            <span>${esc(statusDesc)}</span>
          </div>
        </div>

        <div class="about-update-actions">
          <button type="button" class="btn btn-primary" id="about-check-update-btn" ${checking ? "disabled" : ""} style="flex:1">
            ${icon("refresh", 14)}<span>${checking ? "正在连接 GitHub 检查更新…" : "立即检查更新"}</span>
          </button>
          ${info?.hasUpdate ? `
            <button type="button" class="btn btn-danger" id="about-open-modal-btn">${icon("arrowUpCircle", 14)}查看更新弹窗</button>
          ` : `
            <button type="button" class="btn btn-secondary" data-open-url="${esc(releasesUrl)}">${icon("externalLink", 14)}Releases</button>
          `}
        </div>

        <div class="toggle-row about-auto-row">
          <div class="toggle-copy">
            <strong>启动时自动检查更新</strong>
            <small>每次打开软件后在后台静默检查 GitHub 最新版本并弹窗提示</small>
          </div>
          ${toggleSwitch("about-auto-update", autoCheck, 'id="about-auto-update-toggle"')}
        </div>

        <div class="stack-sm about-notes-wrap">
          <div class="row">
            <span class="field-label">${info ? `发行说明 · ${esc(info.releaseName || info.tagName)}` : "版本亮点"}</span>
            ${info?.publishedAt ? `<span class="field-hint mono">${esc(formatIsoDate(info.publishedAt))}</span>` : ""}
          </div>
          <div class="release-notes-box">${notesHtml}</div>
        </div>
      </section>
    </div>`;
}

export function renderUpdateModal(update) {
  const info = update?.info;
  if (!info || !info.hasUpdate) return "";
  const dlUrl = info.downloadUrl || info.releaseUrl;
  const sizeBadge = info.assetSize ? `<span class="chip mono">${esc(formatBytesMb(info.assetSize))}</span>` : "";
  const dateBadge = info.publishedAt ? `<span class="chip mono">${esc(formatIsoDate(info.publishedAt))}</span>` : "";

  return `
    <div class="modal-card">
      <div class="modal-head">
        <div class="modal-head-left">
          <span class="modal-badge-ico">${icon("arrowUpCircle", 18)}</span>
          <div>
            <h2 class="modal-title" id="update-modal-title">发现新版本 ${esc(info.tagName)}</h2>
            <p class="card-desc">${esc(info.releaseName || "REDMAGIC Control 新版本已发布")}</p>
          </div>
        </div>
        <button type="button" class="icon-btn" id="update-modal-close" aria-label="关闭弹窗">${icon("x", 14)}</button>
      </div>

      <div class="modal-ver-bar">
        <div class="modal-ver-flow mono">
          <span class="chip">当前 v${esc(info.currentVersion)}</span>
          <span class="modal-arrow">→</span>
          <span class="conn-badge is-24g">最新 v${esc(info.latestVersion)}</span>
        </div>
        <div class="row-start">
          ${sizeBadge}
          ${dateBadge}
        </div>
      </div>

      <div class="release-notes-box is-modal">
        ${formatReleaseNotes(info.releaseNotes)}
      </div>

      <div class="modal-foot">
        <button type="button" class="btn btn-secondary btn-sm" id="update-modal-skip">跳过此版本</button>
        <div class="card-actions">
          <button type="button" class="btn btn-secondary btn-sm" id="update-modal-later">稍后提醒</button>
          <button type="button" class="btn btn-secondary btn-sm" data-open-url="${esc(info.releaseUrl)}">${icon("externalLink", 13)}查看发布页</button>
          <button type="button" class="btn btn-primary btn-sm" data-open-url="${esc(dlUrl)}">${icon("download", 13)}立即下载新版</button>
        </div>
      </div>
    </div>`;
}
