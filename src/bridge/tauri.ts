import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import { getCurrentWindow, LogicalSize } from "@tauri-apps/api/window";

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
  type CommandValue,
  type CommandResult,
  type HookApproval,
  type HookEvent,
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
        args: { id: command.id, allow: command.allow },
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
  if (typeof value.session !== "string") return false;
  return (
    value.kind === "stop" ||
    (value.kind === "notification" && typeof value.message === "string")
  );
}

function isHookApproval(payload: unknown): payload is HookApproval {
  if (!payload || typeof payload !== "object") return false;
  const value = payload as Record<string, unknown>;
  return (
    typeof value.id === "string" && value.id.length > 0 &&
    typeof value.session === "string" &&
    typeof value.tool === "string" &&
    typeof value.command === "string" &&
    typeof value.cwd === "string"
  );
}

export function createTauriBridge(): AgentIslandBridge {
  return {
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
      let unlistenApprovals: () => void;
      try {
        unlistenApprovals = await listen<unknown>("hook-approval", ({ payload }) => {
          if (isHookApproval(payload)) handler({ kind: "approval", approval: payload });
        });
      } catch (error) {
        unlistenEvents();
        throw error;
      }
      let listening = true;
      return () => {
        if (!listening) return;
        listening = false;
        try {
          unlistenApprovals();
        } finally {
          unlistenEvents();
        }
      };
    },
    async resizeWindow({ width, height }) {
      await getCurrentWindow().setSize(new LogicalSize(width, height));
    },
  };
}

export const tauriBridge = createTauriBridge();
