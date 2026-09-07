import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";

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
  const [code, retryable] = normalized.includes("prompt is empty")
    ? ["prompt_empty", false]
    : normalized.includes("task not found")
      ? ["task_not_found", false]
      : normalized.includes("not supported")
        ? ["not_supported", false]
        : normalized.includes("permission denied")
          ? ["permission_denied", false]
          : normalized.includes("no process found")
            ? ["no_process_found", true]
            : normalized.includes("agent is not running")
              ? ["agent_not_running", true]
              : normalized.includes("session not found")
                ? ["session_not_found", true]
                : normalized.includes("state lock error")
                  ? ["state_unavailable", true]
                  : ["command_failed", true];
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

export function createTauriBridge(): AgentIslandBridge {
  return {
    async getSnapshot() {
      const snapshot = await invoke<AgentViewSnapshot>("get_agent_snapshot");
      return assertSupportedSnapshot(snapshot);
    },
    async runCommand(command) {
      const invocation = commandInvocation(command);
      try {
        const value = await invoke<CommandValue | undefined>(
          invocation.commandName,
          invocation.args,
        );
        return value === undefined ? { ok: true } : { ok: true, value };
      } catch (error) {
        return commandError(error);
      }
    },
    async listenHookEvents(handler) {
      return listen<unknown>("hook-event", ({ payload }) => {
        if (isHookEvent(payload)) handler(payload);
      });
    },
  };
}

export const tauriBridge = createTauriBridge();
