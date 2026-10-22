import {
  assertSupportedSnapshot,
  type AgentCommand,
  type AgentIslandBridge,
  type CommandResult,
  type HookEvent,
  type TerminalEvent,
} from "./types";

export interface MockBridgeOptions {
  commandResults?: CommandResult[];
  /** probe_cli_version 返回的本地版本 */
  cliVersion?: string;
  /** 应用自身版本（getAppVersion） */
  appVersion?: string;
  /** searchSessions 返回的假数据 */
  searchHits?: Array<{ agent: string; session: string; role: string; snippet: string; at_ms: number; path: string }>;
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
  const searchHits: Array<{ agent: string; session: string; role: string; snippet: string; at_ms: number; path: string }> = options.searchHits ?? [];
  const approvalRules: Array<{ id: string; tool: string; pattern: string }> = [];
  const approvalAudit: Array<{ at_ms: number; id: string; tool: string; command: string; rule: string; decision: string }> = [];

  return {
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
    async searchSessions(query, limit = 20) {
      const needle = query.trim().toLowerCase();
      return searchHits
        .filter((hit) => needle.length >= 2 && hit.snippet.toLowerCase().includes(needle))
        .slice(0, limit)
        .map((hit) => ({ ...hit }));
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
}






