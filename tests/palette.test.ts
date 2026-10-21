import { describe, expect, it } from "vitest";

import {
  PALETTE_ACTIONS,
  buildPaletteCommands,
  filterCommands,
  moveSelection,
  scoreCommand,
  type PaletteSource,
} from "../src/palette/commands";

const source: PaletteSource = {
  selectedAgentId: "claude",
  agents: [
    {
      id: "claude",
      name: "Claude Code",
      display_status: "working",
      active_session: { id: "alpha-live", name: "Alpha live" },
      history_sessions: [{ id: "alpha-old", name: "Alpha yesterday" }],
    },
    {
      id: "codex",
      name: "Codex CLI",
      display_status: "waiting",
      active_session: { id: "beta-live", name: "Beta live" },
      history_sessions: [],
    },
  ],
};

describe("palette commands", () => {
  it("lists agents, the selected agent's sessions, then actions", () => {
    const commands = buildPaletteCommands(source);
    expect(commands[0]).toMatchObject({ id: "agent:claude", kind: "agent", hint: "工作中" });
    expect(commands[1]).toMatchObject({ id: "session:alpha-live", kind: "session", hint: "当前会话" });
    expect(commands[2]).toMatchObject({ id: "session:alpha-old", hint: "历史会话" });
    expect(commands.some((command) => command.id === "agent:codex")).toBe(true);
    // 只列当前 Agent 的会话，别的 Agent 的会话不混进来
    expect(commands.some((command) => command.id === "session:beta-live")).toBe(false);
    expect(commands.slice(-PALETTE_ACTIONS.length).every((command) => command.kind === "action")).toBe(true);
  });

  it("matches sub-sequences and ranks contiguous hits first", () => {
    expect(scoreCommand("", "anything")).toBe(0);
    expect(scoreCommand("claude", "Claude Code")).not.toBeNull();
    expect(scoreCommand("cc", "Claude Code")).not.toBeNull();
    expect(scoreCommand("xyz", "Claude Code")).toBeNull();
    const commands = buildPaletteCommands(source);
    const filtered = filterCommands(commands, "终");
    expect(filtered[0]?.title).toBe("打开内置终端");
  });

  it("keeps the original order for equal scores and returns everything for an empty query", () => {
    const commands = buildPaletteCommands(source);
    expect(filterCommands(commands, "")).toHaveLength(commands.length);
    const doubled = filterCommands(commands, "e");
    expect(doubled.length).toBeGreaterThan(0);
  });

  it("wraps the selection around the list", () => {
    expect(moveSelection(0, -1, 3)).toBe(2);
    expect(moveSelection(2, 1, 3)).toBe(0);
    expect(moveSelection(0, 1, 0)).toBe(0);
    expect(moveSelection(1, 1, 3)).toBe(2);
  });
});
