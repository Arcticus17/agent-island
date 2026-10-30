import { mount, unmount } from "svelte";
import { afterEach, describe, expect, it } from "vitest";
import { userEvent } from "vitest/browser";

import TeamBoard from "../../src/windows/overview/TeamBoard.svelte";
import type {
  AgentIslandBridge,
  TeamCapability,
  TeamPlanTask,
  TeamRun,
  TeamTask,
} from "../../src/bridge/types";

const mounted: Array<ReturnType<typeof mount>> = [];

function task(overrides: Partial<TeamTask> = {}): TeamTask {
  return {
    id: "t1",
    title: "写一个标记文件",
    assignee: "claude",
    assigneeName: "Claude Code",
    prompt: "创建 island.txt",
    dependsOn: [],
    acceptance: "文件存在",
    state: "queued",
    startedAtMs: null,
    endedAtMs: null,
    exitCode: null,
    result: null,
    error: null,
    costUsd: null,
    log: [],
    sessionId: null,
    ...overrides,
  };
}

function run(overrides: Partial<TeamRun> = {}): TeamRun {
  return {
    id: "team-1",
    goal: "写一个标记文件",
    workspace: "C:\\work",
    mainAgent: "claude",
    mainAgentName: "Claude Code",
    phase: "awaiting_plan",
    planSummary: "一个任务",
    summary: null,
    tasks: [task()],
    createdAtMs: 1,
    updatedAtMs: 1,
    error: null,
    plannerSessionId: "session-1",
    ...overrides,
  };
}

const capability: TeamCapability = {
  id: "claude",
  name: "Claude Code",
  program: "claude",
  installed: true,
  canPlan: true,
  canWork: true,
  available: true,
  detail: "可用",
};

function harness() {
  let latest: TeamRun | null = null;
  const calls: string[] = [];
  const bridge: AgentIslandBridge = {
    async getSnapshot() {
      throw new Error("not used");
    },
    async runCommand() {
      return { ok: true };
    },
    async listenHookEvents() {
      return () => {};
    },
    async teamCapabilities() {
      return [capability];
    },
    async startTeamPlan() {
      calls.push("start");
      latest = run({ phase: "awaiting_plan", tasks: [task()] });
      return latest;
    },
    async getTeamRun() {
      return latest;
    },
    async listTeamRuns() {
      return latest ? [latest] : [];
    },
    async confirmTeamPlan(id, tasks: TeamPlanTask[]) {
      calls.push("confirm");
      latest = run({
        id,
        phase: "running",
        tasks: tasks.map((entry) =>
          task({
            id: entry.id,
            title: entry.title,
            assignee: entry.assignee,
            prompt: entry.prompt,
            dependsOn: entry.depends_on,
            acceptance: entry.acceptance,
          }),
        ),
      });
      return latest;
    },
    async cancelTeamRun(id) {
      calls.push("cancel");
      latest = run({ id, phase: "cancelled", error: "已取消" });
      return latest;
    },
    async listenTeamRuns() {
      return () => {};
    },
  };
  return { bridge, calls, current: () => latest };
}

async function waitFor(check: () => boolean, timeoutMs = 1_500) {
  const deadline = Date.now() + timeoutMs;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("browser_test_wait_timeout");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

async function render(bridge: AgentIslandBridge): Promise<HTMLElement> {
  const target = document.createElement("div");
  target.style.height = "640px";
  document.body.append(target);
  mounted.push(mount(TeamBoard, { target, props: { bridge } }));
  await waitFor(() => target.querySelector('[data-testid="team-generate"]') !== null);
  return target;
}

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
});

describe("team board", () => {
  it("edits the generated plan and confirms it", async () => {
    const { bridge, calls, current } = harness();
    const target = await render(bridge);

    await userEvent.fill(target.querySelector<HTMLTextAreaElement>('[data-testid="team-goal"]')!, "写一个标记文件");
    await userEvent.fill(target.querySelector<HTMLInputElement>('[data-testid="team-workspace"]')!, "C:\\work");
    target.querySelector<HTMLButtonElement>('[data-testid="team-generate"]')!.click();

    await waitFor(() => target.textContent?.includes("计划已生成") === true);
    expect(calls).toContain("start");
    expect(current()?.phase).toBe("awaiting_plan");

    const prompt = target.querySelector<HTMLTextAreaElement>(".plan-task textarea")!;
    await userEvent.fill(prompt, "只创建 island.txt");
    target.querySelectorAll<HTMLButtonElement>(".plan-actions button")[1].click();

    await waitFor(() => target.textContent?.includes("执行中") === true);
    expect(calls).toContain("confirm");
    expect(current()?.phase).toBe("running");
  });

  it("cancels a running team run", async () => {
    const { bridge, calls, current } = harness();
    const target = await render(bridge);

    await userEvent.fill(target.querySelector<HTMLTextAreaElement>('[data-testid="team-goal"]')!, "写一个标记文件");
    await userEvent.fill(target.querySelector<HTMLInputElement>('[data-testid="team-workspace"]')!, "C:\\work");
    target.querySelector<HTMLButtonElement>('[data-testid="team-generate"]')!.click();
    await waitFor(() => target.textContent?.includes("计划已生成") === true);
    target.querySelectorAll<HTMLButtonElement>(".plan-actions button")[1].click();
    await waitFor(() => target.textContent?.includes("取消运行") === true);

    const cancel = [...target.querySelectorAll<HTMLButtonElement>("button")].find(
      (button) => button.textContent?.trim() === "取消运行",
    )!;
    cancel.click();
    await waitFor(() => target.textContent?.includes("已取消") === true);
    expect(calls).toContain("cancel");
    expect(current()?.phase).toBe("cancelled");
  });
});
