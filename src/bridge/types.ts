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

const PROCESS_STATES: readonly ProcessState[] = ["stopped", "running"];
const TURN_STATES: readonly TurnState[] = [
  "idle",
  "executing",
  "succeeded",
  "failed",
];
const ATTENTION_STATES: readonly AttentionState[] = [
  "none",
  "input_required",
  "approval_required",
];
const DISPLAY_STATUSES: readonly DisplayStatus[] = [
  "stopped",
  "idle",
  "working",
  "done",
  "error",
  "waiting",
];
const MESSAGE_ROLES: readonly MessageRole[] = ["User", "Assistant", "Tool"];
const SESSION_LIFECYCLES: readonly SessionLifecycle[] = ["Active", "Historical"];

function isObject(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function isNonnegativeInteger(value: unknown): value is number {
  return typeof value === "number" && Number.isSafeInteger(value) && value >= 0;
}

function isNullableString(value: unknown): value is string | null {
  return value === null || typeof value === "string";
}

function isEnumValue<T extends string>(
  value: unknown,
  values: readonly T[],
): value is T {
  return typeof value === "string" && values.includes(value as T);
}

function isFreshness(value: unknown): value is Freshness {
  return (
    isObject(value) &&
    isNonnegativeInteger(value.observed_at_ms) &&
    typeof value.stale === "boolean"
  );
}

function isAgentState(value: unknown): value is AgentState {
  return (
    isObject(value) &&
    isEnumValue(value.process, PROCESS_STATES) &&
    isEnumValue(value.turn, TURN_STATES) &&
    isEnumValue(value.attention, ATTENTION_STATES) &&
    (value.result_at_ms === null || isNonnegativeInteger(value.result_at_ms))
  );
}

function isConversationMessage(value: unknown): value is ConversationMessage {
  return (
    isObject(value) &&
    typeof value.event_id === "string" &&
    isNonnegativeInteger(value.at_ms) &&
    isEnumValue(value.role, MESSAGE_ROLES) &&
    typeof value.text === "string"
  );
}

function isSessionView(value: unknown): value is SessionView {
  return (
    isObject(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    isNullableString(value.cwd) &&
    isNullableString(value.log_path) &&
    Array.isArray(value.records) &&
    value.records.every(isConversationMessage) &&
    Array.isArray(value.recent_output) &&
    value.recent_output.every((line) => typeof line === "string") &&
    isNullableString(value.current_file) &&
    isNullableString(value.log_status) &&
    isNullableString(value.alert) &&
    isEnumValue(value.lifecycle, SESSION_LIFECYCLES) &&
    isNonnegativeInteger(value.last_active_at_ms) &&
    isEnumValue(value.display_status, DISPLAY_STATUSES)
  );
}

function isActiveTurnIdentity(value: unknown): value is ActiveTurnIdentity {
  return (
    isObject(value) &&
    typeof value.agent_id === "string" &&
    typeof value.session_id === "string" &&
    typeof value.turn_id === "string"
  );
}

function isDiagnosticView(value: unknown): value is DiagnosticView {
  return (
    isObject(value) &&
    typeof value.adapter === "string" &&
    typeof value.code === "string" &&
    isFreshness(value.freshness) &&
    isNullableString(value.event_type) &&
    isNonnegativeInteger(value.event_count) &&
    isNonnegativeInteger(value.message_count) &&
    (value.skipped_lines === undefined || isNonnegativeInteger(value.skipped_lines)) &&
    (value.candidate_count === undefined || isNonnegativeInteger(value.candidate_count))
  );
}

function isAgentView(value: unknown): value is AgentView {
  return (
    isObject(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    isAgentState(value.state) &&
    isEnumValue(value.display_status, DISPLAY_STATUSES) &&
    (value.active_session === null || isSessionView(value.active_session)) &&
    (value.active_turn === null || isActiveTurnIdentity(value.active_turn)) &&
    Array.isArray(value.history_sessions) &&
    value.history_sessions.every(isSessionView) &&
    (value.diagnostic === null || isDiagnosticView(value.diagnostic)) &&
    isFreshness(value.freshness)
  );
}

export function assertSupportedSnapshot(snapshot: unknown): AgentViewSnapshot {
  if (
    isObject(snapshot) &&
    "schema_version" in snapshot &&
    snapshot.schema_version !== 1
  ) {
    throw new Error("unsupported_snapshot_schema");
  }
  if (
    !isObject(snapshot) ||
    snapshot.schema_version !== 1 ||
    !isNonnegativeInteger(snapshot.generated_at_ms) ||
    !Array.isArray(snapshot.agents) ||
    !snapshot.agents.every(isAgentView)
  ) {
    throw new Error("invalid_snapshot_shape");
  }
  return snapshot as unknown as AgentViewSnapshot;
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
