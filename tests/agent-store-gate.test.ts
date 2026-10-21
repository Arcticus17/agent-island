import { describe, expect, it } from "vitest";

import { createAgentStore } from "../src/stores/agent-store";
import type { AgentIslandBridge, AgentViewSnapshot } from "../src/bridge/types";

function snapshot(generatedAt: number, status: "working" | "done"): AgentViewSnapshot {
  return {
    generated_at_ms: generatedAt,
    agents: [
      {
        id: "claude",
        name: "Claude Code",
        can_restart: true,
        state: { process: "running", turn: status === "done" ? "succeeded" : "executing", attention: "none", result_at_ms: null },
        display_status: status,
        active_session: {
          id: "alpha-live",
          name: "Alpha live",
          cwd: "D:\\work\\alpha",
          lifecycle: "Active",
          last_activity_ms: generatedAt,
          records: [],
          recent_output: [],
        },
        history_sessions: [],
        usage: null,
        freshness: { observed_at_ms: generatedAt, stale: false },
      },
    ],
  } as unknown as AgentViewSnapshot;
}

function bridgeWith(queue: AgentViewSnapshot[]): AgentIslandBridge {
  return {
    async getSnapshot() {
      return queue.length > 1 ? queue.shift()! : queue[0];
    },
    async runCommand() {
      return { ok: true };
    },
    async listenHookEvents() {
      return () => undefined;
    },
  } as unknown as AgentIslandBridge;
}

describe("agent store interaction gate", () => {
  it("defers a newer snapshot while an interaction is held, then applies it", async () => {
    const store = createAgentStore(bridgeWith([snapshot(100, "working"), snapshot(200, "done")]));
    await store.refresh();
    expect(store.current().snapshot?.agents[0].display_status).toBe("working");

    const release = store.beginInteraction("approval");
    await store.refresh();
    // 交互期间的新快照只能排队，不能替换当前状态
    expect(store.current().snapshot?.agents[0].display_status).toBe("working");

    release();
    expect(store.current().snapshot?.agents[0].display_status).toBe("done");
  });
});
