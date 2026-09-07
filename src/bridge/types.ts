export type ProcessState = "stopped" | "running";
export type TurnState = "idle" | "executing" | "succeeded" | "failed";
export type AttentionState = "none" | "input_required" | "approval_required";
export type DisplayStatus = "stopped" | "idle" | "working" | "done" | "error" | "waiting";

export interface Freshness {
  observed_at_ms: number;
  stale: boolean;
}

export interface AgentState {
  process: ProcessState;
  turn: TurnState;
  attention: AttentionState;
  result_at_ms: number | null;
}

export type MessageRole = "User" | "Assistant" | "Tool";

export interface ConversationMessage {
  event_id: string;
  at_ms: number;
  role: MessageRole;
  text: string;
}

export type SessionLifecycle = "Active" | "Historical";

export interface SessionView {
  id: string;
  name: string;
  cwd: string | null;
  log_path: string | null;
  records: ConversationMessage[];
  recent_output: string[];
  current_file: string | null;
  log_status: string | null;
  alert: string | null;
  lifecycle: SessionLifecycle;
  last_active_at_ms: number;
  display_status: DisplayStatus;
}

export type SessionSummary = SessionView;

export interface ActiveTurnIdentity {
  agent_id: string;
  session_id: string;
  turn_id: string;
}

export interface DiagnosticView {
  adapter: string;
  code: string;
  freshness: Freshness;
  event_type: string | null;
  event_count: number;
  message_count: number;
  skipped_lines?: number;
  candidate_count?: number;
}

export interface AgentView {
  id: string;
  name: string;
  state: AgentState;
  display_status: DisplayStatus;
  active_session: SessionView | null;
  active_turn: ActiveTurnIdentity | null;
  history_sessions: SessionSummary[];
  diagnostic: DiagnosticView | null;
  freshness: Freshness;
}

/** Rust's serialized `AgentViewSnapshot`. The bridge narrows the version to 1. */
export interface AgentViewSnapshot {
  schema_version: number;
  generated_at_ms: number;
  agents: AgentView[];
}

export function assertSupportedSnapshot(
  snapshot: AgentViewSnapshot,
): AgentViewSnapshot {
  if (snapshot.schema_version !== 1) {
    throw new Error("unsupported_snapshot_schema");
  }
  return snapshot;
}

export type HookEvent =
  | { kind: "stop"; session: string }
  | { kind: "notification"; message: string; session: string };

export interface HookApproval {
  id: string;
  session: string;
  tool: string;
  command: string;
  cwd: string;
}

export type AgentCommand =
  | { name: "reload_agent_defs" }
  | { name: "stop_agent"; agentName: string }
  | { name: "restart_agent"; agentName: string }
  | { name: "open_project_dir"; agentName: string }
  | { name: "open_terminal"; agentName: string }
  | { name: "focus_agent_terminal"; agentName: string }
  | { name: "open_path"; path: string }
  | { name: "open_overview" }
  | { name: "open_session_terminal"; agentName: string; sessionId: string }
  | { name: "restart_session"; agentName: string; sessionId: string }
  | { name: "stop_session"; agentName: string; sessionId: string }
  | { name: "send_to_session"; agentName: string; sessionId: string; prompt: string }
  | { name: "get_send_output"; taskId: string }
  | { name: "get_hook_status" }
  | { name: "set_hook_enabled"; enabled: boolean }
  | { name: "respond_hook_approval"; id: string; allow: boolean }
  | { name: "dismiss_hook_approval"; id: string }
  | { name: "get_autostart" }
  | { name: "set_autostart"; enabled: boolean }
  | { name: "privacy_active" }
  | { name: "get_stats_report"; days: number };

export interface SendOutput {
  done: boolean;
  lines: string[];
}

export interface AgentStatsRow {
  name: string;
  total_seconds: number;
  error_count: number;
  done_count: number;
}

export interface StatsReportDay {
  date: string;
  agents: AgentStatsRow[];
}

export interface StatsReport {
  days: StatsReportDay[];
}

export type CommandValue =
  | boolean
  | string
  | SendOutput
  | StatsReport;

export type CommandResult =
  | { ok: true; value?: CommandValue }
  | {
      ok: false;
      code: string;
      message: string;
      retryable: boolean;
    };

export interface AgentIslandBridge {
  getSnapshot(): Promise<AgentViewSnapshot>;
  runCommand(command: AgentCommand): Promise<CommandResult>;
  listenHookEvents(handler: (event: HookEvent) => void): Promise<() => void>;
}
