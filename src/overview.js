import { invoke } from "@tauri-apps/api/core";

const listEl = document.getElementById("session-list");
const detailEl = document.getElementById("detail");
const detailHeader = document.getElementById("detail-header");
const chatLog = document.getElementById("chat-log");
const updatedEl = document.getElementById("updated");
const summaryEl = document.getElementById("summary");
const scrollBottomBtn = document.getElementById("scroll-bottom");
const btnTerm = document.getElementById("btn-term");
const btnRestart = document.getElementById("btn-restart");
const btnStop = document.getElementById("btn-stop");
const promptInput = document.getElementById("prompt-input");
const btnSend = document.getElementById("btn-send");
const statsView = document.getElementById("stats-view");
const statsTable = document.getElementById("stats-table");
const btnStats = document.getElementById("btn-stats");
const btnRemote = document.getElementById("btn-remote");
const remoteUrlEl = document.getElementById("remote-url");
const btnStatsDays = document.getElementById("btn-stats-days");

const ICONS = {
  "Claude Code": "/icons/claude.svg",
  "Codex CLI": "/icons/codex.png",
  OpenCode: "/icons/opencode.png",
  Hermes: "/icons/hermes.png",
  Copilot: "/icons/copilot.svg",
  Cursor: "/icons/cursor.png",
};

const STATUS_TEXT = {
  working: "工作中",
  idle: "等待中",
  high_load: "高负载",
  stopped: "已停止",
  error: "报错",
  waiting: "等待确认",
  done: "已完成",
};

const STATUS_DOT = {
  working: "green",
  idle: "yellow",
  high_load: "yellow",
  stopped: "red",
  error: "red",
  waiting: "yellow",
  done: "green",
};

const SEND_AGENTS = new Set(["Claude Code", "Codex CLI", "OpenCode", "Hermes"]);
const CHAT_KEY = "agent-island-chat";

const DEMO_AGENTS = [
  {
    name: "Claude Code", status: "working", pid: 12345, cpu: 2.3, memory: 156, uptime: 1423,
    cwd: "D:\\demo\\project-a", sessions: 2, last_active_secs: 0, log_status: "working",
    alert: "正在执行", can_restart: true,
    stats: { total_seconds: 3600, error_count: 2, done_count: 5 },
    session_count: 2,
    session_list: [
      { id: "s1", name: "project-a", cwd: "D:\\demo\\project-a", log_path: "demo", recent_output: ["完成了 provider 预设列表", "新增 datalist 建议"], current_file: "D:\\demo\\project-a\\src\\main.ts", log_status: "working", alert: "正在执行" },
      { id: "s2", name: "project-c", cwd: "D:\\demo\\project-c", log_path: "demo", recent_output: ["修复了登录超时问题"], current_file: "D:\\demo\\project-c\\src\\auth.ts", log_status: "done", alert: "已完成" },
    ],
  },
  {
    name: "Codex CLI", status: "done", pid: 12346, cpu: 0.8, memory: 89, uptime: 3420,
    cwd: "D:\\demo\\project-b", sessions: 1, last_active_secs: 18, log_status: "done",
    alert: "已完成", can_restart: true,
    stats: { total_seconds: 7200, error_count: 1, done_count: 8 },
    session_count: 2,
    session_list: [
      { id: "c1", name: "project-b", cwd: "D:\\demo\\project-b", log_path: "demo", recent_output: ["执行: Get-Content README.md", "执行: rg -n TODO"], current_file: "D:\\demo\\project-b\\README.md", log_status: "done", alert: "已完成" },
      { id: "c2", name: "n-blog", cwd: "D:\\测试\\n-blog", log_path: "demo", recent_output: ["执行: npm run build"], current_file: "D:\\测试\\n-blog\\package.json", log_status: "idle", alert: null },
    ],
  },
  {
    name: "Hermes", status: "stopped", pid: null, cpu: null, memory: null, uptime: 0,
    cwd: null, sessions: 0, last_active_secs: null, log_status: null, alert: null,
    can_restart: false, stats: { total_seconds: 0, error_count: 0, done_count: 0 },
    session_count: 1,
    session_list: [
      { id: "h1", name: "灵动岛讨论", cwd: null, log_path: null, recent_output: ["你知道 MAC 笔记本的灵动岛吗"], current_file: null, log_status: null, alert: null },
    ],
  },
];

let agents = [];
let selected = null;
let follow = true;
let confirmingStop = false;
let confirmStopTimer = null;
let confirmSessionId = null;
let chatHistory = loadChat();
let currentSend = null;
let sendPollTimer = null;
const lastRowStatus = {};
let statsMode = "sessions";
let statsDays = 7;

function loadChat() {
  try {
    return JSON.parse(localStorage.getItem(CHAT_KEY)) || {};
  } catch (_) {
    return {};
  }
}

function saveChat() {
  try {
    localStorage.setItem(CHAT_KEY, JSON.stringify(chatHistory));
  } catch (_) {}
}

function escapeHtml(s) {
  return String(s ?? "")
    .replaceAll("&", "&amp;")
    .replaceAll("<", "&lt;")
    .replaceAll(">", "&gt;")
    .replaceAll('"', "&quot;")
    .replaceAll("'", "&#39;");
}

function fmtAgo(secs) {
  if (secs == null) return "-";
  if (secs < 3) return "刚刚";
  if (secs < 60) return `${Math.floor(secs)}秒前`;
  if (secs < 3600) return `${Math.floor(secs / 60)}分钟前`;
  return `${Math.floor(secs / 3600)}小时前`;
}

function statusOf(agent, session) {
  if (session?.log_status) return session.log_status;
  return agent.status || "idle";
}

function rows() {
  const out = [];
  for (const agent of agents) {
    let sessions = (agent.session_list || []).filter(
      (s) => s.name && s.name !== "—" && s.name !== "-"
    );
    if (!sessions.length) sessions = [null];
    for (const session of sessions) {
      out.push({
        agent,
        session,
        id: `${agent.name}::${session?.id || agent.name}`,
        name: session?.name || agent.name,
        status: statusOf(agent, session),
        output: session?.recent_output?.length
          ? session.recent_output.join("\n")
          : (agent.recent_output || []).join("\n") || "暂无输出",
        cwd: session?.cwd || agent.cwd,
        file: session?.current_file || agent.current_file,
      });
    }
  }
  return out;
}

const rowEls = {};

function selectRow(r) {
  if (sendPollTimer) {
    clearInterval(sendPollTimer);
    sendPollTimer = null;
  }
  if (currentSend) {
    currentSend.agentMsg.done = true;
    currentSend.agentMsg.text += "\n[已切换会话，停止跟踪]";
    saveChat();
  }
  currentSend = null;
  selected = { id: r.id, agent: r.agent, session: r.session, row: r };
  follow = true;
  chatLog.classList.remove("chat-switching");
  void chatLog.offsetWidth;
  chatLog.classList.add("chat-switching");
  setTimeout(() => chatLog.classList.remove("chat-switching"), 260);
  renderList();
  renderDetail();
}

function renderList() {
  const all = rows();
  const seen = new Set();
  for (const r of all) {
    seen.add(r.id);
    const prevStatus = lastRowStatus[r.id];
    lastRowStatus[r.id] = r.status;
    const updated = prevStatus && prevStatus !== r.status;
    let row = rowEls[r.id];
    const lastLine = r.output.split("\n").filter(Boolean).slice(-1)[0] || "暂无输出";
    if (!row) {
      row = document.createElement("div");
      row.className = "session-row row-in";
      row.dataset.id = r.id;
      row.innerHTML = `
        <img class="icon" alt="" />
        <div class="row-main">
          <div class="row-title"></div>
          <div class="row-meta"></div>
          <div class="row-output"></div>
        </div>
        <span class="dot"></span>
      `;
      row.addEventListener("click", () => {
        const current = rows().find((x) => x.id === row.dataset.id) || r;
        selectRow(current);
      });
      listEl.appendChild(row);
      rowEls[r.id] = row;
    } else {
      row.classList.remove("selected", "row-updated");
      if (updated) row.classList.add("row-updated");
    }
    row.querySelector(".icon").src = ICONS[r.agent.name] || "";
    row.querySelector(".row-title").textContent = `${r.agent.name} · ${r.name}`;
    row.querySelector(".row-meta").textContent = r.file || r.cwd || "无目录";
    row.querySelector(".row-output").textContent = lastLine;
    row.querySelector(".dot").className =
      "dot " + (STATUS_DOT[r.status] || "gray") + (r.status === "working" ? " pulse" : "");
    if (selected?.id === r.id) row.classList.add("selected");
  }
  for (const id of Object.keys(rowEls)) {
    if (!seen.has(id)) {
      rowEls[id].remove();
      delete rowEls[id];
    }
  }
  const empty = listEl.querySelector(".empty-state");
  if (!all.length) {
    if (!empty) {
      const div = document.createElement("div");
      div.className = "empty-state";
      div.textContent = "暂无会话";
      listEl.appendChild(div);
    }
  } else if (empty) {
    empty.remove();
  }
}

function renderChat() {
  if (!selected) return;
  chatLog.innerHTML = "";
  const msgs = chatHistory[selected.id] || [];
  for (const m of msgs) {
    const div = document.createElement("div");
    div.className = "msg " + m.role + (m.done === false ? " pending" : "");
    const label = m.role === "user" ? "你" : (m.agentName || selected.agent.name);
    if (!m.text && m.done === false) {
      div.innerHTML = `<span class="msg-label">${escapeHtml(label)}</span><span class="typing"><i></i><i></i><i></i></span>`;
    } else {
      const body = m.text || "[完成（无输出）]";
      div.innerHTML = `<span class="msg-label">${escapeHtml(label)}</span>${escapeHtml(body)}`;
    }
    chatLog.appendChild(div);
  }
  if (follow) {
    chatLog.scrollTop = chatLog.scrollHeight;
    scrollBottomBtn.classList.add("hidden");
  }
}

function renderSummary() {
  const counts = {};
  for (const r of rows()) {
    counts[r.status] = (counts[r.status] || 0) + 1;
  }
  const order = ["working", "waiting", "error", "done", "idle", "stopped"];
  summaryEl.innerHTML =
    order
      .filter((s) => counts[s])
      .map((s) => `<span class="summary-pill ${s}">${STATUS_TEXT[s] || s} ${counts[s]}</span>`)
      .join("") || '<span class="summary-pill">无会话</span>';
}

function renderDetail() {
  if (!selected) {
    detailEl.classList.add("hidden");
    return;
  }
  detailEl.classList.remove("hidden");
  const r = selected.row;
  const hasSession = Boolean(selected.session);
  const hasCwd = Boolean(r.cwd);
  btnTerm.disabled = !hasSession || !hasCwd;
  btnRestart.disabled = !hasSession || !r.agent.can_restart;
  btnStop.disabled = !hasSession || r.agent.status === "stopped";
  btnSend.disabled = !SEND_AGENTS.has(r.agent.name) || !promptInput.value.trim();
  if (confirmingStop && confirmSessionId !== selected.session?.id) {
    confirmingStop = false;
    clearTimeout(confirmStopTimer);
    btnStop.textContent = "停止";
  }
  detailHeader.innerHTML = `
    <img src="${ICONS[r.agent.name] || ""}" alt="" />
    <div>
      <div class="detail-title">${escapeHtml(r.agent.name)} · ${escapeHtml(r.name)}</div>
      <div class="detail-status">${escapeHtml(STATUS_TEXT[r.status] || r.status)} · ${escapeHtml(fmtAgo(r.agent.last_active_secs))}</div>
      <div class="detail-cwd">${escapeHtml(r.cwd || "无目录")}</div>
    </div>
  `;
  if (!chatHistory[selected.id]) {
    chatHistory[selected.id] = r.output
      ? [{ role: "agent", agentName: r.agent.name, text: r.output, done: true }]
      : [];
  }
  renderChat();
}

function fmtUptime(s) {
  s = Math.floor(s || 0);
  if (s < 60) return `${s}秒`;
  if (s < 3600) return `${Math.floor(s / 60)}分钟`;
  return `${Math.floor(s / 3600)}小时${Math.floor((s % 3600) / 60)}分`;
}

function todayStr() {
  const d = new Date();
  return `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
}

function demoStatsReport(days) {
  const names = ["Claude Code", "Codex CLI", "OpenCode", "Hermes"];
  const out = [];
  for (let i = days - 1; i >= 0; i--) {
    const d = new Date();
    d.setDate(d.getDate() - i);
    const date = `${d.getFullYear()}-${String(d.getMonth() + 1).padStart(2, "0")}-${String(d.getDate()).padStart(2, "0")}`;
    out.push({
      date,
      agents: names.map((name) => ({
        name,
        total_seconds: Math.floor(Math.random() * 3600),
        error_count: Math.floor(Math.random() * 3),
        done_count: Math.floor(Math.random() * 6),
      })),
    });
  }
  return { days: out };
}

async function loadStats() {
  let report;
  try {
    report = await invoke("get_stats_report", { days: statsDays });
  } catch (_) {
    report = demoStatsReport(statsDays);
  }
  statsTable.innerHTML = "";
  if (!report?.days?.length) {
    statsTable.innerHTML = '<div class="stat-empty">暂无统计数据</div>';
    return;
  }
  for (const day of report.days) {
    const wrap = document.createElement("div");
    wrap.className = "stat-day";
    const title = document.createElement("div");
    title.className = "stat-day-title";
    title.textContent = day.date + (day.date === todayStr() ? "（今天）" : "");
    wrap.appendChild(title);
    const head = document.createElement("div");
    head.className = "stat-row head";
    head.innerHTML = "<span>Agent</span><span>累计用时</span><span>报错</span><span>完成</span>";
    wrap.appendChild(head);
    if (!day.agents?.length) {
      const empty = document.createElement("div");
      empty.className = "stat-empty";
      empty.textContent = "当天暂无数据";
      wrap.appendChild(empty);
    } else {
      for (const a of day.agents) {
        const row = document.createElement("div");
        row.className = "stat-row";
        row.innerHTML = [
          escapeHtml(a.name),
          fmtUptime(a.total_seconds || 0),
          String(a.error_count ?? 0),
          String(a.done_count ?? 0),
        ].map((c) => `<span>${c}</span>`).join("");
        wrap.appendChild(row);
      }
    }
    statsTable.appendChild(wrap);
  }
}

function toggleStats() {
  statsMode = statsMode === "stats" ? "sessions" : "stats";
  const stats = statsMode === "stats";
  statsView.classList.toggle("hidden", !stats);
  listEl.classList.toggle("hidden", stats);
  detailEl.classList.toggle("hidden", stats || !selected);
  btnStats.classList.toggle("active", stats);
  if (stats) loadStats();
}

async function showRemote() {
  let url = "http://localhost:8765";
  try {
    url = await invoke("get_remote_url");
  } catch (_) {}
  remoteUrlEl.textContent = url;
  remoteUrlEl.classList.remove("hidden");
}

async function runSessionAction(action, payload) {
  try {
    await invoke(action, payload);
  } catch (_) {}
}

btnTerm.addEventListener("click", () => {
  if (!selected?.session) return;
  runSessionAction("open_session_terminal", {
    name: selected.agent.name,
    sessionId: selected.session.id,
  });
});

btnRestart.addEventListener("click", () => {
  if (!selected?.session) return;
  runSessionAction("restart_session", {
    name: selected.agent.name,
    sessionId: selected.session.id,
  });
});

btnStop.addEventListener("click", () => {
  if (!selected?.session || selected.agent.status === "stopped") return;
  if (!confirmingStop) {
    confirmingStop = true;
    confirmSessionId = selected.session.id;
    btnStop.textContent = "确认停止?";
    clearTimeout(confirmStopTimer);
    confirmStopTimer = setTimeout(() => {
      confirmingStop = false;
      btnStop.textContent = "停止";
    }, 3000);
    return;
  }
  confirmingStop = false;
  btnStop.textContent = "停止";
  clearTimeout(confirmStopTimer);
  runSessionAction("stop_session", {
    name: selected.agent.name,
    sessionId: selected.session.id,
  });
});

async function sendPrompt() {
  if (!selected || !promptInput.value.trim()) return;
  const prompt = promptInput.value.trim();
  const history = chatHistory[selected.id] || (chatHistory[selected.id] = []);
  history.push({ role: "user", text: prompt, done: true });
  const agentMsg = { role: "agent", agentName: selected.agent.name, text: "", done: false };
  history.push(agentMsg);
  currentSend = { key: selected.id, agentMsg, taskId: "" };
  promptInput.value = "";
  btnSend.disabled = true;
  saveChat();
  renderChat();
  let taskId = "";
  try {
    taskId = await invoke("send_to_session", {
      name: selected.agent.name,
      sessionId: selected.session?.id || "",
      prompt,
    });
  } catch (err) {
    agentMsg.text = `[发送失败: ${err}]`;
    agentMsg.done = true;
    currentSend = null;
    saveChat();
    renderChat();
    return;
  }
  if (!taskId) {
    agentMsg.text = "[发送失败: 未返回任务ID]";
    agentMsg.done = true;
    currentSend = null;
    saveChat();
    renderChat();
    return;
  }
  currentSend.taskId = taskId;
  if (sendPollTimer) clearInterval(sendPollTimer);
  sendPollTimer = setInterval(pollSend, 800);
}

async function pollSend() {
  if (!currentSend) return;
  try {
    const out = await invoke("get_send_output", { taskId: currentSend.taskId });
    currentSend.agentMsg.text = out.lines.join("\n");
    currentSend.agentMsg.done = out.done;
    saveChat();
    renderChat();
    if (out.done) {
      clearInterval(sendPollTimer);
      sendPollTimer = null;
      currentSend = null;
    }
  } catch (_) {
    clearInterval(sendPollTimer);
    sendPollTimer = null;
    currentSend = null;
  }
}

btnSend.addEventListener("click", sendPrompt);
promptInput.addEventListener("keydown", (e) => {
  if (e.key === "Enter") sendPrompt();
});
promptInput.addEventListener("input", () => {
  btnSend.disabled =
    !selected || !SEND_AGENTS.has(selected.agent.name) || !promptInput.value.trim();
});
btnStats.addEventListener("click", toggleStats);
btnRemote.addEventListener("click", showRemote);
btnStatsDays.addEventListener("click", () => {
  statsDays = statsDays === 7 ? 1 : 7;
  btnStatsDays.textContent = statsDays === 7 ? "最近 7 天" : "今天";
  loadStats();
});

chatLog.addEventListener("scroll", () => {
  const nearBottom = chatLog.scrollHeight - chatLog.scrollTop - chatLog.clientHeight < 24;
  follow = nearBottom;
  scrollBottomBtn.classList.toggle("hidden", nearBottom);
});
scrollBottomBtn.addEventListener("click", () => {
  follow = true;
  chatLog.scrollTo({ top: chatLog.scrollHeight, behavior: "smooth" });
  scrollBottomBtn.classList.add("hidden");
});

async function load() {
  try {
    agents = await invoke("get_agents");
  } catch (_) {
    agents = DEMO_AGENTS;
  }

  const all = rows();
  if (selected) {
    const match = all.find((r) => r.id === selected.id);
    if (match) {
      selected = { id: match.id, agent: match.agent, session: match.session, row: match };
    } else {
      selected = null;
    }
  }
  if (!selected && all.length) {
    const first = all[0];
    selected = { id: first.id, agent: first.agent, session: first.session, row: first };
  }

  renderList();
  renderSummary();
  renderDetail();
  if (statsMode === "stats") loadStats();
  updatedEl.textContent = new Date().toLocaleTimeString();
}

load();
setInterval(load, 2000);
