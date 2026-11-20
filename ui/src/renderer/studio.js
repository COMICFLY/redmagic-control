/* 灯效编程工作室引擎：
 * 1. 逐键板载静态涂色（OP 0x0a / 0x0b，支持 4 组板载槽位与 FPS/MOBA/渐变模板）
 * 2. JS Shader 代码编程引擎（实时 60fps 预览 + 30fps 有线 / 8fps 2.4G 零损耗 RAM 推流 OP 0x12 + OP 0x28）
 * 3. 多图层混合器（动态渐变 + WASAPI 音乐频谱 + 屏幕流光溢彩 + 全局打字涟漪）
 * 4. 本地外部控灯桥（127.0.0.1 UDP 27650 + HTTP / WebSocket 27651）监控与示例 */

import { KB_STAGE } from "./kb-layout.js";

const KB_CROP = { x: 92, y: 4, w: 680, h: 248 };
const KB_TOP_OFFSET = 30;

// 预计算 100 颗实体按键的物理归一化坐标 (x, y) 与 hwSlot，供 60fps 零分配渲染
export const STUDIO_KEYS = KB_STAGE.keys.map((k) => {
  const lx = k.x - KB_CROP.x + k.w * 0.5;
  const ly = k.y - KB_TOP_OFFSET - KB_CROP.y + k.h * 0.5;
  return {
    slot: k.hwSlot,
    name: k.name,
    x: Math.max(0, Math.min(1, lx / KB_CROP.w)),
    y: Math.max(0, Math.min(1, ly / KB_CROP.h)),
  };
});

export const LOGO_LED_COUNT = 20;

export function hsvToRgb(h, s, v) {
  h = ((h % 360) + 360) % 360;
  s = Math.max(0, Math.min(1, s));
  v = Math.max(0, Math.min(1, v));
  const c = v * s;
  const x = c * (1 - Math.abs(((h / 60) % 2) - 1));
  const m = v - c;
  let r = 0, g = 0, b = 0;
  if (h < 60) [r, g, b] = [c, x, 0];
  else if (h < 120) [r, g, b] = [x, c, 0];
  else if (h < 180) [r, g, b] = [0, c, x];
  else if (h < 240) [r, g, b] = [0, x, c];
  else if (h < 300) [r, g, b] = [x, 0, c];
  else [r, g, b] = [c, 0, x];
  return [
    Math.round((r + m) * 255),
    Math.round((g + m) * 255),
    Math.round((b + m) * 255),
  ];
}

export function hexToRgb(hex) {
  const m = /^#?([0-9a-f]{6})$/i.exec(String(hex || "").trim());
  if (!m) return [229, 57, 69];
  const n = parseInt(m[1], 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
}

export function rgbToHex(r, g, b) {
  const f = (v) => Math.max(0, Math.min(255, Math.round(v))).toString(16).padStart(2, "0");
  return `#${f(r)}${f(g)}${f(b)}`;
}

function clamp(v, lo, hi) {
  return v < lo ? lo : v > hi ? hi : v;
}

function mixRgb(a, b, f) {
  const k = clamp(f, 0, 1);
  return [
    a[0] + (b[0] - a[0]) * k,
    a[1] + (b[1] - a[1]) * k,
    a[2] + (b[2] - a[2]) * k,
  ];
}

/* ---- 6 套内置可编程灯效脚本预设 ---- */
export const STUDIO_SCRIPTS = [
  {
    id: "aurora",
    name: "赛博极光波",
    sensors: { audio: false, ambilight: false, keys: false },
    p1: 0.55, p2: 0.5, p3: 0.15,
    labels: ["流动速度", "波浪密度", "色相偏移"],
    code: `// (x, y) ∈ [0,1] 按键坐标, t 运行秒数, ctx.p1/p2/p3 滑块
const spd = 1.2 + ctx.p1 * 4.5;
const freq = 3.0 + ctx.p2 * 8.0;
const w1 = Math.sin(x * freq - t * spd + y * 2.2);
const w2 = Math.cos((x * 0.7 - y * 1.4) * freq + t * spd * 0.7);
const hue = (ctx.p3 * 360 + x * 140 + (w1 + w2) * 38 + t * 28) % 360;
const val = 0.55 + 0.45 * (0.5 + 0.5 * w1);
return ctx.hsv(hue, 0.92, val);`,
  },
  {
    id: "radar",
    name: "红魔脉冲雷达",
    sensors: { audio: false, ambilight: false, keys: false },
    p1: 0.5, p2: 0.45, p3: 0.0,
    labels: ["扫描转速", "尾迹宽度", "主色色相"],
    code: `// 极坐标战术扫描雷达 + 顶部 Logo 条同步脉冲
if (ctx.isLogo) {
  const p = 0.5 + 0.5 * Math.sin(x * 6.28 - t * (2 + ctx.p1 * 6));
  return ctx.hsv(ctx.p3 * 360 + 354, 0.95, 0.22 + p * 0.78);
}
const dx = (x - 0.5) * 2.1, dy = y - 0.5;
const dist = Math.hypot(dx, dy);
const ang = Math.atan2(dy, dx);
const sweep = ((ang - t * (1.8 + ctx.p1 * 5)) % 6.2832 + 6.2832) % 6.2832;
const tail = Math.exp(-sweep / (0.45 + ctx.p2 * 1.6));
const ring = Math.max(0, 1 - Math.abs(dist - ((t * 0.7) % 1.2)) * 8) * 0.55;
const hue = (355 + ctx.p3 * 360) % 360;
return ctx.hsv(hue, 0.92, Math.min(1, 0.10 + tail * 0.92 + ring));`,
  },
  {
    id: "ripple",
    name: "打字霓虹涟漪",
    sensors: { audio: false, ambilight: false, keys: true },
    p1: 0.6, p2: 0.5, p3: 0.55,
    labels: ["扩散速度", "波环宽度", "涟漪色相"],
    code: `// 实时感知物理敲击（Win32 60Hz），触发全局扩散光环
let hit = ctx.keys.isPressed(ctx.slot) ? 1.0 : 0.0;
const spd = 0.55 + ctx.p1 * 1.2;
const w   = 0.06 + ctx.p2 * 0.14;
for (const p of ctx.keys.pulses) {
  const age = p.ageMs / 1000;
  const r = age * spd;
  const d = Math.hypot(x - p.x, (y - p.y) * 0.52);
  const ring = Math.exp(-Math.pow((d - r) / w, 2)) * Math.max(0, 1 - age * 0.9);
  hit = Math.max(hit, ring);
}
const base = ctx.hsv((354 + x * 25) % 360, 0.85, 0.12);
const glow = ctx.hsv((ctx.p3 * 360 + hit * 45) % 360, 0.9, Math.min(1, hit * 1.2));
return [Math.min(255, base[0] + glow[0]), Math.min(255, base[1] + glow[1]), Math.min(255, base[2] + glow[2])];`,
  },
  {
    id: "spectrum",
    name: "音乐频谱柱",
    sensors: { audio: true, ambilight: false, keys: false },
    p1: 0.65, p2: 0.45, p3: 0.05,
    labels: ["灵敏度", "底光亮度", "起始色相"],
    code: `// 实时 WASAPI 系统回环 16 段 FFT 频谱（播放音乐/游戏即刻跃动）
const gain = 0.7 + ctx.p1 * 1.4;
if (ctx.isLogo) {
  const lvl = Math.min(1, ctx.audio.rms * gain * 1.35);
  const lit = Math.abs(x - 0.5) * 2 <= lvl ? 1 : 0.08;
  return ctx.hsv((ctx.p3 * 360 + x * 120) % 360, 0.95, lit);
}
const bIdx = Math.min(15, Math.max(0, Math.floor(x * 16)));
const level = Math.min(1, (ctx.audio.bands[bIdx] || 0) * gain);
const rowH = 1 - y; // 底部=0, 顶部=1
if (rowH <= level + 0.06) {
  const hue = (ctx.p3 * 360 + bIdx * 14 + rowH * 45) % 360;
  return ctx.hsv(hue, 0.95, Math.min(1, 0.55 + level * 0.55));
}
return ctx.hsv((ctx.p3 * 360 + 210) % 360, 0.7, 0.05 + ctx.p2 * 0.14);`,
  },
  {
    id: "ambilight",
    name: "屏幕流光溢彩",
    sensors: { audio: false, ambilight: true, keys: false },
    p1: 0.75, p2: 0.4, p3: 0.0,
    labels: ["亮度增益", "饱和增强", "边缘微光"],
    code: `// 实时采样主显示器 16×6 色彩网格 + 顶部边缘映射到 20 颗 Logo 灯条
const c = ctx.isLogo ? ctx.ambilight.logoColor(ctx.logoIndex) : ctx.ambilight.sample(x, y);
const gain = 0.5 + ctx.p1 * 0.9;
const avg = (c[0] + c[1] + c[2]) / 3;
const sat = 1.0 + ctx.p2 * 0.9;
return [
  ctx.clamp((avg + (c[0] - avg) * sat) * gain + ctx.p3 * 25, 0, 255),
  ctx.clamp((avg + (c[1] - avg) * sat) * gain, 0, 255),
  ctx.clamp((avg + (c[2] - avg) * sat) * gain, 0, 255),
];`,
  },
  {
    id: "matrix",
    name: "矩阵数字雨",
    sensors: { audio: false, ambilight: false, keys: false },
    p1: 0.55, p2: 0.5, p3: 0.36,
    labels: ["下落速度", "尾迹长度", "雨滴色相"],
    code: `// 16 列异步下落的赛博数字雨（默认翡翠绿，拖动色相可切红魔赤雨）
const col = Math.floor(x * 16);
const hash = Math.sin(col * 127.1 + 311.7) * 43758.5453;
const phase = hash - Math.floor(hash);
const speed = 0.45 + ctx.p1 * 1.3 + (phase * 0.45);
const head = (t * speed + phase * 3.7) % 1.5;
const dist = head - y;
if (ctx.isLogo) {
  const pulse = Math.max(0, 1 - Math.abs(((t * 0.8 + x) % 1) - 0.5) * 4);
  return ctx.hsv(ctx.p3 * 360, 0.9, 0.15 + pulse * 0.85);
}
if (dist >= 0 && dist < 0.09) return [225, 255, 235];
const trailLen = 0.25 + ctx.p2 * 0.65;
if (dist >= 0 && dist < trailLen) {
  const v = Math.pow(1 - dist / trailLen, 1.6);
  return ctx.hsv(ctx.p3 * 360, 0.95, v);
}
return ctx.hsv(ctx.p3 * 360, 0.8, 0.04);`,
  },
];

/* ---- 逐键涂色快速模板（板载 OP 0x0b） ---- */
export function applyPaintTemplate(buf, templateId) {
  buf.fill(0);
  const setSlot = (slot, r, g, b) => {
    if (slot < 0 || slot >= 126) return;
    buf[slot * 3] = r;
    buf[slot * 3 + 1] = g;
    buf[slot * 3 + 2] = b;
  };
  if (templateId === "clear") return buf;

  if (templateId === "redmagic") {
    for (const k of STUDIO_KEYS) {
      const [r, g, b] = mixRgb([195, 22, 38], [32, 36, 54], k.x * 0.55 + k.y * 0.35);
      setSlot(k.slot, Math.round(r), Math.round(g), Math.round(b));
    }
    for (const s of [0, 14, 9, 15, 21, 35, 81, 94, 89, 95, 101]) setSlot(s, 255, 45, 62);
    return buf;
  }

  if (templateId === "fps") {
    for (const k of STUDIO_KEYS) setSlot(k.slot, 18, 28, 48);
    for (const s of [14, 9, 15, 21]) setSlot(s, 255, 48, 60); // W A S D
    for (const s of [7, 13, 19, 25, 31]) setSlot(s, 255, 158, 24); // 1-5 武器栏
    for (const s of [8, 20, 26, 27, 33, 4, 5, 35, 2]) setSlot(s, 0, 215, 255); // Q E R F G Shift Ctrl Space Tab
    setSlot(0, 255, 255, 255); // Esc
    return buf;
  }

  if (templateId === "moba") {
    for (const k of STUDIO_KEYS) setSlot(k.slot, 22, 16, 44);
    for (const s of [8, 14, 20, 26]) setSlot(s, 0, 232, 178); // Q W E R 技能
    for (const s of [21, 27]) setSlot(s, 255, 188, 24); // D F 召唤师技能
    for (const s of [7, 13, 19, 25, 31, 37, 43]) setSlot(s, 148, 98, 255); // 1-7 装备栏
    for (const s of [40, 35, 2]) setSlot(s, 229, 57, 69); // B Space Tab
    return buf;
  }

  if (templateId === "cyber") {
    for (const k of STUDIO_KEYS) {
      const [r, g, b] = hsvToRgb((195 + k.x * 155 + k.y * 28) % 360, 0.92, 0.95);
      setSlot(k.slot, r, g, b);
    }
    return buf;
  }

  if (templateId === "white") {
    for (const k of STUDIO_KEYS) setSlot(k.slot, 235, 240, 255);
    return buf;
  }
  return buf;
}

export function createInitialStudioState() {
  const paintRgb = new Uint8Array(126 * 3);
  applyPaintTemplate(paintRgb, "redmagic");
  const firstScript = STUDIO_SCRIPTS[0];
  return {
    subMode: "script", // "paint" | "script" | "layers" | "api"
    // 1. 逐键板载涂色
    paintRgb,
    paintGroup: 0,
    paintColor: "#e53945",
    paintTool: "brush", // "brush" | "eraser" | "eyedropper"
    paintLoaded: false,
    // 2. 代码编程
    scriptId: firstScript.id,
    scriptCode: firstScript.code,
    scriptError: "",
    p1: firstScript.p1,
    p2: firstScript.p2,
    p3: firstScript.p3,
    // 3. 多图层混合
    layers: {
      gradient: { enabled: true, colorA: "#e53945", colorB: "#0ea5e9", speed: 0.5, opacity: 0.75 },
      audio: { enabled: false, color: "#22c55e", sensitivity: 0.65, opacity: 0.9 },
      ambilight: { enabled: false, opacity: 0.85 },
      typing: { enabled: true, color: "#ffffff", speed: 0.6, opacity: 0.95 },
    },
    // 4. 实时推流与硬件控制
    streaming: false,
    streamSource: "script", // "script" | "layers"（后台推流源，切换页面/托盘挂起时持续驱动）
    includeLogo: true,
    brightness: 1.0,
    streamFps: 0,
    // 5. 外部接口服务
    udpPort: 27650,
    httpPort: 27651,
    serverStatus: { running: false, packets: 0, fps: 0, lastSource: "", error: null },
    // 6. 传感器实时遥测缓存
    telemetry: {
      audio: { active: false, bands: new Array(16).fill(0), rms: 0, bass: 0, mid: 0, treble: 0, error: null },
      ambilight: { active: false, width: 16, height: 6, grid: new Array(16 * 6 * 3).fill(0), logo: new Array(60).fill(0) },
      keys: { active: false, pressedSlots: [], pulses: [] },
      externalKeys: null,
      externalLogo: null,
    },
  };
}

export function buildExternalExamplePython(udpPort = 27650, httpPort = 27651) {
  return `import socket, time, math

# 1) 高速 UDP 二进制帧 (127.0.0.1:${udpPort})
sock = socket.socket(socket.AF_INET, socket.SOCK_DGRAM)
t0 = time.time()
while True:
    t = time.time() - t0
    keys = bytearray(126 * 3)
    for s in range(126):
        v = int((math.sin(s * 0.2 - t * 5) + 1) * 127)
        keys[s*3:s*3+3] = bytes([v, 30, 255 - v])
    logo = bytes([229, 57, 69] * 20)
    # 帧头: "RMGB" + flags(0x03) + 378B键 + 60B Logo
    sock.sendto(b"RMGB\\x03" + bytes(keys) + logo,
                ("127.0.0.1", ${udpPort}))
    time.sleep(1 / 30)

# 2) HTTP JSON: POST http://127.0.0.1:${httpPort}/api/rgb
#    body: {"fill": [229, 57, 69]}
# 3) WebSocket: ws://127.0.0.1:${httpPort}/ws`;
}

/* ---- 实时 60FPS 预览 + 后台持久非阻塞真机 RAM 推流调度器 ---- */
export class StudioController {
  constructor() {
    this.rafId = null;
    this.bgTimer = null;
    this.root = null;
    this.getState = null;
    this.bgGetState = null;
    this.bgOnStats = null;
    this.compiledFn = null;
    this.compiledSrc = "";
    this.keysBuf = new Uint8Array(126 * 3);
    this.logoBuf = new Uint8Array(LOGO_LED_COUNT * 3);
    this.streamKeysBuf = new Uint8Array(126 * 3);
    this.streamLogoBuf = new Uint8Array(LOGO_LED_COUNT * 3);
    this.lastPushTs = 0;
    this.pushInFlight = false;
    this.lastTelemTs = 0;
    this.telemInFlight = false;
    this.fpsCount = 0;
    this.fpsWindowStart = performance.now();
    this.t0 = performance.now();
    this.ledNodes = [];
    this.logoNodes = [];
    this.onStatsUpdate = null;
  }

  compileScript(code) {
    if (this.compiledSrc === code && this.compiledFn) {
      return { ok: true, fn: this.compiledFn };
    }
    try {
      const fn = new Function("x", "y", "t", "ctx", `"use strict";\n${code}`);
      // 用哑上下文干跑一帧检测基础引用错误
      const dummy = this._makeContext(null, new Set());
      const res = fn(0.5, 0.5, 0, dummy);
      if (!Array.isArray(res) || res.length < 3) {
        return { ok: false, error: "函数必须返回 [r, g, b] 三元数组（0~255）" };
      }
      this.compiledFn = fn;
      this.compiledSrc = code;
      return { ok: true, fn };
    } catch (err) {
      return { ok: false, error: String(err?.message || err) };
    }
  }

  attach(root, getState, onStatsUpdate) {
    this.detach();
    this.root = root;
    this.getState = getState;
    this.onStatsUpdate = onStatsUpdate;
    this.ledNodes = Array.from(root.querySelectorAll(".kb-led[data-slot]")).map((el) => ({
      el,
      slot: Number(el.dataset.slot),
    }));
    this.logoNodes = Array.from(root.querySelectorAll(".logo-seg"));
    this.tick();
  }

  // 仅分离 DOM 预览循环，绝不中断正在后台运行的真机 RAM 推流定时器
  detach() {
    if (this.rafId) {
      cancelAnimationFrame(this.rafId);
      this.rafId = null;
    }
    this.root = null;
    this.ledNodes = [];
    this.logoNodes = [];
  }

  // 后台独立定时器（16ms 节拍避免与 32ms 门限混叠）：在切页、切设备、最小化或托盘挂起时持续 30 FPS / 8 FPS 稳帧推流
  ensureBackgroundLoop(getState, onStatsUpdate) {
    this.bgGetState = getState;
    this.bgOnStats = onStatsUpdate;
    if (this.bgTimer !== null) return;
    this.bgTimer = setInterval(() => this._backgroundStep(), 16);
  }

  stopBackgroundLoop() {
    if (this.bgTimer !== null) {
      clearInterval(this.bgTimer);
      this.bgTimer = null;
    }
    this.fpsCount = 0;
  }

  _makeContext(studio, pressedSet) {
    const telem = studio?.telemetry || {};
    const audio = telem.audio || { bands: new Array(16).fill(0), rms: 0, bass: 0, mid: 0, treble: 0 };
    const ambi = telem.ambilight || { width: 16, height: 6, grid: [], logo: [] };
    const keys = telem.keys || { pressedSlots: [], pulses: [] };
    const aw = ambi.width || 16;
    const ah = ambi.height || 6;
    const agrid = ambi.grid || [];
    const alogo = ambi.logo || [];

    return {
      slot: 0,
      name: "",
      isLogo: false,
      logoIndex: 0,
      p1: studio?.p1 ?? 0.5,
      p2: studio?.p2 ?? 0.5,
      p3: studio?.p3 ?? 0.5,
      audio,
      ambilight: {
        sample(nx, ny) {
          if (!agrid.length) return [0, 0, 0];
          const gx = clamp(Math.floor(nx * aw), 0, aw - 1);
          const gy = clamp(Math.floor(ny * ah), 0, ah - 1);
          const o = (gy * aw + gx) * 3;
          return [agrid[o] || 0, agrid[o + 1] || 0, agrid[o + 2] || 0];
        },
        logoColor(idx) {
          if (!alogo.length) return [0, 0, 0];
          const o = clamp(idx, 0, 19) * 3;
          return [alogo[o] || 0, alogo[o + 1] || 0, alogo[o + 2] || 0];
        },
      },
      keys: {
        pressed: keys.pressedSlots || [],
        pulses: keys.pulses || [],
        isPressed: (s) => pressedSet.has(s),
      },
      hsv: hsvToRgb,
      mix: mixRgb,
      clamp,
    };
  }

  _computeLayersFrame(studio, t, pressedSet, keysOut, logoOut) {
    const L = studio.layers;
    const telem = studio.telemetry;
    const cA = hexToRgb(L.gradient.colorA);
    const cB = hexToRgb(L.gradient.colorB);
    const cAud = hexToRgb(L.audio.color);
    const cTyp = hexToRgb(L.typing.color);
    const gSpd = 0.6 + L.gradient.speed * 3.5;
    const aGain = 0.65 + L.audio.sensitivity * 1.45;
    const tSpd = 0.55 + L.typing.speed * 1.25;
    const ambi = telem.ambilight;
    const bands = telem.audio.bands || [];
    const pulses = telem.keys.pulses || [];

    const evalPoint = (x, y, slot, isLogo, logoIdx) => {
      let r = 0, g = 0, b = 0;

      // Layer 1: 动态渐变底色
      if (L.gradient.enabled && L.gradient.opacity > 0.01) {
        const f = 0.5 + 0.5 * Math.sin(x * 4.5 + y * 1.6 - t * gSpd);
        const m = mixRgb(cA, cB, f);
        const op = L.gradient.opacity;
        r += m[0] * op;
        g += m[1] * op;
        b += m[2] * op;
      }

      // Layer 2: 屏幕流光溢彩
      if (L.ambilight.enabled && L.ambilight.opacity > 0.01 && ambi.grid?.length) {
        let ar = 0, ag = 0, ab = 0;
        if (isLogo && ambi.logo?.length) {
          const o = clamp(logoIdx, 0, 19) * 3;
          ar = ambi.logo[o] || 0; ag = ambi.logo[o + 1] || 0; ab = ambi.logo[o + 2] || 0;
        } else {
          const gx = clamp(Math.floor(x * 16), 0, 15);
          const gy = clamp(Math.floor(y * 6), 0, 5);
          const o = (gy * 16 + gx) * 3;
          ar = ambi.grid[o] || 0; ag = ambi.grid[o + 1] || 0; ab = ambi.grid[o + 2] || 0;
        }
        const op = L.ambilight.opacity;
        r = r * (1 - op) + ar * op;
        g = g * (1 - op) + ag * op;
        b = b * (1 - op) + ab * op;
      }

      // Layer 3: WASAPI 音乐频谱叠加
      if (L.audio.enabled && L.audio.opacity > 0.01) {
        const op = L.audio.opacity;
        if (isLogo) {
          const lvl = Math.min(1, (telem.audio.rms || 0) * aGain * 1.3);
          if (Math.abs(x - 0.5) * 2 <= lvl) {
            r += cAud[0] * op; g += cAud[1] * op; b += cAud[2] * op;
          }
        } else {
          const bIdx = clamp(Math.floor(x * 16), 0, 15);
          const lvl = Math.min(1, (bands[bIdx] || 0) * aGain);
          if (1 - y <= lvl + 0.05) {
            const boost = (0.55 + lvl * 0.45) * op;
            r += cAud[0] * boost; g += cAud[1] * boost; b += cAud[2] * boost;
          }
        }
      }

      // Layer 4: 全局打字涟漪
      if (L.typing.enabled && L.typing.opacity > 0.01) {
        let hit = !isLogo && pressedSet.has(slot) ? 1.0 : 0.0;
        for (let i = 0; i < pulses.length; i++) {
          const p = pulses[i];
          const age = p.ageMs / 1000;
          const rad = age * tSpd;
          const d = Math.hypot(x - p.x, (y - p.y) * 0.55);
          const ring = Math.exp(-((d - rad) * (d - rad)) / 0.009) * Math.max(0, 1 - age * 0.85);
          if (ring > hit) hit = ring;
        }
        if (hit > 0.02) {
          const k = Math.min(1, hit) * L.typing.opacity;
          r += cTyp[0] * k; g += cTyp[1] * k; b += cTyp[2] * k;
        }
      }

      return [clamp(r, 0, 255), clamp(g, 0, 255), clamp(b, 0, 255)];
    };

    keysOut.fill(0);
    for (let i = 0; i < STUDIO_KEYS.length; i++) {
      const k = STUDIO_KEYS[i];
      const [r, g, b] = evalPoint(k.x, k.y, k.slot, false, 0);
      const o = k.slot * 3;
      keysOut[o] = r;
      keysOut[o + 1] = g;
      keysOut[o + 2] = b;
    }
    for (let li = 0; li < LOGO_LED_COUNT; li++) {
      const [r, g, b] = evalPoint(li / (LOGO_LED_COUNT - 1), 0, -1, true, li);
      logoOut[li * 3] = r;
      logoOut[li * 3 + 1] = g;
      logoOut[li * 3 + 2] = b;
    }
  }

  _computeFrame(studio, t, mode = studio.subMode, keysOut = this.keysBuf, logoOut = this.logoBuf) {
    const bri = clamp(studio.brightness ?? 1, 0.05, 1);

    if (mode === "paint") {
      keysOut.set(studio.paintRgb);
      const [pr, pg, pb] = hexToRgb(studio.paintColor);
      for (let i = 0; i < LOGO_LED_COUNT; i++) {
        logoOut[i * 3] = Math.round(pr * 0.65);
        logoOut[i * 3 + 1] = Math.round(pg * 0.65);
        logoOut[i * 3 + 2] = Math.round(pb * 0.65);
      }
      return;
    }

    if (mode === "api") {
      if (studio.telemetry.externalKeys?.length >= 378) {
        for (let i = 0; i < 378; i++) keysOut[i] = studio.telemetry.externalKeys[i];
      } else {
        keysOut.fill(0);
        const v = Math.round((0.18 + 0.14 * Math.sin(t * 2.5)) * 255);
        for (const k of STUDIO_KEYS) {
          keysOut[k.slot * 3] = v;
          keysOut[k.slot * 3 + 1] = Math.round(v * 0.22);
          keysOut[k.slot * 3 + 2] = Math.round(v * 0.35);
        }
      }
      if (studio.telemetry.externalLogo?.length >= 60) {
        for (let i = 0; i < 60; i++) logoOut[i] = studio.telemetry.externalLogo[i];
      }
      return;
    }

    const pressedSet = new Set(studio.telemetry.keys?.pressedSlots || []);
    if (mode === "layers") {
      this._computeLayersFrame(studio, t, pressedSet, keysOut, logoOut);
    } else {
      const comp = this.compileScript(studio.scriptCode);
      if (!comp.ok) {
        studio.scriptError = comp.error;
        return;
      }
      studio.scriptError = "";
      const fn = comp.fn;
      const ctx = this._makeContext(studio, pressedSet);
      keysOut.fill(0);
      try {
        for (let i = 0; i < STUDIO_KEYS.length; i++) {
          const k = STUDIO_KEYS[i];
          ctx.slot = k.slot;
          ctx.name = k.name;
          ctx.isLogo = false;
          ctx.logoIndex = 0;
          const out = fn(k.x, k.y, t, ctx);
          const o = k.slot * 3;
          keysOut[o] = clamp(Math.round(out[0] || 0), 0, 255);
          keysOut[o + 1] = clamp(Math.round(out[1] || 0), 0, 255);
          keysOut[o + 2] = clamp(Math.round(out[2] || 0), 0, 255);
        }
        for (let li = 0; li < LOGO_LED_COUNT; li++) {
          ctx.slot = -1;
          ctx.name = `LOGO_${li}`;
          ctx.isLogo = true;
          ctx.logoIndex = li;
          const out = fn(li / (LOGO_LED_COUNT - 1), 0, t, ctx);
          logoOut[li * 3] = clamp(Math.round(out[0] || 0), 0, 255);
          logoOut[li * 3 + 1] = clamp(Math.round(out[1] || 0), 0, 255);
          logoOut[li * 3 + 2] = clamp(Math.round(out[2] || 0), 0, 255);
        }
      } catch (err) {
        studio.scriptError = `运行时错误: ${err?.message || err}`;
        return;
      }
    }

    // 主亮度系数缩放
    if (bri < 0.999) {
      for (let i = 0; i < keysOut.length; i++) {
        keysOut[i] = Math.round(keysOut[i] * bri);
      }
      for (let i = 0; i < logoOut.length; i++) {
        logoOut[i] = Math.round(logoOut[i] * bri);
      }
    }
  }

  _paintDom() {
    for (let i = 0; i < this.ledNodes.length; i++) {
      const { el, slot } = this.ledNodes[i];
      const o = slot * 3;
      const r = this.keysBuf[o], g = this.keysBuf[o + 1], b = this.keysBuf[o + 2];
      if ((r | g | b) === 0) {
        el.style.opacity = "0.06";
        el.style.color = "rgb(40,44,56)";
      } else {
        el.style.opacity = "0.95";
        el.style.color = `rgb(${r},${g},${b})`;
      }
    }
    const nLogo = this.logoNodes.length;
    if (nLogo > 0) {
      for (let i = 0; i < nLogo; i++) {
        const li = Math.min(LOGO_LED_COUNT - 1, Math.floor((i * LOGO_LED_COUNT) / nLogo));
        const r = this.logoBuf[li * 3], g = this.logoBuf[li * 3 + 1], b = this.logoBuf[li * 3 + 2];
        const seg = this.logoNodes[i];
        if ((r | g | b) === 0) {
          seg.style.opacity = "0.08";
          seg.style.color = "rgb(40,44,56)";
        } else {
          seg.style.opacity = "0.95";
          seg.style.color = `rgb(${r},${g},${b})`;
        }
      }
    }
  }

  _pollTelemetryIfNeeded(studio, now, onStats) {
    const needTelem =
      studio.subMode === "api" ||
      studio.serverStatus?.running ||
      studio.telemetry.audio?.active ||
      studio.telemetry.ambilight?.active ||
      studio.telemetry.keys?.active ||
      (studio.subMode === "layers" && (studio.layers.audio.enabled || studio.layers.ambilight.enabled || studio.layers.typing.enabled)) ||
      (studio.subMode === "script" && (studio.scriptId === "spectrum" || studio.scriptId === "ambilight" || studio.scriptId === "ripple"));

    if (needTelem && !this.telemInFlight && now - this.lastTelemTs >= 38 && window.rm?.pollStudioTelemetry) {
      this.telemInFlight = true;
      this.lastTelemTs = now;
      window.rm.pollStudioTelemetry().then((res) => {
        if (res?.ok && res.data) {
          studio.telemetry.audio = res.data.audio || studio.telemetry.audio;
          studio.telemetry.ambilight = res.data.ambilight || studio.telemetry.ambilight;
          studio.telemetry.keys = res.data.keys || studio.telemetry.keys;
          studio.telemetry.externalKeys = res.data.externalKeys || null;
          studio.telemetry.externalLogo = res.data.externalLogo || null;
          if (res.data.server) studio.serverStatus = res.data.server;
          onStats?.(studio);
        }
      }).catch(() => {}).finally(() => {
        this.telemInFlight = false;
      });
    }
  }

  _pushFrameToHw(studio, dev, hw, now, onStats) {
    if (!studio.streaming || studio.serverStatus?.running || !dev || !window.rm?.streamRgbFrame) return;
    const is24g = (hw?.connectionMode || dev.connectionMode) === "2.4g";
    const intervalMs = is24g ? 124 : 31; // 有线 ~30 FPS / 2.4G ~8 FPS（与 16ms 节拍对齐不混叠）
    if (this.pushInFlight || now - this.lastPushTs < intervalMs) return;

    this.pushInFlight = true;
    this.lastPushTs = now;
    const srcMode = studio.streamSource === "layers" ? "layers" : "script";
    const t = (now - this.t0) / 1000;
    this._computeFrame(studio, t, srcMode, this.streamKeysBuf, this.streamLogoBuf);

    const keysArr = Array.from(this.streamKeysBuf);
    const logoArr = studio.includeLogo ? Array.from(this.streamLogoBuf) : null;
    window.rm.streamRgbFrame(dev.id, keysArr, logoArr).then((res) => {
      if (res?.ok) {
        this.fpsCount += 1;
        const elapsed = (performance.now() - this.fpsWindowStart) / 1000;
        if (elapsed >= 0.6) {
          studio.streamFps = Math.round((this.fpsCount / elapsed) * 10) / 10;
          this.fpsCount = 0;
          this.fpsWindowStart = performance.now();
          onStats?.(studio);
        }
      }
    }).catch(() => {}).finally(() => {
      this.pushInFlight = false;
    });
  }

  // 独立后台循环：切页、切至鼠标、最小化或托盘挂起后仍持续稳帧推流（不触发 DOM 绘制）
  _backgroundStep() {
    if (!this.bgGetState) return;
    const { studio, dev, hw } = this.bgGetState();
    if (!studio) return;
    if (!studio.streaming && !studio.serverStatus?.running) {
      this.stopBackgroundLoop();
      return;
    }
    const now = performance.now();
    this._pollTelemetryIfNeeded(studio, now, this.bgOnStats);
    if (studio.streaming && !studio.serverStatus?.running && dev) {
      this._pushFrameToHw(studio, dev, hw, now, this.bgOnStats);
    }
  }

  // 前台 60fps 舞台预览循环（仅在灯效编程页且窗口可见时运行）
  tick() {
    if (!this.root || !this.getState) return;
    const now = performance.now();
    const { studio, dev } = this.getState();
    if (!studio || !dev) return;

    if (!document.hidden) {
      this._pollTelemetryIfNeeded(studio, now, this.onStatsUpdate);
      const t = (now - this.t0) / 1000;
      this._computeFrame(studio, t, studio.subMode, this.keysBuf, this.logoBuf);
      this._paintDom();
    }

    this.rafId = requestAnimationFrame(() => this.tick());
  }
}
