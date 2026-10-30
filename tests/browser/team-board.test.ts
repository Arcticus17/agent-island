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
    verifyCommand: null,
    verification: null,
    state: "queued",
    startedAtMs: null,
    endedAtMs: null,
    exitCode: null,
    result: null,
    error: null,
    costUsd: null,
    log: [],
    sessionId: null,
    pendingApproval: null,
    approvalId: null,
    worktreePath: null,
    branch: null,
    baseCommit: null,
    changedFiles: [],
    diffStat: null,
    mergeState: "none",
    mergeError: null,
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
    budgetUsd: 2,
    spentUsd: 0,
    repoRoot: "C:\\work",
    baseBranch: "main",
    baseCommit: "abc123",
    worktreeMode: true,
    workspaceWarning: null,
    concurrency: 2,
    parentRunId: null,
    round: 1,
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

function harness(initial: TeamRun | null = null) {
  let latest: TeamRun | null = initial;
  const calls: string[] = [];
  const budgets: Array<number | null | undefined> = [];
  const parallelisms: Array<number | undefined> = [];
  const riskyConfirmations: boolean[] = [];
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
    async startTeamPlan(
      _goal: string,
      _workspace: string,
      budgetUsd?: number | null,
      concurrency?: number,
    ) {
      calls.push("start");
      budgets.push(budgetUsd);
      parallelisms.push(concurrency);
      latest = run({ phase: "awaiting_plan", tasks: [task()] });
      return latest;
    },
    async getTeamRun() {
      return latest;
    },
    async listTeamRuns() {
      return latest ? [latest] : [];
    },
    async confirmTeamPlan(
      id,
      tasks: TeamPlanTask[],
      allowRiskyVerification?: boolean,
    ) {
      calls.push("confirm");
      riskyConfirmations.push(allowRiskyVerification ?? false);
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
            verifyCommand: entry.verify_command || null,
          }),
        ),
      });
      return latest;
    },
    async checkTeamVerification(tasks) {
      return tasks
        .filter((entry) => entry.verify_command.trim().startsWith("rm "))
        .map((entry) => ({
          taskId: entry.id,
          taskTitle: entry.title,
          command: entry.verify_command,
          reason: "会删除文件",
        }));
    },
    async cancelTeamRun(id) {
      calls.push("cancel");
      latest = run({ id, phase: "cancelled", error: "已取消" });
      return latest;
    },
    async retryTeamRun(id) {
      calls.push("retry");
      latest = run({ id, phase: "running", tasks: [task({ state: "running" })] });
      return latest;
    },
    async continueTeamRun(id, instruction) {
      calls.push("continue");
      latest = run({
        id: "team-2",
        goal: instruction,
        phase: "awaiting_plan",
        parentRunId: id,
        round: 2,
        tasks: [task()],
      });
      return latest;
    },
    async teamTaskDiff() {
      return "diff --git a/a.txt b/a.txt\n+hello";
    },
    async mergeTeamTask(id, taskId) {
      calls.push("merge");
      latest = run({
        id,
        phase: "succeeded",
        tasks: [task({ id: taskId, state: "succeeded", mergeState: "merged" })],
      });
      return latest;
    },
    async discardTeamTask(id, taskId) {
      calls.push("discard");
      latest = run({
        id,
        phase: "failed",
        tasks: [task({ id: taskId, state: "failed", mergeState: "discarded" })],
      });
      return latest;
    },
    async listenTeamRuns() {
      return () => {};
    },
  };
  return { bridge, calls, budgets, parallelisms, riskyConfirmations, current: () => latest };
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
    const { bridge, calls, budgets, parallelisms, current } = harness();
    const target = await render(bridge);

    await userEvent.fill(target.querySelector<HTMLTextAreaElement>('[data-testid="team-goal"]')!, "写一个标记文件");
    await userEvent.fill(target.querySelector<HTMLInputElement>('[data-testid="team-workspace"]')!, "C:\\work");
    await userEvent.fill(target.querySelector<HTMLInputElement>('[data-testid="team-budget"]')!, "1.25");
    await userEvent.fill(target.querySelector<HTMLInputElement>('[data-testid="team-concurrency"]')!, "2");
    target.querySelector<HTMLButtonElement>('[data-testid="team-generate"]')!.click();

    await waitFor(() => target.textContent?.includes("计划已生成") === true);
    expect(calls).toContain("start");
    expect(budgets[0]).toBe(1.25);
    expect(parallelisms[0]).toBe(2);
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

  it("shows a waiting team approval with the command", async () => {
    const waiting = run({
      phase: "running",
      tasks: [task({ state: "waiting_approval", pendingApproval: "rm -rf build" })],
    });
    const target = await render(harness(waiting).bridge);
    await waitFor(() => target.textContent?.includes("等待审批") === true);
    expect(target.textContent).toContain("rm -rf build");
  });

  it("retries unfinished tasks after a failure", async () => {
    const failed = run({
      phase: "failed",
      error: "有任务失败或被跳过",
      tasks: [task({ state: "failed", error: "命令被拒绝" })],
    });
    const { bridge, calls } = harness(failed);
    const target = await render(bridge);
    await waitFor(() =>
      [...target.querySelectorAll<HTMLButtonElement>("button")].some(
        (candidate) => candidate.textContent?.trim() === "重试未完成任务",
      ),
    );

    const button = [...target.querySelectorAll<HTMLButtonElement>("button")].find(
      (candidate) => candidate.textContent?.trim() === "重试未完成任务",
    )!;
    button.click();
    await waitFor(() => target.textContent?.includes("确认重试未完成任务") === true);
    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.textContent?.trim() === "确认重试未完成任务")!
      .click();

    await waitFor(() => calls.includes("retry"));
    expect(calls).toContain("retry");
  });

  it("reads the task diff and merges after confirmation", async () => {
    const initial = run({
      phase: "succeeded",
      tasks: [
        task({
          state: "succeeded",
          branch: "agent-island/team-1/t1",
          worktreePath: "C:\\worktrees\\t1",
          baseCommit: "abc123",
          changedFiles: ["a.txt"],
          diffStat: " a.txt | 1 +",
          mergeState: "pending",
        }),
      ],
    });
    const { bridge, calls } = harness(initial);
    const target = await render(bridge);
    await waitFor(() => target.textContent?.includes("待合并") === true);

    const diffSummary = [...target.querySelectorAll<HTMLElement>("summary")].find(
      (summary) => summary.textContent?.includes("查看改动") === true,
    )!;
    diffSummary.click();
    await waitFor(() => target.textContent?.includes("diff --git") === true);

    const merge = [...target.querySelectorAll<HTMLButtonElement>("button")].find(
      (candidate) => candidate.textContent?.trim() === "合并",
    )!;
    merge.click();
    await waitFor(() => target.textContent?.includes("确认合并到主工作区") === true);
    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.textContent?.trim() === "确认合并到主工作区")!
      .click();

    await waitFor(() => calls.includes("merge"));
    expect(calls).toContain("merge");
  });

  it("discards a failed task's worktree after confirmation", async () => {
    const initial = run({
      phase: "failed",
      tasks: [
        task({
          state: "failed",
          branch: "agent-island/team-1/t1",
          worktreePath: "C:\\worktrees\\t1",
          baseCommit: "abc123",
          changedFiles: ["a.txt"],
          mergeState: "pending",
        }),
      ],
    });
    const { bridge, calls } = harness(initial);
    const target = await render(bridge);
    await waitFor(() => target.textContent?.includes("任务未成功") === true);

    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.textContent?.trim() === "丢弃改动")!
      .click();
    await waitFor(() => target.textContent?.includes("确认丢弃改动") === true);
    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.textContent?.trim() === "确认丢弃改动")!
      .click();

    await waitFor(() => calls.includes("discard"));
    expect(calls).toContain("discard");
  });

  it("starts a follow-up round from a finished run", async () => {
    const initial = run({
      phase: "succeeded",
      summary: "上一轮完成",
      tasks: [task({ state: "succeeded" })],
    });
    const { bridge, calls, current } = harness(initial);
    const target = await render(bridge);
    await waitFor(() => target.textContent?.includes("再来一轮") === true);

    target.querySelector<HTMLButtonElement>('[data-testid="team-followup"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="team-followup-instruction"]') !== null);
    await userEvent.fill(
      target.querySelector<HTMLTextAreaElement>('[data-testid="team-followup-instruction"]')!,
      "补齐上一轮没验证的测试",
    );
    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((candidate) => candidate.textContent?.trim() === "继续规划")!
      .click();

    await waitFor(() => calls.includes("continue"));
    expect(current()?.parentRunId).toBe("team-1");
    expect(current()?.round).toBe(2);
  });

  it("blocks a follow-up while a previous merge is still pending", async () => {
    const initial = run({
      phase: "succeeded",
      summary: "上一轮完成",
      tasks: [
        task({
          state: "succeeded",
          branch: "agent-island/team-1/t1",
          changedFiles: ["a.txt"],
          mergeState: "pending",
        }),
      ],
    });
    const target = await render(harness(initial).bridge);
    await waitFor(() => target.textContent?.includes("先合并或丢弃") === true);
    expect(target.querySelector('[data-testid="team-followup"]')).toBeNull();
  });

  it("blocks a follow-up after a failed verification until the changes are discarded", async () => {
    const initial = run({
      phase: "failed",
      summary: null,
      tasks: [
        task({
          state: "failed",
          branch: "agent-island/team-1/t1",
          worktreePath: "C:\\worktrees\\t1",
          changedFiles: ["b.txt"],
          mergeState: "none",
          verifyCommand: "exit 3",
          verification: {
            command: "exit 3",
            passed: false,
            exitCode: 3,
            timedOut: false,
            durationMs: 8,
            output: "VERIFY_B",
          },
        }),
      ],
    });
    const target = await render(harness(initial).bridge);
    await waitFor(() => target.textContent?.includes("先合并或丢弃") === true);
    expect(target.querySelector('[data-testid="team-followup"]')).toBeNull();
  });

  it("shows a dependency and blocks the dependent merge until it is merged", async () => {
    const initial = run({
      phase: "succeeded",
      tasks: [
        task({
          id: "a",
          title: "A",
          state: "succeeded",
          branch: "agent-island/team-1/a",
          changedFiles: ["a.txt"],
          mergeState: "pending",
        }),
        task({
          id: "b",
          title: "B",
          dependsOn: ["a"],
          state: "succeeded",
          branch: "agent-island/team-1/b",
          changedFiles: ["b.txt"],
          mergeState: "pending",
        }),
      ],
    });
    const target = await render(harness(initial).bridge);
    await waitFor(() => target.textContent?.includes("先合并依赖任务：A") === true);
    const articles = [...target.querySelectorAll<HTMLElement>("article.task")];
    const dependent = articles.find(
      (article) => article.querySelector(".task-line strong")?.textContent?.trim() === "B",
    )!;
    expect(dependent.textContent).toContain("依赖：A");
    const mergeButtons = [...dependent.querySelectorAll<HTMLButtonElement>("button")].filter(
      (button) => button.textContent?.trim() === "合并",
    );
    expect(mergeButtons).toHaveLength(0);
  });

  it("shows chain dependencies in the plan preview", async () => {
    const initial = run({
      phase: "awaiting_plan",
      tasks: [
        task({ id: "a", title: "任务 A", state: "queued" }),
        task({ id: "b", title: "任务 B", state: "queued", dependsOn: ["a"] }),
      ],
    });
    const target = await render(harness(initial).bridge);
    await waitFor(() => target.textContent?.includes("依赖：任务 A") === true);
  });

  it("warns that discarding a dependency cascades to its chain", async () => {
    const initial = run({
      phase: "failed",
      tasks: [
        task({
          id: "a",
          title: "A",
          state: "succeeded",
          branch: "agent-island/team-1/a",
          worktreePath: "C:\\worktrees\\a",
          changedFiles: ["a.txt"],
          mergeState: "pending",
        }),
        task({
          id: "b",
          title: "B",
          dependsOn: ["a"],
          state: "failed",
          branch: "agent-island/team-1/b",
          worktreePath: "C:\\worktrees\\b",
          changedFiles: ["b.txt"],
          mergeState: "pending",
        }),
      ],
    });
    const target = await render(harness(initial).bridge);
    await waitFor(() => target.textContent?.includes("丢弃会级联 1 个后续任务") === true);
    const articles = [...target.querySelectorAll<HTMLElement>("article.task")];
    const root = articles.find(
      (article) => article.querySelector(".task-line strong")?.textContent?.trim() === "A",
    )!;
    [...root.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === "丢弃改动")!
      .click();
    await waitFor(() => target.textContent?.includes("确认丢弃（含 1 个后续任务）") === true);
  });

  it("shows a passed execution verification with its command output", async () => {
    const initial = run({
      phase: "succeeded",
      tasks: [
        task({
          state: "succeeded",
          verifyCommand: "echo OK",
          verification: {
            command: "echo OK",
            passed: true,
            exitCode: 0,
            timedOut: false,
            durationMs: 12,
            output: "OK",
          },
        }),
      ],
    });
    const target = await render(harness(initial).bridge);
    await waitFor(() => target.textContent?.includes("验收通过") === true);
    expect(target.textContent).toContain("验收通过");
    expect(target.textContent).toContain("退出码 0");
    expect(target.textContent).toContain("echo OK");
  });

  it("shows execution verification in the plan preview", async () => {
    const initial = run({
      phase: "awaiting_plan",
      tasks: [task({ verifyCommand: "echo OK" })],
    });
    const target = await render(harness(initial).bridge);
    await waitFor(() => target.textContent?.includes("验收：") === true);
    expect(target.textContent).toContain("echo OK");
  });

  it("asks for a second confirmation before a risky verification command", async () => {
    const initial = run({
      phase: "awaiting_plan",
      tasks: [task({ verifyCommand: "rm -rf build" })],
    });
    const { bridge, calls, riskyConfirmations } = harness(initial);
    const target = await render(bridge);
    await waitFor(() => target.textContent?.includes("确认并执行") === true);

    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === "确认并执行")!
      .click();
    await waitFor(() => target.textContent?.includes("验收命令需要再次确认") === true);
    expect(calls).not.toContain("confirm");
    expect(target.textContent).toContain("rm -rf build");

    [...target.querySelectorAll<HTMLButtonElement>("button")]
      .find((button) => button.textContent?.trim() === "仍然执行")!
      .click();
    await waitFor(() => calls.includes("confirm"));
    expect(riskyConfirmations[0]).toBe(true);
  });
});
