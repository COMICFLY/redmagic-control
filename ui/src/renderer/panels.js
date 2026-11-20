/* 四个核心功能视图渲染：概览与性能 / 灯光 / 按键映射 / 硬件诊断。
 * 所有控件均对应真实硬件协议字段，零无意义占位开关。 */

import { icon } from "./icons.js";
import { renderConnectionBadge, renderKeyboardStage, renderMouseStage } from "./stage.js";

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
    pickerHtml = `
      <div class="stack-sm">
        <button type="button" class="capture-btn${ui.capturingKey ? " is-listening" : ""}" id="capture-key-btn">
          ${icon("command", 15)}
          <span>${ui.capturingKey ? "请按下键盘上的目标按键…" : "点击后按物理按键快速录入"}</span>
        </button>
        <label class="field-label" for="key-preset-select">或从标准键表选择</label>
        <select class="select mono" id="key-preset-select">${opts}</select>
      </div>`;
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
