/* 交互式外设舞台渲染：
 * - 精准裁剪 864×256 键盘图多余透明留白 → 680×248 实体机身（视觉放大 ~27%）；
 * - 100 键独立 RGB 键帽灯层 + 精准嵌入左上边框格栅的 24 段 Logo 灯条；
 * - 鼠标实体三灯区（滚轮 / 中段 V 型灯翼 / 尾部红魔标）+ 6 物理按键热点。 */

import { KB_STAGE } from "./kb-layout.js";
import { icon } from "./icons.js";

// GK001S 实体机身在 864×256 原图中的裁剪窗口（原图实测不透明区 97..766 × 8..247）
export const KB_CROP = { x: 92, y: 4, w: 680, h: 248 };
export const KB_TOP_OFFSET = 30; // 原布局坐标含 30px 顶部偏移

// GK001S 左上边框实体 Logo 格栅灯条在 864×256 原图中的精确像素位置（实测 26 格斜栅 x=123..353, y=15..19）
export const LOGO_STRIP = { x: 123, y: 15, w: 230, h: 5, segments: 26 };

// GK001S 右上角 320×172 彩屏在 864×256 原图中的精确像素窗口（实测 x=637..693, y=33..65）
const MINI_SCREEN = { x: 637, y: 33, w: 57, h: 33 };

// 预计算 100 颗实体按键在 680×248 Canvas 坐标系下的像素矩形与归一化坐标
export const KB_LED_RECTS = KB_STAGE.keys.map((k) => {
  const x = k.x - KB_CROP.x;
  const y = k.y - KB_TOP_OFFSET - KB_CROP.y;
  return {
    slot: k.hwSlot,
    x,
    y,
    w: k.w,
    h: k.h,
    nx: Math.max(0, Math.min(1, x / KB_CROP.w)),
    ny: Math.max(0, Math.min(1, y / KB_CROP.h)),
  };
});

// 预计算 26 段左上边框斜栅 Logo 灯条在 680×248 Canvas 坐标系下的平行四边形顶点
export const LOGO_SEG_POLYS = (() => {
  const n = LOGO_STRIP.segments;
  const baseX = LOGO_STRIP.x - KB_CROP.x;
  const baseY = LOGO_STRIP.y - KB_CROP.y;
  const gap = LOGO_STRIP.w * 0.009;
  const segW = (LOGO_STRIP.w - gap * (n - 1)) / n;
  const step = segW + gap;
  const h = LOGO_STRIP.h;
  const skewDx = (h * 0.5) * Math.tan((42 * Math.PI) / 180);
  const polys = [];
  for (let i = 0; i < n; i++) {
    const x0 = baseX + i * step;
    const x1 = x0 + segW;
    polys.push([
      x0 + skewDx, baseY,
      x1 + skewDx, baseY,
      x1 - skewDx, baseY + h,
      x0 - skewDx, baseY + h,
    ]);
  }
  return polys;
})();

/**
 * 单层 Canvas 渲染键盘 100 键 RGB 背光与 26 段顶部 Logo 斜栅灯条（零 DOM 节点重排/重绘）。
 * getKeyStyle(rect, idx) -> { color, op, fillAlpha? } | null
 * getLogoStyle(idx, total) -> { color, op } | null
 */
export function drawKeyboardLedCanvas(ctx, showKeys, getKeyStyle, getLogoStyle) {
  ctx.clearRect(0, 0, KB_CROP.w, KB_CROP.h);

  if (showKeys && getKeyStyle) {
    ctx.lineWidth = 1.65;
    const hasRoundRect = typeof ctx.roundRect === "function";
    for (let i = 0; i < KB_LED_RECTS.length; i++) {
      const k = KB_LED_RECTS[i];
      const st = getKeyStyle(k, i);
      if (!st || st.op <= 0.01) continue;
      const rx = k.x + 1;
      const ry = k.y + 1;
      const rw = Math.max(2, k.w - 2);
      const rh = Math.max(2, k.h - 2);
      ctx.beginPath();
      if (hasRoundRect) ctx.roundRect(rx, ry, rw, rh, 3.5);
      else ctx.rect(rx, ry, rw, rh);
      ctx.fillStyle = st.color;
      ctx.globalAlpha = st.fillAlpha != null ? st.fillAlpha : st.op * 0.24;
      ctx.fill();
      ctx.strokeStyle = st.color;
      ctx.globalAlpha = Math.min(1, st.op * 1.18);
      ctx.stroke();
    }
  }

  if (getLogoStyle) {
    const total = LOGO_SEG_POLYS.length;
    for (let i = 0; i < total; i++) {
      const st = getLogoStyle(i, total);
      if (!st || st.op <= 0.01) continue;
      const p = LOGO_SEG_POLYS[i];
      ctx.beginPath();
      ctx.moveTo(p[0], p[1]);
      ctx.lineTo(p[2], p[3]);
      ctx.lineTo(p[4], p[5]);
      ctx.lineTo(p[6], p[7]);
      ctx.closePath();
      ctx.fillStyle = st.color;
      ctx.globalAlpha = st.op;
      ctx.fill();
    }
  }

  ctx.globalAlpha = 1;
}

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

export function renderKeyboardStage(device, hwState, { interactiveKeys = false, paintKeys = false, selectedSlot = null, showLightPreview = false, large = false, screenDataUrl = null, customHint = null } = {}) {
  const kmBySlot = new Map((hwState?.keymap || []).map((k) => [k.slot, k]));

  // 原图在裁剪框内的相对缩放与偏移（纯百分比，任意窗口尺寸零漂移）
  const imgLeft = ((-KB_CROP.x / KB_CROP.w) * 100).toFixed(3);
  const imgTop = ((-KB_CROP.y / KB_CROP.h) * 100).toFixed(3);
  const imgW = ((KB_STAGE.width / KB_CROP.w) * 100).toFixed(3);
  const imgH = ((KB_STAGE.height / KB_CROP.h) * 100).toFixed(3);

  // 右上角实体小屏画面同步叠加层
  const scLeft = (((MINI_SCREEN.x - KB_CROP.x) / KB_CROP.w) * 100).toFixed(3);
  const scTop = (((MINI_SCREEN.y - KB_CROP.y) / KB_CROP.h) * 100).toFixed(3);
  const scW = ((MINI_SCREEN.w / KB_CROP.w) * 100).toFixed(3);
  const scH = ((MINI_SCREEN.h / KB_CROP.h) * 100).toFixed(3);
  const miniScreenHtml = screenDataUrl
    ? `<img class="kb-mini-screen" src="${esc(screenDataUrl)}" alt="小屏预览" draggable="false" style="left:${scLeft}%;top:${scTop}%;width:${scW}%;height:${scH}%" />`
    : "";

  // 交互式键位映射热点层（点击改键）或逐键涂色热区（拖拽涂色）
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
  } else if (paintKeys) {
    hitLayerHtml = KB_STAGE.keys.map((k) => {
      const lx = k.x - KB_CROP.x;
      const ly = k.y - KB_TOP_OFFSET - KB_CROP.y;
      const left = ((lx / KB_CROP.w) * 100).toFixed(3);
      const top = ((ly / KB_CROP.h) * 100).toFixed(3);
      const width = ((k.w / KB_CROP.w) * 100).toFixed(3);
      const height = ((k.h / KB_CROP.h) * 100).toFixed(3);
      const tip = `${k.name}（槽位 #${k.hwSlot}）· 左键涂色 / 右键擦除`;
      return `<button type="button" class="kb-paint-hit" data-paint-slot="${k.hwSlot}" style="left:${left}%;top:${top}%;width:${width}%;height:${height}%" title="${esc(tip)}" aria-label="${esc(tip)}"></button>`;
    }).join("");
  }

  const knobRow = kmBySlot.get(125);
  const knobSelected = selectedSlot === 125;

  return `
    <div class="stage-box${large ? " is-large" : ""}">
      <div class="stage-kb-crop${large ? " is-large" : ""}">
        <img class="stage-kb-img" src="${esc(device.image)}" alt="${esc(device.name)}" draggable="false" style="left:${imgLeft}%;top:${imgTop}%;width:${imgW}%;height:${imgH}%" />
        <canvas class="kb-led-canvas" width="${KB_CROP.w}" height="${KB_CROP.h}" data-show-keys="${showLightPreview ? 1 : 0}" aria-hidden="true"></canvas>
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
      <span class="stage-hint">${esc(customHint || (interactiveKeys ? "直接点击放大键位即可编辑 · 红点表示已改键" : (hwState?.mock ? "仿真实时预览" : hwState?.linkState === "sleeping" ? "2.4G 设备休眠中" : hwState?.linkState === "bt-readonly" ? "蓝牙 BLE 预览" : "实时同步自设备")))}</span>
    </div>
  `;
}

/**
 * 单层 Canvas 渲染鼠标 3 处真实透光灯区（滚轮两侧透光缝 / 中段 V 型双翼 / 尾部红魔 Logo 徽标）。
 * 与键盘灯效预览保持一致，零 DOM 重排并自带柔和霓虹光晕。
 * zoneStyles: [{ color, op }, { color, op }, { color, op }]
 */
export function drawMouseLedCanvas(ctx, zoneStyles) {
  ctx.clearRect(0, 0, 179, 300);
  if (!zoneStyles || zoneStyles.length < 3) return;
  const hasRoundRect = typeof ctx.roundRect === "function";
  const drawRRect = (x, y, w, h, r) => {
    ctx.beginPath();
    if (hasRoundRect) ctx.roundRect(x, y, w, h, r);
    else ctx.rect(x, y, w, h);
    ctx.fill();
  };

  // 1. 滚轮两侧透光缝（x=81..99, y=32..72）
  const z0 = zoneStyles[0];
  if (z0 && z0.op > 0.01) {
    ctx.save();
    ctx.fillStyle = z0.color;
    ctx.shadowColor = z0.color;
    ctx.shadowBlur = 7;
    ctx.globalAlpha = z0.op * 0.34;
    drawRRect(84.5, 36, 12, 32, 4);
    ctx.globalAlpha = z0.op;
    drawRRect(81, 33, 3.2, 38, 1.6);
    drawRRect(96.8, 33, 3.2, 38, 1.6);
    ctx.restore();
  }

  // 2. 中段左右 V 型导光条（38,135.5 -> 77,154 与 143,135.5 -> 104,154）
  const z1 = zoneStyles[1];
  if (z1 && z1.op > 0.01) {
    ctx.save();
    ctx.strokeStyle = z1.color;
    ctx.lineCap = "round";
    ctx.shadowColor = z1.color;
    ctx.shadowBlur = 8;
    ctx.beginPath();
    ctx.moveTo(38, 135.5);
    ctx.lineTo(77, 154);
    ctx.moveTo(143, 135.5);
    ctx.lineTo(104, 154);
    ctx.globalAlpha = z1.op * 0.32;
    ctx.lineWidth = 8.4;
    ctx.stroke();
    ctx.globalAlpha = z1.op;
    ctx.lineWidth = 4.2;
    ctx.stroke();
    ctx.restore();
  }

  // 3. 尾部红魔 Logo 徽标（bbox x=79..102, y=240..266）
  const z2 = zoneStyles[2];
  if (z2 && z2.op > 0.01) {
    ctx.save();
    ctx.fillStyle = z2.color;
    ctx.strokeStyle = z2.color;
    ctx.lineJoin = "round";
    ctx.shadowColor = z2.color;
    ctx.shadowBlur = 8;
    ctx.beginPath();
    ctx.moveTo(80, 245);
    ctx.lineTo(86, 241);
    ctx.lineTo(89, 250);
    ctx.lineTo(92, 250);
    ctx.lineTo(95, 241);
    ctx.lineTo(101, 245);
    ctx.lineTo(98, 257);
    ctx.lineTo(90.5, 266);
    ctx.lineTo(83, 257);
    ctx.closePath();
    ctx.globalAlpha = z2.op * 0.32;
    ctx.lineWidth = 3.6;
    ctx.stroke();
    ctx.globalAlpha = z2.op;
    ctx.fill();
    ctx.restore();
  }
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

  const ledZonesHtml = showLightPreview
    ? `<canvas class="ms-led-canvas" width="179" height="300" aria-hidden="true"></canvas>`
    : "";

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
