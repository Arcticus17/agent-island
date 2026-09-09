import { describe, expect, it } from "vitest";
import { preferredAgent } from "../src/models/agent-priority";
import type { AgentView, AgentViewSnapshot } from "../src/bridge/types";

function agent(id: string, turn: AgentView["state"]["turn"] = "idle", ended: number | null = null): AgentView {
  return {
    id, name: id, state: { process: "running", turn, attention: "none", result_at_ms: ended },
    display_status: turn === "executing" ? "working" : "idle",
    active_session: null, active_turn: null, history_sessions: [], diagnostic: null,
    freshness: { stale: false, observed_at_ms: 1000 }, usage: null,
  };
}
function choose(agents: AgentView[], current: string | null = null) {
  const snapshot: AgentViewSnapshot = { schema_version: 1, generated_at_ms: 1000, agents };
  return preferredAgent(snapshot, current)?.id;
}
describe("capsule default agent priority", () => {
  it("prefers working over list order and recent completion", () => {
    expect(choose([agent("idle"), agent("done", "succeeded", 990), agent("busy", "executing")])).toBe("busy");
  });
  it("keeps the current working agent when several agents are busy", () => {
    expect(choose([agent("a", "executing"), agent("b", "executing")], "b")).toBe("b");
  });
  it("uses the latest terminal time even after completion highlighting expires", () => {
    expect(choose([agent("older", "succeeded", 100), agent("newer", "succeeded", 900)])).toBe("newer");
    expect(choose([agent("done", "succeeded", 100), agent("failed", "failed", 900)])).toBe("failed");
  });
  it("ignores stale or stopped working state, missing and implausibly future completion times", () => {
    const stale = agent("stale", "executing"); stale.freshness.stale = true;
    const stopped = agent("stopped", "executing"); stopped.state.process = "stopped";
    expect(choose([stale, stopped, agent("unknown", "succeeded"), agent("future", "succeeded", 99_000), agent("done", "succeeded", 900)])).toBe("done");
  });
  it("keeps a stable fallback without mutating order and handles empty input", () => {
    const agents = [agent("a"), agent("b")];
    expect(choose(agents, "b")).toBe("b");
    expect(agents.map((item) => item.id)).toEqual(["a", "b"]);
    expect(choose([])).toBeUndefined();
  });
});
