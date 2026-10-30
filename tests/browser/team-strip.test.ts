import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";

import TeamStrip from "../../src/components/team/TeamStrip.svelte";
import type { AgentIslandBridge, TeamRun, TeamTask } from "../../src/bridge/types";

const mounted: Array<ReturnType<typeof mount>> = [];

function task(id: string, state: TeamTask["state"]): TeamTask {
  return {
    id,
    title: id,
    assignee: "claude",
    assigneeName: "Claude Code",
    prompt: "",
    dependsOn: [],
    acceptance: "",
    state,
    startedAtMs: 1,
    endedAtMs: null,
    exitCode: null,
    result: null,
    error: null,
    costUsd: null,
    log: [],
    sessionId: null,
    pendingApproval: null,
    approvalId: null,
  };
}

function activeRun(): TeamRun {
  return {
    id: "team-1",
    goal: "把设置页补完",
    workspace: "C:\\work",
    mainAgent: "claude",
    mainAgentName: "Claude Code",
    phase: "running",
    planSummary: null,
    summary: null,
    tasks: [task("a", "succeeded"), task("b", "waiting_approval"), task("c", "queued")],
    createdAtMs: Date.now(),
    updatedAtMs: Date.now(),
    error: null,
    plannerSessionId: null,
    budgetUsd: 2,
    spentUsd: 0.12,
  };
}

function bridge(run: TeamRun, opened: string[]): AgentIslandBridge {
  return {
    async getSnapshot() {
      throw new Error("not used");
    },
    async runCommand() {
      return { ok: true };
    },
    async listenHookEvents() {
      return () => {};
    },
    async listTeamRuns() {
      return [run];
    },
    async getTeamRun() {
      return run;
    },
    async listenTeamRuns() {
      return () => {};
    },
    async openTeam() {
      opened.push(run.id);
    },
  };
}

async function waitFor(check: () => boolean, timeoutMs = 1_000) {
  const deadline = Date.now() + timeoutMs;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("browser_test_wait_timeout");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
});

describe("team strip", () => {
  it("shows progress and pending approvals, and opens the team view", async () => {
    const opened: string[] = [];
    const target = document.createElement("div");
    document.body.append(target);
    mounted.push(mount(TeamStrip, { target, props: { bridge: bridge(activeRun(), opened) } }));

    await waitFor(() => target.querySelector('[data-testid="team-strip"]') !== null);
    const strip = target.querySelector<HTMLButtonElement>('[data-testid="team-strip"]')!;
    expect(strip.textContent).toContain("Claude Code 主");
    expect(strip.textContent).toContain("1/3");
    expect(strip.textContent).toContain("1 待审批");

    strip.click();
    expect(opened).toEqual(["team-1"]);
  });
});
