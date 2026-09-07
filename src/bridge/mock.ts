import {
  assertSupportedSnapshot,
  type AgentCommand,
  type AgentIslandBridge,
  type AgentViewSnapshot,
  type CommandResult,
  type HookEvent,
} from "./types";

export interface MockBridgeOptions {
  commandResults?: CommandResult[];
}

export interface MockAgentIslandBridge extends AgentIslandBridge {
  commands(): readonly AgentCommand[];
  emitHookEvent(event: HookEvent): void;
  listenerCount(): number;
}

function clone<T>(value: T): T {
  return structuredClone(value);
}

export function createMockBridge(
  snapshots: AgentViewSnapshot | AgentViewSnapshot[],
  options: MockBridgeOptions = {},
): MockAgentIslandBridge {
  const snapshotQueue = (Array.isArray(snapshots) ? snapshots : [snapshots]).map(clone);
  const resultQueue = (options.commandResults ?? []).map(clone);
  const commandLog: AgentCommand[] = [];
  const handlers = new Set<(event: HookEvent) => void>();

  return {
    async getSnapshot() {
      const snapshot = snapshotQueue.shift();
      if (!snapshot) throw new Error("mock_snapshot_queue_empty");
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
    commands() {
      return commandLog.map(clone);
    },
    emitHookEvent(event) {
      for (const handler of [...handlers]) handler(clone(event));
    },
    listenerCount() {
      return handlers.size;
    },
  };
}
