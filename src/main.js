// v10 — resident island: hover expansion, drag persistence, official Tauri API
import { invoke } from "@tauri-apps/api/core";
import {
  getCurrentWindow,
  primaryMonitor,
  availableMonitors,
  LogicalPosition,
  LogicalSize,
} from "@tauri-apps/api/window";

const inTauri = "__TAURI_INTERNALS__" in window;
const tauriWin = inTauri ? getCurrentWindow() : null;

const WINDOW_W = 420;
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
let privacyManual = localStorage.getItem("agent-island-privacy") === "1";
let privacyAuto = false;
let quietAuto = localStorage.getItem("agent-island-quiet-auto") === "1";
let quietStart = localStorage.getItem("agent-island-quiet-start") || "22:00";
let quietEnd = localStorage.getItem("agent-island-quiet-end") || "08:00";
let lastWheelAt = 0;
let eventCollapseTimer = null;
const notifyCooldown = {};

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
const expPage = $("exp-page");
const btnPrev = $("btn-prev");
const btnNext = $("btn-next");
const btnDir = $("btn-dir");
const btnTerm = $("btn-term");
const btnStop = $("btn-stop");
const btnRestart = $("btn-restart");
const btnOverview = $("btn-overview");
const notifyStackEl = $("notify-stack");
const btnPrivacy = $("btn-privacy");
const btnFocus = $("btn-focus");
const privacyBadge = $("privacy-badge");

const ICON_PATHS = {
  "Claude Code": "/icons/claude.svg",
  "Codex CLI": "/icons/codex.png",
  OpenCode: "/icons/opencode.png",
  Hermes: "/icons/hermes.png",
  Copilot: "/icons/copilot.svg",
  Cursor: "/icons/cursor.png",
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

// Position: restore saved absolute X, clamped to the virtual desktop
async function positionIsland() {
  if (!tauriWin) return;
  try {
    const saved = Number(localStorage.getItem(POS_KEY));
    const bounds = await virtualBounds();
    if (Number.isFinite(saved) && bounds) {
      const maxX = Math.max(bounds.minX, bounds.maxX - WINDOW_W);
      const x = Math.min(Math.max(saved, bounds.minX), maxX);
      await tauriWin.setPosition(new LogicalPosition(x, 0));
      return;
    }
    const mon = await primaryMonitor();
    if (!mon) return;
    scale = mon.scaleFactor || 1;
    const defaultX = Math.max(0, Math.floor((mon.size.width / scale - WINDOW_W) / 2));
    await tauriWin.setPosition(new LogicalPosition(defaultX, 0));
  } catch (_) {}
}

// Horizontal-only drag
let dragging = false;
let startX = 0;
let winStartX = 0;

island.addEventListener("mousedown", (e) => {
  if (e.target.closest("button")) return;
  if (e.target.closest("#exp-output")) return;
  if (!tauriWin) return;
  dragging = true;
  startX = e.screenX;
  Promise.all([tauriWin.scaleFactor(), tauriWin.outerPosition()])
    .then(([s, pos]) => { scale = s; winStartX = pos.x / s; })
    .catch(() => { winStartX = 0; });
});

window.addEventListener("mousemove", async (e) => {
  if (!dragging || !tauriWin) return;
  const dx = e.screenX - startX;
  try {
    await tauriWin.setPosition(new LogicalPosition(winStartX + dx, 0));
  } catch (_) {}
});

async function stopDrag() {
  if (!dragging || !tauriWin) return;
  dragging = false;
  try {
    scale = await tauriWin.scaleFactor();
    const pos = await tauriWin.outerPosition();
    localStorage.setItem(POS_KEY, String(pos.x / scale));
  } catch (_) {}
}

window.addEventListener("mouseup", stopDrag);
window.addEventListener("blur", stopDrag);

// Expand/collapse: resize the native window with the island so transparent
// space never blocks the desktop or triggers false hovers
function setExpanded(next) {
  if (next === expanded) return;
  expanded = next;
  clearTimeout(collapseTimer);
  island.classList.toggle("expanded", next);
  if (!tauriWin) return;
  if (next) {
    tauriWin.setSize(new LogicalSize(WINDOW_W, EXPANDED_H)).catch(() => {});
  } else {
    collapseTimer = setTimeout(() => {
      if (!expanded) {
        tauriWin.setSize(new LogicalSize(WINDOW_W, COLLAPSED_H)).catch(() => {});
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

// Theme
function setTheme() {
  document.documentElement.setAttribute(
    "data-theme",
    matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light"
  );
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
    expOutput.textContent = "暂无日志";
    expStats.textContent = "-";
    island.classList.remove("alert-error", "flash-error");
    island.classList.remove("working-glow");
    statusDot.classList.remove("pulse");
    expPage.textContent = "";
    btnPrev.disabled = true;
    btnNext.disabled = true;
    btnDir.disabled = true;
    btnTerm.disabled = true;
    btnStop.disabled = true;
    btnRestart.disabled = true;
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
  expOutput.textContent = recentLines?.length ? recentLines.join("\n") : "暂无日志";
  const logNearBottom = expOutput.scrollHeight - expOutput.scrollTop - expOutput.clientHeight < 24;
  if (logNearBottom) expOutput.scrollTop = expOutput.scrollHeight;
  expStats.textContent = a.stats
    ? `${fmtUptime(a.stats.total_seconds)} · 报错${a.stats.error_count} · 完成${a.stats.done_count}`
    : "-";
  const sessionCount = a.session_list?.length || 0;
  expPage.textContent = sessionCount > 1 ? `会话 ${sessionIdx + 1}/${sessionCount}` : "单一会话";
  btnPrev.disabled = sessionCount <= 1 || sessionIdx === 0;
  btnNext.disabled = sessionCount <= 1 || sessionIdx >= sessionCount - 1;
  const hasCwd = Boolean(useCwd && a.status !== "stopped");
  btnDir.disabled = !hasCwd;
  btnTerm.disabled = !hasCwd;
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
  statusDot.classList.toggle("pulse", a.status === "working");
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
  const card = document.createElement("div");
  card.className = `notify-card ${kind}`;
  const detail = sess?.name ? ` · ${escapeHtml(sess.name)}` : "";
  card.innerHTML = `
    <span class="notify-dot"></span>
    <span class="notify-text"><strong>${escapeHtml(agent.name)}</strong>${detail} ${label}</span>
    <button class="notify-close" title="关闭">×</button>
  `;
  card.querySelector(".notify-close").addEventListener("click", () => dismissNotify(card));
  notifyStackEl.prepend(card);
  while (notifyStackEl.children.length > 3) notifyStackEl.lastElementChild.remove();
  card.dataset.timer = setTimeout(() => dismissNotify(card), 6500);
  if (!expanded) {
    setExpanded(true);
    clearTimeout(eventCollapseTimer);
    eventCollapseTimer = setTimeout(() => {
      if (!island.matches(":hover")) setExpanded(false);
    }, 8000);
  }
}

function dismissNotify(card) {
  if (!card.isConnected) return;
  clearTimeout(Number(card.dataset.timer));
  card.style.opacity = "0";
  card.style.transform = "translateY(-4px)";
  setTimeout(() => card.remove(), 160);
}

function applyPrivacy() {
  const on = privacyManual || privacyAuto;
  document.body.classList.toggle("privacy-mask", on);
  btnPrivacy.classList.toggle("active", on);
  privacyBadge.classList.toggle("hidden", !on);
}

function togglePrivacy() {
  privacyManual = !privacyManual;
  localStorage.setItem("agent-island-privacy", privacyManual ? "1" : "0");
  applyPrivacy();
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
  if (focusMode === "errors") {
    return agents.map((a, i) => [a, i]).filter(([a]) => a.status === "error").map(([, i]) => i);
  }
  if (focusMode === "pinned" && pinnedAgents.length) {
    return agents.map((a, i) => [a, i]).filter(([a]) => pinnedAgents.includes(a.name)).map(([, i]) => i);
  }
  return agents.map((_, i) => i);
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
  if (!a || !a.cwd) return;
  try { await invoke("open_project_dir", { name: a.name }); } catch (_) {}
}

async function openTerm() {
  const a = agents[cur];
  if (!a || !a.cwd) return;
  try { await invoke("open_terminal", { name: a.name }); } catch (_) {}
}

btnDir.addEventListener("click", (e) => { e.stopPropagation(); openDir(); });
btnTerm.addEventListener("click", (e) => { e.stopPropagation(); openTerm(); });

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
btnPrivacy.addEventListener("click", (e) => { e.stopPropagation(); togglePrivacy(); });
btnFocus.addEventListener("click", (e) => { e.stopPropagation(); cycleFocus(); });
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
    return;
  }
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
positionIsland();
poll();
setInterval(poll, 3000);

if (!inTauri && new URLSearchParams(window.location.search).has("expanded")) {
  setExpanded(true);
}
