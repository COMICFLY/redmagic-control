/* 交互式外设舞台渲染：
 * - 精准裁剪 864×256 键盘图多余透明留白 → 680×248 实体机身（视觉放大 ~27%）；
 * - 100 键独立 RGB 键帽灯层 + 精准嵌入左上边框格栅的 24 段 Logo 灯条；
 * - 鼠标实体三灯区（滚轮 / 中段 V 型灯翼 / 尾部红魔标）+ 6 物理按键热点。 */

import { KB_STAGE } from "./kb-layout.js";
import { icon } from "./icons.js";

// GK001S 实体机身在 864×256 原图中的裁剪窗口（原图实测不透明区 97..766 × 8..247）
const KB_CROP = { x: 92, y: 4, w: 680, h: 248 };
const KB_TOP_OFFSET = 30; // 原布局坐标含 30px 顶部偏移

// GK001S 左上边框实体 Logo 格栅灯条在 864×256 原图中的精确像素位置（实测 26 格斜栅 x=123..353, y=15..19）
const LOGO_STRIP = { x: 123, y: 15, w: 230, h: 5, segments: 26 };

// GK001S 右上角 320×172 彩屏在 864×256 原图中的精确像素窗口（实测 x=637..693, y=33..65）
const MINI_SCREEN = { x: 637, y: 33, w: 57, h: 33 };

function esc(str) {
  return String(str ?? "").replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" }[c]));
}

export function renderBatteryGauge(pct, { size = "sm", showLabel = true, chargingState = null } = {}) {
  const valid = pct != null && Number.isFinite(Number(pct));
  const clamped = valid ? Math.max(0, Math.min(100, Math.round(Number(pct)))) : 0;
  const isCharging = chargingState === 1;
  const isFull = chargingState === 2;
  const tone = !valid ? "is-unknown" : isCharging ? "is-charging" : clamped < 15 ? "is-low" : clamped < 30 ? "is-warn" : "is-ok";
  const tip = !valid
    ? "电量未知"
    : isCharging
      ? `充电中 · ${clamped}%`
      : isFull
        ? `已充满 · ${clamped}%`
        : `电量 ${clamped}%`;

  const svgW = size === "lg" ? 36 : 24;
  const svgH = size === "lg" ? 20 : 13;
  const fillW = valid && clamped > 0 ? Math.max(1.6, (16.0 * clamped) / 100) : 0;
  const boltSize = size === "lg" ? 14 : 11;
  const boltHtml = isCharging
    ? `<span class="bat-bolt" aria-hidden="true">${icon("zap", boltSize)}</span>`
    : "";

  return `
    <span class="bat-gauge bat-${size} ${tone}" title="${esc(tip)}">
      ${boltHtml}
      <svg class="bat-svg" viewBox="0 0 26 14" width="${svgW}" height="${svgH}" fill="none" aria-hidden="true">
        <rect class="bat-outline" x="1" y="1.5" width="20.5" height="11" rx="2.5" stroke-width="1.5" />
        <rect class="bat-cap" x="22.8" y="4.5" width="2.2" height="5" rx="1.1" />
        ${fillW > 0 ? `<rect class="bat-fill-rect" x="3.25" y="3.75" width="${fillW.toFixed(2)}" height="6.5" rx="1.2" />` : ""}
      </svg>
      ${showLabel ? `<span class="bat-pct mono">${valid ? `${clamped}%` : "--"}</span>` : ""}
    </span>`;
}

export function renderConnectionBadge(mode, linkState = "online", { mock = false, compact = false } = {}) {
  if (mock) {
    const label = mode === "2.4g" ? (compact ? "仿真 2.4G" : "仿真 · 2.4G 无线")
      : mode === "bluetooth" ? (compact ? "仿真蓝牙" : "仿真 · 蓝牙 BLE")
      : (compact ? "仿真有线" : "仿真 · USB 有线");
    return `<span class="conn-badge is-mock">${icon(mode === "2.4g" ? "wireless" : mode === "bluetooth" ? "bluetooth" : "usb", 11)}<span>${esc(label)}</span></span>`;
  }
  if (mode === "2.4g" && linkState === "sleeping") {
    return `<span class="conn-badge is-sleeping" title="2.4G 接收器已插入，但键鼠处于休眠或未切至 2.4G 档">${icon("moon", 11)}<span>${compact ? "2.4G 休眠" : "2.4G 待机休眠"}</span></span>`;
  }
  if (mode === "2.4g") {
    return `<span class="conn-badge is-24g" title="2.4G 无线接收器已连接（24B RF 分包模式）">${icon("wireless", 11)}<span>${compact ? "2.4G" : "2.4G 无线"}</span></span>`;
  }
  if (mode === "bluetooth") {
    const ro = linkState === "bt-readonly";
    const text = compact ? "蓝牙" : (ro ? "蓝牙 BLE · 只读" : "蓝牙 BLE");
    const tip = ro ? "已通过蓝牙 BLE 连接（实时电量已同步，写配置需 2.4G 或 USB 有线）" : "蓝牙 BLE 连接";
    return `<span class="conn-badge is-bt" title="${esc(tip)}">${icon("bluetooth", 11)}<span>${esc(text)}</span></span>`;
  }
  return `<span class="conn-badge is-wired" title="USB 有线直连（56B 全速通道）">${icon("usb", 11)}<span>${compact ? "有线" : "USB 有线"}</span></span>`;
}

export function renderKeyboardStage(device, hwState, { interactiveKeys = false, selectedSlot = null, showLightPreview = false, large = false, screenDataUrl = null } = {}) {
  const kmBySlot = new Map((hwState?.keymap || []).map((k) => [k.slot, k]));

  // 原图在裁剪框内的相对缩放与偏移（纯百分比，任意窗口尺寸零漂移）
  const imgLeft = ((-KB_CROP.x / KB_CROP.w) * 100).toFixed(3);
  const imgTop = ((-KB_CROP.y / KB_CROP.h) * 100).toFixed(3);
  const imgW = ((KB_STAGE.width / KB_CROP.w) * 100).toFixed(3);
  const imgH = ((KB_STAGE.height / KB_CROP.h) * 100).toFixed(3);

  // 顶部 Logo 灯条精准覆盖实体左上格栅
  const lsLeft = (((LOGO_STRIP.x - KB_CROP.x) / KB_CROP.w) * 100).toFixed(3);
  const lsTop = (((LOGO_STRIP.y - KB_CROP.y) / KB_CROP.h) * 100).toFixed(3);
  const lsW = ((LOGO_STRIP.w / KB_CROP.w) * 100).toFixed(3);
  const lsH = ((LOGO_STRIP.h / KB_CROP.h) * 100).toFixed(3);
  const logoSegsHtml = Array.from({ length: LOGO_STRIP.segments }, () => '<i class="logo-seg"></i>').join("");

  // 右上角实体小屏画面同步叠加层
  const scLeft = (((MINI_SCREEN.x - KB_CROP.x) / KB_CROP.w) * 100).toFixed(3);
  const scTop = (((MINI_SCREEN.y - KB_CROP.y) / KB_CROP.h) * 100).toFixed(3);
  const scW = ((MINI_SCREEN.w / KB_CROP.w) * 100).toFixed(3);
  const scH = ((MINI_SCREEN.h / KB_CROP.h) * 100).toFixed(3);
  const miniScreenHtml = screenDataUrl
    ? `<img class="kb-mini-screen" src="${esc(screenDataUrl)}" alt="小屏预览" draggable="false" style="left:${scLeft}%;top:${scTop}%;width:${scW}%;height:${scH}%" />`
    : "";

  // 逐键 LED 灯光层（100 键独立精准对齐每个键帽）
  let ledLayerHtml = "";
  if (showLightPreview) {
    ledLayerHtml = KB_STAGE.keys.map((k) => {
      const lx = k.x - KB_CROP.x;
      const ly = k.y - KB_TOP_OFFSET - KB_CROP.y;
      const left = ((lx / KB_CROP.w) * 100).toFixed(3);
      const top = ((ly / KB_CROP.h) * 100).toFixed(3);
      const width = ((k.w / KB_CROP.w) * 100).toFixed(3);
      const height = ((k.h / KB_CROP.h) * 100).toFixed(3);
      const nx = Math.max(0, Math.min(1, lx / KB_CROP.w)).toFixed(3);
      const ny = Math.max(0, Math.min(1, ly / KB_CROP.h)).toFixed(3);
      return `<span class="kb-led" data-nx="${nx}" data-ny="${ny}" style="left:${left}%;top:${top}%;width:${width}%;height:${height}%"></span>`;
    }).join("");
  }

  // 交互式键位映射热点层（点击改键）
  let hitLayerHtml = "";
  if (interactiveKeys) {
    hitLayerHtml = KB_STAGE.keys.map((k) => {
      const row = kmBySlot.get(k.hwSlot);
      const isSel = selectedSlot === k.hwSlot;
      const isMod = row?.modified;
      const lx = k.x - KB_CROP.x;
      const ly = k.y - KB_TOP_OFFSET - KB_CROP.y;
      const left = ((lx / KB_CROP.w) * 100).toFixed(3);
      const top = ((ly / KB_CROP.h) * 100).toFixed(3);
      const width = ((k.w / KB_CROP.w) * 100).toFixed(3);
      const height = ((k.h / KB_CROP.h) * 100).toFixed(3);
      const tip = row ? `${k.name}（槽位 #${k.hwSlot}）· 当前：${row.current.label}` : k.name;
      return `<button type="button" class="kb-key-hit${isSel ? " is-selected" : ""}${isMod ? " is-modified" : ""}" data-select-slot="${k.hwSlot}" style="left:${left}%;top:${top}%;width:${width}%;height:${height}%" title="${esc(tip)}" aria-label="${esc(tip)}" aria-pressed="${isSel}"><span class="kb-hit-cap">${esc(isMod && row ? row.current.label : "")}</span></button>`;
    }).join("");
  }

  const knobRow = kmBySlot.get(125);
  const knobSelected = selectedSlot === 125;

  return `
    <div class="stage-box${large ? " is-large" : ""}">
      <div class="stage-kb-crop${large ? " is-large" : ""}">
        <img class="stage-kb-img" src="${esc(device.image)}" alt="${esc(device.name)}" draggable="false" style="left:${imgLeft}%;top:${imgTop}%;width:${imgW}%;height:${imgH}%" />
        ${showLightPreview ? `<div class="kb-led-layer" aria-hidden="true">${ledLayerHtml}</div>` : ""}
        <div class="kb-logo-strip" style="left:${lsLeft}%;top:${lsTop}%;width:${lsW}%;height:${lsH}%" aria-hidden="true">${logoSegsHtml}</div>
        ${miniScreenHtml}
        ${hitLayerHtml}
      </div>
    </div>
    <div class="stage-bar">
      <div class="stage-chips">
        ${renderConnectionBadge(hwState?.connectionMode || device.connectionMode, hwState?.linkState || device.linkState, { mock: device.mock, compact: true })}
        <span class="keycap">${esc(device.model)}</span>
        <span class="keycap">${esc(device.chip)}</span>
        <span class="keycap">PID 0x${(device.pid || 0).toString(16).padStart(4, "0")}</span>
        ${interactiveKeys && knobRow ? `<button type="button" class="keycap${knobSelected ? " is-selected" : ""}" data-select-slot="125" aria-pressed="${knobSelected}">旋钮 #125 · ${esc(knobRow.current.label)}</button>` : ""}
      </div>
      <span class="stage-hint">${interactiveKeys ? "直接点击放大键位即可编辑 · 红点表示已改键" : (hwState?.mock ? "仿真实时预览" : hwState?.linkState === "sleeping" ? "2.4G 设备休眠中" : hwState?.linkState === "bt-readonly" ? "蓝牙 BLE 预览" : "实时同步自设备")}</span>
    </div>
  `;
}

export function renderMouseStage(device, hwState, meta, { interactiveKeys = false, selectedSlot = null, showLightPreview = false } = {}) {
  const cfg = hwState?.config;
  const activeStage = cfg?.dpiStages?.[cfg?.performance?.activeDpi ?? 0];
  const kmBySlot = new Map((hwState?.keymap || []).map((k) => [k.slot, k]));
  const slots = meta?.mouseSlots || [];

  const pinsHtml = interactiveKeys ? slots.map((s) => {
    const row = kmBySlot.get(s.slot);
    const isSel = selectedSlot === s.slot;
    const isMod = row?.modified;
    const tip = row ? `${s.name} · 当前：${row.current.label}` : s.name;
    return `<button type="button" class="ms-pin mono${isSel ? " is-selected" : ""}${isMod ? " is-modified" : ""}" data-select-slot="${s.slot}" style="left:${s.x}%;top:${s.y}%" title="${esc(tip)}" aria-label="${esc(tip)}" aria-pressed="${isSel}">${esc(s.code)}</button>`;
  }).join("") : "";

  // 鼠标三处真实透光灯区（按 179×300 原图实测像素绘制矢量灯罩：滚轮 / 中段 V 型双翼 / 尾部红魔标）
  const ledZonesHtml = showLightPreview ? `
    <svg class="ms-led-layer" viewBox="0 0 179 300" aria-hidden="true">
      <defs>
        <filter id="ms-led-glow" x="-40%" y="-40%" width="180%" height="180%">
          <feGaussianBlur stdDeviation="2.6" result="blur" />
          <feMerge>
            <feMergeNode in="blur" />
            <feMergeNode in="SourceGraphic" />
          </feMerge>
        </filter>
      </defs>
      <!-- 1. 滚轮两侧透光缝（x=81..99, y=32..72） -->
      <g class="ms-led-zone ms-led-wheel" filter="url(#ms-led-glow)">
        <rect x="81" y="33" width="3.2" height="38" rx="1.6" fill="var(--zone-color)" />
        <rect x="96.8" y="33" width="3.2" height="38" rx="1.6" fill="var(--zone-color)" />
        <rect x="84.5" y="36" width="12" height="32" rx="4" fill="var(--zone-color)" fill-opacity="0.38" />
      </g>
      <!-- 2. 中段左右 V 型导光条（实测红魔导光槽 38,135 -> 78,154 与 143,135 -> 103,154） -->
      <g class="ms-led-zone ms-led-wing" filter="url(#ms-led-glow)">
        <path d="M 38 135.5 L 77 154" stroke="var(--zone-color)" stroke-width="4.2" stroke-linecap="round" fill="none" />
        <path d="M 143 135.5 L 104 154" stroke="var(--zone-color)" stroke-width="4.2" stroke-linecap="round" fill="none" />
      </g>
      <!-- 3. 尾部红魔 Logo 徽标（实测亮区 bbox x=79..102, y=240..266） -->
      <g class="ms-led-zone ms-led-logo" filter="url(#ms-led-glow)">
        <path d="M 80 245 L 86 241 L 89 250 L 92 250 L 95 241 L 101 245 L 98 257 L 90.5 266 L 83 257 Z" fill="var(--zone-color)" />
      </g>
    </svg>` : "";

  return `
    <div class="stage-box">
      <div class="stage-ms-frame">
        <img class="stage-ms-img" src="${esc(device.image)}" alt="${esc(device.name)}" draggable="false" />
        ${ledZonesHtml}
        ${pinsHtml}
      </div>
    </div>
    <div class="stage-bar">
      <div class="stage-chips">
        ${renderConnectionBadge(hwState?.connectionMode || device.connectionMode, hwState?.linkState || device.linkState, { mock: device.mock, compact: true })}
        <span class="keycap">${esc(device.model)}</span>
        <span class="keycap">${esc(device.chip)}</span>
        <span class="keycap">PID 0x${(device.pid || 0).toString(16).padStart(4, "0")}</span>
        ${activeStage ? `<span class="keycap"><i class="dpi-mini-dot" style="background:${esc(activeStage.color)}"></i>DPI ${activeStage.dpiX}</span>` : ""}
      </div>
      <span class="stage-hint">${interactiveKeys ? "点击鼠标按键标注进行映射" : (hwState?.mock ? "仿真实时预览" : hwState?.linkState === "sleeping" ? "2.4G 设备休眠中" : hwState?.linkState === "bt-readonly" ? "蓝牙 BLE 预览" : "实时同步自设备")}</span>
    </div>
  `;
}
