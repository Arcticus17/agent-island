// v10 — resident island: hover expansion, drag persistence, official Tauri API
import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import {
  getCurrentWindow,
  currentMonitor,
  primaryMonitor,
  availableMonitors,
  LogicalPosition,
  LogicalSize,
} from "@tauri-apps/api/window";
import { clampIslandX, getSafeIslandHeight, getSafeIslandWidth } from "./layout.js";

const inTauri = "__TAURI_INTERNALS__" in window;
const tauriWin = inTauri ? getCurrentWindow() : null;

const COLLAPSED_H = 36;
const EXPANDED_H = 570;
const POS_KEY = "agent-island-x";

let agents = [];
let cur = 0;
let sessionIdx = 0;
let expanded = false;
let collapseTimer = null;
let scale = 1;
const prevStatus = {};
let flashTimer = null;
let animateSwitchTimer = null;
let switching = false;
let confirmStopTimer = null;
let confirmingStop = false;
let confirmAgent = null;
let lastAgentName = "";
let focusMode = localStorage.getItem("agent-island-focus") || "off";
let pinnedAgents = [];
try { pinnedAgents = JSON.parse(localStorage.getItem("agent-island-pinned") || "[]") || []; } catch (_) {}
let agentOrder = [];
try { agentOrder = JSON.parse(localStorage.getItem("agent-island-order") || "[]") || []; } catch (_) {}
let themeOpacity = Number(localStorage.getItem("agent-island-theme-opacity")) || 92;
let themeRadius = Number(localStorage.getItem("agent-island-theme-radius")) || 26;
let themeWidth = Number(localStorage.getItem("agent-island-theme-width")) || 420;
let themeStyle = localStorage.getItem("agent-island-theme-style") || "mac";
let effectiveIslandWidth = themeWidth;
let effectiveExpandedHeight = EXPANDED_H;
let snapEnabled = localStorage.getItem("agent-island-theme-snap") === "1";
let showInTaskbar = localStorage.getItem("agent-island-theme-taskbar") === "1";
let privacyManual = localStorage.getItem("agent-island-privacy") === "1";
let privacyAuto = false;
let quietAuto = localStorage.getItem("agent-island-quiet-auto") === "1";
let quietStart = localStorage.getItem("agent-island-quiet-start") || "22:00";
let quietEnd = localStorage.getItem("agent-island-quiet-end") || "08:00";
const FIELD_KEYS = ["status", "cpu", "mem", "pid", "uptime", "last-active", "stats", "usage", "cwd", "file", "output"];
let visibleFields = new Set();
try { visibleFields = new Set(JSON.parse(localStorage.getItem("agent-island-fields") || "[]")); } catch (_) {}
if (!visibleFields.size) visibleFields = new Set(FIELD_KEYS);
let lastOutputKey = "";
let lastOutputLines = [];
let liveBadgeTimer = null;
let lastWheelAt = 0;
let eventCollapseTimer = null;
let longPressTimer = null;
let longPressStart = { x: 0, y: 0 };
const notifyCooldown = {};
const notifyGroups = new Map();

// DOM
const $ = (id) => document.getElementById(id);
const island = $("island");
const compactIcon = $("compact-icon");
const statusDot = $("status-dot");
const agentNameEl = $("agent-name");
const pageIndicator = $("page-indicator");
const navArrow = $("nav-arrow");
const expIcon = $("expanded-icon");
const expName = $("expanded-name");
const expStatus = $("exp-status");
const expPid = $("exp-pid");
const expCpu = $("exp-cpu");
const expMem = $("exp-mem");
const expUptime = $("exp-uptime");
const expCwd = $("exp-cwd");
const expLastActive = $("exp-last-active");
const expFile = $("exp-file");
const expOutput = $("exp-output");
const expStats = $("exp-stats");
const expUsage = $("exp-usage");
const usageBar = $("usage-bar");
const expPage = $("exp-page");
const btnPrev = $("btn-prev");
const btnNext = $("btn-next");
const btnDir = $("btn-dir");
const btnTerm = $("btn-term");
const btnStop = $("btn-stop");
const btnRestart = $("btn-restart");
const btnOverview = $("btn-overview");
const btnJump = $("btn-jump");
const btnHook = $("btn-hook");
const approvalStackEl = $("approval-stack");
const notifyStackEl = $("notify-stack");
const btnPrivacy = $("btn-privacy");
const btnFocus = $("btn-focus");
const privacyBadge = $("privacy-badge");
const iconStackEl = $("icon-stack");
const stackCountEl = $("stack-count");
const agentStripEl = $("agent-strip");
const btnFields = $("btn-fields");
const fieldsPop = $("fields-pop");
const liveBadge = $("live-badge");
const quickMenuEl = $("quick-menu");
const cardPreviewEl = $("card-preview");
const focusPop = $("focus-pop");
const quietAutoInput = $("quiet-auto-input");
const quietStartInput = $("quiet-start-input");
const quietEndInput = $("quiet-end-input");
const btnTheme = $("btn-theme");
const themePop = $("theme-pop");

const ICON_PATHS = {
  "Claude Code": "/icons/claude.svg",
  "Codex CLI": "/icons/codex.png",
  OpenCode: "/icons/opencode.png",
  Hermes: "/icons/hermes.png",
};

async function virtualBounds() {
  const monitors = await availableMonitors();
  if (!monitors.length) return null;
  let minX = Infinity;
  let maxX = -Infinity;
  for (const m of monitors) {
    const s = m.scaleFactor || 1;
    minX = Math.min(minX, m.position.x / s);
    maxX = Math.max(maxX, (m.position.x + m.size.width) / s);
  }
  return { minX, maxX };
}

function fallbackDisplaySize() {
  return {
    width: Math.max(window.screen?.availWidth || 0, window.innerWidth || 0),
    height: Math.max(window.screen?.availHeight || 0, window.innerHeight || 0),
  };
}

async function displaySize() {
  const fallback = fallbackDisplaySize();
  if (!tauriWin) return fallback;
  try {
    const monitor = await currentMonitor() || await primaryMonitor();
    if (!monitor) return fallback;
    const scaleFactor = monitor.scaleFactor || 1;
    return {
      width: monitor.workArea.size.width / scaleFactor,
      height: monitor.workArea.size.height / scaleFactor,
    };
  } catch (_) {
    return fallback;
  }
}

function applyIslandDimensions(width, height) {
  effectiveIslandWidth = width;
  effectiveExpandedHeight = height;
  document.documentElement.style.setProperty("--island-width", `${width}px`);
  document.documentElement.style.setProperty("--island-expanded-height", `${height}px`);
  if (tauriWin) {
    tauriWin.setSize(new LogicalSize(width, expanded ? height : COLLAPSED_H)).catch(() => {});
  }
}

async function syncIslandDimensions() {
  const size = await displaySize();
  applyIslandDimensions(
    getSafeIslandWidth(themeWidth, size.width),
    getSafeIslandHeight(EXPANDED_H, size.height),
  );
}

// Position: restore saved absolute X, clamped to the virtual desktop
async function positionIsland() {
  if (!tauriWin) return;
  try {
    const saved = Number(localStorage.getItem(POS_KEY));
    const bounds = await virtualBounds();
    if (Number.isFinite(saved) && bounds) {
      const x = clampIslandX(saved, bounds.minX, bounds.maxX, effectiveIslandWidth);
      await tauriWin.setPosition(new LogicalPosition(x, 0));
      return;
    }
    const mon = await primaryMonitor();
    if (!mon) return;
    scale = mon.scaleFactor || 1;
    const defaultX = Math.max(0, Math.floor((mon.workArea.size.width / scale - effectiveIslandWidth) / 2));
    await tauriWin.setPosition(new LogicalPosition(defaultX, 0));
  } catch (_) {}
}

// Horizontal-only drag
let dragging = false;
let startX = 0;
let winStartX = 0;
let dragBounds = null;
let dragReady = false;

island.addEventListener("mousedown", (e) => {
  if (e.target.closest("button")) return;
  if (e.target.closest("#exp-output")) return;
  if (e.target.closest(".path-link")) return;
  if (!tauriWin) return;
  dragging = true;
  dragReady = false;
  startX = e.screenX;
  Promise.all([tauriWin.scaleFactor(), tauriWin.outerPosition(), virtualBounds()])
    .then(([s, pos, bounds]) => {
      if (!dragging) return;
      scale = s;
      winStartX = pos.x / s;
      dragBounds = bounds;
      dragReady = true;
    })
    .catch(() => {
      if (!dragging) return;
      winStartX = 0;
      dragBounds = null;
      dragReady = true;
    });
});

window.addEventListener("mousemove", async (e) => {
  if (!dragging || !dragReady || !tauriWin) return;
  const dx = e.screenX - startX;
  try {
    const x = dragBounds
      ? clampIslandX(winStartX + dx, dragBounds.minX, dragBounds.maxX, effectiveIslandWidth)
      : winStartX + dx;
    await tauriWin.setPosition(new LogicalPosition(x, 0));
  } catch (_) {}
});

async function stopDrag() {
  if (!dragging || !tauriWin) return;
  dragging = false;
  if (!dragReady) {
    dragBounds = null;
    return;
  }
  try {
    scale = await tauriWin.scaleFactor();
    const pos = await tauriWin.outerPosition();
    let x = pos.x / scale;
    const bounds = dragBounds || await virtualBounds();
    if (bounds) x = clampIslandX(x, bounds.minX, bounds.maxX, effectiveIslandWidth);
    if (snapEnabled) {
      const mon = await primaryMonitor();
      if (mon) {
        const sw = mon.size.width / scale;
        const w = effectiveIslandWidth;
        const targets = [0, Math.max(0, (sw - w) / 2), Math.max(0, sw - w)];
        x = targets.reduce((a, b) => (Math.abs(b - x) < Math.abs(a - x) ? b : a));
      }
    }
    if (bounds) x = clampIslandX(x, bounds.minX, bounds.maxX, effectiveIslandWidth);
    await tauriWin.setPosition(new LogicalPosition(x, 0));
    localStorage.setItem(POS_KEY, String(x));
  } catch (_) {
  } finally {
    dragBounds = null;
    dragReady = false;
  }
}

window.addEventListener("mouseup", stopDrag);
window.addEventListener("blur", stopDrag);
let resizeTimer = null;
window.addEventListener("resize", () => {
  clearTimeout(resizeTimer);
  resizeTimer = setTimeout(() => {
    syncIslandDimensions();
    positionIsland();
  }, 120);
});

// Expand/collapse: resize the native window with the island so transparent
// space never blocks the desktop or triggers false hovers
function setExpanded(next) {
  if (next === expanded) return;
  expanded = next;
  clearTimeout(collapseTimer);
  island.classList.toggle("expanded", next);
  if (!tauriWin) return;
  if (next) {
    tauriWin.setSize(new LogicalSize(effectiveIslandWidth, effectiveExpandedHeight)).catch(() => {});
  } else {
    collapseTimer = setTimeout(() => {
      if (!expanded) {
        tauriWin.setSize(new LogicalSize(effectiveIslandWidth, COLLAPSED_H)).catch(() => {});
      }
    }, 360);
  }
}

island.addEventListener("mouseenter", () => setExpanded(true));
island.addEventListener("mouseleave", () => setExpanded(false));
island.addEventListener("wheel", (e) => {
  if (e.target.closest("#exp-output")) return;
  e.preventDefault();
  const now = Date.now();
  if (now - lastWheelAt < 300) return;
  lastWheelAt = now;
  if (e.deltaY > 0) nextAgent();
  else prevAgent();
}, { passive: false });
island.addEventListener("contextmenu", (e) => {
  if (e.target.closest("button")) return;
  e.preventDefault();
  showQuickMenu(e.clientX, e.clientY);
});
island.addEventListener("mousedown", (e) => {
  if (e.button !== 0) return;
  if (e.target.closest("button, .path-link, #exp-output")) return;
  clearTimeout(longPressTimer);
  longPressStart = { x: e.clientX, y: e.clientY };
  longPressTimer = setTimeout(() => showQuickMenu(e.clientX, e.clientY), 550);
});
window.addEventListener("mouseup", () => {
  clearTimeout(longPressTimer);
});
window.addEventListener("mousemove", (e) => {
  if (
    longPressTimer &&
    (Math.abs(e.clientX - longPressStart.x) > 5 ||
      Math.abs(e.clientY - longPressStart.y) > 5)
  ) {
    clearTimeout(longPressTimer);
  }
});

function applyTheme() {
  const dark = matchMedia("(prefers-color-scheme: dark)").matches;
  const o = (themeOpacity / 100).toFixed(2);
  const bg = dark ? `rgba(22, 22, 24, ${o})` : `rgba(250, 250, 252, ${o})`;
  const deepO = Math.min(1, themeOpacity / 100 + 0.05).toFixed(2);
  const bgDeep = dark ? `rgba(14, 14, 16, ${deepO})` : `rgba(238, 238, 243, ${deepO})`;
  const root = document.documentElement;
  const mac = themeStyle === "mac";
  document.body.classList.toggle("mac-style", mac);
  if (mac) {
    root.style.setProperty("--bg", "rgba(10, 10, 12, 0.97)");
    root.style.setProperty("--bg-deep", "rgba(6, 6, 8, 0.99)");
  } else {
    root.style.setProperty("--bg", bg);
    root.style.setProperty("--bg-deep", bgDeep);
  }
  root.style.setProperty("--radius-lg", `${themeRadius}px`);
  const fallback = fallbackDisplaySize();
  applyIslandDimensions(
    getSafeIslandWidth(themeWidth, fallback.width),
    getSafeIslandHeight(EXPANDED_H, fallback.height),
  );
  syncIslandDimensions();
  if (inTauri) tauriWin.setSkipTaskbar(!showInTaskbar).catch(() => {});
}

function syncThemePop() {
  themePop.querySelectorAll("[data-theme]").forEach((el) => {
    const key = el.dataset.theme;
    if (key === "opacity") el.value = themeOpacity;
    else if (key === "radius") el.value = themeRadius;
    else if (key === "width") el.value = themeWidth;
    else if (key === "snap") el.checked = snapEnabled;
    else if (key === "taskbar") el.checked = showInTaskbar;
  });
  themePop.querySelectorAll("[data-style]").forEach((el) => {
    el.checked = el.dataset.style === themeStyle;
  });
}

// Theme
function setTheme() {
  document.documentElement.setAttribute(
    "data-theme",
    matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
  );
  applyTheme();
}
setTheme();
matchMedia("(prefers-color-scheme: dark)").addEventListener("change", setTheme);

// Render
function refresh() {
  const activeStatuses = new Set(["working", "idle", "high_load", "error", "waiting", "done", "running"]);
  const hasActive = agents.some((a) => activeStatuses.has(a.status));
  const a = agents[cur] || null;
  if (a && sessionIdx >= (a.session_list?.length || 1)) sessionIdx = 0;
  const sess = a?.session_list?.[sessionIdx] || null;
  const multiSession = Boolean(a?.session_list?.length > 1);

  if (!a || !hasActive) {
    updateDot("gray");
    agentNameEl.textContent = "无运行中";
    navArrow.style.display = "none";
    pageIndicator.style.display = "none";
    compactIcon.style.display = "none";
    compactIcon.removeAttribute("src");
  } else {
    navArrow.style.display = agentIndexes().length > 1 ? "inline-block" : "none";
    updateDot(a.status);
    agentNameEl.textContent = a.name;
    if (agents.length > 1) {
      pageIndicator.style.display = "inline-block";
      pageIndicator.textContent = `${cur + 1}/${agents.length}`;
    } else {
      pageIndicator.style.display = "none";
    }
  }

  if (!a) {
    expIcon.style.display = "none";
    expIcon.removeAttribute("src");
    expName.textContent = "-";
    expStatus.textContent = "⚪";
    expPid.textContent = "-";
    expCpu.textContent = "-";
    expMem.textContent = "-";
    expUptime.textContent = "-";
    expCwd.textContent = "-";
    expLastActive.textContent = "-";
    expFile.textContent = "-";
    expOutput.innerHTML = "";
    lastOutputKey = "";
    lastOutputLines = [];
    liveBadge.classList.add("hidden");
    expStats.textContent = "-";
    expUsage.textContent = "-";
    if (usageBar) { usageBar.style.width = "0"; usageBar.title = ""; }
    island.classList.remove("alert-error", "flash-error");
    island.classList.remove("working-glow", "status-waiting", "status-done");
    statusDot.classList.remove("pulse");
    expPage.textContent = "";
    btnPrev.disabled = true;
    btnNext.disabled = true;
    btnDir.disabled = true;
    btnTerm.disabled = true;
    btnJump.disabled = true;
    btnStop.disabled = true;
    btnRestart.disabled = true;
    updateIconStack();
    updateAgentStrip();
    if (confirmingStop) {
      confirmingStop = false;
      clearTimeout(confirmStopTimer);
      btnStop.textContent = "停止";
      confirmAgent = null;
    }
    return;
  }

  const iconSrc = ICON_PATHS[a.name] || "";
  if (iconSrc) {
    compactIcon.src = iconSrc;
    compactIcon.alt = a.name;
    compactIcon.style.display = "inline-block";
    expIcon.src = iconSrc;
    expIcon.alt = a.name;
    expIcon.style.display = "inline-block";
  } else {
    compactIcon.style.display = "none";
    compactIcon.removeAttribute("src");
    expIcon.style.display = "none";
    expIcon.removeAttribute("src");
  }
  expName.textContent = multiSession && sess ? `${a.name} · ${sess.name}` : a.name;
  expStatus.textContent = { working: "🟢 工作中", idle: "🟡 等待中", high_load: "🟠 高负载", stopped: "🔴 已停止", error: "🔴 报错", waiting: "🟡 等待确认", done: "🟢 已完成" }[a.status] || "⚪";
  const pidText = a.pid != null ? String(a.pid) : "";
  const procText = a.sessions ? `${a.sessions} 进程` : "";
  expPid.textContent = [pidText, procText].filter(Boolean).join(" · ") || "-";
  expCpu.textContent = a.cpu != null ? `${a.cpu.toFixed(1)}%` : "-";
  expMem.textContent = a.memory != null ? `${a.memory.toFixed(0)} MB` : "-";
  expUptime.textContent = fmtUptime(a.uptime || 0);
  const useCwd = sess?.cwd || a.cwd;
  expCwd.textContent = useCwd || "-";
  expCwd.title = useCwd || "";
  expLastActive.textContent = a.status === "stopped" ? "-" : fmtAgo(a.last_active_secs ?? 0);
  expFile.textContent = sess?.current_file || a.current_file || "-";
  expFile.title = sess?.current_file || a.current_file || "";
  const recentLines = sess?.recent_output?.length ? sess.recent_output : a.recent_output;
  renderOutput(recentLines, `${a.name}|${sess?.id || ""}`);
  expStats.textContent = a.stats
    ? `${fmtUptime(a.stats.total_seconds)} · 报错${a.stats.error_count} · 完成${a.stats.done_count}`
    : "-";
  const usageText = usageTextFor(a.usage);
  expUsage.textContent = usageText || "-";
  expUsage.title = usageText || "";
  if (usageBar) {
    const bar = usageBarFor(a.usage);
    if (bar) {
      usageBar.style.width = `${bar.pct.toFixed(2)}%`;
      usageBar.style.background = bar.color;
      usageBar.title = usageText || "";
    } else {
      usageBar.style.width = "0";
      usageBar.title = "";
    }
  }
  const sessionCount = a.session_list?.length || 0;
  expPage.textContent = sessionCount > 1 ? `会话 ${sessionIdx + 1}/${sessionCount}` : "单一会话";
  btnPrev.disabled = sessionCount <= 1 || sessionIdx === 0;
  btnNext.disabled = sessionCount <= 1 || sessionIdx >= sessionCount - 1;
  const hasCwd = Boolean(useCwd && a.status !== "stopped");
  btnDir.disabled = !hasCwd;
  btnTerm.disabled = !hasCwd;
  btnJump.disabled = !hasCwd;
  btnStop.disabled = a.status === "stopped";
  btnRestart.disabled = !a.can_restart;
  if (confirmingStop && confirmAgent !== a.name) {
    confirmingStop = false;
    clearTimeout(confirmStopTimer);
    btnStop.textContent = "停止";
    confirmAgent = null;
  }
  if (a.status === "stopped" && confirmingStop) {
    confirmingStop = false;
    clearTimeout(confirmStopTimer);
    btnStop.textContent = "停止";
    confirmAgent = null;
  }

  const prev = prevStatus[a.name];
  prevStatus[a.name] = a.status;
  if (prev !== undefined && prev !== a.status && !switching && !quietActive()) {
    const kind = eventKind(prev, a.status);
    if (kind) {
      pushNotify(a, kind, sess);
      if (kind === "error" || kind === "waiting") {
        const idx = agents.findIndex((x) => x.name === a.name);
        if (idx >= 0 && idx !== cur) {
          setTimeout(() => { cur = idx; sessionIdx = 0; refresh(); }, 60);
        }
      }
    }
  }
  if (prev !== a.status && !switching && !quietActive()) {
    expStatus.classList.remove("status-flash");
    void expStatus.offsetWidth;
    expStatus.classList.add("status-flash");
  }
  island.classList.toggle("alert-error", a.status === "error");
  island.classList.toggle("working-glow", a.status === "working");
  island.classList.toggle("status-waiting", a.status === "waiting");
  island.classList.toggle("status-done", a.status === "done");
  statusDot.classList.toggle("pulse", a.status === "working");
  updateIconStack();
  updateAgentStrip();
  const shownName = agentNameEl.textContent;
  if (shownName !== lastAgentName) {
    lastAgentName = shownName;
    agentNameEl.classList.remove("fade-swap");
    void agentNameEl.offsetWidth;
    agentNameEl.classList.add("fade-swap");
  }
  if (a.status === "error" && prev !== "error" && !quietActive()) {
    island.classList.add("flash-error");
    clearTimeout(flashTimer);
    flashTimer = setTimeout(() => island.classList.remove("flash-error"), 2400);
  }
}

function updateDot(s) { statusDot.className = "dot " + ({ working: "green", idle: "yellow", high_load: "yellow", running: "green", stopped: "red", error: "red", waiting: "yellow", done: "green" }[s] || "gray"); }
function fmtUptime(s) {
  if (s < 60) return `${Math.floor(s)}秒`;
  if (s < 3600) return `${Math.floor(s / 60)}分钟`;
  return `${Math.floor(s / 3600)}小时${Math.floor((s % 3600) / 60)}分`;
}
function fmtTokens(n) {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(2)}M`;
  if (n >= 1_000) return `${Math.round(n / 1000)}K`;
  return String(n);
}
function fmtReset(secs) {
  if (secs < 60) return "1分钟内";
  if (secs < 3600) return `${Math.floor(secs / 60)}分钟`;
  if (secs < 86400) return `${Math.floor(secs / 3600)}小时`;
  return `${Math.floor(secs / 86400)}天`;
}

function usageTextFor(u) {
  if (!u) return null;
  const parts = [];
  if (u.tokens_total > 0 || u.stale) {
    parts.push(`tokens ${fmtTokens(u.tokens_total)}`);
    if (u.tokens_output > 0) parts.push(`输出 ${fmtTokens(u.tokens_output)}`);
  }
  if (u.cost_usd != null) parts.push(`$${u.cost_usd.toFixed(2)}`);
  if (u.used_percent != null) parts.push(`额度 ${u.used_percent.toFixed(1)}%`);
  if (u.resets_at_secs != null) {
    const remain = Math.max(0, u.resets_at_secs * 1000 - Date.now()) / 1000;
    parts.push(`${fmtReset(remain)}后重置`);
  }
  if (u.unlimited === true) parts.push("无限额度");
  else if (u.credits != null) parts.push(`余额 $${u.credits}`);
  if (!parts.length) return null;
  if (u.stale && u.used_percent == null) parts.push("(窗口外)");
  return parts.join(" · ");
}

function usageBarFor(u) {
  if (!u) return null;
  let pct = null;
  let color = "rgba(48, 209, 88, 0.9)";
  if (u.used_percent != null) {
    pct = Math.min(100, Math.max(0, u.used_percent));
    color = pct >= 85 ? "rgba(255, 69, 58, 0.95)" : pct >= 60 ? "rgba(255, 214, 10, 0.95)" : "rgba(48, 209, 88, 0.9)";
  } else if (u.tokens_total > 0) {
    pct = Math.min(100, (u.tokens_total / 2_000_000) * 100);
    color = pct >= 80 ? "rgba(255, 69, 58, 0.95)" : pct >= 50 ? "rgba(255, 214, 10, 0.95)" : "rgba(48, 209, 88, 0.9)";
  }
  return pct == null ? null : { pct, color };
}
function fmtAgo(secs) {
  if (secs < 3) return "刚刚";
  if (secs < 60) return `${Math.floor(secs)}秒前`;
  if (secs < 3600) return `${Math.floor(secs / 60)}分钟前`;
  return `${Math.floor(secs / 3600)}小时前`;
}

function escapeHtml(s) {
  return String(s).replace(/[&<>"']/g, (c) => ({
    "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;",
  }[c]));
}

function dotClass(s) {
  return ({ working: "green", idle: "yellow", high_load: "yellow", running: "green", stopped: "red", error: "red", waiting: "yellow", done: "green" }[s] || "gray");
}

function updateIconStack() {
  iconStackEl.querySelectorAll("img.stack-icon").forEach((el) => el.remove());
  stackCountEl.classList.add("hidden");
  const stack = agents.filter((x) => x.status !== "stopped");
  if (!stack.length) stack.push(...agents.slice(0, 1));
  const curAgent = agents[cur];
  if (curAgent && ICON_PATHS[curAgent.name]) {
    compactIcon.src = ICON_PATHS[curAgent.name];
    compactIcon.alt = curAgent.name;
    compactIcon.style.display = "inline-block";
  } else {
    compactIcon.style.display = "none";
    compactIcon.removeAttribute("src");
  }
  const others = stack.filter((x) => x.name !== curAgent?.name).slice(0, 2);
  for (const agent of others) {
    const src = ICON_PATHS[agent.name];
    if (!src) continue;
    const img = document.createElement("img");
    img.className = "agent-icon stack-icon";
    img.src = src;
    img.alt = agent.name;
    img.title = agent.name;
    iconStackEl.insertBefore(img, stackCountEl);
  }
  const remaining = stack.length - 3;
  if (remaining > 0) {
    stackCountEl.textContent = "+" + remaining;
    stackCountEl.classList.remove("hidden");
  }
}

function updateAgentStrip() {
  const idxs = agentIndexes();
  const list = idxs.map((i) => agents[i]).filter(Boolean);
  agentStripEl.innerHTML = "";
  for (const agent of list) {
    const chip = document.createElement("button");
    chip.type = "button";
    chip.draggable = list.length > 1;
    chip.className = "agent-chip" + (agent.name === agents[cur]?.name ? " active" : "");
    chip.title = agent.name;
    chip.dataset.name = agent.name;
    const src = ICON_PATHS[agent.name] || "";
    chip.innerHTML = (src
      ? `<img class="agent-chip-icon" alt="" src="${src}">`
      : `<span class="agent-chip-letter">${escapeHtml((agent.name || "?")[0])}</span>`
    ) + `<span class="agent-chip-name">${escapeHtml(agent.name)}</span><span class="agent-chip-dot ${dotClass(agent.status)}"></span><span class="agent-chip-handle" title="拖拽排序">⋮⋮</span>`;
    chip.addEventListener("click", (e) => {
      e.stopPropagation();
      const idx = agents.findIndex((x) => x.name === agent.name);
      if (idx >= 0 && idx !== cur) {
        cur = idx;
        sessionIdx = 0;
        animateSwitch();
        refresh();
      }
    });
    chip.addEventListener("dragstart", (e) => {
      e.dataTransfer.setData("text/plain", agent.name);
      chip.classList.add("dragging");
      cardPreviewEl.classList.add("hidden");
    });
    chip.addEventListener("dragend", () => {
      chip.classList.remove("dragging", "drag-over");
    });
    chip.addEventListener("dragover", (e) => {
      e.preventDefault();
      chip.classList.add("drag-over");
    });
    chip.addEventListener("dragleave", () => chip.classList.remove("drag-over"));
    chip.addEventListener("drop", (e) => {
      e.preventDefault();
      e.stopPropagation();
      chip.classList.remove("drag-over");
      const dragged = e.dataTransfer.getData("text/plain");
      if (!dragged || dragged === agent.name) return;
      const order = agentOrder.filter((n) => n !== dragged);
      const targetIdx = order.indexOf(agent.name);
      order.splice(targetIdx < 0 ? order.length : targetIdx, 0, dragged);
      for (const a of agents) {
        if (!order.includes(a.name)) order.push(a.name);
      }
      agentOrder = order;
      localStorage.setItem("agent-island-order", JSON.stringify(agentOrder));
      refresh();
    });
    let previewTimer = null;
    const clearPreview = () => {
      clearTimeout(previewTimer);
      cardPreviewEl.classList.add("hidden");
    };
    chip.addEventListener("mousedown", (e) => {
      if (e.button !== 0) return;
      previewTimer = setTimeout(() => {
        const lines = (agent.recent_output || []).slice(-3).join("\n") || "暂无日志";
        cardPreviewEl.textContent = lines;
        const rect = chip.getBoundingClientRect();
        const previewWidth = Math.min(300, Math.max(180, effectiveIslandWidth - 16));
        const left = Math.max(8, Math.min(rect.left - 40, effectiveIslandWidth - previewWidth - 8));
        const top = Math.max(36, rect.bottom + 4);
        cardPreviewEl.style.left = `${left}px`;
        cardPreviewEl.style.top = `${top}px`;
        cardPreviewEl.classList.remove("hidden");
      }, 450);
    });
    chip.addEventListener("mouseup", clearPreview);
    chip.addEventListener("mouseleave", clearPreview);
    agentStripEl.appendChild(chip);
  }
}

function renderOutput(lines, key) {
  const next = lines && lines.length ? lines : ["暂无日志"];
  if (key !== lastOutputKey) {
    expOutput.innerHTML = "";
    lastOutputLines = [];
    lastOutputKey = key;
    liveBadge.classList.add("hidden");
  }
  if (next.length < lastOutputLines.length) {
    expOutput.innerHTML = "";
    lastOutputLines = [];
  }
  const added = next.slice(lastOutputLines.length);
  if (!added.length && next.length === lastOutputLines.length) return;
  const frag = document.createDocumentFragment();
  for (const line of added) {
    const div = document.createElement("div");
    div.className = "log-line";
    div.textContent = line;
    frag.appendChild(div);
  }
  expOutput.appendChild(frag);
  while (expOutput.children.length > 200) expOutput.firstChild.remove();
  lastOutputLines = next;
  expOutput.scrollTop = expOutput.scrollHeight;
  if (added.length) {
    liveBadge.classList.remove("hidden");
    clearTimeout(liveBadgeTimer);
    liveBadgeTimer = setTimeout(() => liveBadge.classList.add("hidden"), 2500);
  }
}

const FIELD_SECTIONS = {
  run: ["status", "cpu", "mem", "pid", "uptime", "last-active", "stats", "usage"],
  session: ["cwd"],
  output: ["file", "output"],
};

function syncFieldsPop() {
  document.querySelectorAll("[data-field-check]").forEach((cb) => {
    cb.checked = visibleFields.has(cb.dataset.fieldCheck);
  });
}

function applyFields() {
  localStorage.setItem("agent-island-fields", JSON.stringify([...visibleFields]));
  document.querySelectorAll("[data-field]").forEach((el) => {
    el.classList.toggle("hidden", !visibleFields.has(el.dataset.field));
  });
  for (const [section, keys] of Object.entries(FIELD_SECTIONS)) {
    const label = document.querySelector(`[data-section="${section}"]`);
    if (!label) continue;
    const anyVisible = keys.some((k) => visibleFields.has(k));
    label.classList.toggle("hidden", !anyVisible);
  }
  syncFieldsPop();
}

function eventKind(prev, status) {
  if (status === "error") return "error";
  if (status === "waiting") return "waiting";
  const activeBefore = ["working", "high_load", "waiting", "idle", "running"].includes(prev);
  if (status === "done" && activeBefore) return "done";
  return null;
}

function canNotify(agent, kind) {
  const key = `${agent.name}:${kind}`;
  const now = Date.now();
  if (notifyCooldown[key] && now - notifyCooldown[key] < 20000) return false;
  notifyCooldown[key] = now;
  return true;
}

const NOTIFY_LABEL = { error: "报错", done: "已完成", waiting: "等待确认" };

function pushNotify(agent, kind, sess) {
  if (quietActive()) return;
  if (focusMode === "errors" && kind !== "error") return;
  if (!canNotify(agent, kind)) return;
  const label = NOTIFY_LABEL[kind] || kind;
  let group = notifyGroups.get(agent.name);
  if (!group) {
    group = { agent, items: [], timer: null, open: false };
    notifyGroups.set(agent.name, group);
  }
  group.items.push({ kind, label, sessName: sess?.name || "" });
  if (group.items.length > 8) group.items.shift();
  clearTimeout(group.timer);
  group.timer = setTimeout(() => dismissNotifyGroup(agent.name), 6500);
  while (notifyGroups.size > 3) {
    dismissNotifyGroup(notifyGroups.keys().next().value);
  }
  if (!expanded) {
    setExpanded(true);
    clearTimeout(eventCollapseTimer);
    eventCollapseTimer = setTimeout(() => {
      if (!island.matches(":hover") && !notifyGroups.size) setExpanded(false);
    }, 8000);
  }
  renderNotifyGroups();
}

function dismissNotifyGroup(name) {
  const group = notifyGroups.get(name);
  if (!group) return;
  clearTimeout(group.timer);
  notifyGroups.delete(name);
  renderNotifyGroups();
  if (!notifyGroups.size && !island.matches(":hover")) {
    setExpanded(false);
  }
}

function renderNotifyGroups() {
  notifyStackEl.innerHTML = "";
  for (const [name, group] of notifyGroups) {
    const card = document.createElement("div");
    card.className = "notify-group" + (group.open ? " open" : "");
    const icon = ICON_PATHS[name] || "";
    const latest = group.items[group.items.length - 1] || { kind: "done", label: "" };
    const severity = ["error", "waiting", "done"].find((k) =>
      group.items.some((it) => it.kind === k)
    ) || "done";
    card.innerHTML = `
      <div class="notify-head" role="button" tabindex="0" title="点开查看详情">
        ${icon
          ? `<img class="notify-icon" alt="" src="${icon}">`
          : `<span class="notify-letter">${escapeHtml((name || "?")[0])}</span>`}
        <span class="notify-title">${escapeHtml(name)}</span>
        <span class="notify-summary ${severity}">${escapeHtml(latest.label)}${group.items.length > 1 ? ` +${group.items.length - 1}` : ""}</span>
        <span class="notify-arrow">${group.open ? "▾" : "▸"}</span>
        <button class="notify-close" title="关闭" aria-label="关闭通知">×</button>
      </div>
      <div class="notify-items">
        ${group.items.map((it) => `
          <div class="notify-item ${it.kind}">
            <span class="notify-dot"></span>
            <span>${escapeHtml(it.label)}${it.sessName ? ` · ${escapeHtml(it.sessName)}` : ""}</span>
          </div>
        `).join("")}
      </div>
    `;
    const head = card.querySelector(".notify-head");
    head.addEventListener("click", (e) => {
      if (e.target.closest(".notify-close")) return;
      group.open = !group.open;
      card.classList.toggle("open", group.open);
    });
    head.addEventListener("keydown", (e) => {
      if (e.key === "Enter" || e.key === " ") {
        e.preventDefault();
        head.click();
      }
    });
    card.querySelector(".notify-close").addEventListener("click", (e) => {
      e.stopPropagation();
      dismissNotifyGroup(name);
    });
    notifyStackEl.appendChild(card);
  }
}

function applyPrivacy() {
  const on = privacyManual || privacyAuto;
  document.body.classList.toggle("privacy-mask", on);
  btnPrivacy.classList.toggle("active", on);
  privacyBadge.classList.toggle("hidden", !on);
}

let hookEnabled = false;
const pendingApprovals = new Map();

function refreshHookButton() {
  btnHook.classList.toggle("active", hookEnabled);
  btnHook.title = hookEnabled ? "Claude hooks 已接入，点击断开" : "接入 Claude hooks 事件与审批";
}

function renderApprovals() {
  approvalStackEl.innerHTML = "";
  for (const [id, a] of pendingApprovals) {
    const card = document.createElement("div");
    card.className = "approval-card";
    const tool = a.tool === "Bash" ? "命令" : a.tool || "操作";
    card.innerHTML = `
      <div class="approval-head">
        <span class="approval-title">Claude 待审批</span>
        <span class="approval-meta">${escapeHtml(a.session ? `会话 ${a.session.slice(0, 8)}` : "")}</span>
      </div>
      <div class="approval-body">${tool}：<code>${escapeHtml(a.command || "-")}</code></div>
      <div class="approval-actions">
        <button class="approval-allow" title="允许执行">允许</button>
        <button class="approval-deny" title="拒绝执行">拒绝</button>
      </div>
    `;
    card.querySelector(".approval-allow").addEventListener("click", () => {
      pendingApprovals.delete(id);
      renderApprovals();
      invoke("respond_hook_approval", { id, allow: true }).catch(() => {});
    });
    card.querySelector(".approval-deny").addEventListener("click", () => {
      pendingApprovals.delete(id);
      renderApprovals();
      invoke("respond_hook_approval", { id, allow: false }).catch(() => {});
    });
    approvalStackEl.appendChild(card);
  }
}

async function initHookEvents() {
  if (!inTauri) return;
  try { hookEnabled = await invoke("get_hook_status"); } catch (_) {}
  refreshHookButton();
  listen("hook-approval", (e) => {
    const a = e.payload || {};
    if (!a.id || pendingApprovals.has(a.id)) return;
    pendingApprovals.set(a.id, a);
    while (pendingApprovals.size > 3) {
      const evicted = pendingApprovals.keys().next().value;
      pendingApprovals.delete(evicted);
      invoke("dismiss_hook_approval", { id: evicted }).catch(() => {});
    }
    renderApprovals();
    setExpanded(true);
    clearTimeout(eventCollapseTimer);
    eventCollapseTimer = setTimeout(() => {
      if (!island.matches(":hover") && !notifyGroups.size && !pendingApprovals.size) {
        setExpanded(false);
      }
    }, 15000);
  }).catch(() => {});
  listen("hook-event", (e) => {
    const ev = e.payload || {};
    if (ev.kind === "stop") {
      poll();
    } else if (ev.kind === "notification" && ev.message) {
      const agent = agents.find((x) => x.name === "Claude Code");
      if (agent) pushNotify(agent, "waiting", null);
    }
  }).catch(() => {});
}

async function toggleHook() {
  if (!inTauri) return;
  try {
    const msg = await invoke("set_hook_enabled", { enabled: !hookEnabled });
    hookEnabled = !hookEnabled;
    refreshHookButton();
    console.log(msg);
  } catch (err) {
    console.error(err);
  }
}

function togglePrivacy() {
  privacyManual = !privacyManual;
  localStorage.setItem("agent-island-privacy", privacyManual ? "1" : "0");
  applyPrivacy();
}

function hideQuickMenu() {
  quickMenuEl.classList.add("hidden");
}

function showQuickMenu(x, y) {
  setExpanded(true);
  const left = Math.max(8, Math.min(x - 10, effectiveIslandWidth - 132 - 8));
  const top = Math.max(40, y + 4);
  quickMenuEl.style.left = `${left}px`;
  quickMenuEl.style.top = `${top}px`;
  quickMenuEl.classList.remove("hidden");
}

function minuteOfDay(s) {
  const [h, m] = String(s).split(":").map(Number);
  return (h || 0) * 60 + (m || 0);
}

function inQuietHours() {
  const now = new Date();
  const cur = now.getHours() * 60 + now.getMinutes();
  const s = minuteOfDay(quietStart);
  const e = minuteOfDay(quietEnd);
  return s <= e ? cur >= s && cur < e : cur >= s || cur < e;
}

function quietActive() {
  return focusMode === "quiet" || (quietAuto && inQuietHours());
}

const FOCUS_MODES = ["off", "errors", "quiet", "pinned"];
const FOCUS_LABEL = { off: "专注:关", errors: "专注:报错", quiet: "专注:静音", pinned: "专注:固定" };

function updateFocusButton() {
  btnFocus.textContent = FOCUS_LABEL[focusMode] || "专注";
  btnFocus.classList.toggle("active", focusMode !== "off" || quietActive());
  btnFocus.title = quietActive() ? "勿扰中，按 Q 关闭" : "专注模式";
}

function syncFocusPop() {
  focusPop.querySelectorAll("[data-focus]").forEach((b) => {
    b.classList.toggle("active", b.dataset.focus === focusMode);
  });
  quietAutoInput.checked = quietAuto;
  quietStartInput.value = quietStart;
  quietEndInput.value = quietEnd;
}

function cycleFocus() {
  let i = FOCUS_MODES.indexOf(focusMode);
  for (let step = 1; step <= FOCUS_MODES.length; step++) {
    const next = FOCUS_MODES[(i + step) % FOCUS_MODES.length];
    if (next === "pinned" && !pinnedAgents.length) continue;
    focusMode = next;
    break;
  }
  localStorage.setItem("agent-island-focus", focusMode);
  updateFocusButton();
  syncFocusPop();
  poll();
}

function togglePin(index) {
  const a = agents[index];
  if (!a) return;
  const i = pinnedAgents.indexOf(a.name);
  if (i >= 0) pinnedAgents.splice(i, 1);
  else pinnedAgents.push(a.name);
  localStorage.setItem("agent-island-pinned", JSON.stringify(pinnedAgents));
}

function agentIndexes() {
  let list = agents.map((a, i) => [a, i]);
  if (focusMode === "errors") {
    list = list.filter(([a]) => a.status === "error");
  }
  if (focusMode === "pinned" && pinnedAgents.length) {
    list = list.filter(([a]) => pinnedAgents.includes(a.name));
  }
  if (agentOrder.length) {
    list.sort((x, y) => {
      const ix = agentOrder.indexOf(x[0].name);
      const iy = agentOrder.indexOf(y[0].name);
      return (ix < 0 ? 999 : ix) - (iy < 0 ? 999 : iy);
    });
  }
  return list.map(([, i]) => i);
}

function jumpToAgentByKey(key) {
  const idxs = agentIndexes();
  const target = idxs[Number(key) - 1];
  if (target === undefined || target === cur) return;
  cur = target;
  sessionIdx = 0;
  animateSwitch();
  refresh();
}

// Nav
function animateSwitch() {
  switching = true;
  clearTimeout(animateSwitchTimer);
  animateSwitchTimer = setTimeout(() => { switching = false; }, 120);
}

function nextAgent() {
  const idxs = agentIndexes();
  if (idxs.length < 2) return;
  const pos = idxs.indexOf(cur);
  const next = idxs[(pos + 1) % idxs.length];
  if (next === undefined || next === cur) return;
  cur = next;
  sessionIdx = 0;
  animateSwitch();
  refresh();
}
function prevAgent() {
  const idxs = agentIndexes();
  if (idxs.length < 2) return;
  const pos = idxs.indexOf(cur);
  const next = idxs[(pos - 1 + idxs.length) % idxs.length];
  if (next === undefined || next === cur) return;
  cur = next;
  sessionIdx = 0;
  animateSwitch();
  refresh();
}
function nextSession() {
  const a = agents[cur];
  if (a?.session_list?.length > 1 && sessionIdx < a.session_list.length - 1) {
    sessionIdx += 1;
    animateSwitch();
    refresh();
  }
}
function prevSession() {
  const a = agents[cur];
  if (a?.session_list?.length > 1 && sessionIdx > 0) {
    sessionIdx -= 1;
    animateSwitch();
    refresh();
  }
}
navArrow.addEventListener("click", (e) => { e.stopPropagation(); nextAgent(); });
btnPrev.addEventListener("click", (e) => { e.stopPropagation(); prevSession(); });
btnNext.addEventListener("click", (e) => { e.stopPropagation(); nextSession(); });

async function openDir() {
  const a = agents[cur];
  if (!a) return;
  const path = a.session_list?.[sessionIdx]?.cwd || a.cwd;
  if (!path || a.status === "stopped") return;
  try { await invoke("open_path", { path }); } catch (_) {}
}

async function openFile() {
  const a = agents[cur];
  if (!a) return;
  const path = a.session_list?.[sessionIdx]?.current_file || a.current_file;
  if (!path) return;
  try { await invoke("open_path", { path }); } catch (_) {}
}

async function openTerm() {
  const a = agents[cur];
  if (!a || !a.cwd) return;
  try { await invoke("open_terminal", { name: a.name }); } catch (_) {}
}

btnDir.addEventListener("click", (e) => { e.stopPropagation(); openDir(); });
btnTerm.addEventListener("click", (e) => { e.stopPropagation(); openTerm(); });
expCwd.addEventListener("click", (e) => { e.stopPropagation(); openDir(); });
expFile.addEventListener("click", (e) => { e.stopPropagation(); openFile(); });

async function stopCurrent() {
  const a = agents[cur];
  if (!a || a.status === "stopped") return;
  if (!confirmingStop) {
    confirmingStop = true;
    confirmAgent = a.name;
    btnStop.textContent = "确认停止?";
    clearTimeout(confirmStopTimer);
    confirmStopTimer = setTimeout(() => {
      confirmingStop = false;
      confirmAgent = null;
      btnStop.textContent = "停止";
    }, 3000);
    return;
  }
  confirmingStop = false;
  btnStop.textContent = "停止";
  clearTimeout(confirmStopTimer);
  try { await invoke("stop_agent", { name: a.name }); } catch (_) {}
}

async function restartCurrent() {
  const a = agents[cur];
  if (!a || !a.can_restart) return;
  try { await invoke("restart_agent", { name: a.name }); } catch (_) {}
}

btnStop.addEventListener("click", (e) => { e.stopPropagation(); stopCurrent(); });
btnRestart.addEventListener("click", (e) => { e.stopPropagation(); restartCurrent(); });

async function openOverview() {
  if (!inTauri) return;
  try { await invoke("open_overview"); } catch (_) {}
}

btnOverview.addEventListener("click", (e) => { e.stopPropagation(); openOverview(); });
btnJump.addEventListener("click", async (e) => {
  e.stopPropagation();
  const a = agents[cur];
  if (!a) return;
  try { await invoke("focus_agent_terminal", { name: a.name }); } catch (_) {}
});
btnHook.addEventListener("click", (e) => { e.stopPropagation(); toggleHook(); });
btnPrivacy.addEventListener("click", (e) => { e.stopPropagation(); togglePrivacy(); });
btnFocus.addEventListener("click", (e) => {
  e.stopPropagation();
  focusPop.classList.toggle("hidden");
  syncFocusPop();
});
btnFields.addEventListener("click", (e) => {
  e.stopPropagation();
  fieldsPop.classList.toggle("hidden");
});
quickMenuEl.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-action]");
  if (!btn) return;
  hideQuickMenu();
  const action = btn.dataset.action;
  if (action === "overview") openOverview();
  else if (action === "privacy") togglePrivacy();
  else if (action === "focus") cycleFocus();
  else if (action === "quiet") {
    quietAuto = !quietAuto;
    localStorage.setItem("agent-island-quiet-auto", quietAuto ? "1" : "0");
    updateFocusButton();
    syncFocusPop();
  }
});
document.addEventListener("click", (e) => {
  if (!e.target.closest("#fields-pop") && !e.target.closest("#btn-fields")) {
    fieldsPop.classList.add("hidden");
  }
  if (!e.target.closest("#quick-menu")) {
    quickMenuEl.classList.add("hidden");
  }
  if (!e.target.closest("#focus-pop") && !e.target.closest("#btn-focus")) {
    focusPop.classList.add("hidden");
  }
  if (!e.target.closest("#theme-pop") && !e.target.closest("#btn-theme")) {
    themePop.classList.add("hidden");
  }
});
fieldsPop.addEventListener("change", (e) => {
  const key = e.target.dataset.fieldCheck;
  if (!key) return;
  if (e.target.checked) visibleFields.add(key);
  else visibleFields.delete(key);
  applyFields();
});
focusPop.addEventListener("click", (e) => {
  const btn = e.target.closest("[data-focus]");
  if (!btn) return;
  focusMode = btn.dataset.focus;
  localStorage.setItem("agent-island-focus", focusMode);
  updateFocusButton();
  syncFocusPop();
  poll();
});
quietAutoInput.addEventListener("change", () => {
  quietAuto = quietAutoInput.checked;
  localStorage.setItem("agent-island-quiet-auto", quietAuto ? "1" : "0");
  updateFocusButton();
});
quietStartInput.addEventListener("change", () => {
  quietStart = quietStartInput.value || "22:00";
  localStorage.setItem("agent-island-quiet-start", quietStart);
  updateFocusButton();
});
quietEndInput.addEventListener("change", () => {
  quietEnd = quietEndInput.value || "08:00";
  localStorage.setItem("agent-island-quiet-end", quietEnd);
  updateFocusButton();
});
btnTheme.addEventListener("click", (e) => {
  e.stopPropagation();
  themePop.classList.toggle("hidden");
  syncThemePop();
});
themePop.addEventListener("input", (e) => {
  const key = e.target.dataset.theme;
  if (!key) return;
  if (key === "opacity") themeOpacity = Number(e.target.value);
  else if (key === "radius") themeRadius = Number(e.target.value);
  else if (key === "width") themeWidth = Number(e.target.value);
  else if (key === "snap") snapEnabled = e.target.checked;
  else if (key === "taskbar") showInTaskbar = e.target.checked;
  const boolKey = key === "snap" || key === "taskbar";
  localStorage.setItem(`agent-island-theme-${key}`, String(boolKey ? (e.target.checked ? "1" : "0") : e.target.value));
  applyTheme();
});
themePop.addEventListener("change", (e) => {
  const style = e.target.dataset.style;
  if (!style) return;
  themeStyle = style;
  localStorage.setItem("agent-island-theme-style", style);
  applyTheme();
});
window.addEventListener("keydown", (e) => {
  if (e.target.closest("input,textarea,select")) return;
  if (e.key === "Escape") { setExpanded(false); return; }
  if (e.key === " " || e.code === "Space") { e.preventDefault(); setExpanded(!expanded); return; }
  if (e.altKey && e.key >= "1" && e.key <= "9") { e.preventDefault(); togglePin(Number(e.key) - 1); return; }
  if (e.key >= "1" && e.key <= "9") { jumpToAgentByKey(e.key); return; }
  if (e.key === "ArrowRight") { nextAgent(); return; }
  if (e.key === "ArrowLeft") { prevAgent(); return; }
  if (e.key === "ArrowDown") { nextSession(); return; }
  if (e.key === "ArrowUp") { prevSession(); return; }
  if (e.key === "p" || e.key === "P") { togglePrivacy(); return; }
  if (e.key === "f" || e.key === "F") { cycleFocus(); return; }
  if (e.key === "q" || e.key === "Q") {
    quietAuto = !quietAuto;
    localStorage.setItem("agent-island-quiet-auto", quietAuto ? "1" : "0");
    updateFocusButton();
    syncFocusPop();
    return;
  }
  if (e.key === "v" || e.key === "V") { btnFields.click(); return; }
  if (e.key === "m" || e.key === "M") { showQuickMenu(200, 60); return; }
  if (e.key === "o" || e.key === "O") { openOverview(); }
});

// Poll
const demoAgents = [
  {
    name: "Claude Code", status: "working", pid: 12345, cpu: 2.3, memory: 156, uptime: 1423,
    cwd: "D:\\demo\\project-a", sessions: 2, last_active_secs: 0, log_path: "demo",
    log_status: "working", alert: "正在执行", can_restart: true,
    stats: { total_seconds: 3600, error_count: 2, done_count: 5 },
    session_count: 2,
    session_list: [
      { id: "s1", name: "project-a", cwd: "D:\\demo\\project-a", log_path: "demo", recent_output: ["完成了 provider 预设列表", "新增 datalist 建议"], current_file: "D:\\demo\\project-a\\src\\main.ts", log_status: "working", alert: "正在执行" },
      { id: "s2", name: "project-c", cwd: "D:\\demo\\project-c", log_path: "demo", recent_output: ["修复了登录超时问题"], current_file: "D:\\demo\\project-c\\src\\auth.ts", log_status: "done", alert: "已完成" },
    ],
  },
  {
    name: "Codex CLI", status: "done", pid: 12346, cpu: 0.8, memory: 89, uptime: 3420,
    cwd: "D:\\demo\\project-b", sessions: 1, last_active_secs: 18, log_path: "demo",
    log_status: "done", alert: "已完成", can_restart: true,
    stats: { total_seconds: 7200, error_count: 1, done_count: 8 },
    session_count: 2,
    session_list: [
      { id: "c1", name: "project-b", cwd: "D:\\demo\\project-b", log_path: "demo", recent_output: ["执行: Get-Content README.md", "执行: rg -n TODO"], current_file: "D:\\demo\\project-b\\README.md", log_status: "done", alert: "已完成" },
      { id: "c2", name: "n-blog", cwd: "D:\\测试\\n-blog", log_path: "demo", recent_output: ["执行: npm run build"], current_file: "D:\\测试\\n-blog\\package.json", log_status: "idle", alert: null },
    ],
  },
  {
    name: "Hermes", status: "stopped", pid: null, cpu: null, memory: null, uptime: 0,
    cwd: null, sessions: 0, last_active_secs: null, log_path: null, log_status: null, alert: null,
    can_restart: false, stats: { total_seconds: 0, error_count: 0, done_count: 0 },
    session_count: 0, session_list: [],
  },
];

let demoStep = 0;
function simulateDemoEvents() {
  const claude = demoAgents[0];
  if (!claude) return;
  const cycle = ["working", "done", "error", "working"];
  demoStep = (demoStep + 1) % cycle.length;
  claude.status = cycle[demoStep];
  claude.alert = claude.status === "error" ? "报错" : claude.status === "done" ? "已完成" : "正在执行";
}

async function poll() {
  const prevName = agents[cur]?.name;
  const prevSessionId = agents[cur]?.session_list?.[sessionIdx]?.id;
  try {
    agents = inTauri ? await invoke("get_agents") : demoAgents;
  } catch (_) {}
  const rank = { error: 0, high_load: 0, waiting: 0, working: 1, running: 1, done: 2, idle: 2, stopped: 3 };
  agents.sort((x, y) => (rank[x.status] ?? 3) - (rank[y.status] ?? 3));
  if (prevName) {
    const idx = agents.findIndex((x) => x.name === prevName);
    cur = idx >= 0 ? idx : Math.min(cur, agents.length - 1);
    const agent = agents[cur];
    if (prevSessionId && agent?.session_list) {
      const si = agent.session_list.findIndex((s) => s.id === prevSessionId);
      sessionIdx = si >= 0 ? si : 0;
    } else {
      sessionIdx = 0;
    }
  } else if (agents.length) {
    const active = agents.findIndex((x) => x.status !== "stopped");
    cur = active === -1 ? 0 : active;
    sessionIdx = 0;
  }
  if (!inTauri && new URLSearchParams(window.location.search).has("demo")) {
    simulateDemoEvents();
  }
  if (focusMode === "errors") {
    const err = agents.findIndex((x) => x.status === "error");
    if (err >= 0) { cur = err; sessionIdx = 0; }
  }
  if (focusMode === "pinned" && pinnedAgents.length) {
    const pi = agents.findIndex((x) => pinnedAgents.includes(x.name));
    if (pi >= 0) { cur = pi; sessionIdx = 0; }
  }
  if (inTauri) {
    try { privacyAuto = await invoke("privacy_active"); } catch (_) {}
    applyPrivacy();
  }
refresh();
}

updateFocusButton();
applyPrivacy();
applyFields();
syncFocusPop();
syncThemePop();
syncIslandDimensions().then(positionIsland);
initHookEvents();
poll();
setInterval(poll, 3000);

if (!inTauri && new URLSearchParams(window.location.search).has("expanded")) {
  setExpanded(true);
}
