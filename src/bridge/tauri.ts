import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { availableMonitors, getCurrentWindow, LogicalSize, PhysicalPosition } from "@tauri-apps/api/window";
import type { AgentTaskSnapshot } from "./types";
import { clampWindowPlacement, parseWindowPlacement, WINDOW_PLACEMENT_KEY } from "./window-placement";

// Temporary raw exports keep the reversible legacy UI working while all new
// Svelte code consumes only AgentIslandBridge.
export { invoke, listen };
export {
  availableMonitors,
  currentMonitor,
  getCurrentWindow,
  LogicalPosition,
  LogicalSize,
  primaryMonitor,
} from "@tauri-apps/api/window";

import {
  assertSupportedSnapshot,
  type AgentCommand,
  type AgentIslandBridge,
  type AgentViewSnapshot,
  type ApprovalAuditEntry,
  type AppUpdateInfo,
  type ApprovalRule,
  type AgentDetection,
  type AgentCandidate,
  type SessionSearchHit,
  type SessionView,
  type CliVersionProbe,
  type CommandValue,
  type CommandResult,
  type HookApproval,
  type HookEvent,
  type TeamCapability,
  type TeamPlanTask,
  type TeamRun,
  type VerificationRisk,
} from "./types";

function commandInvocation(command: AgentCommand): {
  commandName: string;
  args?: Record<string, unknown>;
} {
  switch (command.name) {
    case "reload_agent_defs":
    case "open_overview":
    case "get_hook_status":
    case "get_autostart":
    case "privacy_active":
      return { commandName: command.name };
    case "stop_agent":
    case "restart_agent":
    case "open_project_dir":
    case "open_terminal":
    case "focus_agent_terminal":
      return { commandName: command.name, args: { name: command.agentName } };
    case "open_path":
      return { commandName: command.name, args: { path: command.path } };
    case "open_session_terminal":
    case "restart_session":
    case "stop_session":
      return {
        commandName: command.name,
        args: { name: command.agentName, sessionId: command.sessionId },
      };
    case "run_task":
      return { commandName: "start_agent_task", args: { prompt: command.prompt } };
    case "send_to_session":
      return {
        commandName: command.name,
        args: {
          name: command.agentName,
          sessionId: command.sessionId,
          prompt: command.prompt,
        },
      };
    case "get_send_output":
      return { commandName: command.name, args: { taskId: command.taskId } };
    case "set_hook_enabled":
    case "set_autostart":
      return { commandName: command.name, args: { enabled: command.enabled } };
    case "respond_hook_approval":
      return {
        commandName: command.name,
        args: {
          id: command.id,
          allow: command.allow,
          ...(command.scope && command.scope !== "once" ? { scope: command.scope } : {}),
          ...(command.reason ? { reason: command.reason } : {}),
        },
      };
    case "dismiss_hook_approval":
      return { commandName: command.name, args: { id: command.id } };
    case "get_stats_report":
      return { commandName: command.name, args: { days: command.days } };
  }
}

function commandError(error: unknown): Exclude<CommandResult, { ok: true }> {
  const message = error instanceof Error ? error.message : String(error);
  const normalized = message.toLowerCase();
  const classifications: ReadonlyArray<readonly [string, string, boolean]> = [
    ["prompt is empty", "prompt_empty", false],
    ["empty path", "path_empty", false],
    ["unknown agent", "unknown_agent", false],
    ["no command recorded", "command_unavailable", false],
    ["approval_not_found", "approval_not_found", false],
    ["task not found", "task_not_found", false],
    ["not supported", "not_supported", false],
    ["permission denied", "permission_denied", false],
    ["no process found", "no_process_found", true],
    ["agent is not running", "agent_not_running", true],
    ["session not found", "session_not_found", true],
    ["state lock error", "state_unavailable", true],
  ];
  const [, code, retryable] = classifications.find(([needle]) => normalized.includes(needle))
    ?? ["", "command_failed", false];
  return { ok: false, code, message, retryable };
}

function isHookEvent(payload: unknown): payload is HookEvent {
  if (!payload || typeof payload !== "object") return false;
  const value = payload as Record<string, unknown>;
  if (value.kind === "approval_expired") {
    return Array.isArray(value.ids) && value.ids.every((id) => typeof id === "string");
  }
  if (typeof value.session !== "string") return false;
  return (
    value.kind === "stop" ||
    (value.kind === "notification" && typeof value.message === "string")
  );
}

function isHookApproval(payload: unknown): payload is HookApproval {
  if (!payload || typeof payload !== "object") return false;
  const value = payload as Record<string, unknown>;
  if (
    !(typeof value.id === "string" && value.id.length > 0 &&
      typeof value.session === "string" &&
      typeof value.tool === "string" &&
      typeof value.command === "string" &&
      typeof value.cwd === "string")
  ) return false;
  if (value.risk !== undefined && value.risk !== "low" && value.risk !== "medium" && value.risk !== "high") {
    return false;
  }
  if (value.rule_key !== undefined && typeof value.rule_key !== "string") return false;
  if (value.risk_reason !== undefined && typeof value.risk_reason !== "string") return false;
  if (value.repo !== undefined && typeof value.repo !== "string") return false;
  if (value.branch !== undefined && typeof value.branch !== "string") return false;
  if (value.diff !== undefined) {
    const diff = value.diff;
    if (!diff || typeof diff !== "object") return false;
    const candidate = diff as Record<string, unknown>;
    if (typeof candidate.file !== "string" || typeof candidate.added !== "number" ||
      typeof candidate.removed !== "number" || typeof candidate.truncated !== "boolean" ||
      !Array.isArray(candidate.lines)) return false;
    for (const line of candidate.lines) {
      if (!line || typeof line !== "object") return false;
      const entry = line as Record<string, unknown>;
      if (typeof entry.text !== "string" ||
        (entry.kind !== "ctx" && entry.kind !== "add" && entry.kind !== "del")) return false;
    }
  }
  return true;
}

function isTerminalData(payload: unknown): payload is { id: string; data: string } {
  if (!payload || typeof payload !== "object") return false;
  const value = payload as Record<string, unknown>;
  return typeof value.id === "string" && typeof value.data === "string";
}

function isTerminalId(payload: unknown): payload is { id: string } {
  return Boolean(payload) && typeof payload === "object" &&
    typeof (payload as Record<string, unknown>).id === "string";
}

export function createTauriBridge(): AgentIslandBridge {
  async function placement(delta = 0) {
    const window = getCurrentWindow();
    const [position, size, scale, monitors] = await Promise.all([window.outerPosition(), window.innerSize(), window.scaleFactor(), availableMonitors()]);
    return { window, height: size.height / scale, value: clampWindowPlacement({ x: position.x, y: position.y, width: size.width / scale + delta }, monitors, size.height / scale) };
  }
  return {
    async startWindowDrag() { await getCurrentWindow().startDragging(); },
    async startWindowResize() { await getCurrentWindow().startResizeDragging("East"); },
    async adjustWindowWidth(delta) {
      if (!Number.isFinite(delta)) throw new Error("无效窗口宽度");
      const { window, value, height } = await placement(delta);
      await window.setSize(new LogicalSize(value.width, height));
      await window.setPosition(new PhysicalPosition(value.x, value.y));
    },
    async initializeWindowPlacement(onError) {
      const window = getCurrentWindow();
      const [monitors, size, scale] = await Promise.all([availableMonitors(), window.innerSize(), window.scaleFactor()]);
      const height = size.height / scale;
      const saved = parseWindowPlacement(localStorage.getItem(WINDOW_PLACEMENT_KEY));
      const primary = monitors[0];
      // 贴边刘海：默认停在主屏工作区顶部居中。没有主屏信息时交给系统居中。
      const compactWidth = 168;
      const initial = saved ?? (primary
        ? {
            x: Math.round(primary.workArea.position.x + (primary.workArea.size.width - compactWidth * primary.scaleFactor) / 2),
            y: primary.workArea.position.y,
            width: compactWidth,
          }
        : null);
      if (initial) {
        const value = clampWindowPlacement(initial, monitors, height);
        // 刘海永远贴着工作区顶边；水平位置沿用用户拖到的那块屏。
        const monitor = monitors.find((m) => value.x >= m.workArea.position.x
          && value.x < m.workArea.position.x + m.workArea.size.width
          && value.y >= m.workArea.position.y
          && value.y < m.workArea.position.y + m.workArea.size.height) ?? primary;
        const pinnedY = monitor ? monitor.workArea.position.y : value.y;
        await window.setSize(new LogicalSize(value.width, height));
        await window.setPosition(new PhysicalPosition(value.x, pinnedY));
      }
      let disposed = false;
      let timer: ReturnType<typeof setTimeout> | undefined;
      let saving = false;
      let again = false;
      const save = async () => {
        if (disposed) return;
        if (saving) { again = true; return; }
        saving = true;
        try {
          const { value } = await placement();
          if (!disposed) localStorage.setItem(WINDOW_PLACEMENT_KEY, JSON.stringify({ version: 1, ...value }));
        } catch (error) { if (!disposed) onError?.(error); }
        finally { saving = false; if (again) { again = false; schedule(); } }
      };
      const schedule = () => { clearTimeout(timer); timer = setTimeout(() => { void save(); }, 150); };
      const moved = await window.onMoved(schedule);
      let resized: () => void;
      try { resized = await window.onResized(schedule); }
      catch (error) { moved(); throw error; }
      return () => { disposed = true; clearTimeout(timer); moved(); resized(); };
    },
    async getSnapshot() {
      const snapshot = await invoke<AgentViewSnapshot>("get_agent_snapshot");
      return assertSupportedSnapshot(snapshot);
    },
    async runCommand(command) {
      const invocation = commandInvocation(command);
      try {
        const value = await invoke<CommandValue | null | undefined>(
          invocation.commandName,
          invocation.args,
        );
        return value == null ? { ok: true } : { ok: true, value };
      } catch (error) {
        return commandError(error);
      }
    },
    async listenHookEvents(handler) {
      const unlistenEvents = await listen<unknown>("hook-event", ({ payload }) => {
        if (isHookEvent(payload)) handler(payload);
      });
      let unlistenApprovals: (() => void) | undefined;
      let unlistenExpired: (() => void) | undefined;
      try {
        unlistenApprovals = await listen<unknown>("hook-approval", ({ payload }) => {
          if (isHookApproval(payload)) handler({ kind: "approval", approval: payload });
        });
        unlistenExpired = await listen<unknown>("hook-approval-expired", ({ payload }) => {
          if (isHookEvent(payload)) handler(payload);
        });
      } catch (error) {
        unlistenEvents();
        unlistenApprovals?.();
        throw error;
      }
      let listening = true;
      return () => {
        if (!listening) return;
        listening = false;
        try {
          unlistenApprovals?.();
          unlistenExpired?.();
        } finally {
          unlistenEvents();
        }
      };
    },
    async resizeWindow({ width, height }) {
      const window = getCurrentWindow();
      const [size, scale, monitors, position] = await Promise.all([window.innerSize(), window.scaleFactor(), availableMonitors(), window.outerPosition()]);
      // 改宽度时保持窗口原来的水平中心（岛在顶部居中，展开/收起不应该往右漂）
      const centeredX = Math.round(position.x + (size.width - width * scale) / 2);
      const value = clampWindowPlacement({ x: centeredX, y: position.y, width }, monitors, height);
      const monitor = monitors.find(m => value.x >= m.workArea.position.x && value.x < m.workArea.position.x + m.workArea.size.width && value.y >= m.workArea.position.y && value.y < m.workArea.position.y + m.workArea.size.height) ?? monitors[0];
      const boundedHeight = Math.min(height, monitor.workArea.size.height / monitor.scaleFactor);
      if (Math.abs(size.width / scale - value.width) >= 1 || Math.abs(size.height / scale - boundedHeight) >= 1) {
        await window.setSize(new LogicalSize(value.width, boundedHeight));
      }
      if (position.x !== value.x || position.y !== value.y) await window.setPosition(new PhysicalPosition(value.x, value.y));
    },
    async onWindowFocusChange(handler) {
      return await getCurrentWindow().onFocusChanged(({ payload }) => handler(payload));
    },
    async probeCliVersion(program) {
      return await invoke<CliVersionProbe>("probe_cli_version", { program });
    },
    async loadAppSettings() {
      return await invoke<string | null>("load_app_settings");
    },
    async saveAppSettings(settings) {
      await invoke("save_app_settings", { settings });
    },
    async getAppVersion() {
      const { getVersion } = await import("@tauri-apps/api/app");
      return await getVersion();
    },
    async checkAppUpdate() {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (!update) return null;
      return { version: update.version, notes: update.body ?? "" };
    },
    async installAppUpdate(onProgress) {
      const { check } = await import("@tauri-apps/plugin-updater");
      const update = await check();
      if (!update) return;
      let downloaded = 0;
      let total: number | null = null;
      await update.downloadAndInstall((event) => {
        if (event.event === "Started") {
          total = event.data.contentLength ?? null;
          onProgress?.(0, total);
        } else if (event.event === "Progress") {
          downloaded += event.data.chunkLength;
          onProgress?.(downloaded, total);
        } else if (event.event === "Finished") {
          onProgress?.(total ?? downloaded, total);
        }
      });
    },
    async startAgentTask(prompt) {
      return await invoke<string>("start_agent_task", { prompt });
    },
    async cancelAgentTask(id) {
      await invoke("cancel_agent_task", { id });
    },
    async listAgentTasks() {
      return await invoke<AgentTaskSnapshot[]>("list_agent_tasks");
    },
    async onAgentTask(handler) {
      const { listen } = await import("@tauri-apps/api/event");
      return await listen<AgentTaskSnapshot>("agent-task", (event) => handler(event.payload));
    },
    async bindSession(agentId, sessionId) {
      return await invoke<Array<[string, string]>>("bind_session", { agentId, sessionId });
    },
    async unbindSession(agentId) {
      return await invoke<Array<[string, string]>>("unbind_session", { agentId });
    },
    async listSessionBindings() {
      return await invoke<Array<[string, string]>>("list_session_bindings");
    },
    async searchSessions(query, limit) {
      return await invoke<SessionSearchHit[]>("search_sessions_command", { query, limit: limit ?? null });
    },
    async loadSessionByPath(path) {
      return await invoke<SessionView>("load_session_by_path", { path });
    },
    async revealSession(path) {
      await invoke("reveal_session", { path });
    },
    async openExternal(url) {
      await invoke("open_external", { url });
    },
    async listenIslandHotkey(handler) {
      return await listen("island-hotkey", () => handler());
    },
    async listApprovalRules() {
      return await invoke<ApprovalRule[]>("list_approval_rules");
    },
    async addApprovalRule(tool, pattern) {
      return await invoke<ApprovalRule[]>("add_approval_rule", { tool, pattern });
    },
    async removeApprovalRule(id) {
      return await invoke<ApprovalRule[]>("remove_approval_rule", { id });
    },
    async listApprovalAudit(limit) {
      return await invoke<ApprovalAuditEntry[]>("list_approval_audit", { limit: limit ?? null });
    },
    async listInbox() {
      return await invoke<import("./types").InboxItem[]>("list_inbox");
    },
    async resolveInboxItem(id) {
      return await invoke<import("./types").InboxItem[]>("resolve_inbox_item", { id });
    },
    async snoozeInboxItem(id, minutes) {
      return await invoke<import("./types").InboxItem[]>("snooze_inbox_item", { id, minutes });
    },
    async listenInboxChanges(handler) {
      return await listen("inbox-changed", () => handler());
    },
    async detectAgents() {
      return await invoke<AgentDetection[]>("detect_agents");
    },
    async refreshAgentDetection() {
      return await invoke<AgentDetection[]>("refresh_agent_detection");
    },
    async detectAgentCandidates() {
      return await invoke<AgentCandidate[]>("detect_agent_candidates");
    },
    async addAgentDefinition(name, program) {
      return await invoke<void>("add_agent_definition", { name, program });
    },
    async terminalOpen(request) {
      return await invoke<string>("terminal_open", {
        program: request.program,
        args: request.args ?? [],
        cwd: request.cwd ?? null,
        cols: request.cols,
        rows: request.rows,
      });
    },
    async terminalWrite(id, data) {
      await invoke("terminal_write", { id, data });
    },
    async terminalResize(id, cols, rows) {
      await invoke("terminal_resize", { id, cols, rows });
    },
    async terminalClose(id) {
      await invoke("terminal_close", { id });
    },
    async listenTerminalEvents(handler) {
      const unlistenData = await listen<unknown>("terminal-output", ({ payload }) => {
        if (isTerminalData(payload)) handler({ kind: "data", id: payload.id, data: payload.data });
      });
      let unlistenExit: () => void;
      try {
        unlistenExit = await listen<unknown>("terminal-exit", ({ payload }) => {
          if (isTerminalId(payload)) handler({ kind: "exit", id: payload.id });
        });
      } catch (error) {
        unlistenData();
        throw error;
      }
      let listening = true;
      return () => {
        if (!listening) return;
        listening = false;
        try {
          unlistenExit();
        } finally {
          unlistenData();
        }
      };
    },
    async teamCapabilities() {
      return await invoke<TeamCapability[]>("team_capabilities");
    },
    async openTeam() {
      await invoke("open_team");
    },
    async listenOpenTeam(handler) {
      return await listen("open-team", () => handler());
    },
    async startTeamPlan(goal, workspace, budgetUsd, concurrency) {
      return await invoke<TeamRun>("team_start_plan", {
        goal,
        workspace,
        budgetUsd: budgetUsd ?? null,
        concurrency: concurrency ?? 2,
      });
    },
    async continueTeamRun(id, instruction, budgetUsd, concurrency) {
      return await invoke<TeamRun>("team_continue_run", {
        id,
        instruction,
        budgetUsd: budgetUsd ?? null,
        concurrency: concurrency ?? null,
      });
    },
    async getTeamRun(id) {
      return await invoke<TeamRun | null>("team_get_run", { id });
    },
    async listTeamRuns() {
      return await invoke<TeamRun[]>("team_list_runs");
    },
    async confirmTeamPlan(
      id: string,
      tasks: TeamPlanTask[],
      allowRiskyVerification?: boolean,
    ) {
      return await invoke<TeamRun>("team_confirm_plan", {
        id,
        tasks,
        allowRiskyVerification: allowRiskyVerification ?? false,
      });
    },
    async checkTeamVerification(tasks: TeamPlanTask[]) {
      return await invoke<VerificationRisk[]>("team_verification_risks", { tasks });
    },
    async cancelTeamRun(id) {
      return await invoke<TeamRun>("team_cancel_run", { id });
    },
    async retryTeamRun(id) {
      return await invoke<TeamRun>("team_retry_run", { id });
    },
    async teamTaskDiff(id, taskId) {
      return await invoke<string>("team_task_diff", { id, taskId });
    },
    async mergeTeamTask(id, taskId) {
      return await invoke<TeamRun>("team_merge_task", { id, taskId });
    },
    async discardTeamTask(id, taskId) {
      return await invoke<TeamRun>("team_discard_task", { id, taskId });
    },
    async listenTeamRuns(handler) {
      return await listen<TeamRun>("team-run", (event) => handler(event.payload));
    },
  };
}

export const tauriBridge = createTauriBridge();








