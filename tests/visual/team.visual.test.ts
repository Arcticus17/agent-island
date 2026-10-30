import { mount, unmount } from "svelte";
import { afterEach, expect, it, vi } from "vitest";
import { page } from "vitest/browser";

import TeamBoard from "../../src/windows/overview/TeamBoard.svelte";
import TeamStrip from "../../src/components/team/TeamStrip.svelte";
import type {
  AgentIslandBridge,
  TeamCapability,
  TeamRun,
  TeamTask,
} from "../../src/bridge/types";

const NOW = 1_789_130_000_000;
let app: ReturnType<typeof mount> | undefined;
let checkCalls = 0;

const GRAPHITE: Record<string, string> = {
  "--surface-card": "rgba(31, 37, 46, 0.92)",
  "--surface-raised": "rgba(39, 45, 56, 0.76)",
  "--surface-log": "rgba(4, 8, 14, 0.56)",
  "--text-primary": "#f5f7fb",
  "--text-secondary": "#aab4c3",
  "--text-tertiary": "#7f8a9a",
  "--text-log": "#dce4ef",
  "--border-subtle": "rgba(255, 255, 255, 0.11)",
  "--field": "rgba(255, 255, 255, 0.065)",
  "--danger": "rgba(190, 18, 60, 0.18)",
  "--danger-ink": "#fecdd3",
  "--status-idle": "#94a3b8",
  "--status-working": "#7dd3fc",
  "--status-error": "#fb7185",
  "--status-done": "#4ade80",
  "--status-waiting": "#fbbf24",
  "--status-stopped": "#78869a",
  "--accent-primary": "#7dd3fc",
  "--radius-card": "10px",
};

const ISLAND: Record<string, string> = {
  "--ink": "#f5f7fb",
  "--ink-2": "#c3ccda",
  "--ink-3": "#8e9aab",
  "--rule": "rgba(255, 255, 255, 0.1)",
  "--rule-strong": "rgba(255, 255, 255, 0.2)",
  "--field": "rgba(255, 255, 255, 0.065)",
  "--font-readout": "ui-monospace, SFMono-Regular, Consolas, monospace",
};

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

function task(overrides: Partial<TeamTask> = {}): TeamTask {
  return {
    id: "a",
    title: "实现设置页",
    assignee: "claude",
    assigneeName: "Claude Code",
    prompt: "实现设置页",
    dependsOn: [],
    acceptance: "设置页可打开",
    verifyCommand: null,
    verification: null,
    state: "queued",
    startedAtMs: NOW - 12_000,
    endedAtMs: NOW,
    exitCode: 0,
    result: "已实现设置页。",
    error: null,
    costUsd: 0.12,
    log: ["Write src/settings.ts", "Bash(npm run check)"],
    sessionId: "session-1",
    pendingApproval: null,
    approvalId: null,
    worktreePath: "C:\\worktrees\\a",
    branch: "agent-island/team-1/a",
    baseCommit: "abc123",
    changedFiles: ["src/settings.ts"],
    diffStat: "src/settings.ts | 12 ++++++++++++",
    mergeState: "pending",
    mergeError: null,
    ...overrides,
  };
}

function run(overrides: Partial<TeamRun> = {}): TeamRun {
  return {
    id: "team-1",
    goal: "补齐设置页并验收入口",
    workspace: "C:\\demo\\agent-island",
    mainAgent: "claude",
    mainAgentName: "Claude Code",
    phase: "awaiting_plan",
    planSummary: "拆成实现、验收两个任务。",
    summary: null,
    tasks: [task()],
    createdAtMs: NOW - 20_000,
    updatedAtMs: NOW,
    error: null,
    plannerSessionId: "planner-1",
    budgetUsd: 2,
    spentUsd: 0.24,
    repoRoot: "C:\\demo\\agent-island",
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

function makeBridge(value: TeamRun): AgentIslandBridge {
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
    async teamCapabilities() {
      return [capability];
    },
    async listTeamRuns() {
      return [value];
    },
    async getTeamRun() {
      return value;
    },
    async confirmTeamPlan() {
      return value;
    },
    async listenTeamRuns() {
      return () => {};
    },
    async checkTeamVerification(tasks) {
      checkCalls += 1;
      return tasks
        .filter((entry) => entry.verify_command.includes("rm "))
        .map((entry) => ({
          taskId: entry.id,
          taskTitle: entry.title,
          command: entry.verify_command,
          reason: "会删除文件",
        }));
    },
  };
}

function applyVars(target: HTMLElement, vars: Record<string, string>, background: string): void {
  for (const [key, value] of Object.entries(vars)) target.style.setProperty(key, value);
  target.style.background = background;
  target.style.color = vars["--text-primary"] ?? vars["--ink"] ?? "#fff";
  target.style.width = "100%";
  target.style.height = "100%";
  target.style.fontFamily = '"Segoe UI", "Microsoft YaHei", sans-serif';
}

async function mountBoard(value: TeamRun): Promise<HTMLElement> {
  const wrapper = document.createElement("div");
  wrapper.className = "overview";
  wrapper.dataset.themeStyle = "graphite";
  wrapper.style.width = "520px";
  wrapper.style.height = "900px";
  wrapper.style.overflow = "hidden";
  applyVars(wrapper, GRAPHITE, "#101318");
  document.body.append(wrapper);
  const target = document.createElement("div");
  target.style.height = "100%";
  wrapper.append(target);
  app = mount(TeamBoard, { target, props: { bridge: makeBridge(value) } });
  await expect.poll(() => target.querySelector('[data-testid="team-board"]') !== null).toBe(true);
  await expect.poll(() => target.textContent?.includes(value.goal) === true).toBe(true);
  return target;
}

async function mountStrip(value: TeamRun): Promise<HTMLElement> {
  const wrapper = document.createElement("div");
  wrapper.className = "agent-island";
  wrapper.dataset.themeStyle = "graphite";
  wrapper.style.width = "420px";
  wrapper.style.padding = "8px";
  wrapper.style.borderRadius = "14px";
  applyVars(wrapper, ISLAND, "#16161a");
  document.body.append(wrapper);
  const target = document.createElement("div");
  wrapper.append(target);
  app = mount(TeamStrip, { target, props: { bridge: makeBridge(value) } });
  await expect.poll(() => target.querySelector('[data-testid="team-strip"]') !== null).toBe(true);
  return target;
}

afterEach(async () => {
  if (app) await unmount(app);
  app = undefined;
  document.body.replaceChildren();
  localStorage.clear();
  vi.useRealTimers();
  vi.restoreAllMocks();
});

function freeze(): void {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(NOW);
  const style = document.createElement("style");
  style.textContent =
    "html,body { margin:0; background:#0b0d10; } *,*::before,*::after { animation:none!important; transition:none!important; caret-color:transparent!important; }";
  document.body.append(style);
  document.body.style.background = "#0b0d10";
}

it("graphite team plan with dependency and verification", async () => {
  freeze();
  await page.viewport(520, 900);
  await mountBoard(
    run({
      phase: "awaiting_plan",
      tasks: [
        task({
          id: "a",
          title: "实现设置页",
          state: "queued",
          verifyCommand: "npm test -- settings",
          dependsOn: [],
          changedFiles: [],
          mergeState: "none",
        }),
        task({
          id: "b",
          title: "验收设置页",
          state: "queued",
          dependsOn: ["a"],
          prompt: "打开设置页，验证保存后重新进入仍然生效；只改测试文件。",
          acceptance: "设置页可打开且保存生效",
          verifyCommand: "echo OK",
          changedFiles: [],
          mergeState: "none",
        }),
      ],
    }),
  );
  await expect.element(page.getByText("验收命令（可选）").first()).toBeVisible();
  await document.fonts.ready;
  await expect(page).toMatchScreenshot("team-plan", {
    comparatorOptions: { allowedMismatchedPixels: 0, threshold: 0.1 },
    screenshotOptions: { animations: "disabled", caret: "hide" },
  });
});

it("graphite team verification pass and fail", async () => {
  freeze();
  await page.viewport(520, 900);
  await mountBoard(
    run({
      phase: "failed",
      summary: null,
      error: "有任务失败或被跳过，未完成汇总",
      tasks: [
        task({
          id: "a",
          title: "实现设置页",
          state: "succeeded",
          changedFiles: ["src/settings.ts"],
          mergeState: "pending",
          verifyCommand: "npm test -- settings",
          verification: {
            command: "npm test -- settings",
            passed: true,
            exitCode: 0,
            timedOut: false,
            durationMs: 842,
            output: "Tests: 3 passed",
          },
        }),
        task({
          id: "b",
          title: "验收设置页",
          state: "failed",
          dependsOn: ["a"],
          changedFiles: ["src/settings.test.ts"],
          mergeState: "none",
          verifyCommand: "npm test -- settings",
          verification: {
            command: "npm test -- settings",
            passed: false,
            exitCode: 1,
            timedOut: false,
            durationMs: 619,
            output: "Tests: 2 failed\nsettings.test.ts:42 expected true",
          },
          error: "验收命令失败（退出码 1）：Tests: 2 failed",
        }),
      ],
    }),
  );
  await expect.element(page.getByText("验收通过").first()).toBeVisible();
  await expect.element(page.getByText("验收失败").first()).toBeVisible();
  await document.fonts.ready;
  await expect(page).toMatchScreenshot("team-verification", {
    comparatorOptions: { allowedMismatchedPixels: 0, threshold: 0.1 },
    screenshotOptions: { animations: "disabled", caret: "hide" },
  });
});

it("graphite risky verification needs a second confirmation", async () => {
  freeze();
  checkCalls = 0;
  await page.viewport(520, 900);
  const target = await mountBoard(
    run({
      phase: "awaiting_plan",
      tasks: [
        task({
          id: "a",
          title: "清理构建目录",
          state: "queued",
          verifyCommand: "rm -rf build",
          changedFiles: [],
          mergeState: "none",
        }),
      ],
    }),
  );
  await expect.element(page.getByText("确认并执行").first()).toBeVisible();
  [...target.querySelectorAll<HTMLButtonElement>("button")]
    .find((button) => button.textContent?.trim() === "确认并执行")!
    .click();
  await expect.poll(() => checkCalls, { timeout: 3_000 }).toBe(1);
  await expect.element(page.getByText("验收命令需要再次确认")).toBeVisible();
  await document.fonts.ready;
  await expect(page).toMatchScreenshot("team-risk", {
    comparatorOptions: { allowedMismatchedPixels: 0, threshold: 0.1 },
    screenshotOptions: { animations: "disabled", caret: "hide" },
  });
});

it("graphite dependency chain after the first merge", async () => {
  freeze();
  await page.viewport(520, 900);
  await mountBoard(
    run({
      phase: "succeeded",
      summary: "实现任务已合并，验收任务待合并。",
      tasks: [
        task({
          id: "a",
          title: "实现设置页",
          state: "succeeded",
          changedFiles: ["src/settings.ts"],
          mergeState: "merged",
          worktreePath: null,
        }),
        task({
          id: "b",
          title: "验收设置页",
          state: "succeeded",
          dependsOn: ["a"],
          changedFiles: ["src/settings.test.ts"],
          mergeState: "pending",
          verifyCommand: "npm test -- settings",
          verification: {
            command: "npm test -- settings",
            passed: true,
            exitCode: 0,
            timedOut: false,
            durationMs: 755,
            output: "Tests: 3 passed",
          },
        }),
      ],
    }),
  );
  await expect.element(page.getByText("依赖：实现设置页").first()).toBeVisible();
  await expect.element(page.getByRole("button", { name: "合并" }).first()).toBeVisible();
  await document.fonts.ready;
  await expect(page).toMatchScreenshot("team-dependency-merge", {
    comparatorOptions: { allowedMismatchedPixels: 0, threshold: 0.1 },
    screenshotOptions: { animations: "disabled", caret: "hide" },
  });
});

it("island team strip with a waiting approval", async () => {
  freeze();
  await page.viewport(420, 72);
  await mountStrip(
    run({
      phase: "running",
      tasks: [
        task({ id: "a", state: "succeeded" }),
        task({ id: "b", state: "waiting_approval", pendingApproval: "rm -rf build" }),
        task({ id: "c", state: "queued" }),
      ],
    }),
  );
  await document.fonts.ready;
  await expect(page).toMatchScreenshot("team-strip-waiting", {
    comparatorOptions: { allowedMismatchedPixels: 0, threshold: 0.1 },
    screenshotOptions: { animations: "disabled", caret: "hide" },
  });
});
