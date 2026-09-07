import { describe, expect, it, vi } from "vitest";

import type {
  AgentIslandBridge,
  AgentView,
  AgentViewSnapshot,
} from "../src/bridge/types";
import { createAgentStore } from "../src/stores/agent-store";
import {
  createInteractionStore,
  shouldFollowLog,
} from "../src/stores/interaction-store";

function agent(id: string, activeSessionId = `${id}-active`): AgentView {
  return {
    id,
    name: `Agent ${id}`,
    state: {
      process: "running",
      turn: "executing",
      attention: "none",
      result_at_ms: null,
    },
    display_status: "working",
    active_session: {
      id: activeSessionId,
      name: `Session ${activeSessionId}`,
      cwd: null,
      log_path: null,
      records: [],
      recent_output: [],
      current_file: null,
      log_status: null,
      alert: null,
      lifecycle: "Active",
      last_active_at_ms: 1,
      display_status: "working",
    },
    active_turn: null,
    history_sessions: [
      {
        id: `${id}-history`,
        name: `History ${id}`,
        cwd: null,
        log_path: null,
        records: [],
        recent_output: [],
        current_file: null,
        log_status: null,
        alert: null,
        lifecycle: "Historical",
        last_active_at_ms: 0,
        display_status: "done",
      },
    ],
    diagnostic: null,
    freshness: { observed_at_ms: 1, stale: false },
    usage: null,
  };
}

function snapshot(generatedAtMs: number, ...agents: AgentView[]): AgentViewSnapshot {
  return { schema_version: 1, generated_at_ms: generatedAtMs, agents };
}

function queuedBridge(values: Array<AgentViewSnapshot | Error>): AgentIslandBridge {
  const queue = [...values];
  return {
    async getSnapshot() {
      const value = queue.shift();
      if (!value) throw new Error("empty_queue");
      if (value instanceof Error) throw value;
      return structuredClone(value);
    },
    async runCommand() {
      return { ok: true };
    },
    async listenHookEvents() {
      return () => undefined;
    },
  };
}

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason: unknown) => void;
  const promise = new Promise<T>((resolvePromise, rejectPromise) => {
    resolve = resolvePromise;
    reject = rejectPromise;
  });
  return { promise, resolve, reject };
}

describe("agent snapshot reconciliation", () => {
  it("owns an immutable clone of snapshots passed directly by callers", () => {
    const store = createAgentStore(queuedBridge([]));
    const input = snapshot(10, agent("a"));
    store.applySnapshot(input);

    input.agents[0].name = "Mutated outside";
    input.agents.push(agent("b"));
    expect(store.current().snapshot?.agents).toHaveLength(1);
    expect(store.current().selectedAgent?.name).toBe("Agent a");

    expect(() => {
      (store.current().snapshot!.agents as AgentView[]).push(agent("c"));
    }).toThrow();
    expect(() => {
      (store.current().selectedAgent as { name: string }).name = "Mutated current";
    }).toThrow();
    expect(store.current().selectedAgent?.name).toBe("Agent a");
  });

  it("owns an immutable clone of snapshots returned by the bridge", async () => {
    const input = snapshot(10, agent("a"));
    const bridge = {
      ...queuedBridge([]),
      async getSnapshot() {
        return input;
      },
    };
    const store = createAgentStore(bridge);
    await store.refresh();

    input.agents[0].name = "Mutated bridge result";
    expect(store.current().selectedAgent?.name).toBe("Agent a");
    expect(Object.isFrozen(store.current())).toBe(true);
    expect(Object.isFrozen(store.current().snapshot!.agents[0])).toBe(true);
  });

  it("preserves agent and session selection by stable ID across reorder and rename", async () => {
    const firstB = agent("b");
    const renamedB = { ...agent("b"), name: "Renamed B" };
    const store = createAgentStore(
      queuedBridge([snapshot(10, agent("a"), firstB), snapshot(20, renamedB, agent("a"))]),
    );

    await store.refresh();
    store.selectAgent("b");
    store.selectSession("b-history");
    await store.refresh();

    expect(store.current().selectedAgentId).toBe("b");
    expect(store.current().selectedSessionId).toBe("b-history");
    expect(store.current().selectedAgent?.name).toBe("Renamed B");
  });

  it("defers snapshots during nested interactions and applies only the newest on idle", () => {
    const store = createAgentStore(queuedBridge([]));
    store.applySnapshot(snapshot(10, agent("a")));
    store.beginInteraction("input");
    store.beginInteraction("drag");

    store.applySnapshot(snapshot(20, agent("b")));
    store.applySnapshot(snapshot(30, agent("c")));
    store.endInteraction("input");
    expect(store.current().snapshot?.generated_at_ms).toBe(10);

    store.endInteraction("drag");
    expect(store.current().snapshot?.generated_at_ms).toBe(30);
    expect(store.current().selectedAgentId).toBe("c");
  });

  it("falls back only to the same agent active session when a selected session disappears", () => {
    const store = createAgentStore(queuedBridge([]));
    store.applySnapshot(snapshot(10, agent("a"), agent("b")));
    store.selectAgent("a");
    store.selectSession("a-history");

    const updatedA = { ...agent("a", "a-new-active"), history_sessions: agent("a").history_sessions.slice(0, 0) };
    store.applySnapshot(snapshot(20, updatedA, agent("b")));

    expect(store.current().selectedAgentId).toBe("a");
    expect(store.current().selectedSessionId).toBe("a-new-active");
    expect(store.current().selectedSession?.id).not.toBe("b-active");
  });

  it("selects the deterministic first agent, then null, when the selected agent disappears", () => {
    const store = createAgentStore(queuedBridge([]));
    store.applySnapshot(snapshot(10, agent("a"), agent("b")));
    store.selectAgent("b");
    store.selectSession("b-history");

    const firstReplacement = agent("c");
    firstReplacement.history_sessions[0] = {
      ...firstReplacement.history_sessions[0],
      id: "b-history",
    };

    store.applySnapshot(snapshot(20, firstReplacement, agent("a")));
    expect(store.current().selectedAgentId).toBe("c");
    expect(store.current().selectedSessionId).toBe("c-active");

    store.applySnapshot(snapshot(30));
    expect(store.current().selectedAgentId).toBeNull();
    expect(store.current().selectedSessionId).toBeNull();
  });

  it("does not let an older concurrent refresh response overwrite a newer snapshot", async () => {
    const oldResponse = deferred<AgentViewSnapshot>();
    const newResponse = deferred<AgentViewSnapshot>();
    const getSnapshot = vi
      .fn<() => Promise<AgentViewSnapshot>>()
      .mockReturnValueOnce(oldResponse.promise)
      .mockReturnValueOnce(newResponse.promise);
    const bridge = {
      ...queuedBridge([]),
      getSnapshot,
    };
    const store = createAgentStore(bridge);

    const oldRefresh = store.refresh();
    const newRefresh = store.refresh();
    newResponse.resolve(snapshot(20, agent("new")));
    await newRefresh;
    oldResponse.resolve(snapshot(10, agent("old")));
    await oldRefresh;

    expect(store.current().snapshot?.generated_at_ms).toBe(20);
    expect(store.current().selectedAgentId).toBe("new");
  });

  it("ignores an older success that settles after a newer refresh error", async () => {
    const older = deferred<AgentViewSnapshot>();
    const newer = deferred<AgentViewSnapshot>();
    const store = createAgentStore({
      ...queuedBridge([]),
      getSnapshot: vi
        .fn<() => Promise<AgentViewSnapshot>>()
        .mockReturnValueOnce(older.promise)
        .mockReturnValueOnce(newer.promise),
    });
    store.applySnapshot(snapshot(5, agent("initial")));

    const olderRefresh = store.refresh();
    const newerRefresh = store.refresh();
    newer.reject(new Error("invalid_snapshot_shape"));
    await newerRefresh;
    older.resolve(snapshot(20, agent("older-request")));
    await olderRefresh;

    expect(store.current().selectedAgentId).toBe("initial");
    expect(store.current().refreshError?.code).toBe("invalid_snapshot_shape");
  });

  it("ignores an older error that settles after a newer refresh success", async () => {
    const older = deferred<AgentViewSnapshot>();
    const newer = deferred<AgentViewSnapshot>();
    const store = createAgentStore({
      ...queuedBridge([]),
      getSnapshot: vi
        .fn<() => Promise<AgentViewSnapshot>>()
        .mockReturnValueOnce(older.promise)
        .mockReturnValueOnce(newer.promise),
    });

    const olderRefresh = store.refresh();
    const newerRefresh = store.refresh();
    newer.resolve(snapshot(20, agent("newer-request")));
    await newerRefresh;
    older.reject(new Error("invalid_snapshot_shape"));
    await olderRefresh;

    expect(store.current().selectedAgentId).toBe("newer-request");
    expect(store.current().refreshError).toBeNull();
  });

  it("keeps an earlier success when a newer refresh later fails", async () => {
    const earlier = deferred<AgentViewSnapshot>();
    const later = deferred<AgentViewSnapshot>();
    const store = createAgentStore({
      ...queuedBridge([]),
      getSnapshot: vi
        .fn<() => Promise<AgentViewSnapshot>>()
        .mockReturnValueOnce(earlier.promise)
        .mockReturnValueOnce(later.promise),
    });

    const earlierRefresh = store.refresh();
    const laterRefresh = store.refresh();
    earlier.resolve(snapshot(10, agent("earlier-success")));
    await earlierRefresh;
    later.reject(new Error("invalid_snapshot_shape"));
    await laterRefresh;

    expect(store.current().selectedAgentId).toBe("earlier-success");
    expect(store.current().refreshError?.code).toBe("invalid_snapshot_shape");
  });

  it("clears an earlier refresh error after a newer success", async () => {
    const store = createAgentStore(
      queuedBridge([new Error("invalid_snapshot_shape"), snapshot(10, agent("recovered"))]),
    );
    await store.refresh();
    expect(store.current().refreshError?.code).toBe("invalid_snapshot_shape");

    await store.refresh();
    expect(store.current().selectedAgentId).toBe("recovered");
    expect(store.current().refreshError).toBeNull();
  });

  it("retains current state and exposes a structured error when refresh rejects", async () => {
    const store = createAgentStore(
      queuedBridge([snapshot(10, agent("a")), new Error("invalid_snapshot_shape")]),
    );
    await store.refresh();

    await expect(store.refresh()).resolves.toBe(false);

    expect(store.current().snapshot?.generated_at_ms).toBe(10);
    expect(store.current().selectedAgentId).toBe("a");
    expect(store.current().refreshError).toMatchObject({
      code: "invalid_snapshot_shape",
      message: "invalid_snapshot_shape",
    });
  });

  it("does not hide a newer refresh error when an older deferred snapshot is released", async () => {
    const store = createAgentStore(queuedBridge([new Error("invalid_snapshot_shape")]));
    store.applySnapshot(snapshot(10, agent("a")));
    store.beginInteraction("approval");
    store.applySnapshot(snapshot(20, agent("b")));

    await store.refresh();
    store.endInteraction("approval");

    expect(store.current().snapshot?.generated_at_ms).toBe(20);
    expect(store.current().refreshError?.code).toBe("invalid_snapshot_shape");
  });

  it("notifies subscribers immutably and supports unsubscribe", () => {
    const store = createAgentStore(queuedBridge([]));
    const seen = vi.fn();
    const unsubscribe = store.subscribe(seen);

    store.applySnapshot(snapshot(10, agent("a")));
    unsubscribe();
    store.selectAgent("a");

    expect(seen).toHaveBeenCalledTimes(2);
    expect(seen.mock.calls[0][0]).not.toBe(seen.mock.calls[1][0]);
  });

  it("isolates failing subscribers and does not misreport render errors as refresh errors", async () => {
    const store = createAgentStore(queuedBridge([snapshot(10, agent("a"))]));
    const throwsInitially = vi.fn(() => {
      throw new Error("render_failed");
    });
    const throwsOnUpdate = vi.fn().mockImplementationOnce(() => undefined).mockImplementation(() => {
      throw new Error("render_failed");
    });
    const healthy = vi.fn();

    expect(() => store.subscribe(throwsInitially)).not.toThrow();
    store.subscribe(throwsOnUpdate);
    store.subscribe(healthy);
    await expect(store.refresh()).resolves.toBe(true);

    expect(throwsInitially).toHaveBeenCalledTimes(1);
    expect(throwsOnUpdate).toHaveBeenCalledTimes(2);
    expect(healthy).toHaveBeenCalledTimes(2);
    expect(store.current().refreshError).toBeNull();

    store.applySnapshot(snapshot(20, agent("b")));
    expect(throwsOnUpdate).toHaveBeenCalledTimes(2);
    expect(healthy).toHaveBeenCalledTimes(3);
  });

  it("does not republish an identical same-generation snapshot but accepts a real same-ms change", () => {
    const store = createAgentStore(queuedBridge([]));
    const seen = vi.fn();
    store.subscribe(seen);
    const original = snapshot(10, agent("a"));

    expect(store.applySnapshot(original)).toBe(true);
    expect(store.applySnapshot(structuredClone(original))).toBe(false);
    expect(seen).toHaveBeenCalledTimes(2);

    const changed = snapshot(10, { ...agent("a"), name: "Same millisecond update" });
    expect(store.applySnapshot(changed)).toBe(true);
    expect(store.current().selectedAgent?.name).toBe("Same millisecond update");
    expect(seen).toHaveBeenCalledTimes(3);
  });

  it("clears a refresh error when a later success repeats the current snapshot", async () => {
    const original = snapshot(10, agent("a"));
    const store = createAgentStore(
      queuedBridge([original, new Error("invalid_snapshot_shape"), structuredClone(original)]),
    );
    const seen = vi.fn();
    store.subscribe(seen);

    await store.refresh();
    const retainedSnapshot = store.current().snapshot;
    const retainedAgent = store.current().selectedAgent;
    await store.refresh();
    await expect(store.refresh()).resolves.toBe(true);

    expect(store.current().refreshError).toBeNull();
    expect(store.current().snapshot).toBe(retainedSnapshot);
    expect(store.current().selectedAgent).toBe(retainedAgent);
    expect(seen).toHaveBeenCalledTimes(4);
  });

  it("clears a refresh error from a duplicate success while interaction remains active", async () => {
    const original = snapshot(10, agent("a"));
    const store = createAgentStore(
      queuedBridge([original, new Error("invalid_snapshot_shape"), structuredClone(original)]),
    );
    await store.refresh();
    const retainedSnapshot = store.current().snapshot;
    store.beginInteraction("input");
    await store.refresh();

    await expect(store.refresh()).resolves.toBe(true);

    expect(store.interaction.current().interacting).toBe(true);
    expect(store.current().refreshError).toBeNull();
    expect(store.current().snapshot).toBe(retainedSnapshot);
    store.endInteraction("input");
    expect(store.current().snapshot).toBe(retainedSnapshot);
  });

  it("gives direct apply the same duplicate-success error clearing semantics", async () => {
    const original = snapshot(10, agent("a"));
    const store = createAgentStore(
      queuedBridge([original, new Error("invalid_snapshot_shape")]),
    );
    await store.refresh();
    const retainedSnapshot = store.current().snapshot;
    await store.refresh();

    expect(store.applySnapshot(structuredClone(original))).toBe(true);
    expect(store.current().refreshError).toBeNull();
    expect(store.current().snapshot).toBe(retainedSnapshot);
  });
});

describe("interaction coordination", () => {
  it("tracks input, drag, and approval independently and never underflows", () => {
    const interactions = createInteractionStore();
    interactions.beginInteraction("input");
    interactions.beginInteraction("input");
    interactions.beginInteraction("drag");
    interactions.beginInteraction("approval");

    expect(interactions.current()).toMatchObject({
      totalDepth: 4,
      depths: { input: 2, drag: 1, approval: 1 },
    });

    interactions.endInteraction("input");
    interactions.endInteraction("input");
    interactions.endInteraction("input");
    interactions.endInteraction("drag");
    interactions.endInteraction("approval");
    interactions.endInteraction("approval");
    expect(interactions.current()).toMatchObject({
      totalDepth: 0,
      interacting: false,
      depths: { input: 0, drag: 0, approval: 0 },
    });
  });

  it("returns an idempotent release function from beginInteraction", () => {
    const interactions = createInteractionStore();
    const release = interactions.beginInteraction("approval");
    release();
    release();
    expect(interactions.current().totalDepth).toBe(0);
  });

  it("isolates listener errors so begin always returns a usable release", () => {
    const interactions = createInteractionStore();
    const throwsInitially = vi.fn(() => {
      throw new Error("initial_render_failed");
    });
    expect(() => interactions.subscribe(throwsInitially)).not.toThrow();

    const throwsOnUpdate = vi
      .fn()
      .mockImplementationOnce(() => undefined)
      .mockImplementation(() => {
        throw new Error("update_render_failed");
      });
    const healthy = vi.fn();
    interactions.subscribe(throwsOnUpdate);
    interactions.subscribe(healthy);

    const release = interactions.beginInteraction("input");
    expect(interactions.current().totalDepth).toBe(1);
    expect(() => release()).not.toThrow();
    expect(interactions.current().totalDepth).toBe(0);
    expect(throwsInitially).toHaveBeenCalledTimes(1);
    expect(throwsOnUpdate).toHaveBeenCalledTimes(2);
    expect(healthy).toHaveBeenCalledTimes(3);
  });
});

describe("log follow policy", () => {
  it("follows only when the pre-refresh viewport is within 24px of the bottom", () => {
    expect(shouldFollowLog({ scrollTop: 76, clientHeight: 100, scrollHeight: 200 })).toBe(true);
    expect(shouldFollowLog({ scrollTop: 75, clientHeight: 100, scrollHeight: 200 })).toBe(false);
    expect(shouldFollowLog({ scrollTop: 120, clientHeight: 100, scrollHeight: 200 })).toBe(true);
  });

  it.each([
    { scrollTop: -1, clientHeight: 100, scrollHeight: 200 },
    { scrollTop: 0, clientHeight: -1, scrollHeight: 200 },
    { scrollTop: 0, clientHeight: 100, scrollHeight: -1 },
  ])("rejects negative viewport metrics: %o", (viewport) => {
    expect(shouldFollowLog(viewport)).toBe(false);
  });
});
