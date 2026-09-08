import { mount, unmount, type Component } from "svelte";
import { afterEach, describe, expect, it, vi } from "vitest";

import DiagnosticsPanel from "../../src/components/diagnostics/DiagnosticsPanel.svelte";
import {
  createDiagnosticsBridge,
  parseDiagnosticsResponse,
  type DiagnosticsResponse,
} from "../../src/bridge/diagnostics";
import type { DiagnosticView } from "../../src/bridge/types";
import BoundaryHarness from "./fixtures/BoundaryHarness.svelte";

const mounted: Array<ReturnType<typeof mount>> = [];

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((promiseResolve, promiseReject) => {
    resolve = promiseResolve;
    reject = promiseReject;
  });
  return { promise, resolve, reject };
}

async function waitFor(check: () => boolean, timeoutMs = 1_000): Promise<void> {
  const deadline = Date.now() + timeoutMs;
  while (!check()) {
    if (Date.now() >= deadline) throw new Error("browser_test_wait_timeout");
    await new Promise((resolve) => setTimeout(resolve, 10));
  }
}

function render<Props extends Record<string, unknown>>(
  component: Component<Props>,
  props: Props,
): HTMLElement {
  const target = document.createElement("div");
  document.body.append(target);
  mounted.push(mount(component, { target, props }));
  return target;
}

function issue(
  adapter: string,
  code: string,
  overrides: Partial<DiagnosticView> = {},
): DiagnosticView {
  return {
    adapter,
    code,
    freshness: { observed_at_ms: 1_725_456_789_000, stale: false },
    event_type: "assistant_message",
    event_count: 4,
    message_count: 2,
    ...overrides,
  };
}

afterEach(async () => {
  while (mounted.length) await unmount(mounted.pop()!);
  document.body.replaceChildren();
  vi.restoreAllMocks();
});

describe("component-level failure isolation", () => {
  it("contains one failed card, keeps its sibling interactive, and recovers through reload", async () => {
    const target = render(BoundaryHarness, {});

    await waitFor(() => target.querySelector('[data-testid="card-fallback"]') !== null);
    const fallback = target.querySelector<HTMLElement>('[data-testid="card-fallback"]')!;
    expect(fallback.textContent).toContain("会话卡片");
    expect(fallback.textContent).toContain("session_card_render_failed");
    expect(fallback.dataset.errorCode).toBe("session_card_render_failed");
    expect(target.querySelector('[data-testid="recovered-card"]')).toBeNull();

    target.querySelector<HTMLButtonElement>('[data-testid="healthy-action"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="healthy-count"]')?.textContent === "1");

    target.querySelector<HTMLButtonElement>('[data-testid="repair-failed-card"]')!.click();
    fallback.querySelector<HTMLButtonElement>('button[aria-label="重新加载会话卡片"]')!.click();
    await waitFor(() => target.querySelector('[data-testid="recovered-card"]') !== null);
    expect(target.querySelector('[data-testid="card-fallback"]')).toBeNull();
    expect(target.querySelector('[data-testid="healthy-count"]')?.textContent).toBe("1");
  });
});

describe("diagnostics boundary adapter", () => {
  it("calls only the two diagnostics commands with the expected arguments", async () => {
    const invoke = vi.fn(async (command: string) => command === "get_diagnostics"
      ? { generated_at_ms: 7, issues: [] }
      : undefined);
    const bridge = createDiagnosticsBridge(invoke);

    await expect(bridge.getDiagnostics()).resolves.toEqual({ generated_at_ms: 7, issues: [] });
    await expect(bridge.exportDiagnostics("D:\\exports\\report.json")).resolves.toBeUndefined();

    expect(invoke.mock.calls).toEqual([
      ["get_diagnostics"],
      ["export_diagnostics", { destination: "D:\\exports\\report.json" }],
    ]);
  });

  it("rejects an empty export path before invoking the backend", async () => {
    const invoke = vi.fn();
    const bridge = createDiagnosticsBridge(invoke);

    await expect(bridge.exportDiagnostics("   ")).rejects.toThrow("diagnostics_destination_empty");
    expect(invoke).not.toHaveBeenCalled();
  });

  it("keeps only the public diagnostic fields and discards sensitive extras", () => {
    const response = parseDiagnosticsResponse({
      generated_at_ms: 1_725_456_789_100,
      issues: [
        {
          adapter: "codex",
          code: "parse_failed",
          freshness: { observed_at_ms: 1_725_456_789_000, stale: false },
          event_type: "assistant_message",
          event_count: 4,
          message_count: 2,
          skipped_lines: 1,
          prompt: "secret prompt text",
          project_path: "D:\\private\\client\\project-alpha",
          session_id: "private-session-id",
        },
      ],
      prompt: "top-level secret",
      project_path: "D:\\private\\client",
    });

    expect(response).toEqual({
      generated_at_ms: 1_725_456_789_100,
      issues: [
        {
          adapter: "codex",
          code: "parse_failed",
          freshness: { observed_at_ms: 1_725_456_789_000, stale: false },
          event_type: "assistant_message",
          event_count: 4,
          message_count: 2,
          skipped_lines: 1,
        },
      ],
    } satisfies DiagnosticsResponse);
    expect(JSON.stringify(response)).not.toContain("secret prompt text");
    expect(JSON.stringify(response)).not.toContain("project-alpha");
    expect(JSON.stringify(response)).not.toContain("private-session-id");
  });

  it.each([
    ["generated_at_ms", { generated_at_ms: -1, issues: [] }],
    ["event_count", {
      generated_at_ms: 1,
      issues: [{
        adapter: "codex",
        code: "parse_failed",
        freshness: { observed_at_ms: 1, stale: false },
        event_type: null,
        event_count: -1,
        message_count: 0,
      }],
    }],
    ["message_count", {
      generated_at_ms: 1,
      issues: [{
        adapter: "claude",
        code: "log_unavailable",
        freshness: { observed_at_ms: 1, stale: true },
        event_type: null,
        event_count: 0,
        message_count: -1,
      }],
    }],
  ])("rejects a negative %s", (_field, payload) => {
    expect(() => parseDiagnosticsResponse(payload)).toThrow("invalid_diagnostics_response");
  });
});

describe("diagnostics panel", () => {
  const issues = [
    issue("codex", "parse_failed", { skipped_lines: 3 }),
    issue("claude", "session_ambiguous", { candidate_count: 2, event_type: null }),
  ];

  it("shows two structured adapter issues and exports to the exact user path", async () => {
    const pendingExport = deferred<void>();
    const exportDiagnostics = vi.fn(async (_destination: string) => pendingExport.promise);
    const target = render(DiagnosticsPanel, { issues, exportDiagnostics });

    expect(target.querySelectorAll('[data-testid="diagnostic-issue"]')).toHaveLength(2);
    expect(target.textContent).toContain("codex");
    expect(target.textContent).toContain("parse_failed");
    expect(target.textContent).toContain("claude");
    expect(target.textContent).toContain("session_ambiguous");

    const destination = "D:\\exports\\agent-island-diagnostics.json";
    const input = target.querySelector<HTMLInputElement>('[data-testid="diagnostics-export-path"]')!;
    input.value = destination;
    input.dispatchEvent(new Event("input", { bubbles: true }));
    target.querySelector<HTMLButtonElement>('[data-testid="diagnostics-export"]')!.click();

    await waitFor(() => exportDiagnostics.mock.calls.length === 1);
    expect(exportDiagnostics).toHaveBeenCalledWith(destination);
    expect(input.disabled).toBe(true);
    pendingExport.resolve();
    await waitFor(() => target.querySelector('[data-testid="diagnostics-export-feedback"]')?.textContent?.includes("导出成功") === true);
    expect(target.querySelector('[data-testid="diagnostics-export-feedback"]')?.getAttribute("role")).toBe("status");
  });

  it("shows a stable failure state when export rejects", async () => {
    const exportDiagnostics = vi.fn(async (_destination: string) => {
      throw new Error("disk unavailable");
    });
    const target = render(DiagnosticsPanel, { issues, exportDiagnostics });
    const input = target.querySelector<HTMLInputElement>('[data-testid="diagnostics-export-path"]')!;
    input.value = "D:\\exports\\blocked.json";
    input.dispatchEvent(new Event("input", { bubbles: true }));
    target.querySelector<HTMLButtonElement>('[data-testid="diagnostics-export"]')!.click();

    await waitFor(() => target.querySelector('[data-testid="diagnostics-export-feedback"]')?.textContent?.includes("export_failed") === true);
    expect(target.querySelector('[data-testid="diagnostics-export-feedback"]')?.getAttribute("role")).toBe("alert");
    expect(target.querySelector('[data-testid="diagnostics-export-feedback"]')?.textContent).not.toContain("D:\\exports\\blocked.json");
  });

  it("distinguishes an unavailable diagnostics read from a confirmed healthy state", () => {
    const target = render(DiagnosticsPanel, {
      issues: [],
      status: "failed" as const,
      exportDiagnostics: async () => undefined,
    });

    const state = target.querySelector('[data-testid="diagnostics-state"]');
    expect(state?.getAttribute("role")).toBe("alert");
    expect(state?.textContent).toContain("diagnostics_fetch_failed");
    expect(target.textContent).not.toContain("状态正常");
  });
});
