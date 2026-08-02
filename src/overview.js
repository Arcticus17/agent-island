import { invoke } from "@tauri-apps/api/core";

const listEl = document.getElementById("session-list");
const detailEl = document.getElementById("detail");
const detailHeader = document.getElementById("detail-header");
const detailOutput = document.getElementById("detail-output");
const updatedEl = document.getElementById("updated");
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

let agents = [];
let selected = null;
let follow = true;
let confirmingStop = false;
let confirmStopTimer = null;
let confirmSessionId = null;
let sendState = null;
let sendPollTimer = null;

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
      <span class="dot ${STATUS_DOT[r.status] || "gray"}"></span>
    `;
    row.addEventListener("click", () => {
      if (sendPollTimer) {
        clearInterval(sendPollTimer);
        sendPollTimer = null;
      }
      sendState = null;
      selected = { id: r.id, agent: r.agent, session: r.session, row: r };
      follow = true;
      renderList();
      renderDetail();
    });
    listEl.appendChild(row);
  }
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
  let text = r.output || "";
  if (sendState) {
    text += `\n\n> 发送: ${sendState.prompt}\n`;
    if (sendState.error) {
      text += `[发送失败: ${sendState.error}]`;
    } else if (sendState.starting) {
      text += "(正在启动...)";
    } else {
      text += sendState.lines.length ? sendState.lines.join("\n") : "(等待输出...)";
      text += sendState.done ? "\n[完成]" : "\n[运行中...]";
    }
  }
  detailOutput.textContent = text;
  if (follow) detailOutput.scrollTop = detailOutput.scrollHeight;
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
  sendState = { prompt, taskId: "", lines: [], done: false, starting: true };
  promptInput.value = "";
  btnSend.disabled = true;
  renderDetail();
  let taskId = "";
  try {
    taskId = await invoke("send_to_session", {
      name: selected.agent.name,
      sessionId: selected.session?.id || "",
      prompt,
    });
  } catch (err) {
    sendState = { prompt, taskId: "", lines: [], done: true, error: String(err) };
    renderDetail();
    return;
  }
  if (!taskId) return;
  sendState = { prompt, taskId, lines: [], done: false };
  renderDetail();
  if (sendPollTimer) clearInterval(sendPollTimer);
  sendPollTimer = setInterval(pollSend, 800);
}

async function pollSend() {
  if (!sendState) return;
  try {
    const out = await invoke("get_send_output", { taskId: sendState.taskId });
    sendState.lines = out.lines;
    sendState.done = out.done;
    renderDetail();
    if (out.done) {
      clearInterval(sendPollTimer);
      sendPollTimer = null;
    }
  } catch (_) {
    clearInterval(sendPollTimer);
    sendPollTimer = null;
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

detailOutput.addEventListener("scroll", () => {
  const nearBottom = detailOutput.scrollHeight - detailOutput.scrollTop - detailOutput.clientHeight < 24;
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
  renderDetail();
  updatedEl.textContent = new Date().toLocaleTimeString();
}

load();
setInterval(load, 2000);
