/// <reference types="vite/client" />

import { beforeEach, describe, expect, it, vi } from "vitest";

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

beforeEach(() => {
  tauri.invoke.mockReset();
  tauri.listen.mockReset();
});

const emptySnapshot = (schemaVersion = 1, generatedAtMs = 1): AgentViewSnapshot => ({
  schema_version: schemaVersion,
  generated_at_ms: generatedAtMs,
  agents: [],
});

const populatedSnapshot = (): AgentViewSnapshot => ({
  schema_version: 1,
  generated_at_ms: 100,
  agents: [
    {
      id: "codex",
      name: "Codex CLI",
      state: {
        process: "running",
        turn: "executing",
        attention: "none",
        result_at_ms: null,
      },
      display_status: "working",
      active_session: {
        id: "session-1",
        name: "Project",
        cwd: "D:\\Project",
        log_path: "D:\\logs\\session.jsonl",
        records: [
          { event_id: "event-1", at_ms: 90, role: "User", text: "hello" },
        ],
        recent_output: ["hello"],
        current_file: null,
        log_status: null,
        alert: null,
        lifecycle: "Active",
        last_active_at_ms: 90,
        display_status: "working",
      },
      active_turn: {
        agent_id: "codex",
        session_id: "session-1",
        turn_id: "turn-1",
      },
      history_sessions: [
        {
          id: "session-old",
          name: "Old project",
          cwd: null,
          log_path: null,
          records: [],
          recent_output: [],
          current_file: null,
          log_status: null,
          alert: null,
          lifecycle: "Historical",
          last_active_at_ms: 10,
          display_status: "done",
        },
      ],
      diagnostic: {
        adapter: "codex",
        code: "partial_parse",
        freshness: { observed_at_ms: 90, stale: false },
        event_type: "turn_started",
        event_count: 1,
        message_count: 1,
        skipped_lines: 1,
      },
      freshness: { observed_at_ms: 90, stale: false },
    },
  ],
});

describe("Agent Island bridge contract", () => {
  it("rejects an unsupported snapshot schema at the bridge boundary", async () => {
    const bridge = createMockBridge(emptySnapshot(2));

    await expect(bridge.getSnapshot()).rejects.toThrow(
      "unsupported_snapshot_schema",
    );
  });

  it.each([
    null,
    {},
    { schema_version: 1, generated_at_ms: 1, agents: null },
    {
      ...populatedSnapshot(),
      agents: [{ ...populatedSnapshot().agents[0], display_status: "busy" }],
    },
    {
      ...populatedSnapshot(),
      agents: [
        {
          ...populatedSnapshot().agents[0],
          state: { ...populatedSnapshot().agents[0].state, process: "alive" },
        },
      ],
    },
    {
      ...populatedSnapshot(),
      agents: [
        {
          ...populatedSnapshot().agents[0],
          active_session: {
            ...populatedSnapshot().agents[0].active_session,
            records: [{ event_id: "x", at_ms: 1, role: "system", text: "x" }],
          },
        },
      ],
    },
    {
      ...populatedSnapshot(),
      agents: [
        {
          ...populatedSnapshot().agents[0],
          active_turn: { agent_id: "codex", session_id: "session-1" },
        },
      ],
    },
    {
      ...populatedSnapshot(),
      agents: [
        {
          ...populatedSnapshot().agents[0],
          history_sessions: [
            {
              ...populatedSnapshot().agents[0].history_sessions[0],
              recent_output: [1],
            },
          ],
        },
      ],
    },
    {
      ...populatedSnapshot(),
      agents: [
        {
          ...populatedSnapshot().agents[0],
          diagnostic: {
            ...populatedSnapshot().agents[0].diagnostic,
            event_count: -1,
          },
        },
      ],
    },
    {
      ...populatedSnapshot(),
      agents: [
        {
          ...populatedSnapshot().agents[0],
          freshness: { observed_at_ms: 90, stale: "no" },
        },
      ],
    },
  ])("rejects malformed schema-v1 snapshots with a stable shape error", async (snapshot) => {
    const bridge = createMockBridge(snapshot as AgentViewSnapshot);

    await expect(bridge.getSnapshot()).rejects.toThrow("invalid_snapshot_shape");
  });

  it("accepts the complete Rust snapshot shape", async () => {
    await expect(createMockBridge(populatedSnapshot()).getSnapshot()).resolves.toEqual(
      populatedSnapshot(),
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

  it("normalizes null and undefined command returns to an empty success", async () => {
    tauri.invoke.mockResolvedValueOnce(null).mockResolvedValueOnce(undefined);
    const bridge = createTauriBridge();

    await expect(bridge.runCommand({ name: "open_overview" })).resolves.toEqual({
      ok: true,
    });
    await expect(bridge.runCommand({ name: "open_overview" })).resolves.toEqual({
      ok: true,
    });
  });

  it.each([
    ["empty path", "empty path"],
    ["unknown agent", "unknown agent: Other"],
    ["missing command", "no command recorded for this agent"],
    ["unclassified", "unexpected backend rejection"],
  ])("does not mark deterministic %s failures retryable", async (_case, error) => {
    tauri.invoke.mockRejectedValueOnce(error);

    await expect(
      createTauriBridge().runCommand({ name: "open_overview" }),
    ).resolves.toMatchObject({ ok: false, retryable: false });
  });

  it("uses an explicit transient allowlist for retryable failures", async () => {
    tauri.invoke.mockRejectedValueOnce("state lock error");

    await expect(
      createTauriBridge().runCommand({ name: "open_overview" }),
    ).resolves.toMatchObject({
      ok: false,
      code: "state_unavailable",
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

  it("filters production hook payloads and calls raw cleanup only once", async () => {
    const rawUnlisten = vi.fn();
    let listener: ((event: { payload: unknown }) => void) | undefined;
    tauri.listen.mockImplementationOnce(async (_name, handler) => {
      listener = handler;
      return rawUnlisten;
    });
    const handler = vi.fn<(event: HookEvent) => void>();
    const unlisten = await createTauriBridge().listenHookEvents(handler);
    const event: HookEvent = { kind: "stop", session: "session-1" };

    listener?.({ payload: event });
    listener?.({ payload: { kind: "notification", session: 7, message: "bad" } });
    expect(handler).toHaveBeenCalledOnce();
    expect(handler).toHaveBeenCalledWith(event);

    unlisten();
    unlisten();
    expect(rawUnlisten).toHaveBeenCalledOnce();
  });

  it("keeps every Tauri API import inside bridge/tauri.ts", () => {
    const sourceModules = import.meta.glob("../src/**/*.{js,ts,svelte}", {
      eager: true,
      import: "default",
      query: "?raw",
    }) as Record<string, string>;
    const violations = Object.entries(sourceModules)
      .filter(([path]) => !path.endsWith("/src/bridge/tauri.ts"))
      .filter(([, source]) => source.includes("@tauri-apps/api"))
      .map(([path]) => path);

    expect(Object.keys(sourceModules).length).toBeGreaterThan(0);
    expect(violations).toEqual([]);
  });
});
