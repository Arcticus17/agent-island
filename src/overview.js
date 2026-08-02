import { invoke } from "@tauri-apps/api/core";

const listEl = document.getElementById("session-list");
const detailEl = document.getElementById("detail");
const detailHeader = document.getElementById("detail-header");
const chatLog = document.getElementById("chat-log");
const updatedEl = document.getElementById("updated");
const summaryEl = document.getElementById("summary");
const btnTerm = document.getElementById("btn-term");
const btnRestart = document.getElementById("btn-restart");
const btnStop = document.getElementById("btn-stop");
const promptInput = document.getElementById("prompt-input");
const btnSend = document.getElementById("btn-send");

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

let agents = [];
let selected = null;
let follow = true;
let confirmingStop = false;
let confirmStopTimer = null;
let confirmSessionId = null;
let chatHistory = loadChat();
let currentSend = null;
let sendPollTimer = null;

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

function renderList() {
  const all = rows();
  listEl.innerHTML = "";
  for (const r of all) {
    const row = document.createElement("div");
    row.className = "session-row" + (selected?.id === r.id ? " selected" : "");
    const lastLine = r.output.split("\n").filter(Boolean).slice(-1)[0] || "暂无输出";
    row.innerHTML = `
      <img class="icon" src="${ICONS[r.agent.name] || ""}" alt="" />
      <div class="row-main">
        <div class="row-title">${escapeHtml(r.agent.name)} · ${escapeHtml(r.name)}</div>
        <div class="row-meta">${escapeHtml(r.file || r.cwd || "无目录")}</div>
        <div class="row-output">${escapeHtml(lastLine)}</div>
      </div>
      <span class="dot ${STATUS_DOT[r.status] || "gray"}${r.status === "working" ? " pulse" : ""}"></span>
    `;
    row.addEventListener("click", () => {
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
    });
    listEl.appendChild(row);
  }
  if (!all.length) {
    const empty = document.createElement("div");
    empty.className = "empty-state";
    empty.textContent = "暂无会话";
    listEl.appendChild(empty);
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
  if (follow) chatLog.scrollTop = chatLog.scrollHeight;
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

chatLog.addEventListener("scroll", () => {
  const nearBottom = chatLog.scrollHeight - chatLog.scrollTop - chatLog.clientHeight < 24;
  follow = nearBottom;
});

async function load() {
  try {
    agents = await invoke("get_agents");
  } catch (_) {}

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
  updatedEl.textContent = new Date().toLocaleTimeString();
}

load();
setInterval(load, 2000);
