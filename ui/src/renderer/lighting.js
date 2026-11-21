/* 轻量实时灯光预览引擎（单层 Canvas 零 DOM 重排，支持键盘 100 键背光 + 26 段顶部 Logo 灯条 + 鼠标三灯区）。
 * 自动响应 prefers-reduced-motion 与关闭/静态模式（静态时零轮询开销）。 */

import { drawKeyboardLedCanvas } from "./stage.js";

function hexToHsl(hex) {
  const clean = String(hex || "#e53945").replace("#", "");
  const r = parseInt(clean.slice(0, 2), 16) / 255;
  const g = parseInt(clean.slice(2, 4), 16) / 255;
  const b = parseInt(clean.slice(4, 6), 16) / 255;
  const max = Math.max(r, g, b), min = Math.min(r, g, b);
  let h = 0, s = 0;
  const l = (max + min) / 2;
  if (max !== min) {
    const d = max - min;
    s = l > 0.5 ? d / (2 - max - min) : d / (max + min);
    if (max === r) h = ((g - b) / d + (g < b ? 6 : 0)) / 6;
    else if (max === g) h = ((b - r) / d + 2) / 6;
    else h = ((r - g) / d + 4) / 6;
  }
  const sat = s * 100;
  const lum = sat < 12 ? Math.max(78, l * 100) : Math.max(48, Math.min(62, l * 100));
  return [h * 360, sat, lum];
}

function hslCss(h, s, l) {
  return `hsl(${((h % 360) + 360) % 360 | 0} ${s | 0}% ${l | 0}%)`;
}

function pickColor(multi, hueOffset, baseHsl) {
  const [bh, bs, bl] = baseHsl;
  if (multi) return hslCss(hueOffset, 92, 56);
  return bs < 12 ? hslCss(0, 0, bl) : hslCss(bh, Math.max(bs, 78), bl);
}

/* 计算单个键盘按键在时刻 t 的颜色与不透明度（严格对齐 Goper.exe 0x44bb20 各模式色彩能力） */
const KB_FIXED_MULTI_MODES = new Set([3, 9, 11, 16, 17]); // 混彩 / 焰火 / 滚动 / 逐浪 / 极速：固件内置全彩光谱
const KB_SINGLE_ONLY_MODES = new Set([18]);               // 自定义：仅单色

function evalKbKey(mode, nx, ny, t, baseHsl, multi, maxOp, dir = 0) {
  if (mode === 19 || maxOp <= 0.01) return { color: "transparent", op: 0 };
  const effectiveMulti = KB_FIXED_MULTI_MODES.has(mode) ? true : KB_SINGLE_ONLY_MODES.has(mode) ? false : Boolean(multi);
  const hueAt = (offset) => pickColor(effectiveMulti, offset, baseHsl);
  const dirSign = dir ? -1 : 1;

  switch (mode) {
    case 0: // 波浪（水平流动）
      return { color: hueAt(nx * 360 - dirSign * t * 140), op: maxOp };
    case 1: // 彩云（斜向柔和渐变）
      return { color: hueAt((nx * 0.7 + ny * 0.6) * 320 - dirSign * t * 100), op: maxOp * (0.72 + 0.28 * Math.sin(nx * 5 - dirSign * t * 2.5)) };
    case 2: { // 环绕（中心旋转极角）
      const ang = (Math.atan2(ny - 0.5, nx - 0.5) * 180) / Math.PI;
      return { color: hueAt(ang - dirSign * t * 160), op: maxOp };
    }
    case 3: // 混彩（全局光谱缓慢流转）
      return { color: hslCss(t * 70, 90, 56), op: maxOp };
    case 4: { // 呼吸
      const wave = 0.16 + 0.84 * (0.5 + 0.5 * Math.sin(t * 2.4));
      return { color: hueAt(t * 45), op: maxOp * wave };
    }
    case 5: // 恒亮
    case 18: // 自定义
      return { color: hueAt(nx * 300), op: maxOp };
    case 6: { // 涟漪（中心扩散同心圆）
      const dist = Math.hypot((nx - 0.5) * 2.2, (ny - 0.5) * 1.4);
      const ring = 0.25 + 0.75 * (0.5 + 0.5 * Math.sin(dist * 10 - t * 5));
      return { color: hueAt(dist * 240 - t * 140), op: maxOp * ring };
    }
    case 8: // 游行
    case 11: // 滚动
    case 12: // 流光
    case 16: // 逐浪
    case 17: { // 极速
      const spd = mode === 17 ? 5.5 : 3.2;
      const band = Math.pow(0.5 + 0.5 * Math.sin(nx * 8 - dirSign * t * spd), 2);
      return { color: hueAt(nx * 300 - dirSign * t * 180), op: maxOp * (0.22 + 0.78 * band) };
    }
    case 15: { // 扫描（左右往返光束）
      const pos = 0.5 + 0.5 * Math.sin(t * 2.6);
      const d = Math.abs(nx - pos);
      const glow = Math.max(0, 1 - d * 5.5);
      return { color: hueAt(pos * 360), op: maxOp * (0.16 + 0.84 * glow) };
    }
    case 9: // 焰火
    case 10: // 星空
    case 13: { // 卡通
      const seed = Math.sin(nx * 91.7 + ny * 47.3) * 43758.5453;
      const phase = (seed - Math.floor(seed)) * Math.PI * 2;
      const tw = Math.pow(0.5 + 0.5 * Math.sin(t * 3.4 + phase), 3);
      return { color: hueAt(phase * 90 + t * 40), op: maxOp * (0.15 + 0.85 * tw) };
    }
    case 14: { // 雨滴（按列下落）
      const colPhase = Math.sin(nx * 67.3) * 10;
      const dropY = ((t * 0.9 + colPhase) % 1 + 1) % 1;
      const d = Math.abs(ny - dropY);
      const trail = d < 0.28 ? 1 - d / 0.28 : 0.14;
      return { color: hueAt(nx * 320 + t * 50), op: maxOp * trail };
    }
    default: // 触发等
      return { color: hueAt(nx * 280), op: maxOp * 0.88 };
  }
}

/* 计算顶部 26 段 Logo 灯条单段颜色（对齐 Goper.exe 0x44cc00：模式 1 光谱固定多彩，模式 3 常亮固定单色） */
function evalLogoSeg(mode, ratio, t, baseHsl, sevenColor, maxOp) {
  if (mode === 8 || mode === 4 || maxOp <= 0.01) return { color: "transparent", op: 0 };
  const effectiveMulti = mode === 1 ? true : mode === 3 ? false : Boolean(sevenColor);
  const col = (h) => pickColor(effectiveMulti, h, baseHsl);
  if (mode === 0) return { color: col(ratio * 320 - t * 180), op: maxOp }; // 随波逐流
  if (mode === 1) return { color: hslCss(t * 90, 92, 58), op: maxOp };     // 光谱（内置全彩）
  if (mode === 2) {                                                        // 呼吸
    const w = 0.18 + 0.82 * (0.5 + 0.5 * Math.sin(t * 2.5));
    return { color: col(t * 50), op: maxOp * w };
  }
  return { color: col(ratio * 260), op: maxOp };                           // 常亮（单色）
}

/* 计算鼠标 3 个实体灯区（对齐 Goper.exe 0x44e370：模式 0/1 固定多彩光谱，模式 2/3 固定指定单色） */
function evalMouseZone(mode, idx, t, baseHsl, multi, maxOp) {
  if (mode === 4 || maxOp <= 0.01) return { color: "transparent", op: 0 };
  const effectiveMulti = mode === 0 || mode === 1;
  const col = (h) => pickColor(effectiveMulti, h, baseHsl);
  if (mode === 0) return { color: col(idx * 95 - t * 150), op: maxOp };             // 随波逐流（内置光谱）
  if (mode === 1) return { color: hslCss(idx * 55 + t * 110, 92, 56), op: maxOp };  // 流光（内置光谱）
  if (mode === 2) {                                                                 // 呼吸（单色）
    const w = 0.18 + 0.82 * (0.5 + 0.5 * Math.sin(t * 2.5));
    return { color: col(t * 45), op: maxOp * w };
  }
  return { color: col(idx * 80), op: maxOp };                                       // 常亮（单色）
}

export class LightingAnimator {
  constructor() {
    this._raf = null;
    this._timer = null;
    this._reduced = window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  }

  stop() {
    if (this._timer) clearTimeout(this._timer);
    if (this._raf) cancelAnimationFrame(this._raf);
    this._timer = null;
    this._raf = null;
  }

  start(rootEl, device, hwState) {
    this.stop();
    if (!rootEl || !device || !hwState?.config) return;

    const kbCanvas = device.kind === "keyboard" ? rootEl.querySelector(".kb-led-canvas") : null;
    const kbCtx = kbCanvas ? kbCanvas.getContext("2d") : null;
    const showKeys = kbCanvas?.dataset.showKeys === "1";
    const msZones = device.kind === "mouse"
      ? [...rootEl.querySelectorAll(".ms-led-zone")].map((el) => ({ el, c: "", o: "" }))
      : [];
    if (!kbCtx && !msZones.length) return;

    const cfg = hwState.config;
    const kbHsl = device.kind === "keyboard" ? hexToHsl(cfg.light.color) : null;
    const logoHsl = device.kind === "keyboard" ? hexToHsl(cfg.logo.color) : null;
    const msHsl = device.kind === "mouse" ? hexToHsl(cfg.light.color) : null;

    // 键帽灯不透明度克制在 0.62 以内，清晰透出键帽字符
    const kbOp = device.kind === "keyboard" ? (cfg.light.brightness / 4) * 0.62 : 0;
    const logoOp = device.kind === "keyboard" ? (cfg.logo.brightness / 4) * 0.9 : 0;
    const msOp = device.kind === "mouse" ? (cfg.light.brightness / 4) * 0.86 : 0;

    const kbSpeedMul = device.kind === "keyboard" ? 0.55 + cfg.light.speed * 0.45 : 1;
    const logoSpeedMul = device.kind === "keyboard" ? 0.5 + cfg.logo.speed * 0.35 : 1;
    const msSpeedMul = device.kind === "mouse" ? 0.5 + cfg.light.speed * 0.3 : 1;

    const paint = (tSec) => {
      if (kbCtx) {
        const kbT = tSec * kbSpeedMul;
        const logoT = tSec * logoSpeedMul;
        drawKeyboardLedCanvas(
          kbCtx,
          showKeys,
          (k) => evalKbKey(cfg.light.mode, k.nx, k.ny, kbT, kbHsl, cfg.light.multiColor, kbOp, cfg.light.direction),
          (i, total) => evalLogoSeg(cfg.logo.mode, i / total, logoT, logoHsl, cfg.logo.sevenColor, logoOp),
        );
      } else {
        const msT = tSec * msSpeedMul;
        for (let i = 0; i < msZones.length; i++) {
          const item = msZones[i];
          const r = evalMouseZone(cfg.light.mode, i, msT, msHsl, cfg.light.multiColor, msOp);
          const opStr = r.op.toFixed(2);
          if (item.c !== r.color) { item.c = r.color; item.el.style.setProperty("--zone-color", r.color); }
          if (item.o !== opStr) { item.o = opStr; item.el.style.opacity = opStr; }
        }
      }
    };

    // 首帧立即渲染
    paint(0.6);

    const kbKeysStatic = !showKeys || cfg.light.mode === 5 || cfg.light.mode === 18 || cfg.light.mode === 19 || kbOp <= 0.01;
    const kbLogoStatic = cfg.logo.mode === 3 || cfg.logo.mode === 8 || logoOp <= 0.01;
    const isStatic = this._reduced
      || (device.kind === "keyboard" && kbKeysStatic && kbLogoStatic)
      || (device.kind === "mouse" && (cfg.light.mode === 3 || cfg.light.mode === 4 || msOp <= 0.01));
    if (isStatic) return;

    const startMs = performance.now();
    const scheduleNext = () => {
      if (document.hidden || document.body.classList.contains("is-tray-hidden")) return;
      const gap = document.hasFocus() ? 55 : 220; // 聚焦 ~18fps，失焦 ~4.5fps；帧间零 rAF 空转
      this._timer = setTimeout(() => {
        this._timer = null;
        if (document.hidden || document.body.classList.contains("is-tray-hidden")) return;
        this._raf = requestAnimationFrame((now) => {
          this._raf = null;
          if (document.hidden || document.body.classList.contains("is-tray-hidden")) return;
          paint((now - startMs) / 1000 + 0.6);
          scheduleNext();
        });
      }, gap);
    };
    scheduleNext();
  }
}
