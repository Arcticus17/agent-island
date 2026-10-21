import { describe, expect, it } from "vitest";

import {
  PINNED_AGENT_KEY,
  pinnedSelection,
  resolvePinnedAgent,
  storedPinnedAgent,
} from "../src/models/pinned-agent";

const agents = [
  { id: "claude", name: "Claude Code" },
  { id: "codex", name: "Codex CLI" },
];

describe("pinned agent", () => {
  it("resolves a pinned agent that is still visible", () => {
    expect(resolvePinnedAgent(agents, "codex")?.name).toBe("Codex CLI");
    expect(resolvePinnedAgent(agents, "open-code")).toBeNull();
    expect(resolvePinnedAgent(agents, null)).toBeNull();
    expect(resolvePinnedAgent(agents, "")).toBeNull();
  });

  it("reports a stale pin when the agent disappears", () => {
    expect(pinnedSelection(agents, "codex")).toEqual({ agent: agents[1], stalePin: false });
    expect(pinnedSelection(agents, "gone")).toEqual({ agent: null, stalePin: true });
    expect(pinnedSelection(agents, null)).toEqual({ agent: null, stalePin: false });
  });

  it("reads the stored pin and ignores blanks", () => {
    expect(storedPinnedAgent((key) => (key === PINNED_AGENT_KEY ? "codex" : null))).toBe("codex");
    expect(storedPinnedAgent(() => "   ")).toBeNull();
    expect(storedPinnedAgent(() => null)).toBeNull();
  });
});
