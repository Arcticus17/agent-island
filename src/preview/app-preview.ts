import { mount } from "svelte";

import App from "../app/App.svelte";
import { createMockBridge } from "../bridge/mock";
import type { AgentViewSnapshot, DisplayStatus } from "../bridge/types";

const NOW = 1_789_130_000_000;
const STATUSES: DisplayStatus[] = ["working", "waiting", "done", "error"];

function snapshot(status: DisplayStatus, records: number): AgentViewSnapshot {
  return {
    schema_version: 1,
    generated_at_ms: NOW,
    agents: [
      {
        id: "preview-agent",
        name: "Claude Code",
        can_restart: true,
        state: {
          process: "running",
          turn: status === "working" ? "executing" : status === "error" ? "failed" : "idle",
          attention: status === "waiting" ? "input_required" : "none",
          result_at_ms: status === "error" ? NOW : null,
        },
        display_status: status,
        active_session: {
          id: "preview-session",
          name: "灵动岛界面验收",
          cwd: "D:\\demo\\agent-island",
          log_path: null,
          records: Array.from({ length: records }, (_, index) => ({
            event_id: `record-${index}`,
            at_ms: NOW - index * 1_000,
            role: index % 2 === 0 ? ("Assistant" as const) : ("User" as const),
            text: "占位事件",
          })),
          recent_output: [],
          current_file: "src/app/App.svelte",
          log_status: null,
          alert: null,
          lifecycle: "Active",
          last_active_at_ms: NOW,
          display_status: status,
        },
        active_turn: null,
        history_sessions: [],
        diagnostic: null,
        freshness: { observed_at_ms: NOW, stale: false },
        usage: { tokens_total: 12_500, tokens_output: 2_500, cost_usd: 0.42, used_percent: 42, window_secs: 18_000, resets_at_secs: null, credits: null, unlimited: false, stale: false },
      },
    ],
  };
}

const host = document.getElementById("preview");
const params = new URLSearchParams(location.search);
const onlyStatus = params.get("status") as DisplayStatus | null;
const expanded = params.get("expanded") === "1";
const multi = params.get("multi") === "1";

/** 多 Agent 快照：两个在跑、两个已停止，用来对比「全部检测 / 仅运行中」。 */
function multiSnapshot(): AgentViewSnapshot {
  const base = snapshot("working", 12).agents[0];
  const running = (id: string, name: string, status: DisplayStatus) => ({
    ...structuredClone(base),
    id,
    name,
    display_status: status,
    state: { ...base.state, process: "running" as const, turn: status === "working" ? "executing" as const : "idle" as const },
    active_session: base.active_session ? { ...structuredClone(base.active_session), id: `${id}-session`, display_status: status } : null,
  });
  const stopped = (id: string, name: string) => ({
    ...structuredClone(base),
    id,
    name,
    display_status: "stopped" as DisplayStatus,
    state: { process: "stopped" as const, turn: "idle" as const, attention: "none" as const, result_at_ms: null },
    active_session: null,
    usage: null,
  });
  return {
    schema_version: 1,
    generated_at_ms: NOW,
    agents: [
      running("claude", "Claude Code", "working"),
      running("opencode", "OpenCode", "done"),
      stopped("codex", "Codex CLI"),
      stopped("hermes", "Hermes"),
    ],
  };
}

if (host) {
  for (const status of onlyStatus ? [onlyStatus] : STATUSES) {
    const cell = document.createElement("div");
    cell.className = "preview-cell";
    const label = document.createElement("p");
    label.className = "preview-label";
    label.textContent = `真实 App 渲染 · ${status}`;
    const stage = document.createElement("div");
    stage.className = "preview-stage";
    cell.append(label, stage);
    host.append(cell);
    mount(App, {
      target: stage,
      props: {
        bridge: createMockBridge([snapshot(status, 12)]),
        diagnosticsBridge: {
          getDiagnostics: async () => ({ generated_at_ms: NOW, issues: [], sources: [] }),
          exportDiagnostics: async () => undefined,
        },
        pollIntervalMs: 0,
        initiallyExpanded: expanded,
        iconStyle: "pixel",
      },
    });
  }
}

if (multi && host) {
  const cell = document.createElement("div");
  cell.className = "preview-cell multi";
  const label = document.createElement("p");
  label.className = "preview-label";
  label.textContent = `多 Agent · 显示模式 = ${localStorage.getItem("agent-island-agent-mode") === "running" ? "仅运行中" : "全部检测"}`;
  const stage = document.createElement("div");
  stage.className = "preview-stage";
  cell.append(label, stage);
  host.append(cell);
  mount(App, {
    target: stage,
    props: {
      bridge: createMockBridge([multiSnapshot()]),
      diagnosticsBridge: {
        getDiagnostics: async () => ({ generated_at_ms: NOW, issues: [], sources: [] }),
        exportDiagnostics: async () => undefined,
      },
      pollIntervalMs: 0,
      initiallyExpanded: true,
      iconStyle: "pixel",
    },
  });
}
