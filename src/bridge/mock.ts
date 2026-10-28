import {
  assertSupportedSnapshot,
  type AgentCommand,
  type AgentIslandBridge,
  type CommandResult,
  type HookEvent,
  type SessionView,
  type TerminalEvent,
} from "./types";

export interface MockBridgeOptions {
  commandResults?: CommandResult[];
  /** probe_cli_version 返回的本地版本 */
  cliVersion?: string;
  /** 应用自身版本（getAppVersion） */
  appVersion?: string;
  /** updater 插件返回的可用版本；null/缺省表示已是最新 */
  appUpdateInfo?: { version: string; notes: string } | null;
  /** installAppUpdate 是否失败（用于测错误分支） */
  installFails?: boolean;
  /** 模拟"宿主没有 updater 插件"，用来覆盖退回到 GitHub release 提示的路径 */
  disableUpdater?: boolean;
  /** loadSessionByPath 返回的假会话（缺省则用路径构造一个空会话） */
  loadedSession?: SessionView | null;
  /** searchSessions 返回的假数据 */
  searchHits?: Array<{ agent: string; session: string; role: string; snippet: string; at_ms: number; path: string }>;
  /** 收件箱初始数据（浏览器测试用） */
  inbox?: import("./types").InboxItem[];
  /** Agent 检测诊断数据 */
  agentDetections?: import("./types").AgentDetection[];
}

export interface MockAgentIslandBridge extends AgentIslandBridge {
  commands(): readonly AgentCommand[];
  emitHookEvent(event: HookEvent): void;
  listenerCount(): number;
  windowSizes(): readonly { width: number; height: number }[];
  savedSettings(): readonly string[];
  openedUrls(): readonly string[];
  revealedPaths(): readonly string[];
  emitIslandHotkey(): void;
  terminalCalls(): readonly { kind: string; [key: string]: unknown }[];
  emitTerminal(event: TerminalEvent): void;
  emitInboxChanged(): void;
  inbox(): Array<import("./types").InboxItem>;
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

export function createMockBridge(
  snapshots: unknown | unknown[],
  options: MockBridgeOptions = {},
): MockAgentIslandBridge {
  const snapshotQueue = (Array.isArray(snapshots) ? snapshots : [snapshots]).map(clone);
  const resultQueue = (options.commandResults ?? []).map(clone);
  const commandLog: AgentCommand[] = [];
  const handlers = new Set<(event: HookEvent) => void>();
  const terminalHandlers = new Set<(event: TerminalEvent) => void>();
  const resizeLog: Array<{ width: number; height: number }> = [];
  const terminalLog: Array<{ kind: string; [key: string]: unknown }> = [];
  let terminalSequence = 0;
  let settingsBlob: string | null = null;
  const savedSettings: string[] = [];
  const hotkeyHandlers = new Set<() => void>();
  const openedUrls: string[] = [];
  const revealedPaths: string[] = [];
  const sessionBindings = new Map<string, string>();
  const agentTasks: Array<{ id: string; prompt: string }> = [];
  const taskHandlers = new Set<(task: import("./types").AgentTaskSnapshot) => void>();
  const searchHits: Array<{ agent: string; session: string; role: string; snippet: string; at_ms: number; path: string }> = options.searchHits ?? [];
  const approvalRules: Array<{ id: string; tool: string; pattern: string }> = [];
  const approvalAudit: Array<{ at_ms: number; id: string; tool: string; command: string; rule: string; decision: string }> = [];
  let inboxItems = clone(options.inbox ?? []);
  const agentDetections = clone(options.agentDetections ?? []);
  const inboxHandlers = new Set<() => void>();

  const bridge: MockAgentIslandBridge = {
    async getSnapshot() {
      if (snapshotQueue.length === 0) throw new Error("mock_snapshot_queue_empty");
      const snapshot = snapshotQueue.shift();
      return assertSupportedSnapshot(clone(snapshot));
    },
    async runCommand(command) {
      commandLog.push(clone(command));
      const result = resultQueue.shift();
      if (!result) throw new Error("mock_command_result_queue_empty");
      return clone(result);
    },
    async listenHookEvents(handler) {
      handlers.add(handler);
      let listening = true;
      return () => {
        if (!listening) return;
        listening = false;
        handlers.delete(handler);
      };
    },
    async resizeWindow(size) {
      resizeLog.push(clone(size));
    },
    async probeCliVersion(program) {
      return { program, version: options.cliVersion ?? "0.1.0", raw: "mock", ok: true };
    },
    async loadAppSettings() {
      return settingsBlob;
    },
    async saveAppSettings(settings) {
      settingsBlob = settings;
      savedSettings.push(settings);
    },
    async getAppVersion() {
      return options.appVersion ?? "1.9.0";
    },
    async checkAppUpdate() {
      return options.appUpdateInfo ?? null;
    },
    async installAppUpdate(onProgress) {
      if (options.installFails) throw new Error("download failed");
      onProgress?.(50, 100);
      onProgress?.(100, 100);
    },
    async startAgentTask(prompt) {
      const id = `task-${agentTasks.length + 1}`;
      agentTasks.push({ id, prompt });
      return id;
    },
    async cancelAgentTask(id) {
      for (const handler of taskHandlers) {
        handler({ id, agentId: "codex", state: "cancelled", sessionId: null, message: null });
      }
    },
    async listAgentTasks() {
      return agentTasks.map((task) => ({ id: task.id, agentId: "codex", state: "running" as const, sessionId: null, message: null }));
    },
    async onAgentTask(handler) {
      taskHandlers.add(handler);
      return () => taskHandlers.delete(handler);
    },
    async bindSession(agentId, sessionId) {
      sessionBindings.set(agentId, sessionId);
      return [...sessionBindings.entries()];
    },
    async unbindSession(agentId) {
      sessionBindings.delete(agentId);
      return [...sessionBindings.entries()];
    },
    async listSessionBindings() {
      return [...sessionBindings.entries()];
    },
    async searchSessions(query, limit = 20) {
      const needle = query.trim().toLowerCase();
      return searchHits
        .filter((hit) => needle.length >= 2 && hit.snippet.toLowerCase().includes(needle))
        .slice(0, limit)
        .map((hit) => ({ ...hit }));
    },
    async loadSessionByPath(path) {
      if (options.loadedSession !== undefined) {
        if (!options.loadedSession) throw new Error("无法解析这个会话文件");
        return structuredClone(options.loadedSession);
      }
      return {
        id: path, name: "搜索结果会话", cwd: null, log_path: path, records: [],
        recent_output: [], current_file: null, log_status: null, alert: null,
        lifecycle: "Historical", last_active_at_ms: 0, display_status: "idle",
      };
    },
    async revealSession(path) {
      revealedPaths.push(path);
    },
    async openExternal(url) {
      openedUrls.push(url);
    },
    async listenIslandHotkey(handler) {
      hotkeyHandlers.add(handler);
      return () => hotkeyHandlers.delete(handler);
    },
    async listApprovalRules() {
      return approvalRules.map((rule) => ({ ...rule }));
    },
    async addApprovalRule(tool, pattern) {
      approvalRules.push({ id: `mock-rule-${approvalRules.length + 1}`, tool, pattern });
      return approvalRules.map((rule) => ({ ...rule }));
    },
    async removeApprovalRule(id) {
      const index = approvalRules.findIndex((rule) => rule.id === id);
      if (index >= 0) approvalRules.splice(index, 1);
      return approvalRules.map((rule) => ({ ...rule }));
    },
    async listApprovalAudit(limit) {
      return approvalAudit.slice(0, limit ?? 20).map((entry) => ({ ...entry }));
    },
    async listInbox() {
      return clone(inboxItems);
    },
    async resolveInboxItem(id) {
      inboxItems = inboxItems.map((item) => item.id === id
        ? { ...item, state: "resolved" as const, resolved_at_ms: Date.now(), snooze_until_ms: null }
        : item);
      for (const handler of inboxHandlers) handler();
      return clone(inboxItems);
    },
    async snoozeInboxItem(id, minutes) {
      inboxItems = inboxItems.map((item) => item.id === id
        ? { ...item, state: "snoozed" as const, snooze_until_ms: Date.now() + minutes * 60_000 }
        : item);
      for (const handler of inboxHandlers) handler();
      return clone(inboxItems);
    },
    async listenInboxChanges(handler) {
      inboxHandlers.add(handler);
      return () => inboxHandlers.delete(handler);
    },
    async detectAgents() {
      return clone(agentDetections);
    },
    async refreshAgentDetection() {
      return clone(agentDetections);
    },
    async terminalOpen(request) {
      terminalSequence += 1;
      const id = `mock-term-${terminalSequence}`;
      terminalLog.push({ kind: "open", id, program: request.program, cwd: request.cwd ?? null });
      return id;
    },
    async terminalWrite(id, data) {
      terminalLog.push({ kind: "write", id, data });
    },
    async terminalResize(id, cols, rows) {
      terminalLog.push({ kind: "resize", id, cols, rows });
    },
    async terminalClose(id) {
      terminalLog.push({ kind: "close", id });
    },
    async listenTerminalEvents(handler) {
      terminalHandlers.add(handler);
      let listening = true;
      return () => {
        if (!listening) return;
        listening = false;
        terminalHandlers.delete(handler);
      };
    },
    terminalCalls() {
      return terminalLog.map((entry) => clone(entry));
    },
    emitTerminal(event) {
      for (const handler of [...terminalHandlers]) handler({ ...event });
    },
    emitInboxChanged() {
      for (const handler of inboxHandlers) handler();
    },
    inbox() {
      return clone(inboxItems);
    },
    commands() {
      return commandLog.map(clone);
    },
    emitHookEvent(event) {
      for (const handler of [...handlers]) handler(clone(event));
    },
    listenerCount() {
      return handlers.size;
    },
    emitIslandHotkey() {
      for (const handler of hotkeyHandlers) handler();
    },
    revealedPaths() {
      return [...revealedPaths];
    },
    openedUrls() {
      return openedUrls.map((url) => url);
    },
    savedSettings() {
      return savedSettings.map((entry) => entry);
    },
    windowSizes() {
      return resizeLog.map(clone);
    },
  };
  if (options.disableUpdater) {
    // 模拟没有 updater 插件的宿主：界面应退回"打开下载页"
    delete (bridge as Partial<MockAgentIslandBridge>).checkAppUpdate;
    delete (bridge as Partial<MockAgentIslandBridge>).installAppUpdate;
  }
  return bridge;
}














