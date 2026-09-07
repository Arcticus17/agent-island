import { describe, expect, it, vi } from "vitest";

import { createMockBridge } from "../src/bridge/mock";
import { createTauriBridge } from "../src/bridge/tauri";
import type {
  AgentCommand,
  AgentViewSnapshot,
  HookEvent,
} from "../src/bridge/types";

const tauri = vi.hoisted(() => ({
  invoke: vi.fn(),
  listen: vi.fn(),
}));

vi.mock("@tauri-apps/api/core", () => ({ invoke: tauri.invoke }));
vi.mock("@tauri-apps/api/event", () => ({ listen: tauri.listen }));

const emptySnapshot = (schemaVersion = 1, generatedAtMs = 1): AgentViewSnapshot => ({
  schema_version: schemaVersion,
  generated_at_ms: generatedAtMs,
  agents: [],
});

describe("Agent Island bridge contract", () => {
  it("rejects an unsupported snapshot schema at the bridge boundary", async () => {
    const bridge = createMockBridge(emptySnapshot(2));

    await expect(bridge.getSnapshot()).rejects.toThrow(
      "unsupported_snapshot_schema",
    );
  });

  it("enforces schema v1 on production snapshots", async () => {
    tauri.invoke.mockResolvedValueOnce(emptySnapshot(7));

    await expect(createTauriBridge().getSnapshot()).rejects.toThrow(
      "unsupported_snapshot_schema",
    );
    expect(tauri.invoke).toHaveBeenCalledWith("get_agent_snapshot");
  });

  it("maps typed commands to the existing Tauri command names and arguments", async () => {
    tauri.invoke.mockResolvedValueOnce(undefined);

    await expect(
      createTauriBridge().runCommand({
        name: "stop_session",
        agentName: "Codex CLI",
        sessionId: "session-7",
      }),
    ).resolves.toEqual({ ok: true });
    expect(tauri.invoke).toHaveBeenCalledWith("stop_session", {
      name: "Codex CLI",
      sessionId: "session-7",
    });
  });

  it("normalizes Tauri command failures into structured retry metadata", async () => {
    tauri.invoke.mockRejectedValueOnce("no process found for this session");

    await expect(
      createTauriBridge().runCommand({
        name: "stop_session",
        agentName: "Codex CLI",
        sessionId: "session-7",
      }),
    ).resolves.toEqual({
      ok: false,
      code: "no_process_found",
      message: "no process found for this session",
      retryable: true,
    });
  });

  it("returns queued snapshots in deterministic order", async () => {
    const bridge = createMockBridge([
      emptySnapshot(1, 10),
      emptySnapshot(1, 20),
    ]);

    await expect(bridge.getSnapshot()).resolves.toMatchObject({
      generated_at_ms: 10,
    });
    await expect(bridge.getSnapshot()).resolves.toMatchObject({
      generated_at_ms: 20,
    });
    await expect(bridge.getSnapshot()).rejects.toThrow(
      "mock_snapshot_queue_empty",
    );
  });

  it("queues structured command results and records exact commands", async () => {
    const command: AgentCommand = {
      name: "stop_session",
      agentName: "Codex CLI",
      sessionId: "session-7",
    };
    const bridge = createMockBridge(emptySnapshot(), {
      commandResults: [
        {
          ok: false,
          code: "no_process_found",
          message: "no process found for this session",
          retryable: true,
        },
      ],
    });

    await expect(bridge.runCommand(command)).resolves.toEqual({
      ok: false,
      code: "no_process_found",
      message: "no process found for this session",
      retryable: true,
    });
    expect(bridge.commands()).toEqual([command]);
    await expect(bridge.runCommand(command)).rejects.toThrow(
      "mock_command_result_queue_empty",
    );
  });

  it("unsubscribes hook listeners cleanly", async () => {
    const bridge = createMockBridge(emptySnapshot());
    const handler = vi.fn<(event: HookEvent) => void>();
    const unlisten = await bridge.listenHookEvents(handler);
    const first: HookEvent = { kind: "stop", session: "session-1" };
    const second: HookEvent = {
      kind: "notification",
      message: "permission needed",
      session: "session-1",
    };

    bridge.emitHookEvent(first);
    expect(handler).toHaveBeenCalledWith(first);

    unlisten();
    unlisten();
    bridge.emitHookEvent(second);
    expect(handler).toHaveBeenCalledTimes(1);
    expect(bridge.listenerCount()).toBe(0);
  });
});
