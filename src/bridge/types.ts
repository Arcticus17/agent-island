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
  /** 工具名（Bash / Edit / Read …）；普通消息没有。 */
  tool?: string | null;
  /** 工具的一行入参（命令、文件路径、搜索词）。 */
  detail?: string | null;
  /** 工具结果是否成功；结果还没到或不是工具消息时为空。 */
  success?: boolean | null;
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

export interface SourceHealth {
  source: string;
  status: string;
  records: number;
}

export interface UsageInfo {
  tokens_total: number;
  tokens_output: number;
  cost_usd: number | null;
  used_percent: number | null;
  window_secs: number;
  resets_at_secs: number | null;
  credits: number | null;
  unlimited: boolean | null;
  stale: boolean;
}

export interface AgentView {
  id: string;
  name: string;
  can_restart?: boolean;
  state: AgentState;
  display_status: DisplayStatus;
  active_session: SessionView | null;
  active_turn: ActiveTurnIdentity | null;
  history_sessions: SessionSummary[];
  diagnostic: DiagnosticView | null;
  freshness: Freshness;
  usage: UsageInfo | null;
  /** 会话身份的可信来源：launched（岛内终端后代）/ hooked（Agent 自报）/ inferred（推断） */
  binding?: "launched" | "hooked" | "inferred" | "manual";
  /** 是否有日志适配器；自定义 Agent（如 Pi）只有进程状态 */
  has_log_adapter?: boolean;
  /** 来源：builtin（默认四个）/ custom（用户添加）/ detected（本机自动发现） */
  source?: "builtin" | "detected" | "custom";
  /** 终端里可直接启动的命令或完整路径 */
  program?: string;
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

/** 可选字段：后端会跳过空值，读回来可能是 undefined。 */
function isOptionalString(value: unknown): value is string | null | undefined {
  return value === undefined || value === null || typeof value === "string";
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
    typeof value.text === "string" &&
    isOptionalString(value.tool) &&
    isOptionalString(value.detail) &&
    (value.success === undefined || value.success === null || typeof value.success === "boolean")
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

function isNullableNonnegativeNumber(value: unknown): value is number | null {
  return value === null ||
    (typeof value === "number" && Number.isFinite(value) && value >= 0);
}

function isUsageInfo(value: unknown): value is UsageInfo {
  return (
    isObject(value) &&
    isNonnegativeInteger(value.tokens_total) &&
    isNonnegativeInteger(value.tokens_output) &&
    isNullableNonnegativeNumber(value.cost_usd) &&
    isNullableNonnegativeNumber(value.used_percent) &&
    isNonnegativeInteger(value.window_secs) &&
    (value.resets_at_secs === null || isNonnegativeInteger(value.resets_at_secs)) &&
    isNullableNonnegativeNumber(value.credits) &&
    (value.unlimited === null || typeof value.unlimited === "boolean") &&
    typeof value.stale === "boolean"
  );
}

function isAgentView(value: unknown): value is AgentView {
  return (
    isObject(value) &&
    typeof value.id === "string" &&
    typeof value.name === "string" &&
    (value.can_restart === undefined || typeof value.can_restart === "boolean") &&
    isAgentState(value.state) &&
    isEnumValue(value.display_status, DISPLAY_STATUSES) &&
    (value.active_session === null || isSessionView(value.active_session)) &&
    (value.active_turn === null || isActiveTurnIdentity(value.active_turn)) &&
    Array.isArray(value.history_sessions) &&
    value.history_sessions.every(isSessionView) &&
    (value.diagnostic === null || isDiagnosticView(value.diagnostic)) &&
    isFreshness(value.freshness) &&
    (value.usage === null || isUsageInfo(value.usage)) &&
    (value.has_log_adapter === undefined || typeof value.has_log_adapter === "boolean") &&
    (value.source === undefined ||
      value.source === "builtin" ||
      value.source === "detected" ||
      value.source === "custom") &&
    (value.program === undefined || typeof value.program === "string")
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
  | { kind: "notification"; message: string; session: string }
  | { kind: "approval"; approval: HookApproval }
  | { kind: "approval_expired"; ids: string[] };

export interface HookApproval {
  id: string;
  session: string;
  tool: string;
  command: string;
  cwd: string;
  /** 用于“记住这条”的匹配键：Bash 是命令，Edit/Write 是文件路径。 */
  rule_key?: string;
  /** 本地规则评估出的风险等级（缺省表示旧宿主没有评估） */
  risk?: "low" | "medium" | "high";
  risk_reason?: string;
  repo?: string;
  branch?: string;
  diff?: ApprovalDiff;
  created_at_ms?: number;
  expires_at_ms?: number;
}

export interface ApprovalDiff {
  file: string;
  added: number;
  removed: number;
  lines: ApprovalDiffLine[];
  truncated: boolean;
}

export interface ApprovalDiffLine {
  kind: "ctx" | "add" | "del";
  text: string;
}

export type ApprovalScope = "once" | "ten_minutes" | "session";

export type InboxKind = "approval" | "notification" | "done" | "question" | "system";
export type InboxSeverity = "low" | "normal" | "high" | "critical";
export type InboxState = "open" | "snoozed" | "resolved";

export interface InboxItem {
  id: string;
  kind: InboxKind;
  severity: InboxSeverity;
  agent_id: string;
  agent_name: string;
  session_id: string;
  title: string;
  body: string;
  state: InboxState;
  created_at_ms: number;
  snooze_until_ms: number | null;
  resolved_at_ms: number | null;
  dedupe_key: string;
  approval_id: string | null;
}

export interface AgentDetection {
  id: string;
  name: string;
  keyword: string;
  installed: boolean;
  running: boolean;
  process_count: number;
  /** 实际解析到的可执行文件；未安装时为 null */
  path: string | null;
  /** process | path | env:变量名 | missing */
  source: string;
}

export interface AgentCandidate {
  name: string;
  program: string;
  description: string;
  installed: boolean;
  configured: boolean;
  path: string | null;
  keyword: string;
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
  | { name: "run_task"; agentName: string; prompt: string }
  | { name: "get_send_output"; taskId: string }
  | { name: "get_hook_status" }
  | { name: "set_hook_enabled"; enabled: boolean }
  | {
      name: "respond_hook_approval";
      id: string;
      allow: boolean;
      /** 缺省/once 表示只允许这一次；ten_minutes/session 创建内存授权 */
      scope?: ApprovalScope;
      /** 拒绝理由，会回传给 Agent */
      reason?: string;
    }
  | { name: "dismiss_hook_approval"; id: string }
  | { name: "get_autostart" }
  | { name: "set_autostart"; enabled: boolean }
  | { name: "privacy_active" }
  | { name: "get_stats_report"; days: number };

export interface AgentTaskSnapshot {
  id: string;
  agentId: string;
  state: "running" | "succeeded" | "failed" | "cancelled";
  sessionId: string | null;
  message: string | null;
  /** 已产生的输出行（前端累积，用于复用发送回执） */
  lines?: string[];
}

export interface SendOutput {
  /** 任务模式：结束但失败（用于把「已结束」和「失败」区分开） */
  failed?: boolean;
  done: boolean;
  lines: string[];
}

export type TeamRunPhase =
  | "planning"
  | "awaiting_plan"
  | "running"
  | "summarizing"
  | "succeeded"
  | "failed"
  | "cancelled";

export type TeamTaskState =
  | "queued"
  | "running"
  | "waiting_approval"
  | "succeeded"
  | "failed"
  | "cancelled"
  | "skipped";

export type TeamMergeState = "none" | "pending" | "merged" | "discarded" | "conflict";

export interface TeamCapability {
  id: string;
  name: string;
  program: string;
  installed: boolean;
  canPlan: boolean;
  canWork: boolean;
  available: boolean;
  /** 面向用户的说明：为什么可用/不可用。 */
  detail: string;
}

/** 主 Agent 产出的计划任务；字段沿用后端 snake_case 协议。 */
export interface TeamPlanTask {
  id: string;
  title: string;
  assignee: string;
  prompt: string;
  depends_on: string[];
  acceptance: string;
  /** 可选的执行级验收命令；空字符串表示不跑。 */
  verify_command: string;
}

export interface VerificationRisk {
  taskId: string;
  taskTitle: string;
  command: string;
  reason: string;
}

export interface TaskVerification {
  command: string;
  passed: boolean;
  exitCode: number | null;
  timedOut: boolean;
  durationMs: number;
  output: string;
}

export interface TeamTask {
  id: string;
  title: string;
  assignee: string;
  assigneeName: string;
  prompt: string;
  dependsOn: string[];
  acceptance: string;
  /** 计划中声明的验收命令；null 表示不跑执行级验收。 */
  verifyCommand: string | null;
  /** 最近一次执行级验收结果。 */
  verification: TaskVerification | null;
  state: TeamTaskState;
  startedAtMs: number | null;
  endedAtMs: number | null;
  exitCode: number | null;
  result: string | null;
  error: string | null;
  costUsd: number | null;
  log: string[];
  sessionId: string | null;
  pendingApproval: string | null;
  approvalId: string | null;
  worktreePath: string | null;
  branch: string | null;
  baseCommit: string | null;
  changedFiles: string[];
  diffStat: string | null;
  mergeState: TeamMergeState;
  mergeError: string | null;
}

export interface TeamRun {
  id: string;
  goal: string;
  workspace: string;
  mainAgent: string;
  mainAgentName: string;
  phase: TeamRunPhase;
  planSummary: string | null;
  summary: string | null;
  tasks: TeamTask[];
  createdAtMs: number;
  updatedAtMs: number;
  error: string | null;
  plannerSessionId: string | null;
  /** 本轮 USD 预算；null 表示不设上限（旧记录）。 */
  budgetUsd: number | null;
  spentUsd: number;
  repoRoot: string | null;
  baseBranch: string | null;
  baseCommit: string | null;
  worktreeMode: boolean;
  workspaceWarning: string | null;
  concurrency: number;
  parentRunId: string | null;
  round: number;
}

export interface AgentStatsRow {
  name: string;
  total_seconds: number;
  error_count: number;
  done_count: number;
}export interface StatsReportDay {
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
  /** 调整原生窗口尺寸；minWidth 防止展开态被拖到内容无法排版的宽度。 */
  resizeWindow?(size: { width: number; height: number; minWidth?: number }): Promise<void>;
  /** 设置窗口最小宽度；展开态 360，收起态 80。 */
  setWindowMinWidth?(width: number): Promise<void>;
  /** 窗口获得/失去焦点：展开态点外面空白处收起靠它判断 */
  onWindowFocusChange?(handler: (focused: boolean) => void): Promise<() => void>;
  startWindowDrag?(): Promise<void>;
  startWindowResize?(): Promise<void>;
  initializeWindowPlacement?(onError?: (error: unknown) => void): Promise<() => void>;
  adjustWindowWidth?(delta: number, minWidth?: number): Promise<void>;
  /** 内置终端：在岛里直接跑 Agent 的交互式 CLI（Windows 走 ConPTY）。 */
  terminalOpen?(request: TerminalOpenRequest): Promise<string>;
  terminalWrite?(id: string, data: string): Promise<void>;
  terminalResize?(id: string, cols: number, rows: number): Promise<void>;
  terminalClose?(id: string): Promise<void>;
  listenTerminalEvents?(handler: (event: TerminalEvent) => void): Promise<() => void>;
  /** 探测 Agent CLI 的本地版本（跑 `<cli> --version`，不需要联网）。 */
  probeCliVersion?(program: string): Promise<CliVersionProbe>;
  /** 设置的磁盘镜像（强杀进程也不丢）：读回上次保存的 JSON。 */
  loadAppSettings?(): Promise<string | null>;
  /** 设置的磁盘镜像：原子写入 JSON。 */
  saveAppSettings?(settings: string): Promise<void>;
  /** 应用自身版本（用于"关于/更新"提示）。 */
  getAppVersion?(): Promise<string>;
  /** 通过 Tauri updater 检查（签名校验的内置更新通道）；null 表示已是最新。 */
  checkAppUpdate?(): Promise<AppUpdateInfo | null>;
  /** 下载并安装更新；安装完成后应用会退出。 */
  installAppUpdate?(onProgress?: (downloaded: number, total: number | null) => void): Promise<void>;
  /** 岛内任务模式：用 `codex exec --json` 跑一次性任务，事件权威。 */
  startAgentTask?(prompt: string): Promise<string>;
  cancelAgentTask?(id: string): Promise<void>;
  listAgentTasks?(): Promise<AgentTaskSnapshot[]>;
  onAgentTask?(handler: (task: AgentTaskSnapshot) => void): Promise<() => void>;
  /** 手动把某个 Agent 绑定到指定会话（跨重启保留）。 */
  bindSession?(agentId: string, sessionId: string): Promise<Array<[string, string]>>;
  unbindSession?(agentId: string): Promise<Array<[string, string]>>;
  listSessionBindings?(): Promise<Array<[string, string]>>;
  /** 跨会话搜索：在本地会话文件里找关键字。 */
  searchSessions?(query: string, limit?: number): Promise<SessionSearchHit[]>;
  /** 按会话文件路径加载完整会话（搜索结果跳转用）。 */
  loadSessionByPath?(path: string): Promise<SessionView>;
  /** 在资源管理器里定位会话文件。 */
  revealSession?(path: string): Promise<void>;
  /** 用系统默认浏览器打开 https 链接（下载页）。 */
  openExternal?(url: string): Promise<void>;
  /** 全局快捷键（Ctrl+Shift+A）唤出/收起。 */
  listenIslandHotkey?(handler: () => void): Promise<() => void>;
  /** 审批允许清单：命中规则的 PreToolUse 直接放行，并写入审计。 */
  listApprovalRules?(): Promise<ApprovalRule[]>;
  addApprovalRule?(tool: string, pattern: string): Promise<ApprovalRule[]>;
  removeApprovalRule?(id: string): Promise<ApprovalRule[]>;
  listApprovalAudit?(limit?: number): Promise<ApprovalAuditEntry[]>;
  /** 收件箱：审批 / 通知 / 完成事件的本地持久队列。 */
  listInbox?(): Promise<InboxItem[]>;
  resolveInboxItem?(id: string): Promise<InboxItem[]>;
  snoozeInboxItem?(id: string, minutes: number): Promise<InboxItem[]>;
  listenInboxChanges?(handler: () => void): Promise<() => void>;
  /** 诊断：说明每个配置的 Agent 为什么显示/隐藏，以及解析到的可执行文件。 */
  detectAgents?(): Promise<AgentDetection[]>;
  /** 同上，但绕过 60 秒可执行文件缓存。 */
  refreshAgentDetection?(): Promise<AgentDetection[]>;
  /** “+” 面板：配置内 Agent + 本机找到的候选 CLI。 */
  detectAgentCandidates?(): Promise<AgentCandidate[]>;
  /** 把本机检测到的候选写进 ~/.agent-island/agents.json。 */
  addAgentDefinition?(name: string, program: string): Promise<void>;
  /** 团队任务：当前可规划/可派发的 Agent 及原因。 */
  /** 打开始终在顶部的总览窗口，并切到团队页。 */
  openTeam?(): Promise<void>;
  listenOpenTeam?(handler: () => void): Promise<() => void>;
  teamCapabilities?(): Promise<TeamCapability[]>;
  /** 团队任务：让主 Agent 生成计划；budgetUsd 是本次运行的硬上限，concurrency 默认 2。 */
  startTeamPlan?(
    goal: string,
    workspace: string,
    budgetUsd?: number | null,
    concurrency?: number,
  ): Promise<TeamRun>;
  /** 基于上一轮汇总继续规划下一轮；要求上一轮改动都已合并或丢弃。 */
  continueTeamRun?(
    id: string,
    instruction: string,
    budgetUsd?: number | null,
    concurrency?: number,
  ): Promise<TeamRun>;
  getTeamRun?(id: string): Promise<TeamRun | null>;
  listTeamRuns?(): Promise<TeamRun[]>;
  /** 确认（可编辑的）计划并开始执行。 */
  confirmTeamPlan?(
    id: string,
    tasks: TeamPlanTask[],
    allowRiskyVerification?: boolean,
  ): Promise<TeamRun>;
  /** 检查计划里的验收命令风险；高风险需要再次确认。 */
  checkTeamVerification?(tasks: TeamPlanTask[]): Promise<VerificationRisk[]>;
  cancelTeamRun?(id: string): Promise<TeamRun>;
  /** 把未完成的任务重新排队执行（已成功的任务不重跑）。 */
  retryTeamRun?(id: string): Promise<TeamRun>;
  /** 查看某个任务 worktree 分支相对基准提交的 diff。 */
  teamTaskDiff?(id: string, taskId: string): Promise<string>;
  /** 把任务分支合并回主工作区（要求主工作区干净）。 */
  mergeTeamTask?(id: string, taskId: string): Promise<TeamRun>;
  /** 丢弃任务 worktree 与分支，保留任务记录。 */
  discardTeamTask?(id: string, taskId: string): Promise<TeamRun>;
  listenTeamRuns?(handler: (run: TeamRun) => void): Promise<() => void>;
}

export interface AppUpdateInfo {
  version: string;
  notes: string;
}

export interface SessionSearchHit {
  agent: string;
  session: string;
  role: string;
  snippet: string;
  at_ms: number;
  path: string;
}

export interface ApprovalRule {
  id: string;
  tool: string;
  pattern: string;
}

export interface ApprovalAuditEntry {
  at_ms: number;
  id: string;
  tool: string;
  command: string;
  rule: string;
  decision: string;
}

export interface CliVersionProbe {
  program: string;
  version: string | null;
  raw: string;
  ok: boolean;
}

export interface TerminalOpenRequest {
  program: string;
  args?: string[];
  cwd?: string | null;
  cols: number;
  rows: number;
}

export type TerminalEvent =
  | { kind: "data"; id: string; data: string }
  | { kind: "exit"; id: string };








